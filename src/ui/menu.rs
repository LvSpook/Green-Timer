//! The overlay's right-click menu.
//!
//! `Response::context_menu` draws inside the viewport it belongs to, so in the
//! overlay — a window only a few dozen pixels wide — the menu was clipped to
//! that rectangle. The menu is therefore a *deferred* egui viewport: a real
//! child window that eframe drives through its normal event loop and that sizes
//! itself to its contents.

use crate::config::{Config, ContextMenuConfig};
use crate::render::{ui_palette, ui_palette_for_config, UiPalette};
use egui::{Color32, Pos2, Rect, Rounding, Stroke, Vec2};
use std::cell::RefCell;
use std::time::Instant;

/// What the user selected from the right-click menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    StartPause,
    Reset,
    Lap,
    AddMinute,
    SubMinute,
    CopyTime,
    MiniMode,
    GhostMode,
    LoadProfile(String),
    Settings,
    Exit,
}

#[derive(Clone, Copy)]
enum MenuIcon {
    StartPause,
    Reset,
    Lap,
    Plus,
    Minus,
    Copy,
    MiniMode,
    Ghost,
    Profile,
    Settings,
    Exit,
}

#[derive(Clone)]
enum MenuRow {
    Separator,
    Item {
        icon: MenuIcon,
        label: String,
        checked: bool,
        danger: bool,
        action: MenuAction,
    },
}

/// Build the list of rows to display based on the user's menu customization.
fn build_rows(cfg: &ContextMenuConfig, config: &Config) -> Vec<MenuRow> {
    let mut rows = Vec::new();
    let push_sep = |rows: &mut Vec<MenuRow>| {
        if !rows.is_empty() && !matches!(rows.last(), Some(MenuRow::Separator)) {
            rows.push(MenuRow::Separator);
        }
    };

    if cfg.start_pause {
        rows.push(MenuRow::Item {
            icon: MenuIcon::StartPause,
            label: "Start / Pause".to_string(),
            checked: false,
            danger: false,
            action: MenuAction::StartPause,
        });
    }
    if cfg.reset {
        rows.push(MenuRow::Item {
            icon: MenuIcon::Reset,
            label: "Reset".to_string(),
            checked: false,
            danger: false,
            action: MenuAction::Reset,
        });
    }
    if cfg.lap {
        rows.push(MenuRow::Item {
            icon: MenuIcon::Lap,
            label: "Lap".to_string(),
            checked: false,
            danger: false,
            action: MenuAction::Lap,
        });
    }
    if cfg.quick_adjust {
        rows.push(MenuRow::Item {
            icon: MenuIcon::Plus,
            label: "+1 min".to_string(),
            checked: false,
            danger: false,
            action: MenuAction::AddMinute,
        });
        rows.push(MenuRow::Item {
            icon: MenuIcon::Minus,
            label: "-1 min".to_string(),
            checked: false,
            danger: false,
            action: MenuAction::SubMinute,
        });
    }
    if cfg.copy_time {
        rows.push(MenuRow::Item {
            icon: MenuIcon::Copy,
            label: "Copy Time".to_string(),
            checked: false,
            danger: false,
            action: MenuAction::CopyTime,
        });
    }

    if cfg.mini_mode || cfg.ghost_mode {
        push_sep(&mut rows);
        if cfg.mini_mode {
            rows.push(MenuRow::Item {
                icon: MenuIcon::MiniMode,
                label: "Mini Mode".to_string(),
                checked: config.mini_mode,
                danger: false,
                action: MenuAction::MiniMode,
            });
        }
        if cfg.ghost_mode {
            rows.push(MenuRow::Item {
                icon: MenuIcon::Ghost,
                label: "Ghost Mode".to_string(),
                checked: config.ghost_mode,
                danger: false,
                action: MenuAction::GhostMode,
            });
        }
    }

    if cfg.profiles && !config.profiles.is_empty() {
        push_sep(&mut rows);
        for profile in &config.profiles {
            rows.push(MenuRow::Item {
                icon: MenuIcon::Profile,
                label: profile.name.clone(),
                checked: false,
                danger: false,
                action: MenuAction::LoadProfile(profile.name.clone()),
            });
        }
    }

    if cfg.settings || cfg.exit {
        push_sep(&mut rows);
        if cfg.settings {
            rows.push(MenuRow::Item {
                icon: MenuIcon::Settings,
                label: "Settings\u{2026}".to_string(),
                checked: false,
                danger: false,
                action: MenuAction::Settings,
            });
        }
        if cfg.exit {
            rows.push(MenuRow::Item {
                icon: MenuIcon::Exit,
                label: "Exit".to_string(),
                checked: false,
                danger: true,
                action: MenuAction::Exit,
            });
        }
    }

    // Always keep Settings reachable even if the user unchecked everything.
    if rows.is_empty() {
        rows.push(MenuRow::Item {
            icon: MenuIcon::Settings,
            label: "Settings\u{2026}".to_string(),
            checked: false,
            danger: false,
            action: MenuAction::Settings,
        });
    }
    rows
}

/// Draw a crisp 12x12 vector icon centred at `c` (never relies on font glyphs).
fn draw_icon(painter: &egui::Painter, c: Pos2, icon: MenuIcon, color: Color32) {
    let stroke = Stroke::new(1.5_f32, color);
    match icon {
        MenuIcon::StartPause => {
            let pts = vec![
                c + Vec2::new(-3.5, -4.5),
                c + Vec2::new(4.5, 0.0),
                c + Vec2::new(-3.5, 4.5),
            ];
            painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        MenuIcon::Reset => {
            let r = 4.5_f32;
            let steps = 12;
            let mut pts = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let a = -0.4 + (i as f32 / steps as f32) * 4.9;
                pts.push(c + Vec2::new(a.cos() * r, a.sin() * r));
            }
            painter.add(egui::Shape::line(pts, stroke));
            // Arrowhead at the start of the arc.
            let tip = c + Vec2::new((-0.4_f32).cos() * r, (-0.4_f32).sin() * r);
            painter.line_segment([tip, tip + Vec2::new(-3.0, -0.5)], stroke);
            painter.line_segment([tip, tip + Vec2::new(0.5, -3.0)], stroke);
        }
        MenuIcon::Lap => {
            painter.line_segment(
                [c + Vec2::new(-3.5, -5.0), c + Vec2::new(-3.5, 5.0)],
                stroke,
            );
            let flag = vec![
                c + Vec2::new(-3.5, -4.5),
                c + Vec2::new(4.5, -2.0),
                c + Vec2::new(-3.5, 0.5),
            ];
            painter.add(egui::Shape::convex_polygon(flag, color, Stroke::NONE));
        }
        MenuIcon::Plus => {
            painter.line_segment([c + Vec2::new(-4.0, 0.0), c + Vec2::new(4.0, 0.0)], stroke);
            painter.line_segment([c + Vec2::new(0.0, -4.0), c + Vec2::new(0.0, 4.0)], stroke);
        }
        MenuIcon::Minus => {
            painter.line_segment([c + Vec2::new(-4.0, 0.0), c + Vec2::new(4.0, 0.0)], stroke);
        }
        MenuIcon::Copy => {
            let back = Rect::from_center_size(c + Vec2::new(-1.5, -1.5), Vec2::new(6.5, 7.5));
            let front = Rect::from_center_size(c + Vec2::new(1.5, 1.5), Vec2::new(6.5, 7.5));
            painter.rect_stroke(back, Rounding::same(1.5), stroke);
            painter.rect_stroke(front, Rounding::same(1.5), stroke);
        }
        MenuIcon::MiniMode => {
            let outer = Rect::from_center_size(c, Vec2::new(10.0, 8.0));
            painter.rect_stroke(outer, Rounding::same(1.5), stroke);
            let corner = Rect::from_min_max(
                outer.center(),
                outer.max - Vec2::new(1.5, 1.5),
            );
            painter.rect_filled(corner, Rounding::same(1.0), color);
        }
        MenuIcon::Ghost => {
            painter.circle_stroke(c, 4.5, stroke);
            painter.circle_filled(c + Vec2::new(-1.6, -0.8), 1.0, color);
            painter.circle_filled(c + Vec2::new(1.6, -0.8), 1.0, color);
        }
        MenuIcon::Profile => {
            let pts = vec![
                c + Vec2::new(0.0, -5.0),
                c + Vec2::new(4.5, 0.0),
                c + Vec2::new(0.0, 5.0),
                c + Vec2::new(-4.5, 0.0),
            ];
            painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        MenuIcon::Settings => {
            for (dy, knob_x) in [(-3.5_f32, -1.5_f32), (0.0, 2.0), (3.5, -1.0)] {
                painter.line_segment(
                    [c + Vec2::new(-5.0, dy), c + Vec2::new(5.0, dy)],
                    Stroke::new(1.2_f32, color),
                );
                painter.circle_filled(c + Vec2::new(knob_x, dy), 1.7, color);
            }
        }
        MenuIcon::Exit => {
            painter.line_segment(
                [c + Vec2::new(-3.8, -3.8), c + Vec2::new(3.8, 3.8)],
                stroke,
            );
            painter.line_segment(
                [c + Vec2::new(3.8, -3.8), c + Vec2::new(-3.8, 3.8)],
                stroke,
            );
        }
    }
}

fn draw_checkmark(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.8_f32, color);
    painter.line_segment([c + Vec2::new(-4.0, 0.0), c + Vec2::new(-1.0, 3.0)], stroke);
    painter.line_segment([c + Vec2::new(-1.0, 3.0), c + Vec2::new(4.5, -3.0)], stroke);
}

/// Identity of the menu's window for a given open generation.
fn viewport_id(generation: u64) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("green-timer-context-menu", generation))
}

struct State {
    generation: u64,
    position: Option<Pos2>,
    result: Option<Option<MenuAction>>,
    applied_size: Option<[u32; 2]>,
    rows: Vec<MenuRow>,
    palette: UiPalette,
    seen_focus: bool,
    opened_at: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        State {
            generation: 0,
            position: None,
            result: None,
            applied_size: None,
            rows: Vec::new(),
            palette: ui_palette(crate::config::Theme::Default),
            seen_focus: false,
            opened_at: None,
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Open the menu at `position` (monitor points).
pub fn open(position: Pos2) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.generation = state.generation.wrapping_add(1);
        state.position = Some(position);
        state.result = None;
        state.applied_size = None;
        state.seen_focus = false;
        state.opened_at = Some(Instant::now());
    });
}

/// Whether the menu is currently showing.
pub fn is_open() -> bool {
    STATE.with(|state| state.borrow().position.is_some())
}

/// Draw the menu window if it is open. Must be called every frame.
pub fn show(ctx: &egui::Context, config: &Config) {
    let Some((position, generation)) = STATE.with(|state| {
        let state = state.borrow();
        state.position.map(|p| (p, state.generation))
    }) else {
        return;
    };

    // Clicking back on the overlay itself dismisses the menu.
    if ctx.input(|i| i.pointer.primary_pressed()) {
        finish(None);
        return;
    }

    let rows = build_rows(&config.context_menu, config);
    let palette = ui_palette_for_config(config);
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.rows = rows;
        s.palette = palette;
    });

    ctx.request_repaint();

    ctx.show_viewport_deferred(
        viewport_id(generation),
        egui::ViewportBuilder::default()
            .with_title("Green Timer Menu")
            .with_decorations(false)
            .with_transparent(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_position(position)
            .with_inner_size([210.0, 240.0]),
        move |ctx, _class| draw(ctx),
    );
}

/// Take the pending choice: `Some(action)`, or `Some(None)` when dismissed.
pub fn take_choice() -> Option<Option<MenuAction>> {
    STATE.with(|state| state.borrow_mut().result.take())
}

fn finish(result: Option<MenuAction>) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.position = None;
        state.result = Some(result);
    });
}

fn draw(ctx: &egui::Context) {
    if !is_open() {
        return;
    }

    let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
    let lost_focus = STATE.with(|state| {
        let mut s = state.borrow_mut();
        if focused {
            s.seen_focus = true;
            false
        } else if s.seen_focus {
            true
        } else {
            s.opened_at
                .map(|t| t.elapsed().as_millis() > 250)
                .unwrap_or(false)
        }
    });
    if lost_focus {
        finish(None);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        return;
    }

    let (rows, palette) = STATE.with(|state| {
        let s = state.borrow();
        (s.rows.clone(), s.palette)
    });
    let danger_color = Color32::from_rgb(235, 92, 92);
    let font = egui::FontId::proportional(13.5);
    let row_width = 200.0;

    let mut clicked: Option<MenuAction> = None;
    let mut content = Rect::NOTHING;

    egui::Area::new(egui::Id::new("green-timer-context-menu-body"))
        .fixed_pos(Pos2::ZERO)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(palette.window_bg)
                .inner_margin(egui::Margin::symmetric(6.0, 6.0))
                .stroke(Stroke::new(1.5_f32, palette.border))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for row in &rows {
                        match row {
                            MenuRow::Separator => {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(row_width, 9.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().hline(
                                    (rect.left() + 8.0)..=(rect.right() - 8.0),
                                    rect.center().y,
                                    Stroke::new(1.0_f32, palette.border),
                                );
                            }
                            MenuRow::Item {
                                icon,
                                label,
                                checked,
                                danger,
                                action,
                            } => {
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(row_width, 28.0),
                                    egui::Sense::click(),
                                );
                                let hovered = response.hovered();
                                if hovered {
                                    ui.painter().rect_filled(
                                        rect,
                                        Rounding::same(6.0),
                                        palette.hover_bg,
                                    );
                                    let bar = Rect::from_min_size(
                                        rect.left_top() + Vec2::new(2.0, 5.0),
                                        Vec2::new(3.0, rect.height() - 10.0),
                                    );
                                    ui.painter().rect_filled(
                                        bar,
                                        Rounding::same(1.5),
                                        if *danger { danger_color } else { palette.accent },
                                    );
                                }

                                let fg = if *danger && hovered {
                                    danger_color
                                } else {
                                    palette.text
                                };
                                let icon_fg = if *checked {
                                    palette.accent
                                } else if *danger {
                                    danger_color
                                } else {
                                    palette.muted_text
                                };

                                draw_icon(
                                    ui.painter(),
                                    rect.left_center() + Vec2::new(17.0, 0.0),
                                    *icon,
                                    icon_fg,
                                );
                                ui.painter().text(
                                    rect.left_center() + Vec2::new(32.0, 0.0),
                                    egui::Align2::LEFT_CENTER,
                                    label,
                                    font.clone(),
                                    fg,
                                );

                                if *checked {
                                    draw_checkmark(
                                        ui.painter(),
                                        rect.right_center() - Vec2::new(13.0, 0.0),
                                        palette.accent,
                                    );
                                }

                                if hovered && ui.input(|i| i.pointer.any_pressed()) {
                                    clicked = Some(action.clone());
                                }
                            }
                        }
                    }
                });
            content = ui.min_rect();
        });

    let desired = content.size().max(egui::vec2(160.0, 40.0));
    let pixels = [
        (desired.x * 100.0).round() as u32,
        (desired.y * 100.0).round() as u32,
    ];
    let size_changed = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.applied_size == Some(pixels) {
            false
        } else {
            state.applied_size = Some(pixels);
            true
        }
    });
    if size_changed {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(desired));
    }

    if let Some(action) = clicked {
        finish(Some(action));
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        return;
    }

    let escaped = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if escaped {
        finish(None);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        return;
    }

    ctx.request_repaint();
}
