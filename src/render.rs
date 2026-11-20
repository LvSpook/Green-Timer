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

/// The colour the timer should currently use for the given role.
pub fn target_color(config: &Config, role: ColorRole) -> Color32 {
    match role {
        ColorRole::Normal => to_color32(config.normal_color, 255),
        ColorRole::Threshold | ColorRole::Overdue => to_color32(config.gray_color, 255),
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
pub fn draw_background(painter: &egui::Painter, config: &Config, rect: Rect, color: Color32) {
    if !config.show_background {
        return;
    }

    let is_chalk = config.theme == Theme::Chalkboard;
    let background = if is_chalk {
        to_color32(CHALK_BOARD, background_alpha(config))
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, background_alpha(config))
    };

    let thickness = config.border_thickness;
    let paint_rect = if config.border_style == BorderStyle::None {
        rect
    } else {
        rect.shrink(thickness / 2.0)
    };

    let stroke = match config.border_style {
        BorderStyle::Thin => Stroke::new(thickness, to_color32(config.thin_border_color, 255)),
        BorderStyle::Neon => Stroke::new(thickness, color),
        BorderStyle::None if is_chalk => Stroke::new(4.0_f32, Color32::from_rgb(100, 70, 40)),
        BorderStyle::None => Stroke::NONE,
    };

    let rounding = Rounding::same(corner_radius(config, paint_rect));
    painter.rect(paint_rect, rounding, background, stroke);
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

    let galley = ctx.fonts(|fonts| {
        let job = egui::text::LayoutJob::simple_singleline(
            text.to_owned(),
            FontId::new(size, family.clone()),
            fill,
        );
        fonts.layout_job(job)
    });

    // `center` is expressed in the unrotated layout space, so it must be
    // rotated about the window centre to land in the right place.
    let center = orientation.point(center);

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
            paint_galley(
                painter,
                &galley,
                center + orientation.vector(Vec2::new(dx, dy)),
                Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), alpha),
                orientation.angle,
            );
        }
        paint_galley(painter, &galley, center, fill, orientation.angle);
        return;
    }

    if config.text_outline {
        // A ring of offsets approximates a round join, matching Qt's round stroke.
        let radius = (config.text_outline_thickness / 2.0).max(0.5);
        let steps = if radius > 2.5 { 12 } else { 8 };
        let outline_color = to_color32(config.text_outline_color, 255);
        for step in 0..steps {
            let theta = step as f32 / steps as f32 * std::f32::consts::TAU;
            let delta = orientation.vector(Vec2::new(theta.cos(), theta.sin()) * radius);
            paint_galley(painter, &galley, center + delta, outline_color, orientation.angle);
        }
    }

    paint_galley(painter, &galley, center, fill, orientation.angle);
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
        Stroke::new(thickness, Color32::from_rgba_unmultiplied(60, 60, 60, track_alpha)),
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

/// Draw the sprite style: greyscale image with the coloured image revealed
/// bottom-up according to progress.
fn draw_sprite(painter: &egui::Painter, rect: Rect, progress: f32, textures: &Textures) {
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
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));

    painter.image(gray.id(), target, uv, Color32::WHITE);

    let fill_height = target.height() * progress.clamp(0.0, 1.0);
    if fill_height <= 0.0 {
        return;
    }
    let clip = Rect::from_min_max(
        Pos2::new(target.left(), target.bottom() - fill_height),
        target.max,
    );
    painter.with_clip_rect(clip).image(color.id(), target, uv, Color32::WHITE);
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
}

/// Paint the whole overlay into `rect` (the full window rectangle).
pub fn draw(ctx: &egui::Context, painter: &egui::Painter, rect: Rect, frame: &Frame<'_>) {
    let config = frame.config;
    draw_background(painter, config, rect, frame.color);

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

    let mut text_center = local.center();
    if frame.snapshot.lap_visible {
        text_center.y -= local.height() * 0.1;
    }

    match config.display_style {
        DisplayStyle::Arc => draw_arc(
            painter,
            config,
            local,
            frame.snapshot.progress,
            frame.color,
            orientation,
        ),
        DisplayStyle::Bar => draw_bar(
            painter,
            config,
            local,
            frame.snapshot.progress,
            frame.color,
            orientation,
        ),
        DisplayStyle::Fluid => draw_fluid(
            painter,
            config,
            local,
            frame.snapshot.progress,
            frame.color,
            frame.fluid_phase,
            orientation,
        ),
        DisplayStyle::Sprite => draw_sprite(
            painter,
            local,
            frame.snapshot.progress,
            frame.textures,
        ),
        DisplayStyle::Text => {}
    }

    if config.show_text {
        let text_color = match config.display_style {
            DisplayStyle::Bar => to_color32(config.bar_text_color, 255),
            _ => frame.color,
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
            Color32::from_rgba_unmultiplied(255, 255, 100, 170)
        } else {
            Color32::from_rgb(255, 255, 0)
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
pub fn measure_text(
    ctx: &egui::Context,
    family: &FontFamily,
    text: &str,
    size: f32,
) -> Vec2 {
    ctx.fonts(|fonts| {
        let job = egui::text::LayoutJob::simple_singleline(
            text.to_owned(),
            FontId::new(size, family.clone()),
            Color32::WHITE,
        );
        fonts.layout_job(job).rect.size()
    })
}

/// Window size needed to display `text` with `padding`.
///
/// A little is added to the measured galley because its rect is the *layout*
/// box: glyph ink (round bowls, descenders) can overshoot it slightly, and
/// without the margin a zero-padding window clips the text.
pub fn text_box_metrics(
    ctx: &egui::Context,
    family: &FontFamily,
    text: &str,
    size: f32,
    padding: u32,
    lap_visible: bool,
) -> Vec2 {
    let measured = measure_text(ctx, family, text, size);
    let pad = padding as f32 * 2.0;
    let margin = (size * 0.12).max(2.0) * 2.0;
    let width = measured.x + pad + margin;
    let mut height = measured.y + pad + margin;
    if lap_visible {
        height += measured.y * 0.55;
    }
    Vec2::new(width.max(1.0), height.max(1.0))
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
        if measure_text(ctx, family, text, mid).x <= available_width {
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
        let orientation = Orientation {
            angle: 1.1,
            pivot,
        };
        let point = Pos2::new(20.0, 12.0);
        let before = (point - pivot).length();
        let after = (orientation.point(point) - pivot).length();
        assert!((before - after).abs() < 1e-3);
    }

    #[test]
    fn chalk_colours_match_the_original() {
        assert_eq!(CHALK_BOARD, Rgb(46, 68, 50));
        assert_eq!(CHALK_TEXT, Rgb(240, 255, 240));
    }
}
