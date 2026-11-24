//! Painting of the timer overlay.
//!
//! All drawing lives here so the window code stays focused on behaviour. The
//! module holds no window state: it takes a [`Frame`] and paints it.
//!
//! Two details are worth knowing:
//!
//! * Text is laid out with [`egui::Context::fonts`] and painted from the
//!   resulting galley's row meshes instead of with `Painter::text`. Painting the
//!   raw meshes is what makes both the outline and the rotation possible.
//! * `egui`'s `TSTransform` can only translate and scale, so rotation is applied
//!   by rotating geometry (mesh vertices and polygon points) about the window
//!   centre. With no rotation this is a straight pass-through.

use crate::config::{BorderStyle, Config, DisplayStyle, Rgb, Theme, WindowShape};
use crate::engine::{ColorRole, Snapshot};
use egui::{Color32, FontFamily, FontId, Pos2, Rect, Rounding, Stroke, Vec2};
use std::sync::Arc;

pub const CHALK_BOARD: Rgb = Rgb(46, 68, 50);
/// The faint chalk fill the original declared; actual chalk text is painted in
/// the timer colour, so this is kept only to document the original palette.
#[allow(dead_code)]
pub const CHALK_TEXT: Rgb = Rgb(240, 255, 240);

/// Decoded sprite image plus its recoloured variant.
#[derive(Default)]
pub struct Textures {
    pub color: Option<egui::TextureHandle>,
    pub gray: Option<egui::TextureHandle>,
    pub aspect: f32,
}

impl Textures {
    #[allow(dead_code)]
    pub fn is_loaded(&self) -> bool {
        self.color.is_some()
    }
}

/// Decode a sprite from disk. Failure is non-fatal: the style simply draws
/// nothing, exactly like the original app.
pub fn load_textures(ctx: &egui::Context, path: &str) -> Textures {
    if path.is_empty() {
        return Textures::default();
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::warn!("Could not read sprite {path}: {err}");
            return Textures::default();
        }
    };
    let image = match image::load_from_memory(&bytes) {
        Ok(image) => image.to_rgba8(),
        Err(err) => {
            log::warn!("Could not decode sprite {path}: {err}");
            return Textures::default();
        }
    };

    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Textures::default();
    }

    let mut gray_pixels = Vec::with_capacity((width * height) as usize);
    for pixel in image.pixels() {
        // Rec. 601 luma, matching the Qt Format_Grayscale8 conversion.
        let luma = (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32)
            .round() as u8;
        gray_pixels.push(Color32::from_rgba_unmultiplied(luma, luma, luma, pixel[3]));
    }

    let color_image = egui::ColorImage {
        size: [width as usize, height as usize],
        pixels: image
            .pixels()
            .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
            .collect(),
    };
    let gray_image = egui::ColorImage {
        size: [width as usize, height as usize],
        pixels: gray_pixels,
    };

    let color = ctx.load_texture("sprite-color", color_image, Default::default());
    let gray = ctx.load_texture("sprite-gray", gray_image, Default::default());

    Textures {
        color: Some(color),
        gray: Some(gray),
        aspect: width as f32 / height as f32,
    }
}

/// Corner radius implied by the background shape.
pub fn corner_radius(config: &Config, rect: Rect) -> f32 {
    match config.window_shape {
        WindowShape::Capsule => rect.width().min(rect.height()) / 2.0,
        WindowShape::Rounded => config.corner_radius as f32,
        WindowShape::Rectangular => 0.0,
    }
}

pub fn to_color32(rgb: Rgb, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(rgb.0, rgb.1, rgb.2, alpha)
}

/// Linear interpolation between two colours, used for the fade when the timer
/// crosses its threshold.
pub fn mix_color(start: Color32, end: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        lerp(start.r(), end.r()),
        lerp(start.g(), end.g()),
        lerp(start.b(), end.b()),
        lerp(start.a(), end.a()),
    )
}

/// Multiply a colour's alpha by `scale` (`0.0..=1.0`).
fn scale_alpha(color: Color32, scale: f32) -> Color32 {
    let a = (color.a() as f32 * scale.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a)
}

/// The colour the timer should currently use for the given role.
pub fn target_color(config: &Config, role: ColorRole) -> Color32 {
    if let Some(custom) = config.active_custom_theme_data() {
        return match role {
            ColorRole::Normal => to_color32(custom.text_color, 255),
            ColorRole::Threshold | ColorRole::Overdue => to_color32(custom.warning_color, 255),
        };
    }
    match config.theme {
        Theme::Default | Theme::Chalkboard => match role {
            ColorRole::Normal => to_color32(config.normal_color, 255),
            ColorRole::Threshold | ColorRole::Overdue => to_color32(config.gray_color, 255),
        },
        Theme::Cyberpunk => match role {
            ColorRole::Normal => Color32::from_rgb(0, 245, 255),
            ColorRole::Threshold | ColorRole::Overdue => Color32::from_rgb(255, 42, 133),
        },
        Theme::MinimalGlass => match role {
            ColorRole::Normal => Color32::from_rgb(245, 248, 255),
            ColorRole::Threshold | ColorRole::Overdue => Color32::from_rgb(255, 170, 90),
        },
        Theme::RetroLcd => match role {
            ColorRole::Normal => Color32::from_rgb(28, 42, 24),
            ColorRole::Threshold | ColorRole::Overdue => Color32::from_rgb(120, 35, 25),
        },
        Theme::WarmPaper => match role {
            ColorRole::Normal => Color32::from_rgb(48, 38, 30),
            ColorRole::Threshold | ColorRole::Overdue => Color32::from_rgb(190, 68, 42),
        },
        Theme::HighContrast => match role {
            ColorRole::Normal => Color32::from_rgb(255, 255, 0),
            ColorRole::Threshold | ColorRole::Overdue => Color32::from_rgb(255, 60, 60),
        },
    }
}

pub fn background_alpha(config: &Config) -> u8 {
    if config.window_blur {
        100
    } else {
        140
    }
}

/// Paint the translucent background panel and its optional border.
pub fn draw_background(
    painter: &egui::Painter,
    config: &Config,
    rect: Rect,
    color: Color32,
    pulse: f32,
    alpha_scale: f32,
) {
    if !config.show_background {
        return;
    }

    let mut background = if let Some(custom) = config.active_custom_theme_data() {
        to_color32(custom.background, 235)
    } else {
        match config.theme {
            Theme::Default => Color32::from_rgba_unmultiplied(0, 0, 0, background_alpha(config)),
            Theme::Chalkboard => to_color32(CHALK_BOARD, 255),
            Theme::Cyberpunk => Color32::from_rgba_unmultiplied(14, 10, 32, 215),
            Theme::MinimalGlass => Color32::from_rgba_unmultiplied(28, 34, 44, 115),
            Theme::RetroLcd => Color32::from_rgb(156, 178, 126),
            Theme::WarmPaper => Color32::from_rgb(245, 238, 224),
            Theme::HighContrast => Color32::from_rgb(0, 0, 0),
        }
    };
    if pulse > 0.0 {
        background = mix_color(
            background,
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), background.a()),
            pulse * 0.25,
        );
    }
    background = scale_alpha(background, alpha_scale);

    let thickness = config.border_thickness;
    let mut stroke = match config.border_style {
        BorderStyle::Thin => Stroke::new(thickness, to_color32(config.thin_border_color, 255)),
        BorderStyle::Neon => Stroke::new(thickness, color),
        BorderStyle::None => {
            if let Some(custom) = config.active_custom_theme_data() {
                Stroke::new(2.0_f32, to_color32(custom.border_color, 255))
            } else {
                match config.theme {
                    Theme::Chalkboard => Stroke::new(4.0_f32, Color32::from_rgb(100, 70, 40)),
                    Theme::Cyberpunk => Stroke::new(2.0_f32, Color32::from_rgb(255, 0, 170)),
                    Theme::MinimalGlass => {
                        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 70))
                    }
                    Theme::RetroLcd => Stroke::new(3.0_f32, Color32::from_rgb(56, 72, 46)),
                    Theme::WarmPaper => Stroke::new(2.0_f32, Color32::from_rgb(198, 182, 158)),
                    Theme::HighContrast => Stroke::new(2.5_f32, Color32::from_rgb(255, 255, 255)),
                    Theme::Default => Stroke::NONE,
                }
            }
        }
    };
    if pulse > 0.0 && stroke.width == 0.0 {
        stroke = Stroke::new(2.5_f32, scale_alpha(color, pulse));
    } else {
        stroke.color = scale_alpha(stroke.color, alpha_scale);
    }

    // Always inset by half the actual stroke width so theme borders never clip
    // at the window edges or distort the rounded corners.
    let paint_rect = if stroke.width > 0.0 {
        rect.shrink(stroke.width / 2.0 + 0.5)
    } else {
        rect
    };

    let max_radius = (paint_rect.width().min(paint_rect.height()) / 2.0).max(0.0);
    let radius = corner_radius(config, paint_rect).min(max_radius);
    let rounding = Rounding::same(radius);
    painter.rect(paint_rect, rounding, background, stroke);
}

/// Shared UI colour palette derived from the active [`Theme`], used by the
/// Settings window and the right-click context menu.
#[derive(Clone, Copy)]
pub struct UiPalette {
    pub dark_mode: bool,
    pub window_bg: Color32,
    pub panel_bg: Color32,
    pub widget_bg: Color32,
    pub hover_bg: Color32,
    pub accent: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted_text: Color32,
}

pub fn ui_palette(theme: Theme) -> UiPalette {
    match theme {
        Theme::Default => UiPalette {
            dark_mode: true,
            window_bg: Color32::from_rgb(27, 27, 27),
            panel_bg: Color32::from_rgb(22, 22, 22),
            widget_bg: Color32::from_rgb(42, 42, 42),
            hover_bg: Color32::from_rgb(56, 62, 56),
            accent: Color32::from_rgb(0, 210, 80),
            border: Color32::from_rgb(66, 66, 66),
            text: Color32::from_gray(232),
            muted_text: Color32::from_gray(155),
        },
        Theme::Chalkboard => UiPalette {
            dark_mode: true,
            window_bg: Color32::from_rgb(38, 56, 42),
            panel_bg: Color32::from_rgb(30, 46, 34),
            widget_bg: Color32::from_rgb(52, 74, 56),
            hover_bg: Color32::from_rgb(66, 92, 70),
            accent: Color32::from_rgb(235, 225, 160),
            border: Color32::from_rgb(108, 78, 48),
            text: Color32::from_rgb(240, 240, 232),
            muted_text: Color32::from_rgb(185, 195, 178),
        },
        Theme::Cyberpunk => UiPalette {
            dark_mode: true,
            window_bg: Color32::from_rgb(18, 12, 36),
            panel_bg: Color32::from_rgb(13, 8, 28),
            widget_bg: Color32::from_rgb(32, 22, 60),
            hover_bg: Color32::from_rgb(54, 24, 82),
            accent: Color32::from_rgb(0, 245, 255),
            border: Color32::from_rgb(255, 0, 170),
            text: Color32::from_rgb(235, 248, 255),
            muted_text: Color32::from_rgb(165, 150, 200),
        },
        Theme::MinimalGlass => UiPalette {
            dark_mode: true,
            window_bg: Color32::from_rgb(28, 34, 44),
            panel_bg: Color32::from_rgb(22, 27, 36),
            widget_bg: Color32::from_rgb(42, 50, 64),
            hover_bg: Color32::from_rgb(56, 68, 86),
            accent: Color32::from_rgb(130, 195, 255),
            border: Color32::from_rgb(76, 88, 108),
            text: Color32::from_rgb(240, 245, 252),
            muted_text: Color32::from_rgb(165, 178, 198),
        },
        Theme::RetroLcd => UiPalette {
            dark_mode: false,
            window_bg: Color32::from_rgb(164, 184, 134),
            panel_bg: Color32::from_rgb(148, 168, 120),
            widget_bg: Color32::from_rgb(134, 154, 108),
            hover_bg: Color32::from_rgb(120, 140, 96),
            accent: Color32::from_rgb(28, 42, 24),
            border: Color32::from_rgb(56, 72, 46),
            text: Color32::from_rgb(24, 36, 20),
            muted_text: Color32::from_rgb(56, 72, 48),
        },
        Theme::WarmPaper => UiPalette {
            dark_mode: false,
            window_bg: Color32::from_rgb(245, 238, 224),
            panel_bg: Color32::from_rgb(235, 226, 208),
            widget_bg: Color32::from_rgb(225, 214, 194),
            hover_bg: Color32::from_rgb(212, 198, 175),
            accent: Color32::from_rgb(180, 78, 44),
            border: Color32::from_rgb(186, 170, 146),
            text: Color32::from_rgb(48, 38, 30),
            muted_text: Color32::from_rgb(112, 98, 84),
        },
        Theme::HighContrast => UiPalette {
            dark_mode: true,
            window_bg: Color32::from_rgb(0, 0, 0),
            panel_bg: Color32::from_rgb(0, 0, 0),
            widget_bg: Color32::from_rgb(24, 24, 24),
            hover_bg: Color32::from_rgb(48, 48, 0),
            accent: Color32::from_rgb(255, 255, 0),
            border: Color32::from_rgb(255, 255, 255),
            text: Color32::from_rgb(255, 255, 255),
            muted_text: Color32::from_rgb(220, 220, 140),
        },
    }
}

/// Apply the active [`Theme`] to an egui viewport (Settings window, etc.).
pub fn apply_theme_visuals(ctx: &egui::Context, theme: Theme) {
    apply_palette_visuals(ctx, ui_palette(theme));
}

/// Resolve the UI palette for `config`, respecting any active custom theme.
pub fn ui_palette_for_config(config: &Config) -> UiPalette {
    if let Some(custom) = config.active_custom_theme_data() {
        let bg = to_color32(custom.background, 255);
        let accent = to_color32(custom.text_color, 255);
        let border = to_color32(custom.border_color, 255);
        let shift = if custom.dark_mode { 14i16 } else { -14i16 };
        let adjust = |c: Color32, d: i16| {
            Color32::from_rgb(
                (c.r() as i16 + d).clamp(0, 255) as u8,
                (c.g() as i16 + d).clamp(0, 255) as u8,
                (c.b() as i16 + d).clamp(0, 255) as u8,
            )
        };
        let text = if custom.dark_mode {
            Color32::from_gray(236)
        } else {
            Color32::from_gray(28)
        };
        let muted_text = if custom.dark_mode {
            Color32::from_gray(165)
        } else {
            Color32::from_gray(95)
        };
        return UiPalette {
            dark_mode: custom.dark_mode,
            window_bg: bg,
            panel_bg: adjust(bg, -shift / 2),
            widget_bg: adjust(bg, shift),
            hover_bg: adjust(bg, shift * 2),
            accent,
            border,
            text,
            muted_text,
        };
    }
    ui_palette(config.theme)
}

pub fn apply_theme_visuals_for_config(ctx: &egui::Context, config: &Config) {
    apply_palette_visuals(ctx, ui_palette_for_config(config));
}

fn apply_palette_visuals(ctx: &egui::Context, p: UiPalette) {
    let mut visuals = if p.dark_mode {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.window_fill = p.window_bg;
    visuals.panel_fill = p.panel_bg;
    visuals.faint_bg_color = p.widget_bg;
    visuals.extreme_bg_color = p.panel_bg;
    visuals.override_text_color = Some(p.text);
    visuals.selection.bg_fill = p.accent;
    visuals.selection.stroke = Stroke::new(1.0_f32, p.window_bg);
    visuals.widgets.noninteractive.bg_fill = p.panel_bg;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.inactive.bg_fill = p.widget_bg;
    visuals.widgets.inactive.weak_bg_fill = p.widget_bg;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.hovered.bg_fill = p.hover_bg;
    visuals.widgets.hovered.weak_bg_fill = p.hover_bg;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.accent);
    visuals.widgets.active.bg_fill = p.accent;
    visuals.widgets.active.weak_bg_fill = p.accent;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.text);
    ctx.set_visuals(visuals);
}

/// Effective opaque background color of the overlay for contrast checking.
pub fn effective_background_color(config: &Config) -> Color32 {
    if let Some(custom) = config.active_custom_theme_data() {
        return to_color32(custom.background, 255);
    }
    match config.theme {
        Theme::Default => Color32::from_rgb(0, 0, 0),
        Theme::Chalkboard => to_color32(CHALK_BOARD, 255),
        Theme::Cyberpunk => Color32::from_rgb(14, 10, 32),
        Theme::MinimalGlass => Color32::from_rgb(28, 34, 44),
        Theme::RetroLcd => Color32::from_rgb(156, 178, 126),
        Theme::WarmPaper => Color32::from_rgb(245, 238, 224),
        Theme::HighContrast => Color32::from_rgb(0, 0, 0),
    }
}

/// WCAG relative luminance contrast ratio (`1.0`..=`21.0`).
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    fn channel(c: u8) -> f32 {
        let s = c as f32 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    fn lum(c: Color32) -> f32 {
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }
    let l1 = lum(a);
    let l2 = lum(b);
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

/// Generate a crisp 32x32 themed timer icon (RGBA bytes + width/height) shared
/// by the Settings window taskbar icon and the system tray icon.
pub fn themed_icon_rgba(p: UiPalette) -> (Vec<u8>, u32, u32) {
    const SIZE: usize = 32;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    let center = (SIZE as f32 - 1.0) / 2.0;
    let outer_r = 14.5_f32;
    let inner_r = 11.5_f32;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = (y * SIZE + x) * 4;

            if dist <= outer_r {
                let col = if dist >= inner_r {
                    p.accent
                } else {
                    p.window_bg
                };
                let edge_alpha = ((outer_r - dist).clamp(0.0, 1.0) * 255.0).round() as u8;
                rgba[idx] = col.r();
                rgba[idx + 1] = col.g();
                rgba[idx + 2] = col.b();
                rgba[idx + 3] = edge_alpha;
            }
        }
    }

    let mut set_px = |x: i32, y: i32, col: Color32| {
        if x >= 0 && y >= 0 && (x as usize) < SIZE && (y as usize) < SIZE {
            let idx = (y as usize * SIZE + x as usize) * 4;
            rgba[idx] = col.r();
            rgba[idx + 1] = col.g();
            rgba[idx + 2] = col.b();
            rgba[idx + 3] = 255;
        }
    };
    for y in 8..=16 {
        set_px(15, y, p.accent);
        set_px(16, y, p.accent);
    }
    for x in 15..=22 {
        set_px(x, 15, p.accent);
        set_px(x, 16, p.accent);
    }
    for dy in -1..=1 {
        for dx in -1..=1 {
            set_px(16 + dx, 16 + dy, p.text);
        }
    }

    (rgba, SIZE as u32, SIZE as u32)
}

/// Build an `egui::IconData` for the Settings window taskbar icon.
pub fn themed_window_icon(config: &Config) -> egui::IconData {
    let (rgba, width, height) = themed_icon_rgba(ui_palette_for_config(config));
    egui::IconData {
        rgba,
        width,
        height,
    }
}

/// Geometry transform for one frame.
#[derive(Clone, Copy)]
pub struct Orientation {
    angle: f32,
    pivot: Pos2,
}

impl Orientation {
    pub fn point(self, point: Pos2) -> Pos2 {
        if self.angle.abs() < f32::EPSILON {
            return point;
        }
        let (sin, cos) = self.angle.sin_cos();
        let local = point - self.pivot;
        self.pivot + Vec2::new(local.x * cos - local.y * sin, local.x * sin + local.y * cos)
    }

    /// Rotate a direction vector (no translation).
    fn vector(self, vector: Vec2) -> Vec2 {
        if self.angle.abs() < f32::EPSILON {
            return vector;
        }
        let (sin, cos) = self.angle.sin_cos();
        Vec2::new(
            vector.x * cos - vector.y * sin,
            vector.x * sin + vector.y * cos,
        )
    }
}

/// Lay out `text` and paint it centred on `center`, outline included.
#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    ctx: &egui::Context,
    painter: &egui::Painter,
    config: &Config,
    family: &FontFamily,
    text: &str,
    center: Pos2,
    size: f32,
    fill: Color32,
    orientation: Orientation,
) {
    if text.is_empty() {
        return;
    }

    // Each digit is drawn inside the slot a `0` would occupy, so the gap
    // between the digits and the panel edge is identical for every digit and
    // only changes when a whole slot (a leading zero) is dropped.
    let (slots, _) = layout_slots(ctx, family, text, size);

    // `center` is expressed in the unrotated layout space, so it must be
    // rotated about the window centre to land in the right place.
    let paint = |color: Color32, offset: Vec2| {
        for slot in &slots {
            let center = orientation.point(center + slot.paint) + orientation.vector(offset);
            paint_galley(painter, &slot.galley, center, color, orientation.angle);
        }
    };

    if config.theme == Theme::Chalkboard {
        // Chalk rubbed onto the board: seven offset, low-opacity passes then a
        // faint solid layer, matching the original's dusty look.
        for (dx, dy, alpha) in [
            (-1.0, -1.0, 40),
            (1.0, 1.0, 40),
            (-1.0, 1.0, 40),
            (1.0, -1.0, 40),
            (0.0, 0.0, 90),
            (0.0, 2.0, 60),
            (2.0, 0.0, 60),
        ] {
            paint(
                Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), alpha),
                Vec2::new(dx, dy),
            );
        }
        paint(fill, Vec2::ZERO);
        return;
    }

    if config.use_text_shadow {
        let shadow_alpha = (fill.a() as f32 * 0.55).round() as u8;
        let shadow_color = Color32::from_rgba_unmultiplied(0, 0, 0, shadow_alpha);
        paint(shadow_color, Vec2::new(2.5, 3.0));
    }

    if config.text_outline {
        // A ring of offsets approximates a round join, matching Qt's round stroke.
        let radius = (config.text_outline_thickness / 2.0).max(0.5);
        let steps = if radius > 2.5 { 12 } else { 8 };
        let outline_color = to_color32(config.text_outline_color, fill.a());
        for step in 0..steps {
            let theta = step as f32 / steps as f32 * std::f32::consts::TAU;
            let delta = Vec2::new(theta.cos(), theta.sin()) * radius;
            paint(outline_color, delta);
        }
    }

    paint(fill, Vec2::ZERO);
}

/// One character of the timer text, positioned for painting.
struct Slot {
    galley: Arc<egui::Galley>,
    /// Where to paint the galley's layout box, relative to the reference box's
    /// centre.
    paint: Vec2,
}

/// The box a galley actually paints, ignoring line leading.
fn ink_rect(galley: &egui::Galley) -> Rect {
    if galley.mesh_bounds.is_positive() {
        galley.mesh_bounds
    } else {
        galley.rect
    }
}

/// Lay out `text` one character per `0`-wide slot, plus the reference box.
///
/// The reference box is what a `0` would paint in the first and last slots, so
/// the window edge keeps the same distance from the digits whichever digit is
/// showing, and dropping a leading zero removes exactly one slot.
fn layout_slots(
    ctx: &egui::Context,
    family: &FontFamily,
    text: &str,
    size: f32,
) -> (Vec<Slot>, Rect) {
    let font = FontId::new(size, family.clone());
    let (slot_rects, zero_ink) = ctx.fonts(|fonts| {
        let galley = fonts.layout_job(egui::text::LayoutJob::simple_singleline(
            slot_text(text),
            font.clone(),
            Color32::WHITE,
        ));
        let zero = fonts.layout_job(egui::text::LayoutJob::simple_singleline(
            "0".to_owned(),
            font.clone(),
            Color32::WHITE,
        ));
        let Some(row) = galley.rows.first() else {
            return (Vec::new(), Rect::ZERO);
        };
        let rects = row
            .glyphs
            .iter()
            .map(|glyph| {
                Rect::from_min_size(
                    Pos2::new(glyph.pos.x, row.rect.top()),
                    Vec2::new(glyph.advance_width, row.rect.height()),
                )
            })
            .collect::<Vec<_>>();
        (rects, ink_rect(&zero))
    });

    if slot_rects.is_empty() {
        return (Vec::new(), Rect::ZERO);
    }

    let (first, last) = (slot_rects[0], slot_rects[slot_rects.len() - 1]);
    // The reference box is a `0` in the first and last slots: exactly as wide
    // as the text would be if every digit were a zero. Its centre is where the
    // timer text is anchored.
    let left = first.left() + zero_ink.min.x;
    let right = last.left() + zero_ink.max.x;
    let reference_center = Pos2::new((left + right) / 2.0, first.center().y);

    let slots = text
        .chars()
        .zip(&slot_rects)
        .map(|(character, slot)| {
            let galley = ctx.fonts(|fonts| {
                fonts.layout_job(egui::text::LayoutJob::simple_singleline(
                    character.to_string(),
                    font.clone(),
                    Color32::WHITE,
                ))
            });
            // Centre each glyph's ink in its fixed slot, so a narrow `1` never
            // shifts the digits around it or the panel edge.
            let ink = ink_rect(&galley);
            let center = Pos2::new(
                slot.center().x + (galley.rect.center().x - ink.center().x),
                reference_center.y + (galley.rect.center().y - ink.center().y),
            );
            Slot {
                galley,
                paint: center - reference_center,
            }
        })
        .collect();

    let reference =
        Rect::from_center_size(reference_center, Vec2::new(right - left, zero_ink.height()));

    (slots, reference)
}

/// Width `text` will occupy, measured in `0`-wide slots.
fn slot_width(ctx: &egui::Context, family: &FontFamily, text: &str, size: f32) -> f32 {
    let (_, reference) = layout_slots(ctx, family, text, size);
    if reference.is_positive() {
        reference.width()
    } else {
        measure_text(ctx, family, text, size).x
    }
}

/// `text` with every digit replaced by a `0`, so the slot widths are fixed.
fn slot_text(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_digit() { '0' } else { c })
        .collect()
}

/// Paint one galley centred on `center`, rotated clockwise by `angle`.
///
/// egui's [`egui::epaint::TextShape`] rotates about its `pos` (the text's
/// top-left corner), so `pos` is solved for such that the rotated text is
/// centred on `center`.
fn paint_galley(
    painter: &egui::Painter,
    galley: &Arc<egui::Galley>,
    center: Pos2,
    color: Color32,
    angle: f32,
) {
    let half = galley.rect.size() / 2.0;

    if angle.abs() < f32::EPSILON {
        painter.galley_with_override_text_color(center - half, galley.clone(), color);
        return;
    }

    let (sin, cos) = angle.sin_cos();
    let rotated_half = Vec2::new(half.x * cos - half.y * sin, half.x * sin + half.y * cos);

    let mut shape = egui::epaint::TextShape::new(center - rotated_half, galley.clone(), color);
    shape.override_text_color = Some(color);
    shape.angle = angle;
    painter.add(shape);
}

/// Content rectangle for the styles, already inset by the padding.
fn content_rect(config: &Config, rect: Rect) -> Rect {
    let padding = config.padding as f32;
    Rect::from_min_size(
        rect.min + Vec2::splat(padding),
        Vec2::new(
            (rect.width() - padding * 2.0).max(1.0),
            (rect.height() - padding * 2.0).max(1.0),
        ),
    )
}

fn rotate_points(points: &mut [Pos2], orientation: Orientation) {
    if orientation.angle.abs() < f32::EPSILON {
        return;
    }
    for point in points {
        *point = orientation.point(*point);
    }
}

/// Points along a quarter circle, for rounding polygon corners.
fn arc_points(center: Pos2, radius: f32, start: f32, end: f32) -> Vec<Pos2> {
    if radius <= 0.5 {
        return Vec::new();
    }
    // Scale the step count with the radius so large radii stay smooth.
    let steps = ((radius / 3.0).ceil() as usize).clamp(3, 16);
    (0..=steps)
        .map(|step| {
            let t = start + (end - start) * step as f32 / steps as f32;
            center + Vec2::new(t.cos(), t.sin()) * radius
        })
        .collect()
}

/// Outline of a rectangle whose corners are rounded by the given radii.
fn rounded_rect_points(rect: Rect, nw: f32, ne: f32, se: f32, sw: f32) -> Vec<Pos2> {
    let limit = rect.width().min(rect.height()) / 2.0;
    let nw = nw.min(limit);
    let ne = ne.min(limit);
    let se = se.min(limit);
    let sw = sw.min(limit);

    let mut points = Vec::new();
    // Top-left, sweeping clockwise from 180° to 270°.
    points.extend(arc_points(
        Pos2::new(rect.left() + nw, rect.top() + nw),
        nw,
        std::f32::consts::PI,
        std::f32::consts::PI * 1.5,
    ));
    points.extend(arc_points(
        Pos2::new(rect.right() - ne, rect.top() + ne),
        ne,
        std::f32::consts::PI * 1.5,
        std::f32::consts::TAU,
    ));
    points.extend(arc_points(
        Pos2::new(rect.right() - se, rect.bottom() - se),
        se,
        0.0,
        std::f32::consts::FRAC_PI_2,
    ));
    points.extend(arc_points(
        Pos2::new(rect.left() + sw, rect.bottom() - sw),
        sw,
        std::f32::consts::FRAC_PI_2,
        std::f32::consts::PI,
    ));

    if points.is_empty() {
        // No rounding at all: a plain rectangle.
        points = vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ];
    }
    points
}

/// Draw a filled polygon, rotating its points when the overlay is rotated.
fn fill_polygon(
    painter: &egui::Painter,
    mut points: Vec<Pos2>,
    color: Color32,
    orientation: Orientation,
) {
    if points.len() < 3 {
        return;
    }
    rotate_points(&mut points, orientation);
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// Draw the arc style: a track ring plus a progress arc starting at the top
/// and sweeping clockwise, like Qt's `drawArc`.
fn draw_arc(
    painter: &egui::Painter,
    config: &Config,
    rect: Rect,
    progress: f32,
    color: Color32,
    orientation: Orientation,
) {
    let dim = rect.width().min(rect.height());
    let thickness = config.arc_thickness;
    // Qt draws the arc on a `dim x dim` square with the pen centred on the
    // path, so the path radius is half the dimension (the outer half of the
    // stroke is clipped by the panel, exactly as the original did).
    let radius = dim / 2.0;
    if radius <= 0.0 {
        return;
    }
    let center = rect.center();
    let track_alpha = if config.show_background { 150 } else { 60 };
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(
            thickness,
            Color32::from_rgba_unmultiplied(60, 60, 60, track_alpha),
        ),
    );

    let sweep = progress.clamp(0.0, 1.0) * std::f32::consts::TAU;
    if sweep <= 0.0 {
        return;
    }
    let segments = ((sweep / 0.08).ceil() as usize).max(2);
    let mut points = Vec::with_capacity(segments + 1);
    for step in 0..=segments {
        // Start at 12 o'clock and go clockwise.
        let angle = -std::f32::consts::FRAC_PI_2 + sweep * step as f32 / segments as f32;
        points.push(center + Vec2::new(angle.cos(), angle.sin()) * radius);
    }
    rotate_points(&mut points, orientation);
    let cap_radius = thickness / 2.0;
    if let (Some(&start), Some(&end)) = (points.first(), points.last()) {
        painter.circle_filled(start, cap_radius, color);
        painter.circle_filled(end, cap_radius, color);
    }
    painter.add(egui::Shape::line(points, Stroke::new(thickness, color)));
}

/// Draw the bar style: a horizontal fill from the left.
fn draw_bar(
    painter: &egui::Painter,
    config: &Config,
    rect: Rect,
    progress: f32,
    color: Color32,
    orientation: Orientation,
) {
    let right = rect.left() + rect.width() * progress.clamp(0.0, 1.0);
    if right <= rect.left() + 0.5 {
        return;
    }
    let radius = corner_radius(config, rect)
        .min(rect.width() / 2.0)
        .min(rect.height() / 2.0);

    // The original intersected a full-height fill rectangle with the rounded
    // panel, so the visible shape is the container outline cut off at the
    // progress line: the left edge keeps the panel's real corner curves even
    // when the fill is narrower than the corner radius, and the right edge is
    // a straight vertical line until the bar completes.
    let mut points = rounded_rect_points(rect, radius, radius, radius, radius);
    if right < rect.right() - 0.5 {
        for point in &mut points {
            point.x = point.x.min(right);
        }
    }
    fill_polygon(painter, points, color, orientation);
}

/// Draw the fluid style: a wavy surface filled up to the progress level,
/// clipped to the rounded panel.
fn draw_fluid(
    painter: &egui::Painter,
    config: &Config,
    rect: Rect,
    progress: f32,
    color: Color32,
    phase: f32,
    orientation: Orientation,
) {
    let surface = rect.bottom() - rect.height() * progress.clamp(0.0, 1.0);
    let amplitude = (rect.height() / 8.0).min(6.0);
    // Matches the original's sampling density (`width / 240`, at least 1 px).
    let step = (rect.width() / 240.0).max(1.0);
    let radius = corner_radius(config, rect)
        .min(rect.width() / 2.0)
        .min(rect.height() / 2.0);

    // The figure is concave wherever the sine dips, and `convex_polygon` would
    // drop those dents, so it is tessellated as a strip: for every x there is
    // one vertex on the wavy surface and one on the panel's bottom edge (which
    // follows the rounded corners). Two triangles join each pair of columns.
    let mut samples = Vec::with_capacity(300);
    let mut x = rect.left();
    while x < rect.right() {
        samples.push(x);
        x += step;
    }
    samples.push(rect.right());

    let mut mesh = egui::Mesh::default();
    for &x in &samples {
        let top = Pos2::new(
            x,
            (surface + (x * 0.05 + phase).sin() * amplitude).clamp(rect.top(), rect.bottom()),
        );
        let bottom = Pos2::new(x, bottom_edge_y(rect, radius, x));
        mesh.colored_vertex(orientation.point(top), color);
        mesh.colored_vertex(orientation.point(bottom), color);
    }
    for index in 0..samples.len().saturating_sub(1) {
        let base = index as u32 * 2;
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base + 2, base + 1, base + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The lowest visible y at `x` for a rectangle whose bottom corners are
/// rounded by `radius`. Used to keep the fluid fill inside the panel shape.
fn bottom_edge_y(rect: Rect, radius: f32, x: f32) -> f32 {
    if radius <= 0.5 {
        return rect.bottom();
    }
    let centre_x = if x < rect.left() + radius {
        rect.left() + radius
    } else if x > rect.right() - radius {
        rect.right() - radius
    } else {
        return rect.bottom();
    };
    let dx = (x - centre_x).abs().min(radius);
    let centre_y = rect.bottom() - radius;
    centre_y + (radius * radius - dx * dx).max(0.0).sqrt()
}

/// Draw a textured quad rotated by `orientation`.
fn draw_rotated_quad(
    painter: &egui::Painter,
    texture_id: egui::TextureId,
    rect: Rect,
    uv: Rect,
    tint: Color32,
    orientation: Orientation,
) {
    let mut mesh = egui::Mesh::with_texture(texture_id);
    let corners = [
        (rect.left_top(), uv.left_top()),
        (rect.right_top(), uv.right_top()),
        (rect.right_bottom(), uv.right_bottom()),
        (rect.left_bottom(), uv.left_bottom()),
    ];
    for (pos, tex) in corners {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: orientation.point(pos),
            uv: tex,
            color: tint,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// Draw the sprite style: greyscale image with the coloured image revealed
/// bottom-up according to progress, rotated along with the overlay.
fn draw_sprite(
    painter: &egui::Painter,
    rect: Rect,
    progress: f32,
    textures: &Textures,
    alpha_scale: f32,
    orientation: Orientation,
) {
    let (Some(color), Some(gray)) = (&textures.color, &textures.gray) else {
        return;
    };

    let mut width = rect.width();
    let mut height = width / textures.aspect.max(0.01);
    if height > rect.height() {
        height = rect.height();
        width = height * textures.aspect.max(0.01);
    }
    let target = Rect::from_center_size(rect.center(), Vec2::new(width, height));
    let full_uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    let tint = scale_alpha(Color32::WHITE, alpha_scale);

    draw_rotated_quad(painter, gray.id(), target, full_uv, tint, orientation);

    let p = progress.clamp(0.0, 1.0);
    if p <= 0.0 {
        return;
    }
    let fill_height = target.height() * p;
    let fill_rect = Rect::from_min_max(
        Pos2::new(target.left(), target.bottom() - fill_height),
        target.max,
    );
    let fill_uv = Rect::from_min_max(Pos2::new(0.0, 1.0 - p), Pos2::new(1.0, 1.0));
    draw_rotated_quad(painter, color.id(), fill_rect, fill_uv, tint, orientation);
}

/// Everything needed to paint one frame.
pub struct Frame<'a> {
    pub config: &'a Config,
    pub snapshot: &'a Snapshot,
    pub color: Color32,
    pub textures: &'a Textures,
    pub font_size: f32,
    pub family: FontFamily,
    pub fluid_phase: f32,
    pub alpha_scale: f32,
}

/// Paint the whole overlay into `rect` (the full window rectangle).
pub fn draw(ctx: &egui::Context, painter: &egui::Painter, rect: Rect, frame: &Frame<'_>) {
    let config = frame.config;
    let alpha_scale = frame.alpha_scale.clamp(0.05, 1.0);
    let pulse = if config.pulse_on_warning
        && !config.reduce_motion
        && frame.snapshot.color_role != ColorRole::Normal
    {
        let t = ctx.input(|i| i.time) as f32;
        (t * 4.5).sin() * 0.5 + 0.5
    } else {
        0.0
    };
    let active_color = scale_alpha(frame.color, alpha_scale);
    draw_background(painter, config, rect, active_color, pulse, alpha_scale);

    // Qt rotates clockwise, so the angle is negated.
    let orientation = Orientation {
        angle: -config.rotation.degrees().to_radians(),
        pivot: rect.center(),
    };

    // The content is laid out unrotated then rotated about the window centre,
    // so vertical modes put the text along the window's long axis.
    let content = content_rect(config, rect);
    let local_size = if config.rotation.is_vertical() {
        Vec2::new(content.height(), content.width())
    } else {
        content.size()
    };
    let local = Rect::from_center_size(rect.center(), local_size);

    let has_label = !config.custom_label.trim().is_empty();
    let mut text_center = local.center();
    if frame.snapshot.lap_visible {
        text_center.y -= local.height() * 0.1;
    } else if has_label {
        text_center.y += local.height() * 0.1;
    }

    let progress_color = if config.use_gradient {
        let end = scale_alpha(to_color32(config.gradient_end_color, 255), alpha_scale);
        mix_color(active_color, end, 1.0 - frame.snapshot.progress)
    } else {
        active_color
    };

    match config.display_style {
        DisplayStyle::Arc => draw_arc(
            painter,
            config,
            local,
            frame.snapshot.progress,
            progress_color,
            orientation,
        ),
        DisplayStyle::Bar => draw_bar(
            painter,
            config,
            local,
            frame.snapshot.progress,
            progress_color,
            orientation,
        ),
        DisplayStyle::Fluid => draw_fluid(
            painter,
            config,
            local,
            frame.snapshot.progress,
            progress_color,
            frame.fluid_phase,
            orientation,
        ),
        DisplayStyle::Sprite => draw_sprite(
            painter,
            local,
            frame.snapshot.progress,
            frame.textures,
            alpha_scale,
            orientation,
        ),
        DisplayStyle::Text => {}
    }

    if has_label {
        let label_size = (frame.font_size * 0.32).clamp(9.0, 22.0);
        let label_center = Pos2::new(
            local.center().x,
            (text_center.y - frame.font_size * 0.65).max(local.top() + label_size * 0.6),
        );
        draw_text(
            ctx,
            painter,
            config,
            &frame.family,
            config.custom_label.trim(),
            label_center,
            label_size,
            scale_alpha(active_color, 0.85),
            orientation,
        );
    }

    if config.show_text {
        let text_color = match config.display_style {
            DisplayStyle::Bar => scale_alpha(to_color32(config.bar_text_color, 255), alpha_scale),
            _ => active_color,
        };
        draw_text(
            ctx,
            painter,
            config,
            &frame.family,
            &frame.snapshot.text,
            text_center,
            frame.font_size,
            text_color,
            orientation,
        );
    }

    if frame.snapshot.lap_visible && !frame.snapshot.lap_text.is_empty() {
        let lap_size = (frame.font_size * 0.4).max(8.0);
        let lap_color = if config.theme == Theme::Chalkboard {
            scale_alpha(
                Color32::from_rgba_unmultiplied(255, 255, 100, 170),
                alpha_scale,
            )
        } else {
            scale_alpha(Color32::from_rgb(255, 255, 0), alpha_scale)
        };
        let lap_center =
            text_center + Vec2::new(0.0, frame.font_size * 0.75 + local.height() * 0.05);
        draw_text(
            ctx,
            painter,
            config,
            &frame.family,
            &frame.snapshot.lap_text,
            lap_center,
            lap_size,
            lap_color,
            orientation,
        );
    }
}

/// Measure `text` at `size` in the given family.
pub fn measure_text(ctx: &egui::Context, family: &FontFamily, text: &str, size: f32) -> Vec2 {
    ctx.fonts(|fonts| {
        let job = egui::text::LayoutJob::simple_singleline(
            text.to_owned(),
            FontId::new(size, family.clone()),
            Color32::WHITE,
        );
        fonts.layout_job(job).rect.size()
    })
}

/// Window size needed to display `text` with `gap` between the digit ink and
/// the panel edge, on all four sides.
///
/// The digits are measured in `0`-width slots, exactly like [`draw_text`], so
/// the box shrinks toward the middle only when a whole slot disappears — the
/// gap never changes as the digits change.
pub fn text_box_metrics(
    ctx: &egui::Context,
    family: &FontFamily,
    text: &str,
    size: f32,
    gap: f32,
    lap_visible: bool,
) -> Vec2 {
    let (_, ink) = layout_slots(ctx, family, text, size);
    let ink = if ink.is_positive() {
        ink
    } else {
        Rect::from_min_size(Pos2::ZERO, measure_text(ctx, family, text, size))
    };
    let pad = gap.max(0.0);
    let width = ink.width() + pad * 2.0;
    let mut height = ink.height() + pad * 2.0;
    if lap_visible {
        height += ink.height() * 0.55;
    }
    Vec2::new(width.max(1.0), height.max(1.0))
}

/// Extra room the text decorations need beyond the digit ink: the outline ring
/// and the drop shadow. Kept separate from the user's gap so the visible
/// distance from the digits to the edge is exactly the padding setting.
pub fn decoration_padding(config: &Config) -> f32 {
    let mut extra = 0.0_f32;
    if config.text_outline {
        extra = extra.max(config.text_outline_thickness / 2.0);
    }
    if config.use_text_shadow {
        extra = extra.max(3.0);
    }
    if config.theme == Theme::Chalkboard {
        extra = extra.max(2.0);
    }
    extra
}

/// The full distance from the digit ink to the panel edge, on every side.
pub fn edge_gap(config: &Config) -> f32 {
    config.padding as f32 + decoration_padding(config)
}

/// Largest size (binary searched) whose text fits `available_width`.
pub fn fit_font_size(
    ctx: &egui::Context,
    family: &FontFamily,
    text: &str,
    available_width: f32,
    min_size: f32,
    max_size: f32,
) -> f32 {
    if text.is_empty() || available_width <= 1.0 {
        return min_size;
    }
    let mut low = min_size;
    let mut high = max_size;
    let mut best = min_size;
    while high - low > 0.5 {
        let mid = (low + high) / 2.0;
        if slot_width(ctx, family, text, mid) <= available_width {
            best = mid;
            low = mid;
        } else {
            high = mid;
        }
    }
    best.round()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_color_endpoints() {
        let black = Color32::BLACK;
        let white = Color32::WHITE;
        assert_eq!(mix_color(black, white, 0.0), black);
        assert_eq!(mix_color(black, white, 1.0), white);
        let mid = mix_color(black, white, 0.5);
        assert!((mid.r() as i32 - 128).abs() <= 1);
    }

    #[test]
    fn mix_color_clamps_t() {
        let a = Color32::from_rgb(10, 20, 30);
        let b = Color32::from_rgb(200, 100, 50);
        assert_eq!(mix_color(a, b, -5.0), a);
        assert_eq!(mix_color(a, b, 5.0), b);
    }

    #[test]
    fn capsule_radius_is_half_the_shorter_side() {
        let config = Config {
            window_shape: WindowShape::Capsule,
            ..Default::default()
        };
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(300.0, 100.0));
        assert_eq!(corner_radius(&config, rect), 50.0);
    }

    #[test]
    fn rectangular_has_no_radius() {
        let config = Config {
            window_shape: WindowShape::Rectangular,
            corner_radius: 30,
            ..Default::default()
        };
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(300.0, 100.0));
        assert_eq!(corner_radius(&config, rect), 0.0);
    }

    #[test]
    fn orientation_with_no_rotation_is_identity() {
        let orientation = Orientation {
            angle: 0.0,
            pivot: Pos2::new(10.0, 10.0),
        };
        let point = Pos2::new(3.0, 7.0);
        assert_eq!(orientation.point(point), point);
    }

    #[test]
    fn quarter_turn_rotates_clockwise() {
        let pivot = Pos2::new(0.0, 0.0);
        let orientation = Orientation {
            // -90° so that a point to the right ends up below the pivot.
            angle: -std::f32::consts::FRAC_PI_2,
            pivot,
        };
        let rotated = orientation.point(Pos2::new(10.0, 0.0));
        assert!((rotated.x - 0.0).abs() < 1e-3);
        assert!((rotated.y + 10.0).abs() < 1e-3);
    }

    #[test]
    fn rotation_preserves_distance_from_pivot() {
        let pivot = Pos2::new(5.0, -3.0);
        let orientation = Orientation { angle: 1.1, pivot };
        let point = Pos2::new(20.0, 12.0);
        let before = (point - pivot).length();
        let after = (orientation.point(point) - pivot).length();
        assert!((before - after).abs() < 1e-3);
    }

    #[test]
    fn slot_text_replaces_every_digit() {
        assert_eq!(slot_text("01:59.25"), "00:00.00");
        assert_eq!(slot_text("+1:05"), "+0:00");
        assert_eq!(slot_text(""), "");
    }

    #[test]
    fn decoration_padding_tracks_outline_and_shadow() {
        let plain = Config {
            text_outline: false,
            use_text_shadow: false,
            theme: Theme::Default,
            ..Default::default()
        };
        assert_eq!(decoration_padding(&plain), 0.0);

        let outlined = Config {
            text_outline: true,
            text_outline_thickness: 4.0,
            ..plain.clone()
        };
        assert_eq!(decoration_padding(&outlined), 2.0);

        let shadowed = Config {
            text_outline: false,
            use_text_shadow: true,
            ..plain
        };
        assert_eq!(decoration_padding(&shadowed), 3.0);
    }

    #[test]
    fn edge_gap_is_padding_plus_decorations() {
        let config = Config {
            padding: 12,
            text_outline: true,
            text_outline_thickness: 6.0,
            ..Default::default()
        };
        assert_eq!(edge_gap(&config), 15.0);
    }

    /// A context whose fonts have been loaded (measuring needs a real frame).
    fn measured_context() -> egui::Context {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        ctx
    }

    #[test]
    fn digits_measure_in_equal_slots() {
        let ctx = measured_context();
        let family = egui::FontFamily::Proportional;
        // Same length, different digits: the box must be identical.
        let a = text_box_metrics(&ctx, &family, "11:11", 40.0, 20.0, false);
        let b = text_box_metrics(&ctx, &family, "00:00", 40.0, 20.0, false);
        assert!((a.x - b.x).abs() < 0.01, "{a:?} vs {b:?}");
        // Widths that differ only by a narrow digit's ink still match, because
        // every digit occupies a `0` slot.
        let c = text_box_metrics(&ctx, &family, "1:11", 40.0, 20.0, false);
        let d = text_box_metrics(&ctx, &family, "0:00", 40.0, 20.0, false);
        assert!((c.x - d.x).abs() < 0.01, "{c:?} vs {d:?}");
    }

    #[test]
    fn dropping_a_leading_zero_removes_one_slot() {
        let ctx = measured_context();
        let family = egui::FontFamily::Proportional;
        let with_zero = text_box_metrics(&ctx, &family, "00:42", 40.0, 20.0, false);
        let without = text_box_metrics(&ctx, &family, "0:42", 40.0, 20.0, false);
        // One digit slot is exactly the advance a zero adds.
        let one = text_box_metrics(&ctx, &family, "0", 40.0, 20.0, false);
        let two = text_box_metrics(&ctx, &family, "00", 40.0, 20.0, false);
        let slot = two.x - one.x;
        assert!(with_zero.x > without.x);
        assert!(
            (with_zero.x - without.x - slot).abs() < 0.5,
            "{} vs {} (slot {slot})",
            with_zero.x,
            without.x
        );
    }

    #[test]
    fn box_width_is_ink_plus_twice_the_gap() {
        let ctx = measured_context();
        let family = egui::FontFamily::Proportional;
        let gap = 18.0;
        let box_size = text_box_metrics(&ctx, &family, "12:34", 40.0, gap, false);
        let ink = slot_width(&ctx, &family, "12:34", 40.0);
        assert!(
            (box_size.x - (ink + gap * 2.0)).abs() < 0.5,
            "{} vs {}",
            box_size.x,
            ink + gap * 2.0
        );
    }

    #[test]
    fn chalk_colours_match_the_original() {
        assert_eq!(CHALK_BOARD, Rgb(46, 68, 50));
        assert_eq!(CHALK_TEXT, Rgb(240, 255, 240));
    }
}
