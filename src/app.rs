//! The overlay window and application wiring.

use crate::config::{self, Config, Mode};
use crate::engine::{ColorRole, Event, Snapshot, TimerEngine};
use crate::fonts;
use crate::hotkeys::{Action, HotkeyManager};
use crate::platform;
use crate::render::{self, Textures};
use crate::settings::SettingsState;
use crate::tray::{Tray, TrayCommand};
use crate::{updater, APP_NAME, APP_VERSION, ICON_FILENAME};
use eframe::egui;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(16);
const IDLE_TICK: Duration = Duration::from_millis(100);
const COLOR_FADE: f32 = 0.35;

pub struct TimerApp {
    pub config: Config,
    pub engine: TimerEngine,
    pub snapshot: Snapshot,
    pub settings: SettingsState,
    pub textures: Textures,
    pub hotkeys: HotkeyManager,
    pub tray: Tray,
    pub font_size: f32,
    /// Window handle, captured once the viewport exists.
    hwnd: isize,
    /// Colour fade state.
    display_color: egui::Color32,
    fade_from: egui::Color32,
    fade_started: Option<Instant>,
    /// Geometry that has been pushed to the OS, to avoid redundant calls.
    applied: AppliedState,
    /// Update check result channel, if a check is running.
    pub update_rx: Option<std::sync::mpsc::Receiver<Option<updater::UpdateInfo>>>,
    pub update_available: Option<updater::UpdateInfo>,
    /// Installer download channel, if a download is running.
    install_rx: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    show_window: bool,
    quit: bool,
    /// Size at which the current grip drag began (logical px).
    grip_start: Option<egui::Vec2>,
    /// Last profile activated automatically by a per-app rule.
    last_auto_profile: Option<String>,
    /// Saved font size before entering compact mini-mode.
    pre_mini_font_size: Option<u32>,
}

#[derive(Default, Clone, PartialEq)]
struct AppliedState {
    hwnd: isize,
    blur: bool,
    ghost: bool,
    opacity: u32,
    vertical: bool,
}

impl TimerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = config::load();
        fonts::install(&cc.egui_ctx, &config.font_family);

        let mut hotkeys = HotkeyManager::new();
        hotkeys.rebind(&config);

        let tray = Tray::new(&config);
        if tray.is_available() == false {
            log::info!("Running without a tray icon");
        }

        let textures = render::load_textures(&cc.egui_ctx, &config.sprite_path);
        let mut engine = TimerEngine::new(crate::engine::RealClock);
        if config.auto_start_on_launch && !matches!(config.mode, Mode::Clock | Mode::TargetTime) {
            engine.start(&config);
        }
        let snapshot = Snapshot::default();
        let display_color = render::target_color(&config, ColorRole::Normal);

        let update_rx = if config.auto_check_updates {
            Some(updater::spawn_check(APP_VERSION.to_string(), config.include_prereleases))
        } else {
            None
        };

        TimerApp {
            show_window: !config.start_minimized || !tray.is_available(),
            font_size: config.font_size as f32,
            config,
            engine,
            snapshot,
            settings: SettingsState::default(),
            textures,
            hotkeys,
            tray,
            hwnd: 0,
            display_color,
            fade_from: display_color,
            fade_started: None,
            applied: AppliedState::default(),
            update_rx,
            update_available: None,
            install_rx: None,
            quit: false,
            grip_start: None,
            last_auto_profile: None,
            pre_mini_font_size: None,
        }
    }

    // -- actions ---------------------------------------------------------
    pub fn toggle(&mut self) {
        if matches!(self.config.mode, Mode::Clock | Mode::TargetTime) {
            return;
        }
        self.engine.toggle(&self.config);
        self.tick();
    }

    pub fn reset(&mut self) {
        self.engine.reset();
        self.tick();
    }

    pub fn adjust_time(&mut self, delta_seconds: f64) {
        self.engine.adjust_time(&self.config, delta_seconds);
        self.tick();
    }

    pub fn toggle_mini_mode(&mut self) {
        self.config.mini_mode = !self.config.mini_mode;
        if self.config.mini_mode {
            self.pre_mini_font_size = Some(self.config.font_size);
            self.config.font_size = 18;
            self.font_size = 18.0;
        } else if let Some(prev) = self.pre_mini_font_size.take() {
            self.config.font_size = prev;
            self.font_size = prev as f32;
        }
        self.resize_for_text();
        self.persist();
    }

    pub fn apply_profile(&mut self, name: &str) {
        if self.config.apply_profile(name) {
            self.font_size = self.config.font_size as f32;
            self.hotkeys.rebind(&self.config);
            if let Some(ctx) = current_context() {
                fonts::install(&ctx, &self.config.font_family);
                self.textures = render::load_textures(&ctx, &self.config.sprite_path);
            }
            self.display_color = render::target_color(&self.config, self.snapshot.color_role);
            self.resize_for_text();
            self.tick();
            self.persist();
        }
    }

    pub fn trigger_lap(&mut self) {
        if self.engine.trigger_lap(&self.config) {
            self.resize_for_text();
        }
    }

    pub fn toggle_ghost(&mut self) {
        self.config.ghost_mode = !self.config.ghost_mode;
        self.apply_appearance();
        self.persist();
    }

    pub fn persist(&mut self) {
        self.sync_geometry();
        if let Err(err) = config::save(&self.config) {
            log::warn!("Settings could not be persisted: {err}");
        }
    }

    /// Record the window's current position and size, like the original did
    /// whenever it saved. Dragging is delegated to the OS, so the config would
    /// otherwise keep the position from the last launch.
    fn sync_geometry(&mut self) {
        let Some(ctx) = self::current_context() else {
            return;
        };
        let (outer, inner) = ctx.input(|i| {
            let viewport = i.viewport();
            (viewport.outer_rect, viewport.inner_rect)
        });
        if let Some(outer) = outer {
            self.config.pos_x = outer.min.x.round() as i32;
            self.config.pos_y = outer.min.y.round() as i32;
        }
        if let Some(inner) = inner {
            self.config.width = inner.width().round().max(1.0) as u32;
            self.config.height = inner.height().round().max(1.0) as u32;
        }
    }

    pub fn reload_assets(&mut self, ctx: &egui::Context) {
        self.textures = render::load_textures(ctx, &self.config.sprite_path);
    }

    pub fn apply_appearance(&mut self) {
        let was_vertical = self.applied.vertical;
        self.tray.set_theme(&self.config);
        self.sync_platform();
        if self.config.rotation.is_vertical() != was_vertical {
            self.resize_for_text();
        }
    }

    /// Push blur / click-through / opacity / topmost to the OS when they change.
    fn sync_platform(&mut self) {
        if self.hwnd == 0 {
            return;
        }
        let blur = platform::IS_WINDOWS
            && self.config.window_blur
            && self.config.show_background
            && self.config.theme != crate::config::Theme::Chalkboard;
        let opacity = (self.config.opacity * 100.0).round() as u32;

        let desired = AppliedState {
            hwnd: self.hwnd,
            blur,
            ghost: self.config.ghost_mode,
            opacity,
            vertical: self.config.rotation.is_vertical(),
        };
        if desired == self.applied {
            return;
        }

        if desired.blur != self.applied.blur || self.applied.hwnd != self.hwnd {
            platform::apply_blur(self.hwnd, blur);
        }
        if desired.ghost != self.applied.ghost {
            platform::set_click_through(self.hwnd, self.config.ghost_mode);
        }
        self.applied = desired;
    }

    /// Size the window to the current text.
    pub fn resize_for_text(&mut self) {
        let Some(ctx) = CURRENT_CTX.with(|cell| cell.borrow().clone()) else {
            return;
        };
        let text = if self.snapshot.text.is_empty() {
            self.engine.current_text(&self.config)
        } else {
            self.snapshot.text.clone()
        };
        let family = fonts::timer_family(&ctx, &self.config.font_family);
        let size = render::text_box_metrics(
            &ctx,
            &family,
            &text,
            self.font_size,
            self.config.padding,
            self.snapshot.lap_visible,
        );
        let (mut width, mut height) = (size.x, size.y);
        if self.config.rotation.is_vertical() {
            std::mem::swap(&mut width, &mut height);
        }
        let width = width.max(80.0);
        let height = height.max(40.0);
        if (width - self.config.width as f32).abs() > 1.0
            || (height - self.config.height as f32).abs() > 1.0
        {
            self.config.width = width.round() as u32;
            self.config.height = height.round() as u32;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(width, height)));
        }
    }

    /// Pick the largest font that fits `available` and resize to match, the
    /// behaviour of the original's proportional resize grip.
    pub fn fit_to_width(&mut self, available: f32) {
        let Some(ctx) = CURRENT_CTX.with(|cell| cell.borrow().clone()) else {
            return;
        };
        let text = if self.snapshot.text.is_empty() {
            self.engine.current_text(&self.config)
        } else {
            self.snapshot.text.clone()
        };
        let family = fonts::timer_family(&ctx, &self.config.font_family);
        let padding = self.config.padding as f32;
        let text_width = (available - padding * 2.0).max(10.0);
        let size = render::fit_font_size(
            &ctx,
            &family,
            &text,
            text_width,
            4.0,
            500.0,
        );
        self.font_size = size;
        self.config.font_size = size.round() as u32;

        let metrics = render::text_box_metrics(
            &ctx,
            &family,
            &text,
            self.font_size,
            self.config.padding,
            self.snapshot.lap_visible,
        );
        let (mut width, mut height) = (metrics.x, metrics.y);
        if self.config.rotation.is_vertical() {
            std::mem::swap(&mut width, &mut height);
        }
        let width = width.max(80.0);
        let height = height.max(40.0);
        self.config.width = width.round() as u32;
        self.config.height = height.round() as u32;
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(width, height)));
    }

    fn tick(&mut self) {
        let snapshot = self.engine.tick(&self.config);

        // Auto-pause when a target window is configured and focus leaves it,
        // and per-app auto-switch profiles when a rule matches.
        if (self.engine.running() && self.config.target_hwnd != 0)
            || !self.config.app_profile_rules.is_empty()
        {
            let current = platform::foreground_window();
            if current != 0 {
                if self.engine.running()
                    && self.config.target_hwnd != 0
                    && current != self.config.target_hwnd
                {
                    self.engine.pause();
                }
                if let Some(profile_name) = self
                    .config
                    .app_profile_rules
                    .iter()
                    .find(|r| r.target_hwnd == current)
                    .map(|r| r.profile_name.clone())
                {
                    if self.last_auto_profile.as_deref() != Some(profile_name.as_str()) {
                        self.last_auto_profile = Some(profile_name.clone());
                        self.apply_profile(&profile_name);
                    }
                }
            }
        }

        for event in &snapshot.events {
            if *event == Event::Cue && self.config.sound_cue {
                platform::play_sound(&self.config.custom_sound_path);
            }
        }

        if snapshot.color_role != self.snapshot.color_role {
            self.fade_from = self.display_color;
            self.fade_started = Some(Instant::now());
        }
        if snapshot.text_length_changed
            && self.config.display_style == crate::config::DisplayStyle::Text
            && !self.config.window_locked
        {
            // Like the original, the window hugs the text as its length changes.
            // Other styles own their own geometry, so they are left alone.
            self.resize_for_text();
        }
        self.snapshot = snapshot;
    }

    fn current_color(&mut self) -> egui::Color32 {
        let target = render::target_color(&self.config, self.snapshot.color_role);
        if self.config.reduce_motion {
            return target;
        }
        let Some(started) = self.fade_started else {
            return target;
        };
        let elapsed = started.elapsed().as_secs_f32();
        if elapsed >= COLOR_FADE {
            self.fade_started = None;
            return target;
        }
        render::mix_color(self.fade_from, target, elapsed / COLOR_FADE)
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        // Hotkeys and tray are polled every frame.
        for action in self.hotkeys.poll() {
            match action {
                Action::Toggle => self.toggle(),
                Action::Reset => self.reset(),
                Action::Settings => self.settings.open = true,
                Action::Lap => self.trigger_lap(),
                Action::Ghost => self.toggle_ghost(),
                Action::ToggleVisibility => {
                    self.show_window = !self.show_window;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.show_window));
                }
                Action::ToggleMiniMode => self.toggle_mini_mode(),
            }
        }
        for command in self.tray.poll() {
            match command {
                TrayCommand::Toggle => self.toggle(),
                TrayCommand::Reset => self.reset(),
                TrayCommand::Lap => self.trigger_lap(),
                TrayCommand::ToggleMiniMode => self.toggle_mini_mode(),
                TrayCommand::ToggleGhost => self.toggle_ghost(),
                TrayCommand::Settings => self.settings.open = true,
                TrayCommand::ToggleVisibility => {
                    self.show_window = !self.show_window;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.show_window));
                }
                TrayCommand::Exit => self.request_quit(ctx),
            }
        }

        if self.update_available.is_some() {
            // Handled by the settings window / banner.
        }

        // Escape quits and Space toggles, matching the original. While the
        // menu is open it owns the keyboard, so presses must not leak through.
        if self.show_window && !self.config.ghost_mode && !crate::menu::is_open() {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.request_quit(ctx);
            } else if self.config.mode != Mode::Clock
                && ctx.input(|i| i.key_pressed(egui::Key::Space))
            {
                self.toggle();
            }
        }
    }

    pub fn request_quit(&mut self, ctx: &egui::Context) {
        self.persist();
        self.quit = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Recompute the snapshot without changing the running state, so edits are
    /// visible immediately even while paused.
    pub fn refresh(&mut self) {
        self.tick();
    }

    /// Re-register hotkeys; returns a description of any that failed.
    pub fn hotkeys_rebind(&mut self) -> Result<(), String> {
        self.hotkeys.rebind(&self.config);
        let failures = self.hotkeys.failures();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures
                .iter()
                .map(|(action, reason)| format!("{}: {reason}", action.label()))
                .collect::<Vec<_>>()
                .join(", "))
        }
    }

    fn poll_updates(&mut self) {
        let Some(rx) = &self.update_rx else {
            return;
        };
        if let Ok(result) = rx.try_recv() {
            if let Some(info) = result {
                log::info!("Update available: {}", info.latest);
                if platform::confirm(
                    "Update Available",
                    &format!(
                        "{APP_NAME} {} is available (you have {}).{}\n\n\
                         Download and run the installer now?",
                        info.latest,
                        APP_VERSION,
                        if info.prerelease {
                            "\n\nThis is a pre-release (work in progress) build."
                        } else {
                            ""
                        }
                    ),
                ) {
                    self.install_update(&info);
                } else {
                    // Declined: keep it available in the About tab.
                    self.update_available = Some(info);
                }
            }
            self.update_rx = None;
        }
    }

    /// Download the installer to the temp directory, start it, and exit so it
    /// can replace this executable — the original updater's flow.
    ///
    /// Downloading runs on a background thread; the result is handed straight
    /// to the event loop so the overlay never blocks on the network.
    pub fn install_update(&mut self, info: &updater::UpdateInfo) {
        self.persist();
        log::info!("Downloading update {} from {}", info.latest, info.url);
        let info = info.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(updater::download_installer(&info));
        });
        self.install_rx = Some(rx);
    }

    fn poll_install(&mut self) {
        let Some(rx) = &self.install_rx else {
            return;
        };
        // Keep the loop awake while the download is in flight.
        if let Some(ctx) = current_context() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let Ok(result) = rx.try_recv() else {
            return;
        };
        self.install_rx = None;
        match result {
            Ok(path) => match std::process::Command::new(&path).spawn() {
                Ok(_) => {
                    log::info!("Installer launched from {}", path.display());
                    std::process::exit(0);
                }
                Err(err) => {
                    log::warn!("Could not start the installer: {err}");
                    platform::alert(
                        "Update Error",
                        &format!("Could not start the installer:\n{err}"),
                    );
                }
            },
            Err(err) => {
                log::warn!("Update download failed: {err}");
                platform::alert(
                    "Update Error",
                    &format!("Could not download the update:\n{err}"),
                );
            }
        }
    }
}

thread_local! {
    /// The egui context, kept for helpers that need text measurement outside
    /// of `update` (for example when a settings control resizes the window).
    pub static CURRENT_CTX: std::cell::RefCell<Option<egui::Context>> = const { std::cell::RefCell::new(None) };
}

/// The current egui context, if the app has started a frame.
pub fn current_context() -> Option<egui::Context> {
    CURRENT_CTX.with(|cell| cell.borrow().clone())
}

thread_local! {
    /// Installer URL requested from the settings window, picked up by the
    /// app on its next frame so the download does not block the dialog.
    static PENDING_INSTALL: std::cell::RefCell<Option<updater::UpdateInfo>> = const { std::cell::RefCell::new(None) };
}

/// Ask the app to download and run the installer described by `info`.
pub fn request_install(info: &updater::UpdateInfo) {
    PENDING_INSTALL.with(|cell| *cell.borrow_mut() = Some(info.clone()));
}

fn take_pending_install() -> Option<updater::UpdateInfo> {
    PENDING_INSTALL.with(|cell| cell.borrow_mut().take())
}

impl eframe::App for TimerApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.hwnd == 0 {
            if let Some(hwnd) = window_handle(_frame) {
                self.hwnd = hwnd;
                platform::set_topmost(self.hwnd, true);
                platform::set_dark_title_bar(self.hwnd, true);
                self.apply_appearance();
            }
        }
        CURRENT_CTX.with(|cell| *cell.borrow_mut() = Some(ctx.clone()));

        self.poll_updates();
        self.poll_install();
        if let Some(info) = take_pending_install() {
            self.install_update(&info);
        }
        self.handle_input(ctx);
        self.tick();
        self.display_color = self.current_color();
        self.sync_platform();

        let style = ctx.style();
        let _ = style;

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let family = fonts::timer_family(ctx, &self.config.font_family);
                let pointer_in = ctx.input(|i| i.pointer.hover_pos().is_some());
                let alpha_scale = if self.config.hover_to_fade
                    && pointer_in
                    && !crate::menu::is_open()
                {
                    self.config.hover_fade_opacity
                } else {
                    1.0
                };
                let frame = render::Frame {
                    config: &self.config,
                    snapshot: &self.snapshot,
                    color: self.display_color,
                    textures: &self.textures,
                    font_size: self.font_size,
                    family,
                    fluid_phase: self.engine.fluid_phase,
                    alpha_scale,
                };
                let mut painter = ui.painter().clone();
                painter.set_opacity(self.config.opacity);
                render::draw(ctx, &painter, rect, &frame);

                self.overlay_interaction(ctx, ui, rect);
            });

        // Magnetic edge snapping when dragging near a screen edge.
        if self.config.edge_snapping && !self.config.window_locked {
            if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                let is_dragging = ctx.input(|i| i.pointer.any_down());
                if !is_dragging {
                    let (left, top, sw, sh) = platform::primary_work_area(ctx.pixels_per_point());
                    let snap_dist = 18.0;
                    let mut x = outer.min.x;
                    let mut y = outer.min.y;
                    let w = outer.width();
                    let h = outer.height();
                    if (x - left).abs() <= snap_dist {
                        x = left;
                    } else if ((x + w) - (left + sw)).abs() <= snap_dist {
                        x = left + sw - w;
                    }
                    if (y - top).abs() <= snap_dist {
                        y = top;
                    } else if ((y + h) - (top + sh)).abs() <= snap_dist {
                        y = top + sh - h;
                    }
                    if (x - outer.min.x).abs() > 0.5 || (y - outer.min.y).abs() > 0.5 {
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                            x, y,
                        )));
                        self.config.pos_x = x.round() as i32;
                        self.config.pos_y = y.round() as i32;
                    }
                }
            }
        }

        self.context_menu(ctx);

        if self.settings.open {
            // Move the state out so the settings window can borrow the app
            // mutably while it edits its own draft.
            let mut state = std::mem::take(&mut self.settings);
            crate::settings::show_window(ctx, &mut state, self);
            self.settings = state;
        }

        // Animate fluid and colour fades smoothly; otherwise poll less often.
        let animated = self.engine.running()
            || self.config.mode == Mode::Clock
            || self.config.display_style == crate::config::DisplayStyle::Fluid
            || self.fade_started.is_some();
        ctx.request_repaint_after(if animated { TICK } else { IDLE_TICK });

        if self.config.display_style == crate::config::DisplayStyle::Fluid
            && self.config.mode != Mode::Clock
        {
            self.engine.fluid_phase += 0.25;
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist();
    }
}

impl TimerApp {
    /// Dragging, double-click-to-toggle and the right-click menu.
    fn overlay_interaction(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, rect: egui::Rect) {
        if self.config.ghost_mode {
            return;
        }
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());

        // A small grip in the bottom-right corner scales the text to the
        // Resize from any of the four corners when unlocked.
        if !self.config.window_locked {
            let g = 16.0;
            let corners = [
                (
                    "grip-tl",
                    egui::Rect::from_min_size(rect.min, egui::vec2(g, g)),
                    -1.0_f32,
                    -1.0_f32,
                    egui::CursorIcon::ResizeNwSe,
                ),
                (
                    "grip-tr",
                    egui::Rect::from_min_max(
                        egui::pos2(rect.right() - g, rect.top()),
                        egui::pos2(rect.right(), rect.top() + g),
                    ),
                    1.0_f32,
                    -1.0_f32,
                    egui::CursorIcon::ResizeNeSw,
                ),
                (
                    "grip-bl",
                    egui::Rect::from_min_max(
                        egui::pos2(rect.left(), rect.bottom() - g),
                        egui::pos2(rect.left() + g, rect.bottom()),
                    ),
                    -1.0_f32,
                    1.0_f32,
                    egui::CursorIcon::ResizeNeSw,
                ),
                (
                    "grip-br",
                    egui::Rect::from_min_max(
                        egui::pos2(rect.right() - g, rect.bottom() - g),
                        rect.max,
                    ),
                    1.0_f32,
                    1.0_f32,
                    egui::CursorIcon::ResizeNwSe,
                ),
            ];

            for (id, grip_rect, sx, sy, cursor) in corners {
                let grip = ui.interact(grip_rect, ui.id().with(id), egui::Sense::drag());
                let is_br = id == "grip-br";
                if is_br || grip.hovered() || grip.dragged() {
                    let grip_alpha = if grip.hovered() || grip.dragged() {
                        190
                    } else {
                        70
                    };
                    let grip_stroke = egui::Stroke::new(
                        1.25_f32,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, grip_alpha),
                    );
                    let painter = ui.painter();
                    if is_br {
                        for offset in [4.0_f32, 8.0, 12.0] {
                            painter.line_segment(
                                [
                                    egui::pos2(rect.right() - offset, rect.bottom() - 2.0),
                                    egui::pos2(rect.right() - 2.0, rect.bottom() - offset),
                                ],
                                grip_stroke,
                            );
                        }
                    } else {
                        painter.rect_stroke(grip_rect.shrink(3.0), 2.0, grip_stroke);
                    }
                }
                if grip.hovered() || grip.dragged() {
                    ctx.set_cursor_icon(cursor);
                }
                if grip.drag_started() {
                    self.grip_start = Some(egui::vec2(
                        self.config.width as f32,
                        self.config.height as f32,
                    ));
                }
                if grip.dragged() {
                    if let Some(size) = self.grip_start.as_mut() {
                        let d = grip.drag_delta();
                        size.x += d.x * sx;
                        size.y += d.y * sy;
                        let available = if self.config.rotation.is_vertical() {
                            size.y
                        } else {
                            size.x
                        };
                        self.fit_to_width(available);
                    }
                }
                if grip.drag_stopped() {
                    self.grip_start = None;
                    self.persist();
                }
                if grip.dragged() || grip.drag_started() {
                    return;
                }
            }
        }

        if response.double_clicked() && self.config.mode != Mode::Clock {
            self.toggle();
        }

        if !self.config.window_locked {
            if response.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }

        // The menu is its own OS window rather than an egui `context_menu`,
        // which would be clipped to the overlay's small viewport.
        let clicked_at = response
            .secondary_clicked()
            .then(|| response.interact_pointer_pos())
            .flatten();
        if let Some(pos) = clicked_at {
            // `interact_pointer_pos` is viewport-local; the window wants
            // monitor coordinates.
            let origin = ctx
                .input(|i| i.viewport().outer_rect.map(|r| r.min))
                .unwrap_or(egui::Pos2::ZERO);
            crate::menu::open(origin + pos.to_vec2());
        }
    }

    /// Draw the context menu (if open) and act on the user's choice.
    fn context_menu(&mut self, ctx: &egui::Context) {
        crate::menu::show(ctx, &self.config);

        let Some(choice) = crate::menu::take_choice() else {
            return;
        };
        let Some(action) = choice else {
            return;
        };
        use crate::menu::MenuAction;
        match action {
            MenuAction::StartPause => self.toggle(),
            MenuAction::Reset => self.reset(),
            MenuAction::Lap => self.trigger_lap(),
            MenuAction::AddMinute => self.adjust_time(60.0),
            MenuAction::SubMinute => self.adjust_time(-60.0),
            MenuAction::CopyTime => {
                if !self.snapshot.text.is_empty() {
                    ctx.copy_text(self.snapshot.text.clone());
                }
            }
            MenuAction::MiniMode => self.toggle_mini_mode(),
            MenuAction::GhostMode => self.toggle_ghost(),
            MenuAction::LoadProfile(name) => self.apply_profile(&name),
            MenuAction::Settings => self.settings.open = true,
            MenuAction::Exit => self.request_quit(ctx),
        }
    }
}

/// Extract the Win32 window handle from the eframe frame.
fn window_handle(frame: &eframe::Frame) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = frame.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Some(win32.hwnd.get()),
        _ => None,
    }
}

pub fn run() -> eframe::Result<()> {
    let config = config::load();
    let (x, y) = if config.pos_x >= 0 && config.pos_y >= 0 {
        (config.pos_x as f32, config.pos_y as f32)
    } else {
        default_position(&config)
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_visible(!config.start_minimized)
            .with_inner_size([config.width as f32, config.height as f32])
            .with_position([x, y])
            .with_resizable(false)
            .with_taskbar(false)
            .with_title(APP_NAME)
            // Prefer the themed vector icon so the window keeps matching the
            // selected theme, and fall back to the bundled .ico file.
            .with_icon(load_icon(&config)),
        renderer: eframe::Renderer::Glow,
        vsync: true,
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(TimerApp::new(cc)))),
    )
}

/// Top-right of the primary screen's work area, inset by a small margin.
fn default_position(config: &Config) -> (f32, f32) {
    let (left, top, screen_w, _) = platform::primary_work_area(1.0);
    let width = config.width as f32;
    ((left + screen_w - width - 24.0).max(left), top + 24.0)
}

fn load_icon(config: &Config) -> egui::IconData {
    // The standalone release download is often just the .exe with no .ico next
    // to it, so never rely on that file existing: draw the themed icon first,
    // then try the bundled .ico, then an empty placeholder.
    let path = crate::resource_path(ICON_FILENAME);
    if path.exists() {
        match image::open(&path) {
            Ok(image) => {
                let image = image.to_rgba8();
                let (width, height) = image.dimensions();
                return egui::IconData {
                    rgba: image.into_raw(),
                    width,
                    height,
                };
            }
            Err(err) => log::warn!("Could not load {ICON_FILENAME}: {err}"),
        }
    }

    crate::render::themed_window_icon(config)
}
