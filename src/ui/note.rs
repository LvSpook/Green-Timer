//! The timer's single note, shown in its own frameless window.
//!
//! Like the right-click menu this is a *deferred* egui viewport: a real OS
//! window that eframe drives. An in-viewport widget would be clipped to the
//! overlay, which is only a few dozen pixels tall.
//!
//! The window is deliberately blank: no title bar, no buttons, no counters,
//! just the text on the timer's own background, framed in the timer's colour.
//! Only a click on the timer itself closes it, so it stays put while the user
//! works in another window.
//!
//! The text is stored in `Config::note`, so it is saved with every other
//! setting and survives a restart. Edits are handed back as they are typed, so
//! nothing is lost if the app is killed mid-sentence.

use crate::config::{Config, NOTE_MAX_CHARS, NOTE_MIN_SIDE};
use crate::render::to_color32;
use egui::{Color32, Pos2, Rect, Stroke};
use std::cell::RefCell;
use std::time::Instant;

/// Gap between the overlay and the note window.
const GAP: f32 = 10.0;
/// Inset from the frame to the text, in points.
const MARGIN: f32 = 10.0;
/// Title of the note's OS window. Stable so the compositor fade can find it.
const NOTE_WINDOW_TITLE: &str = "Green Timer Note";

/// The appearance the open window is currently drawn with, so a change made in
/// Settings can repaint it at once without polling every frame.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    frame: Color32,
    paper: Color32,
    ink: Color32,
    resizable: bool,
    border: bool,
    radius: f32,
    size: egui::Vec2,
}

#[derive(Default)]
struct State {
    open: bool,
    /// Bumped on every open so each showing gets a fresh viewport.
    generation: u64,
    /// Where the window was placed when it opened.
    position: Option<Pos2>,
    /// The overlay rect last seen, so the note can follow the timer as it is
    /// dragged around the desktop.
    anchor: Option<Rect>,
    /// The work area, in points, captured when the window opened.
    screen: Option<Rect>,
    /// Inner size seen on the previous frame, so a resize drag is detected
    /// without a dedicated "resizing" flag from the OS.
    live_inner: Option<egui::Vec2>,
    /// Size at which the window opened. Drags are applied on top of it.
    size: Option<egui::Vec2>,
    /// The text being edited. Mirrors `Config::note`, plus any unsaved edits.
    buffer: String,
    /// Text last handed to the app, so each edit is saved only once.
    reported: String,
    /// Edits waiting for the app to store.
    pending: Option<String>,
    /// Viewport currently on screen, so it can be closed from the overlay.
    shown: Option<u64>,
    /// When the window dismissed itself, so a click on the timer that caused it
    /// does not immediately reopen the note.
    dismissed_at: Option<Instant>,
    /// The editor still needs the keyboard handed to it.
    autofocus: bool,
    /// Size reported back to the app by the last drag, so each change is
    /// written once rather than every frame.
    reported_size: Option<(u32, u32)>,
    /// Size the window currently has, and when it last changed. A drag emits
    /// many intermediate sizes, so only the size the window settles on is
    /// handed over: otherwise a flick of the mouse would save a stray size.
    live_size: Option<(u32, u32)>,
    live_size_at: Option<Instant>,
    /// Whether the window has been told to composite its own alpha yet.
    made_transparent: bool,
    /// Colours the window was last drawn with.
    look: Option<Look>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn viewport_id(generation: u64) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("green-timer-note", generation))
}

/// Size of the note window, clamped to what the config allows.
fn note_size(config: &Config) -> egui::Vec2 {
    let width = config.note_width.max(NOTE_MIN_SIDE) as f32;
    let height = config.note_height.max(NOTE_MIN_SIDE) as f32;
    egui::vec2(width, height)
}

/// Show the note window beneath `anchor` (the overlay's monitor rect).
///
/// `anchor` and `pixels_per_point` are the overlay's own, so the note is placed
/// in the same units egui uses for the overlay. The Windows work area comes back
/// in physical pixels, so it is divided by the scale before use; otherwise the
/// note lands in the wrong place on a scaled display.
pub fn open(anchor: Rect, config: &Config, pixels_per_point: f32) {
    let (left, top, width, height) =
        crate::platform::primary_work_area(pixels_per_point.max(0.5));
    let screen = Rect::from_min_size(Pos2::new(left, top), egui::vec2(width, height));
    let size = note_size(config);
    let position = place_near(anchor, screen, size);
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.generation = state.generation.wrapping_add(1);
        state.open = true;
        state.size = Some(size);
        state.position = Some(position);
        state.anchor = Some(anchor);
        state.screen = Some(screen);
        state.live_inner = None;
        state.buffer = config.note.clone();
        state.reported = config.note.clone();
        state.pending = None;
        state.autofocus = true;
        state.reported_size = None;
        state.live_size = None;
        state.live_size_at = None;
        state.made_transparent = false;
        state.look = None;
    });
}

/// Close the window, keeping whatever was typed.
pub fn close() {
    // Typing in the note must never leave global hotkeys switched off.
    crate::hotkeys::set_suspended(false);
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.open = false;
        state.position = None;
        state.anchor = None;
        state.screen = None;
        state.live_inner = None;
        state.size = None;
        // Closing right after a drag must still remember the new size, even
        // though it had not yet settled long enough to be reported.
        if let Some(size) = state.live_size {
            if state.reported_size != Some(size) {
                state.reported_size = Some(size);
            }
        }
        state.live_size = None;
        state.live_size_at = None;
        // A queued edit is deliberately kept: the app may not have collected it
        // yet, and closing the window must not swallow the last keystroke.
    });
}

/// Whether a left-click on the timer should be ignored because the note closed
/// itself a moment ago (that same click is what dismissed it).
pub fn just_dismissed() -> bool {
    STATE.with(|state| {
        state
            .borrow()
            .dismissed_at
            .is_some_and(|at| at.elapsed().as_millis() < 400)
    })
}

pub fn is_open() -> bool {
    STATE.with(|state| state.borrow().open)
}

/// Adopt text changed outside the window (Settings saving or clearing a note).
pub fn set_text(text: &str) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.buffer != text {
            state.buffer = text.to_string();
            state.reported = text.to_string();
            state.pending = None;
        }
    });
}

/// Take the edited text, if it changed since it was last handed over.
pub fn take_edit() -> Option<String> {
    STATE.with(|state| state.borrow_mut().pending.take())
}

/// Take the size the user dragged the window to, if it changed.
pub fn take_size() -> Option<(u32, u32)> {
    STATE.with(|state| state.borrow_mut().reported_size.take())
}

/// Draw the note window if it is open. Must be called every frame.
pub fn show(ctx: &egui::Context, config: &Config) {
    let Some((generation, size)) = STATE.with(|state| {
        let state = state.borrow();
        state.open.then(|| (state.generation, state.size))
    }) else {
        // Closing hides the window from `close()`; make sure the viewport is
        // told too, or the empty window would linger on screen.
        let stale = STATE.with(|state| state.borrow_mut().shown.take());
        if let Some(generation) = stale {
            ctx.send_viewport_cmd_to(viewport_id(generation), egui::ViewportCommand::Close);
        }
        return;
    };
    let Some(size) = size else {
        return;
    };

    STATE.with(|state| state.borrow_mut().shown = Some(generation));

    // Only a click on the timer closes the note. That click arrives here as
    // root input while the overlay is the active window; `note_click_at` in the
    // app decides when a click counts, and calls `close()` itself.
    if crate::menu::is_open() {
        // The right-click menu on the overlay covers the note's purpose.
        dismiss(ctx);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        return;
    }

    let config = config.clone();
    let resizable = config.note_resizable;
    let position = follow(ctx, viewport_id(generation), size);
    ctx.show_viewport_deferred(
        viewport_id(generation),
        egui::ViewportBuilder::default()
            .with_title(NOTE_WINDOW_TITLE)
            .with_icon(std::sync::Arc::new(crate::render::themed_window_icon(
                &config,
            )))
            .with_decorations(false)
            // The transparency flag is left off deliberately: the driver refuses
            // a transparent config for a second window and eframe would log an
            // error every time the note opens. `set_window_transparent` asks the
            // desktop compositor instead, which works on this driver.
            .with_transparent(false)
            .with_resizable(resizable)
            .with_taskbar(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_position(position)
            .with_inner_size([size.x, size.y])
            .with_min_inner_size([NOTE_MIN_SIDE as f32, NOTE_MIN_SIDE as f32])
            // Take the keyboard at once: a bound letter should type, not run
            // its shortcut.
            .with_active(true),
        move |ctx, _class| draw(ctx, &config, generation),
    );
}

/// Keep the note under the timer: follow the overlay as it is dragged around,
/// and recentre the note under it whenever its own size changes.
///
/// The window has no title bar, so its position is always derived rather than
/// remembered. Repositioning is held back while the note itself is being
/// resized, or a drag of its edges would be fought by a stream of moves; the
/// note recentres under the timer as soon as that drag ends.
fn follow(
    ctx: &egui::Context,
    id: egui::ViewportId,
    config_size: egui::Vec2,
) -> egui::Pos2 {
    // The overlay is the root viewport, and `ctx.input` would read whichever
    // viewport happens to be current, so both shapes are named explicitly.
    let anchor = ctx.input_for(egui::ViewportId::ROOT, |i| i.viewport().outer_rect);
    let (note_rect, pointer_down) =
        ctx.input_for(id, |i| (i.viewport().inner_rect, i.pointer.any_down()));

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(anchor) = anchor {
            state.anchor = Some(anchor);
        }
        let Some(anchor) = state.anchor else {
            return state.position.unwrap_or(egui::Pos2::ZERO);
        };
        let Some(screen) = state.screen else {
            return state.position.unwrap_or(egui::Pos2::ZERO);
        };

        // A size the user dragged to is honoured straight away, before the app
        // has had a chance to store it, so the note recentres without a lag.
        let current_size = note_rect.map(|rect| rect.size()).unwrap_or(config_size);
        let resizing = state
            .live_inner
            .is_some_and(|last| (last - current_size).length() > 0.5);
        state.live_inner = Some(current_size);
        if resizing || pointer_down {
            // Mid-drag: leave the window where the OS is putting it, so the
            // drag is not fought. The next quiet frame recentres it.
            return state.position.unwrap_or(egui::Pos2::ZERO);
        }

        let position = place_near(anchor, screen, current_size);
        state.position = Some(position);
        position
    })
}

/// Put the window near the overlay, kept on the screen it is on.
fn place_near(anchor: Rect, screen: Rect, size: egui::Vec2) -> Pos2 {
    let below = anchor.bottom() + GAP;
    let y = if below + size.y <= screen.bottom() {
        below
    } else {
        // No room underneath, so sit above the overlay instead.
        (anchor.top() - GAP - size.y).max(screen.top())
    };
    let x = (anchor.center().x - size.x / 2.0)
        .clamp(screen.left(), (screen.right() - size.x).max(screen.left()));
    Pos2::new(x.round(), y.round())
}

/// The note's frame colour: the timer's own numbers, so the two always match.
fn frame_color(config: &Config) -> Color32 {
    crate::render::target_color(config, crate::engine::ColorRole::Normal)
}

/// The colour of the note's text: the user's pick, or the timer's numbers.
fn text_color(config: &Config) -> Color32 {
    match config.note_text_color {
        Some(rgb) => to_color32(rgb, 255),
        None => frame_color(config),
    }
}

/// The background the note is written on: the timer's own panel colour, faded
/// by the note's opacity.
///
/// Only the paper carries this alpha: the text and frame are drawn on top at
/// full strength, so the note fades towards whatever is behind it while the
/// writing stays crisp. That needs a window surface with a real alpha channel,
/// which `set_window_transparent` asks the desktop compositor for.
///
/// Following the timer copies its panel exactly, theme overrides and all, so a
/// theme that makes the timer solid makes the note solid too.
pub fn paper_color(config: &Config) -> Color32 {
    let panel = crate::render::panel_color(config, crate::render::background_alpha(config));
    match config.note_bg_opacity {
        None => panel,
        Some(opacity) => Color32::from_rgba_unmultiplied(
            panel.r(),
            panel.g(),
            panel.b(),
            (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
        ),
    }
}

/// Opacity of the note's paper, `0.0..=1.0`.
///
/// `None` means "same as the timer's background", so the two match by default.
pub fn note_opacity(config: &Config) -> f32 {
    match config.note_bg_opacity {
        Some(opacity) => opacity.clamp(0.0, 1.0),
        None => timer_note_opacity(config),
    }
}

/// Opacity of the timer's own background, `0.0..=1.0`, which is what the note
/// uses by default so the two match.
///
/// This is the alpha the panel is really painted with, so it accounts for the
/// theme's own override rather than just the background setting.
pub fn timer_note_opacity(config: &Config) -> f32 {
    let panel = crate::render::panel_color(config, crate::render::background_alpha(config));
    panel.a() as f32 / 255.0
}

/// The note window's alpha, `0..=255`, over the desktop.
pub fn note_alpha(config: &Config) -> u8 {
    paper_color(config).a()
}

/// The note's border, in the timer's number colour.
///
/// Switched off entirely by the "Show border" setting, so the text can use the
/// space the stroke would have taken.
fn frame_stroke(config: &Config) -> Stroke {
    if config.note_border {
        Stroke::new(2.0_f32, frame_color(config))
    } else {
        Stroke::NONE
    }
}

/// Corner radius for the note, in points.
///
/// `None` follows the timer's own corners, so the note echoes the overlay. The
/// radius is capped at half the shorter side, past which the rounding would
/// stop looking like a rounded rectangle.
pub fn note_radius(config: &Config, rect: Rect) -> f32 {
    let timer = match config.note_corner_radius {
        Some(radius) => radius as f32,
        None => crate::render::corner_radius(config, rect),
    };
    timer
        .max(0.0)
        .min(rect.width().min(rect.height()) / 2.0)
}

/// Visuals used only inside the note window: the user's ink colour on a clear
/// background, with the caret in the ink colour and the selection highlight in
/// a translucent version of the frame colour.
///
/// This is applied to the note's own `Ui` rather than the context, because the
/// overlay, menu and Settings dialog share the same `egui::Context` and must
/// keep their themed styling.
fn note_visuals(ink: Color32, frame: Color32) -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.window_fill = Color32::TRANSPARENT;
    visuals.extreme_bg_color = Color32::TRANSPARENT;
    visuals.faint_bg_color = Color32::TRANSPARENT;
    visuals.override_text_color = Some(ink);
    visuals.text_cursor.stroke = Stroke::new(2.0_f32, ink);
    visuals.selection.bg_fill = frame.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0_f32, frame);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = Color32::TRANSPARENT;
        widget.weak_bg_fill = Color32::TRANSPARENT;
        widget.bg_stroke = Stroke::NONE;
    }
    // The scroll bar handle is the only widget with a fill here (the editor's
    // own frame is off), so tint it with the frame colour and keep it faint.
    for (widget, alpha) in [
        (&mut visuals.widgets.inactive, 0.40),
        (&mut visuals.widgets.hovered, 0.70),
        (&mut visuals.widgets.active, 1.0),
    ] {
        widget.bg_fill = frame.gamma_multiply(alpha);
        widget.weak_bg_fill = frame.gamma_multiply(alpha);
        widget.bg_stroke = Stroke::NONE;
    }
    visuals
}

fn draw(ctx: &egui::Context, config: &Config, generation: u64) {
    if !is_open() {
        return;
    }

    // A key press on the overlay cannot reach this window, but Escape here
    // still closes it when the note itself has focus.
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        dismiss(ctx);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        return;
    }

    let frame = frame_color(config);
    let paper = paper_color(config);
    let ink = text_color(config);
    let size = note_size(config);

    // A colour or size changed in Settings while the note is open: repaint the
    // note's own window, which is deferred and so is not redrawn with the
    // overlay.
    let look = Look {
        frame,
        paper,
        ink,
        resizable: config.note_resizable,
        border: config.note_border,
        radius: note_radius(config, Rect::from_min_size(Pos2::ZERO, size)),
        size,
    };
    let changed = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let changed = state.look != Some(look);
        state.look = Some(look);
        changed
    });
    if changed {
        let id = viewport_id(generation);
        ctx.request_repaint_of(id);
        ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Resizable(config.note_resizable));
        // A size change is only nudged when it really differs from what the
        // window shows, so a drag-resize is never fought by a stale value.
        let current = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
        let differs = current.map_or(true, |current| (current - size).length() > 2.0);
        if differs {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::InnerSize(size));
        }
    }

    // The editor must use the note's own colours for its caret and selection
    // rather than the timer's theme. Setting a style on this `Ui` scopes the
    // change to this window; `ctx.set_visuals` would leak into the overlay and
    // the Settings dialog, which share the context.
    let mut style = (*ctx.style()).clone();
    style.visuals = note_visuals(ink, frame);
    let mut text = STATE.with(|state| state.borrow().buffer.clone());
    let mut edited = false;

    // The paper and frame are painted on the background layer so the text can
    // use the whole block. The stroke is inset by half its width: on the window
    // edge a centred stroke would be clipped to half thickness.
    let window = ctx.screen_rect();
    let edge = frame_stroke(config);
    let radius = note_radius(config, window);
    let paper_rect = window.shrink(edge.width * 0.5);
    // The surface carries a real alpha channel, so the rounded corners simply
    // show whatever is behind the note, exactly like the timer's own panel.
    ctx.layer_painter(egui::LayerId::background()).rect(
        paper_rect,
        egui::Rounding::same(radius),
        paper,
        edge,
    );

    // Without a border the text keeps the space the stroke would have taken.
    let inset = MARGIN + edge.width;

    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(Color32::TRANSPARENT)
                .inner_margin(egui::Margin::same(inset)),
        )
        .show(ctx, |ui| {
            ui.set_style(style);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(1)
                            .frame(false)
                            .margin(egui::Margin::ZERO)
                            .char_limit(NOTE_MAX_CHARS),
                    );
                    if STATE.with(|state| state.borrow_mut().autofocus) {
                        STATE.with(|state| state.borrow_mut().autofocus = false);
                        response.request_focus();
                    }
                    // Global hotkeys are only muted while the editor is really
                    // typing. Without the window-focus check, clicking another
                    // app would leave the note's editor focused and mute every
                    // hotkey until the note was closed.
                    crate::hotkeys::set_suspended(
                        response.has_focus() && ui.ctx().input(|i| i.focused),
                    );
                    edited = response.changed();
                });
        });

    if edited {
        store(&text);
    }

    // The paper carries its own alpha now that the window composites it, so the
    // note never needs fading as a whole: the text stays at full strength while
    // the page fades. The overlay gets a see-through surface from Windows when
    // it is created; a child window like this does not, so ask for it once.
    let needs_transparency = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.made_transparent {
            return false;
        }
        state.made_transparent = crate::platform::set_window_transparent(NOTE_WINDOW_TITLE);
        log::debug!("note transparency: {}", state.made_transparent);
        !state.made_transparent
    });
    if needs_transparency {
        // The window does not exist yet on the first frame; try again next one.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
    // A frameless window gets no native resize border, so the edges and corners
    // are hit-tested here and handed to the OS with `BeginResize`.
    if config.note_resizable {
        resize_grips(ctx);
    }

    if ctx.input(|i| i.viewport().close_requested()) {
        dismiss(ctx);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        return;
    }

    report_size(ctx);
}

/// Thickness of the draggable border, in points.
const GRIP: f32 = 8.0;

/// Let the window be resized by dragging its edges and corners.
///
/// egui has no built-in support for resizing a frameless window, so the outer
/// few points are hit-tested directly and an OS-driven resize is started. The
/// grips sit outside the text area's own inset, so they never take a click
/// meant for the editor or a selection drag.
fn resize_grips(ctx: &egui::Context) {
    use egui::viewport::ResizeDirection;
    let area = ctx.screen_rect();
    let inner = area.shrink(GRIP);

    let Some(pointer) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    // Only the border band is a grip, never the whole window.
    if !area.contains(pointer) || inner.contains(pointer) {
        return;
    }

    let near_west = pointer.x <= inner.left();
    let near_east = pointer.x >= inner.right();
    let near_north = pointer.y <= inner.top();
    let near_south = pointer.y >= inner.bottom();

    let (direction, cursor) = match (near_north, near_south, near_west, near_east) {
        (true, _, true, _) => (ResizeDirection::NorthWest, egui::CursorIcon::ResizeNwSe),
        (true, _, _, true) => (ResizeDirection::NorthEast, egui::CursorIcon::ResizeNeSw),
        (_, true, true, _) => (ResizeDirection::SouthWest, egui::CursorIcon::ResizeNeSw),
        (_, true, _, true) => (ResizeDirection::SouthEast, egui::CursorIcon::ResizeNwSe),
        (true, _, _, _) => (ResizeDirection::North, egui::CursorIcon::ResizeVertical),
        (_, true, _, _) => (ResizeDirection::South, egui::CursorIcon::ResizeVertical),
        (_, _, true, _) => (ResizeDirection::West, egui::CursorIcon::ResizeHorizontal),
        _ => (ResizeDirection::East, egui::CursorIcon::ResizeHorizontal),
    };

    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }
}

/// Persist a size the user dragged the window to, in points.
///
/// A drag produces a stream of sizes, so the current one is only handed over
/// once it has stopped changing for a moment. Otherwise a fast drag would save
/// an arbitrary intermediate size as the note's remembered size.
fn report_size(ctx: &egui::Context) {
    const SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

    let Some(inner) = ctx.input(|i| i.viewport().inner_rect) else {
        return;
    };
    let size = (
        inner.width().round().max(NOTE_MIN_SIDE as f32) as u32,
        inner.height().round().max(NOTE_MIN_SIDE as f32) as u32,
    );

    let now = Instant::now();
    let due = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.live_size != Some(size) {
            // Still moving: remember it and restart the wait.
            state.live_size = Some(size);
            state.live_size_at = Some(now);
            ctx.request_repaint_after(SETTLE);
            return false;
        }
        state
            .live_size_at
            .is_some_and(|at| now.saturating_duration_since(at) >= SETTLE)
            && state.reported_size != Some(size)
    });

    if due {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.reported_size = Some(size);
            state.live_size_at = None;
        });
    }
}

/// Tear the window down, remembering that it was dismissed deliberately.
fn dismiss(ctx: &egui::Context) {
    let shown = STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.dismissed_at = Some(Instant::now());
        s.shown.take()
    });
    close();
    if let Some(generation) = shown {
        ctx.send_viewport_cmd_to(viewport_id(generation), egui::ViewportCommand::Close);
    }
}

/// Remember edits and queue them for the app to persist.
fn store(text: &str) {
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        if s.buffer == text {
            return;
        }
        s.buffer = text.to_string();
        if s.reported != *text {
            s.reported = text.to_string();
            s.pending = Some(text.to_string());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1040.0))
    }

    fn size() -> egui::Vec2 {
        egui::vec2(280.0, 280.0)
    }

    #[test]
    fn note_sits_below_the_overlay() {
        let anchor = Rect::from_min_size(Pos2::new(1500.0, 60.0), egui::vec2(211.0, 103.0));
        let pos = place_near(anchor, screen(), size());
        assert_eq!(pos.y, anchor.bottom() + GAP);
        // Centred on the overlay horizontally...
        let centre = pos.x + size().x / 2.0;
        assert!((centre - anchor.center().x).abs() <= 1.0, "{centre}");
        // ...but never past the screen edge.
        assert!(pos.x + size().x <= screen().right() + 0.01);
    }

    #[test]
    fn note_flips_above_when_there_is_no_room_below() {
        let anchor = Rect::from_min_size(Pos2::new(800.0, 900.0), egui::vec2(211.0, 103.0));
        let pos = place_near(anchor, screen(), size());
        assert!(pos.y + size().y <= anchor.top(), "{pos:?}");
        assert!(pos.y >= screen().top());
    }

    #[test]
    fn note_is_nudged_back_onto_the_screen() {
        for x in [0.0_f32, 100.0, 1800.0] {
            let anchor = Rect::from_min_size(Pos2::new(x, 40.0), egui::vec2(211.0, 103.0));
            let pos = place_near(anchor, screen(), size());
            assert!(pos.x >= screen().left(), "{pos:?}");
            assert!(pos.x + size().x <= screen().right() + 0.01, "{pos:?}");
        }
    }

    /// Each test drives the module state directly, so state from a previous
    /// test in the same thread must not leak in.
    fn reset() {
        STATE.with(|state| *state.borrow_mut() = State::default());
    }

    #[test]
    fn an_edit_is_handed_to_the_app_once() {
        reset();
        STATE.with(|state| {
            let mut s = state.borrow_mut();
            s.open = true;
            s.buffer = "before".to_string();
            s.reported = "before".to_string();
        });

        store("before");
        assert_eq!(take_edit(), None, "an unchanged frame reports nothing");

        store("before and after");
        assert_eq!(take_edit().as_deref(), Some("before and after"));
        assert_eq!(take_edit(), None, "the same edit is not reported twice");

        store("before and after");
        assert_eq!(take_edit(), None, "typing then deleting back reports once");
    }

    #[test]
    fn settings_text_replaces_the_buffer_and_is_not_reported_back() {
        reset();
        STATE.with(|state| {
            let mut s = state.borrow_mut();
            s.open = true;
            s.buffer = "old".to_string();
            s.reported = "old".to_string();
        });

        set_text("cleared by the settings dialog");
        let buffer = STATE.with(|state| state.borrow().buffer.clone());
        assert_eq!(buffer, "cleared by the settings dialog");
        assert_eq!(take_edit(), None, "a change from Settings is not an edit");
    }

    #[test]
    fn closing_keeps_the_last_edit_for_the_app() {
        reset();
        STATE.with(|state| {
            let mut s = state.borrow_mut();
            s.open = true;
            s.pending = Some("typed just before closing".to_string());
        });

        close();
        assert!(!is_open());
        assert_eq!(
            take_edit().as_deref(),
            Some("typed just before closing"),
            "closing must not swallow the last keystroke"
        );

        // A dismissal stamp means the click that closed the note is ignored.
        STATE.with(|state| state.borrow_mut().dismissed_at = Some(Instant::now()));
        assert!(just_dismissed());
    }

    #[test]
    fn text_colour_follows_the_timer_until_overridden() {
        let mut config = Config {
            normal_color: crate::config::Rgb(0x11, 0x22, 0x33),
            ..Default::default()
        };
        // No explicit pick: the note inherits the timer's numbers.
        assert_eq!(text_color(&config), frame_color(&config));

        // An explicit pick is used for the text only, never the frame.
        config.note_text_color = Some(crate::config::Rgb(0xFF, 0xEE, 0xDD));
        assert_eq!(text_color(&config), Color32::from_rgb(0xFF, 0xEE, 0xDD));
        assert_eq!(frame_color(&config), Color32::from_rgb(0x11, 0x22, 0x33));
    }

    #[test]
    fn a_smaller_note_is_clamped_to_the_minimum() {
        let config = Config {
            note_width: 10,
            note_height: 5,
            ..Default::default()
        };
        let size = note_size(&config);
        assert_eq!(size.x, NOTE_MIN_SIDE as f32);
        assert_eq!(size.y, NOTE_MIN_SIDE as f32);
    }

    #[test]
    fn the_frame_is_the_timer_colour_and_the_paper_is_its_panel() {
        let config = Config {
            normal_color: crate::config::Rgb(0x00, 0xFF, 0x66),
            ..Default::default()
        };
        // The frame is exactly the colour the numbers are drawn in.
        assert_eq!(
            frame_color(&config),
            crate::render::target_color(&config, crate::engine::ColorRole::Normal)
        );
        // The paper is the timer's own panel colour, faded by the note's
        // opacity. Only the paper carries that alpha, so the writing above it
        // stays fully opaque.
        let panel = crate::render::panel_color(&config, crate::render::background_alpha(&config));
        let paper = paper_color(&config);
        assert_eq!(
            (paper.r(), paper.g(), paper.b()),
            (panel.r(), panel.g(), panel.b())
        );
        assert_eq!(
            paper.a(),
            panel.a(),
            "following the timer copies its panel alpha exactly"
        );
    }

    #[test]
    fn the_note_background_matches_the_timer_until_overridden() {
        let mut config = Config::default();
        assert_eq!(config.note_bg_opacity, None, "default is same as timer");
        assert_eq!(note_alpha(&config), crate::render::background_alpha(&config));
        assert_eq!(timer_note_opacity(&config), 140.0 / 255.0);

        // A slider value on the timer's background moves the note with it.
        config.background_opacity = Some(0.4);
        assert_eq!(note_alpha(&config), 102);

        // A theme that paints its own panel alpha is followed exactly, rather
        // than multiplied by the background slider on top of it.
        config.theme = crate::config::Theme::Chalkboard;
        assert_eq!(note_alpha(&config), 255, "the chalkboard is solid");

        // An explicit note value wins and is clamped.
        config.note_bg_opacity = Some(0.5);
        assert_eq!(note_alpha(&config), 128);
        config.note_bg_opacity = Some(9.0);
        assert_eq!(note_alpha(&config), 255, "out of range is clamped");
        config.note_bg_opacity = Some(-3.0);
        assert_eq!(note_alpha(&config), 0, "out of range is clamped");

        // Only the paper fades; the text colour is never touched.
        config.note_bg_opacity = Some(0.2);
        assert_eq!(text_color(&config).a(), 255);
        assert_eq!(paper_color(&config).a(), 51);
    }

    #[test]
    fn the_scale_slider_default_shows_the_timers_own_background() {
        let config = Config {
            background_opacity: Some(0.7),
            opacity: 0.3,
            ..Default::default()
        };
        // The note follows the background, never the whole-window fade.
        assert!((timer_note_opacity(&config) - 0.7).abs() < 0.01);
        assert!((note_opacity(&config) - 0.7).abs() < 0.01);
    }

    #[test]
    fn the_border_can_be_switched_off() {
        let mut config = Config::default();
        assert!(config.note_border, "the border is on by default");
        assert_eq!(frame_stroke(&config).width, 2.0);
        assert_eq!(frame_stroke(&config).color, frame_color(&config));

        config.note_border = false;
        assert_eq!(frame_stroke(&config).width, 0.0);
        assert_eq!(frame_stroke(&config).color, Color32::TRANSPARENT);
    }

    #[test]
    fn the_note_corners_follow_the_timer_until_overridden() {
        let mut config = Config::default();
        let window = Rect::from_min_size(Pos2::ZERO, egui::vec2(280.0, 200.0));

        // Following the timer means using the shape and radius it uses.
        config.window_shape = crate::config::WindowShape::Rounded;
        config.corner_radius = 18;
        assert_eq!(note_radius(&config, window), 18.0);
        config.window_shape = crate::config::WindowShape::Rectangular;
        assert_eq!(note_radius(&config, window), 0.0);

        // An explicit radius wins, and never exceeds half the shorter side.
        config.note_corner_radius = Some(24);
        assert_eq!(note_radius(&config, window), 24.0);
        config.note_corner_radius = Some(10_000);
        assert_eq!(note_radius(&config, window), 100.0, "capped at half the height");
    }

    #[test]
    fn a_stored_opacity_and_radius_are_sanitised() {
        let config = Config {
            background_opacity: Some(4.0),
            note_corner_radius: Some(9_999),
            note_width: 200,
            note_height: 300,
            ..Default::default()
        }
        .validated();
        assert_eq!(config.background_opacity, Some(1.0));
        assert_eq!(config.note_corner_radius, Some(100));
    }

    #[test]
    fn a_moved_timer_takes_the_note_with_it() {
        let before = Rect::from_min_size(Pos2::new(800.0, 40.0), egui::vec2(211.0, 103.0));
        let after = Rect::from_min_size(Pos2::new(300.0, 500.0), egui::vec2(211.0, 103.0));
        let first = place_near(before, screen(), size());
        let moved = place_near(after, screen(), size());

        // The note keeps the same offset from the timer, so it tracks the drag.
        assert_eq!(
            moved.x - after.center().x,
            first.x - before.center().x,
            "horizontal offset is kept"
        );
        assert!((moved.y - after.bottom()).abs() - (first.y - before.bottom()).abs() < 0.01);
    }

    #[test]
    fn a_resized_note_stays_centred_under_the_timer() {
        let anchor = Rect::from_min_size(Pos2::new(600.0, 100.0), egui::vec2(211.0, 103.0));
        for width in [120.0_f32, 280.0, 420.0, 900.0] {
            let resized = egui::vec2(width, 200.0);
            let pos = place_near(anchor, screen(), resized);
            let centre = pos.x + resized.x / 2.0;
            assert!(
                (centre - anchor.center().x).abs() <= 1.0,
                "width {width}: centre {centre} vs {}",
                anchor.center().x
            );
            assert!(pos.x >= screen().left());
            assert!(pos.x + resized.x <= screen().right() + 0.01);
        }
    }

    #[test]
    fn the_caret_uses_the_ink_and_the_selection_uses_the_frame() {
        let ink = Color32::from_rgb(0xFF, 0xCC, 0x00);
        let frame = Color32::from_rgb(0x00, 0xFF, 0x66);
        let visuals = note_visuals(ink, frame);

        assert_eq!(visuals.override_text_color, Some(ink));
        assert_eq!(visuals.text_cursor.stroke.color, ink, "caret follows the text");
        assert_eq!(visuals.selection.stroke.color, frame, "highlight is the frame");
        // The highlight is translucent, so selected text stays readable.
        assert_eq!(visuals.selection.bg_fill.r(), frame.r());
        assert!(visuals.selection.bg_fill.a() < 255);
        // Nothing else paints a background over the paper.
        assert_eq!(visuals.panel_fill, Color32::TRANSPARENT);
        assert_eq!(visuals.extreme_bg_color, Color32::TRANSPARENT);
    }

    #[test]
    fn a_dragged_size_is_reported_once() {
        reset();
        STATE.with(|state| {
            let mut s = state.borrow_mut();
            s.open = true;
            s.reported_size = Some((400, 300));
        });
        assert_eq!(take_size(), Some((400, 300)));
        assert_eq!(take_size(), None);
    }
}
