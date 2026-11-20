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
    show_window: bool,
    quit: bool,
    /// Size at which the current grip drag began (logical px).
    grip_start: Option<egui::Vec2>,
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

        let tray = Tray::new();
        if tray.is_available() == false {
            log::info!("Running without a tray icon");
        }

        let textures = render::load_textures(&cc.egui_ctx, &config.sprite_path);
        let engine = TimerEngine::new(crate::engine::RealClock);
        let snapshot = Snapshot::default();
        let display_color = render::target_color(&config, ColorRole::Normal);

        let update_rx = if config.auto_check_updates {
            Some(updater::spawn_check(APP_VERSION.to_string()))
        } else {
            None
        };

        TimerApp {
            show_window: !config.start_minimized,
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
            quit: false,
            grip_start: None,
        }
    }

    // -- actions ---------------------------------------------------------
    pub fn toggle(&mut self) {
        if self.config.mode == Mode::Clock {
            return;
        }
        self.engine.toggle(&self.config);
        self.tick();
    }

    pub fn reset(&mut self) {
        self.engine.reset();
        self.tick();
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

    pub fn persist(&self) {
        if let Err(err) = config::save(&self.config) {
            log::warn!("Settings could not be persisted: {err}");
        }
    }

    pub fn reload_assets(&mut self, ctx: &egui::Context) {
        self.textures = render::load_textures(ctx, &self.config.sprite_path);
    }

    pub fn apply_appearance(&mut self) {
        self.config.font_family = self.config.font_family.clone();
        self.sync_platform();
        if self.config.rotation.is_vertical() {
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
            8.0,
            self.config.font_size.max(8) as f32 * 3.0,
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

        // Auto-pause when a target window is configured and focus leaves it.
        if self.engine.running() && self.config.target_hwnd != 0 {
            let current = platform::foreground_window();
            if current != 0 && current != self.config.target_hwnd {
                self.engine.pause();
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
        if snapshot.text_length_changed {
            self.config.width = self.config.width.max(1);
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
            }
        }
        for command in self.tray.poll() {
            match command {
                TrayCommand::Toggle => self.toggle(),
                TrayCommand::Reset => self.reset(),
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

        // Escape quits, matching the original.
        if self.show_window
            && !self.config.ghost_mode
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.request_quit(ctx);
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
                self.update_available = Some(info);
            }
            self.update_rx = None;
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
                let frame = render::Frame {
                    config: &self.config,
                    snapshot: &self.snapshot,
                    color: self.display_color,
                    textures: &self.textures,
                    font_size: self.font_size,
                    family,
                    fluid_phase: self.engine.fluid_phase,
                };
                let mut painter = ui.painter().clone();
                painter.set_opacity(self.config.opacity);
                render::draw(ctx, &painter, rect, &frame);

                self.overlay_interaction(ctx, ui, rect);
            });

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
        // available width instead of just cropping it, matching the original.
        // Allocated after the body rect so it wins the drag.
        if !self.config.window_locked {
            let grip_size = 16.0;
            let grip_rect = egui::Rect::from_min_max(
                egui::pos2(rect.right() - grip_size, rect.bottom() - grip_size),
                rect.max,
            );
            let grip = ui.interact(
                grip_rect,
                ui.id().with("size-grip"),
                egui::Sense::drag(),
            );
            if grip.hovered() {
                ctx.set_cursor_icon(if self.config.rotation.is_vertical() {
                    egui::CursorIcon::ResizeVertical
                } else {
                    egui::CursorIcon::ResizeHorizontal
                });
            }
            if grip.drag_started() {
                self.grip_start = Some(egui::vec2(
                    self.config.width as f32,
                    self.config.height as f32,
                ));
            }
            if grip.dragged() {
                if let Some(start) = self.grip_start {
                    let delta = grip.drag_delta();
                    let available = if self.config.rotation.is_vertical() {
                        start.y + delta.y
                    } else {
                        start.x + delta.x
                    };
                    self.fit_to_width(available);
                }
            }
            if grip.drag_stopped() {
                self.grip_start = None;
                self.persist();
            }
            if grip.dragged() || grip.drag_started() {
                // The grip owns this drag; don't also drag the window.
                return;
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

        response.context_menu(|ui| {
            if ui.button("Start / Pause").clicked() {
                self.toggle();
                ui.close_menu();
            }
            if ui.button("Reset").clicked() {
                self.reset();
                ui.close_menu();
            }
            if ui.button("Lap").clicked() {
                self.trigger_lap();
                ui.close_menu();
            }
            ui.separator();
            let mut ghost = self.config.ghost_mode;
            if ui.checkbox(&mut ghost, "Ghost Mode").changed() {
                self.toggle_ghost();
            }
            if ui.button("Settings\u{2026}").clicked() {
                self.settings.open = true;
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                self.request_quit(ctx);
            }
        });
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
            .with_icon(load_icon()),
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

/// Top-right of the primary screen, inset by a small margin.
fn default_position(config: &Config) -> (f32, f32) {
    let width = config.width as f32;
    // 1920x1080 is the safe assumption without querying the monitor API here;
    // the window clamps itself onto the screen regardless.
    (1920.0 - width - 24.0, 24.0)
}

fn load_icon() -> egui::IconData {
    let path = crate::resource_path(ICON_FILENAME);
    match image::open(&path) {
        Ok(image) => {
            let image = image.to_rgba8();
            let (width, height) = image.dimensions();
            egui::IconData {
                rgba: image.into_raw(),
                width,
                height,
            }
        }
        Err(err) => {
            log::warn!("Could not load {ICON_FILENAME}: {err}");
            egui::IconData {
                rgba: vec![0; 4],
                width: 1,
                height: 1,
            }
        }
    }
}
