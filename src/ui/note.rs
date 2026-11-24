//! The timer's single note, in a small square window beneath the overlay.
//!
//! Like the right-click menu this is a *deferred* egui viewport: a real OS
//! window that eframe sizes to its contents. An in-viewport widget would be
//! clipped to the overlay, which is only a few dozen pixels tall.
//!
//! The text is stored in `Config::note`, so it is saved with every other
//! setting and survives a restart. Edits are handed back as they are typed, so
//! nothing is lost if the app is killed mid-sentence.

use crate::config::{Config, NOTE_MAX_CHARS};
use crate::render::{apply_theme_visuals_for_config, ui_palette_for_config};
use egui::{Color32, Pos2, Rect, Stroke};
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// Side of the note window, in points.
const SIDE: f32 = 280.0;
/// Gap between the overlay and the note window.
const GAP: f32 = 10.0;

#[derive(Default)]
struct State {
    open: bool,
    /// Bumped on every open so each showing gets a fresh viewport.
    generation: u64,
    /// Where the window was placed when it opened.
    position: Option<Pos2>,
    /// The text being edited. Mirrors `Config::note`, plus any unsaved edits.
    buffer: String,
    /// Text last handed to the app, so each edit is saved only once.
    reported: String,
    /// Edits waiting for the app to store.
    pending: Option<String>,
    /// Focus has been seen once, so a later loss can be trusted.
    seen_focus: bool,
    /// Viewport currently on screen, so it can be closed from the overlay.
    shown: Option<u64>,
    /// When the window dismissed itself (focus loss), so a click on the timer
    /// that caused it does not immediately reopen the note.
    dismissed_at: Option<Instant>,
    /// The editor still needs the keyboard handed to it.
    autofocus: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn viewport_id(generation: u64) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("green-timer-note", generation))
}

/// Show the note window beneath `anchor` (the overlay's monitor rect).
pub fn open(anchor: Rect, text: &str) {
    let (left, top, width, height) = crate::platform::primary_work_area(1.0);
    let screen = Rect::from_min_size(Pos2::new(left, top), egui::vec2(width, height));
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.generation = state.generation.wrapping_add(1);
        state.open = true;
        state.position = Some(place_beneath(anchor, screen));
        state.buffer = text.to_string();
        state.reported = text.to_string();
        state.pending = None;
        state.seen_focus = false;
        state.autofocus = true;
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
            .is_some_and(|at| at.elapsed() < Duration::from_millis(400))
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

/// Draw the note window if it is open. Must be called every frame.
pub fn show(ctx: &egui::Context, config: &Config) {
    let Some((generation, position)) = STATE.with(|state| {
        let state = state.borrow();
        state.open.then(|| (state.generation, state.position))
    }) else {
        // Closing hides the window from `close()`; make sure the viewport is
        // told too, or the empty window would linger on screen.
        let stale = STATE.with(|state| state.borrow_mut().shown.take());
        if let Some(generation) = stale {
            ctx.send_viewport_cmd_to(viewport_id(generation), egui::ViewportCommand::Close);
        }
        return;
    };
    let Some(position) = position else {
        return;
    };

    STATE.with(|state| state.borrow_mut().shown = Some(generation));

    // A press on the overlay dismisses the note, like the right-click menu.
    // The note window's own focus is unreliable: the overlay does not take
    // focus when clicked, so focus-loss alone never fires. Root input is also
    // the only path available while the overlay is the active window.
    if ctx.input(|i| i.pointer.primary_pressed()) {
        dismiss(ctx);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        return;
    }

    let config = config.clone();
    ctx.show_viewport_deferred(
        viewport_id(generation),
        egui::ViewportBuilder::default()
            .with_title("Green Timer Note")
            .with_icon(std::sync::Arc::new(crate::render::themed_window_icon(
                &config,
            )))
            .with_decorations(false)
            .with_transparent(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_position(position)
            .with_inner_size([SIDE, SIDE])
            // Take the keyboard at once: a bound letter should type, not run
            // its shortcut, and an unfocused window would dismiss itself.
            .with_active(true),
        move |ctx, _class| draw(ctx, &config),
    );
}

/// Put the window under the overlay, kept on the screen it is on.
fn place_beneath(anchor: Rect, screen: Rect) -> Pos2 {
    let below = anchor.bottom() + GAP;
    let y = if below + SIDE <= screen.bottom() {
        below
    } else {
        // No room underneath, so sit above the overlay instead.
        (anchor.top() - GAP - SIDE).max(screen.top())
    };
    let x = (anchor.center().x - SIDE / 2.0)
        .clamp(screen.left(), (screen.right() - SIDE).max(screen.left()));
    Pos2::new(x.round(), y.round())
}

fn draw(ctx: &egui::Context, config: &Config) {
    if !is_open() {
        return;
    }

    // Dismiss on a click elsewhere. Only a focus this window actually had and
    // then lost counts, so a window that opens unfocused is not closed.
    if dismissed(ctx) {
        return;
    }

    apply_theme_visuals_for_config(ctx, config);
    let palette = ui_palette_for_config(config);

    // Escape closes the window, like the menu. It arrives here only while this
    // window has focus, so the overlay's own Escape handling never sees it.
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        dismiss(ctx);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        return;
    }

    let mut text = STATE.with(|state| state.borrow().buffer.clone());

    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(palette.window_bg)
                .inner_margin(egui::Margin::same(8.0))
                .stroke(Stroke::new(1.5_f32, palette.border)),
        )
        .show(ctx, |ui| {
            if !header(ui, ctx, palette) {
                return;
            }

            ui.add_space(4.0);
            let response = ui.add(
                egui::TextEdit::multiline(&mut text)
                    .desired_width(f32::INFINITY)
                    .desired_rows(9)
                    .hint_text("Type a note\u{2026}")
                    .char_limit(NOTE_MAX_CHARS),
            );
            if STATE.with(|state| state.borrow_mut().autofocus) {
                STATE.with(|state| state.borrow_mut().autofocus = false);
                response.request_focus();
            }

            // A bound letter key must type a letter, not run its shortcut.
            if response.has_focus() {
                crate::hotkeys::set_suspended(true);
            } else if response.lost_focus() {
                crate::hotkeys::set_suspended(false);
            }

            store(&text);

            let used = text.chars().count();
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{used} / {NOTE_MAX_CHARS}"))
                        .size(10.0)
                        .color(palette.muted_text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("Saved automatically")
                            .size(10.0)
                            .color(Color32::from_gray(120)),
                    );
                });
            });
        });
}

/// Close the window when it loses focus or its Close button is clicked.
/// Returns `true` once the window has been dismissed.
fn dismissed(ctx: &egui::Context) -> bool {
    let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
    // Only a focus that was actually gained and then lost dismisses the
    // window. A window that never got focus (another app kept it) stays put,
    // which is safer than closing under the user's hands.
    let lost = STATE.with(|state| {
        let mut s = state.borrow_mut();
        if focused {
            s.seen_focus = true;
            false
        } else {
            s.seen_focus
        }
    });

    if lost || ctx.input(|i| i.viewport().close_requested()) {
        dismiss(ctx);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        return true;
    }
    false
}

/// Tear the window down, remembering that it was this frame's click that did
/// it so the overlay does not reopen it straight away.
fn dismiss(ctx: &egui::Context) {
    crate::hotkeys::set_suspended(false);
    let shown = STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.dismissed_at = Some(Instant::now());
        s.shown.take()
    });
    close();
    // `dismiss` can run in either viewport: from the note itself, or from the
    // overlay (a click there). Target the note's viewport explicitly so the
    // close always lands on the right window.
    if let Some(generation) = shown {
        ctx.send_viewport_cmd_to(viewport_id(generation), egui::ViewportCommand::Close);
    }
}

/// The slim title strip. It doubles as the drag handle, since the window has
/// no decorations. Returns `false` when the window was just closed.
fn header(ui: &mut egui::Ui, ctx: &egui::Context, palette: crate::render::UiPalette) -> bool {
    let bar = ui.allocate_response(
        egui::vec2(ui.available_width(), 20.0),
        egui::Sense::click_and_drag(),
    );
    if bar.dragged() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    ui.painter().text(
        bar.rect.left_center(),
        egui::Align2::LEFT_CENTER,
        "Note",
        egui::FontId::proportional(12.0),
        palette.muted_text,
    );

    let centre = Pos2::new(bar.rect.right() - 8.0, bar.rect.center().y);
    let close_button = ui.interact(
        Rect::from_center_size(centre, egui::vec2(16.0, 16.0)),
        ui.id().with("note-close"),
        egui::Sense::click(),
    );
    let colour = if close_button.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        palette.accent
    } else {
        palette.muted_text
    };
    ui.painter().text(
        centre,
        egui::Align2::CENTER_CENTER,
        "\u{00D7}",
        egui::FontId::proportional(15.0),
        colour,
    );

    if close_button.clicked() {
        dismiss(ctx);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        return false;
    }
    true
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

    #[test]
    fn note_sits_below_the_overlay() {
        let anchor = Rect::from_min_size(Pos2::new(1500.0, 60.0), egui::vec2(211.0, 103.0));
        let pos = place_beneath(anchor, screen());
        assert_eq!(pos.y, anchor.bottom() + GAP);
        // Centred on the overlay horizontally...
        let centre = pos.x + SIDE / 2.0;
        assert!((centre - anchor.center().x).abs() <= 1.0, "{centre}");
        // ...but never past the screen edge.
        assert!(pos.x + SIDE <= screen().right() + 0.01);
    }

    #[test]
    fn note_flips_above_when_there_is_no_room_below() {
        let anchor = Rect::from_min_size(Pos2::new(800.0, 900.0), egui::vec2(211.0, 103.0));
        let pos = place_beneath(anchor, screen());
        assert!(pos.y + SIDE <= anchor.top(), "{pos:?}");
        assert!(pos.y >= screen().top());
    }

    #[test]
    fn note_is_nudged_back_onto_the_screen() {
        for x in [0.0_f32, 100.0, 1800.0] {
            let anchor = Rect::from_min_size(Pos2::new(x, 40.0), egui::vec2(211.0, 103.0));
            let pos = place_beneath(anchor, screen());
            assert!(pos.x >= screen().left(), "{pos:?}");
            assert!(pos.x + SIDE <= screen().right() + 0.01, "{pos:?}");
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
            s.seen_focus = true;
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
}
