//! First-launch tip telling the user where the settings live.
//!
//! The overlay has no title bar and no shared header; on a new install there is
//! nothing on screen that suggests right-clicking. One dismissal is enough, so
//! the flag lives in the config and the banner never returns.

use crate::config::Config;
use crate::render::ui_palette_for_config;
use egui::{Align, Color32, Layout, Pos2, Rect, Rounding, Stroke, Vec2};

const MARGIN: f32 = 10.0;
const CARD_HEIGHT: f32 = 62.0;

/// Where the tip is placed inside the overlay.
pub fn card_rect(rect: Rect) -> Rect {
    let width = (rect.width() - MARGIN * 2.0).clamp(120.0, 320.0);
    Rect::from_min_size(
        Pos2::new(
            rect.center().x - width / 2.0,
            rect.bottom() - CARD_HEIGHT - MARGIN,
        ),
        Vec2::new(width, CARD_HEIGHT),
    )
    .intersect(rect)
}

/// Whether the overlay is big enough to show a readable tip.
pub fn fits(rect: Rect) -> bool {
    let card = card_rect(rect);
    card.width() >= 120.0 && card.height() >= 44.0
}

/// Draw the tip and register a click on it. Returns `true` once the user has
/// acknowledged it.
pub fn show(ui: &mut egui::Ui, rect: Rect, config: &Config, alpha: f32) -> bool {
    let palette = ui_palette_for_config(config);
    let card = card_rect(rect);
    if !fits(rect) {
        return false;
    }
    let response = ui.interact(card, ui.id().with("first-run-hint"), egui::Sense::click());
    let hovered = response.hovered();

    // The tip sits over the timer, so it is always a dark card with a readable
    // accent — independent of the (possibly bright) overlay theme.
    let bg = Color32::from_rgba_unmultiplied(18, 18, 18, (238.0 * alpha).round() as u8);
    let accent = blend_accent(palette.accent, alpha);
    let title_color = Color32::from_rgba_unmultiplied(238, 238, 238, (255.0 * alpha) as u8);
    let sub_color = Color32::from_rgba_unmultiplied(170, 170, 170, (255.0 * alpha) as u8);
    let border = if hovered {
        accent
    } else {
        Color32::from_rgba_unmultiplied(90, 90, 90, (255.0 * alpha) as u8)
    };

    let painter = ui.painter();
    painter.rect_filled(card, Rounding::same(10.0), bg);
    painter.rect_stroke(card, Rounding::same(10.0), Stroke::new(1.5_f32, border));

    draw_mouse(painter, Pos2::new(card.left() + 22.0, card.center().y), accent);

    // Wrapped to the card so the text can never spill past its edge.
    let text_rect = Rect::from_min_max(
        Pos2::new(card.left() + 40.0, card.top() + 6.0),
        Pos2::new(card.right() - 10.0, card.bottom() - 6.0),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::top_down(Align::LEFT)),
    );
    child.add_space(2.0);
    child.label(
        egui::RichText::new("Right-click the timer for settings")
            .color(title_color)
            .size(13.0)
            .strong(),
    );
    child.label(
        egui::RichText::new("Drag to move \u{00B7} double-click to start or pause")
            .color(sub_color)
            .size(11.0),
    );

    if hovered && ui.ctx().input(|i| i.pointer.any_click()) {
        return true;
    }
    response.clicked()
}

/// Scale the theme accent so the card matches the overlay's current opacity.
fn blend_accent(accent: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        accent.r(),
        accent.g(),
        accent.b(),
        (255.0 * alpha.clamp(0.0, 1.0)).round() as u8,
    )
}

/// A 13x18 mouse outline with its right-hand button lit.
fn draw_mouse(painter: &egui::Painter, center: Pos2, color: Color32) {
    let body = Rect::from_center_size(center, Vec2::new(13.0, 18.0));
    painter.rect_stroke(body, Rounding::same(6.0), Stroke::new(1.4_f32, color));
    painter.line_segment(
        [
            Pos2::new(center.x, body.top() + 0.5),
            Pos2::new(center.x, center.y + 1.0),
        ],
        Stroke::new(1.2_f32, color),
    );
    let lit = Rect::from_min_max(
        Pos2::new(center.x + 1.0, body.top() + 1.2),
        Pos2::new(body.right() - 2.2, body.top() + 8.0),
    );
    painter.rect_filled(lit, Rounding::same(3.0), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlay(width: f32, height: f32) -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(width, height))
    }

    #[test]
    fn card_stays_inside_the_overlay() {
        for (w, h) in [(300.0, 120.0), (211.0, 103.0), (600.0, 300.0)] {
            let rect = overlay(w, h);
            let card = card_rect(rect);
            assert!(card.left() >= rect.left() - 0.01, "{w}x{h}: {card:?}");
            assert!(card.right() <= rect.right() + 0.01, "{w}x{h}: {card:?}");
            assert!(card.bottom() <= rect.bottom() + 0.01, "{w}x{h}: {card:?}");
            assert!(fits(rect));
        }
    }

    #[test]
    fn tiny_overlay_has_no_room_for_the_tip() {
        // Mini mode shrinks the overlay; a card would cover the whole timer.
        assert!(!fits(overlay(90.0, 40.0)));
        assert!(!fits(overlay(120.0, 44.0)));
    }
}
