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
/// Radians per second the fluid surface travels. Chosen to match the old
/// fixed 0.25-radians-per-frame step on a 60 Hz display.
const FLUID_SPEED: f32 = 15.0;

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
    /// Time of the previous frame, for frame-rate independent animation.
    last_frame: Instant,
    /// A left-click that is waiting out the double-click window before it opens
    /// the note, so a double-click (pause) never also opens the note.
    note_click_at: Option<Instant>,
    /// When an edit in the note window should be written to disk. Kept short so
    /// a crash loses at most a moment's typing, but long enough that a burst of
    /// keystrokes does not rewrite the settings file on every character.
    note_save_at: Option<Instant>,
    /// Whether the first tick has produced text. Before that the geometry is
    /// whatever the config says, and measuring empty text would collapse it.
    measured_once: bool,
    /// The size the user last chose, directly or through Settings. The automatic
    /// text hug may shrink back toward this but never below it, so a short time
    /// such as `0` cannot collapse the window.
    hug_floor: egui::Vec2,
    /// When the last show/hide request was sent, so the OS-visibility poll does
    /// not read the old state before egui applies the change.
    visibility_requested: Option<Instant>,
}

#[derive(Default, Clone, PartialEq)]
struct AppliedState {
    hwnd: isize,
    blur: bool,
    ghost: bool,
    vertical: bool,
}

impl TimerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Capture this before `load`, which may write a settings file out.
        let first_run = config::is_first_run();
        let mut config = config::load();
        fonts::install(&cc.egui_ctx, &config.font_family);

        let mut hotkeys = HotkeyManager::new();
        hotkeys.rebind(&config);

        let tray = Tray::new(&config);
        if !tray.is_available() {
            log::info!("Running without a tray icon");
        }

        let textures = render::load_textures(&cc.egui_ctx, &config.sprite_path);
        let mut engine = TimerEngine::new(crate::engine::RealClock);
        if config.auto_start_on_launch && !matches!(config.mode, Mode::Clock | Mode::TargetTime) {
            engine.start(&config);
        }
        // Start from the real current text rather than the `00:00.00` placeholder,
        // so the first frame sizes the box to what will actually be shown.
        let snapshot = Snapshot {
            text: engine.current_text(&config),
            ..Snapshot::default()
        };
        let display_color = render::target_color(&config, ColorRole::Normal);

        let update_rx = if config.auto_check_updates {
            Some(updater::spawn_check(APP_VERSION.to_string(), config.include_prereleases))
        } else {
            None
        };

        // On a brand-new install there is nothing to discover the app with, so
        // show a one-time tip pointing at the right-click menu. The settings
        // file is written now as well: otherwise a user who never changes
        // anything has no file and every launch would look like a first run.
        let settings = SettingsState::default();
        if first_run {
            log::info!("First run: showing the right-click tip");
            config.show_first_run_tip = true;
            if let Err(err) = config::save(&config) {
                log::warn!("Could not create the settings file: {err}");
            }
        }

        TimerApp {
            show_window: !config.start_minimized || !tray.is_available(),
            font_size: config.font_size as f32,
            hug_floor: egui::vec2(config.width as f32, config.height as f32),
            config,
            engine,
            snapshot,
            settings,
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
            last_frame: Instant::now(),
            note_click_at: None,
            note_save_at: None,
            measured_once: false,
            visibility_requested: None,
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
        // The Settings window is a child viewport, and egui's input calls read
        // whichever viewport is current, so this must name the overlay.
        let (outer, inner) = root_input(&ctx, |i| {
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

    /// Resize the box after a setting that changes its shape (edge distance,
    /// outline, shadow, font). Vertical rotation is handled there too.
    pub fn refresh_geometry(&mut self) {
        self.resize_for_text();
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

        let desired = AppliedState {
            hwnd: self.hwnd,
            blur,
            ghost: self.config.ghost_mode,
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

    /// Size the window to the current text, shrinking or growing as needed.
    ///
    /// Used when the user asked for a new size, with a grip or through a
    /// setting, so it also records that size as the new floor.
    pub fn resize_for_text(&mut self) {
        self.fit_text_box(false);
        self.hug_floor = egui::vec2(self.config.width as f32, self.config.height as f32);
    }

    /// Hug the text while the timer runs, shrinking back toward the size the
    /// user chose but never below it.
    ///
    /// Shrinking without a floor is what made the window collapse: a timer
    /// showing `0` (paused, just reset, or merely launched) measures as a single
    /// character, so the box fell to its 80 px minimum and that tiny size was
    /// then saved over the user's own. The floor keeps the box at least as large
    /// as the size that was actually picked.
    fn hug_text(&mut self) {
        self.fit_text_box(true);
    }

    fn fit_text_box(&mut self, respect_floor: bool) {
        let Some(ctx) = CURRENT_CTX.with(|cell| cell.borrow().clone()) else {
            return;
        };
        let text = if self.snapshot.text.is_empty() {
            self.engine.current_text(&self.config)
        } else {
            self.snapshot.text.clone()
        };
        let family = fonts::timer_family(&ctx, &self.config.font_family);
        let measured = render::text_box_metrics(
            &ctx,
            &family,
            &text,
            self.font_size,
            self.edge_gap(),
            self.snapshot.lap_visible,
        );
        // Before the first tick the timer has no text yet. Measuring that
        // would collapse the window to its minimum, so leave whatever geometry
        // was saved from the last session alone until there is a snapshot.
        if !self.measured_once && text.is_empty() {
            return;
        }
        self.measured_once = true;
        let floor = respect_floor.then_some(self.hug_floor);
        let (width, height) = sized_box(
            measured,
            floor,
            self.config.rotation.is_vertical(),
        );
        if (width - self.config.width as f32).abs() > 1.0
            || (height - self.config.height as f32).abs() > 1.0
        {
            self.config.width = width.round() as u32;
            self.config.height = height.round() as u32;
            self.resize_window(&ctx, egui::vec2(width, height), true);
        }
    }

    /// Distance from the digit ink to the panel edge: the padding setting plus
    /// whatever the outline and shadow need so nothing is clipped.
    fn edge_gap(&self) -> f32 {
        render::edge_gap(&self.config)
    }

    /// Resize the window. `recentre` keeps the centre fixed, so a leading zero
    /// disappearing makes the box shrink toward the middle rather than jump to
    /// one side; a grip drag keeps its anchor corner instead.
    fn resize_window(&mut self, ctx: &egui::Context, size: egui::Vec2, recentre: bool) {
        // The Settings window draws in a child viewport, and there the current
        // viewport is *that* window: an unpinned command would shrink the
        // dialog instead of the overlay. Name the overlay explicitly.
        let outer = root_input(ctx, |i| i.viewport().outer_rect);
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::InnerSize(size));
        let Some(outer) = outer.filter(|_| recentre) else {
            return;
        };
        // `InnerSize` excludes the window frame, so the change is split evenly
        // around the old centre.
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::OuterPosition(egui::pos2(
                outer.center().x - size.x / 2.0,
                outer.center().y - size.y / 2.0,
            )),
        );
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
        let padding = self.edge_gap();
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
            self.edge_gap(),
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
        // The grip is the user choosing a size, so the text hug must not shrink
        // below it later.
        self.hug_floor = egui::vec2(width, height);
        self.resize_window(&ctx, egui::vec2(width, height), false);
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
        // Adopt the new snapshot before any resize: measuring the previous
        // frame's text would size the box for the digits that have just gone,
        // leaving it a step stale (and stuck at one character after the timer
        // hit zero, which is what made the window look tiny).
        self.snapshot = snapshot;

        if self.snapshot.text_length_changed
            && self.config.display_style == crate::config::DisplayStyle::Text
            && !self.config.window_locked
        {
            // Like the original, the window hugs the text as its length changes.
            // Other styles own their own geometry, so they are left alone. The
            // hug grows for longer text but never shrinks below the chosen size.
            self.hug_text();
        }
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
                    let next = !self.show_window;
                    self.set_window_visible(ctx, next);
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
                    let next = !self.show_window;
                    self.set_window_visible(ctx, next);
                }
                TrayCommand::Exit => self.request_quit(ctx),
            }
        }

        // Escape hides the overlay; it does not quit, so a stray keypress can
        // never end a running timer. Quit stays on the tray menu and the
        // settings dialog. Space toggles, matching the original. While the
        // menu or the note is open it owns the keyboard, so presses must not
        // leak through.
        if self.show_window
            && !self.config.ghost_mode
            && !crate::menu::is_open()
            && !crate::note::is_open()
        {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                // Hiding is only safe with a tray icon to bring it back.
                if self.tray.is_available() {
                    self.set_window_visible(ctx, false);
                }
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

    /// Show or hide the overlay, remembering when it was asked for so the
    /// OS-visibility poll does not fight the change.
    fn set_window_visible(&mut self, ctx: &egui::Context, visible: bool) {
        self.show_window = visible;
        self.visibility_requested = Some(Instant::now());
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(visible));
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

/// Read input from the overlay window.
///
/// `Context::input` reads whichever viewport is currently drawing. Settings and
/// the note are child viewports, so code that runs while they are open must use
/// this to see the overlay's geometry instead of theirs.
pub fn root_input<R>(ctx: &egui::Context, reader: impl FnOnce(&egui::InputState) -> R) -> R {
    ctx.input_for(egui::ViewportId::ROOT, reader)
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

        // Something outside egui can show the window again (a second launch
        // calls `ShowWindow`). Adopt the real state so keys and clicks work.
        // Skip the frame or two right after we ask for a change ourselves,
        // while the OS still reports the previous state.
        let settled = match self.visibility_requested {
            Some(at) => at.elapsed() > Duration::from_millis(400),
            None => true,
        };
        if settled && self.hwnd != 0 && platform::is_window_visible(self.hwnd) {
            self.visibility_requested = None;
            if !self.show_window {
                log::debug!("Window was shown externally; syncing visible state");
                self.show_window = true;
            }
        }

        self.poll_updates();
        self.poll_install();
        if let Some(info) = take_pending_install() {
            self.install_update(&info);
        }
        self.handle_input(ctx);
        self.tick();
        self.display_color = self.current_color();
        self.sync_platform();

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

                // Drawn last so its click is registered above the overlay's
                // drag/click surface. Any click dismisses it, as does using
                // the timer, so it never lingers.
                if self.config.show_first_run_tip
                    && !self.config.ghost_mode
                    && (crate::hint::show(ui, rect, &self.config, alpha_scale)
                        || ctx.input(|i| i.pointer.any_click()))
                {
                    self.config.show_first_run_tip = false;
                    self.persist();
                }
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
        crate::note::show(ctx, &self.config);
        self.poll_note_edit();

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
            || self.fade_started.is_some()
            || self.config.show_first_run_tip;
        ctx.request_repaint_after(if animated { TICK } else { IDLE_TICK });

        if self.config.display_style == crate::config::DisplayStyle::Fluid
            && self.config.mode != Mode::Clock
        {
            // Advance by wall-clock time rather than a fixed step per frame, so
            // the wave moves at a steady speed whatever the refresh rate is (a
            // 144 Hz monitor used to run the wave six times faster than 24 Hz).
            let now = Instant::now();
            let delta = now
                .saturating_duration_since(self.last_frame)
                .as_secs_f32()
                .min(0.25);
            self.last_frame = now;
            if self.config.reduce_motion {
                // Respect the accessibility setting: keep the surface still.
                self.engine.fluid_phase = 0.0;
            } else {
                self.engine.fluid_phase =
                    (self.engine.fluid_phase + delta * FLUID_SPEED).rem_euclid(std::f32::consts::TAU);
            }
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

        // A double-click always cancels a pending note open, even in clock mode
        // where it has no other effect.
        if response.double_clicked() {
            self.note_click_at = None;
            if self.config.mode != Mode::Clock {
                self.toggle();
            }
        }

        if !self.config.window_locked && response.drag_started() {
            // Dragging is not a note request.
            self.note_click_at = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        // A single left-click opens the note, but a double-click means pause, so
        // the open waits out egui's own double-click window (300 ms) first. A
        // second click in that time cancels it and the note never appears.
        //
        // A click that just dismissed the note is ignored here, or the same
        // click would close the window and immediately reopen it.
        if response.clicked() && !response.double_clicked() {
            if crate::note::is_open() {
                // The note only leaves on a click on the timer, so it goes at
                // once rather than waiting out the double-click window.
                self.note_click_at = None;
                self.toggle_note(ctx, rect);
            } else if !crate::note::just_dismissed() {
                self.note_click_at = Some(Instant::now());
                ctx.request_repaint_after(std::time::Duration::from_millis(340));
            }
        }
        if let Some(at) = self.note_click_at {
            if at.elapsed() >= std::time::Duration::from_millis(320) {
                self.note_click_at = None;
                self.toggle_note(ctx, rect);
            }
        }

        // The menu is its own OS window rather than an egui `context_menu`,
        // which would be clipped to the overlay's small viewport.
        let clicked_at = response
            .secondary_clicked()
            .then(|| response.interact_pointer_pos())
            .flatten();
        if let Some(pos) = clicked_at {
            // Right-clicking the timer is exactly what the first-run tip asks
            // for, so opening the menu counts as having read it.
            self.config.show_first_run_tip = false;
            // `interact_pointer_pos` is viewport-local; the window wants
            // monitor coordinates.
            let origin = ctx
                .input(|i| i.viewport().outer_rect.map(|r| r.min))
                .unwrap_or(egui::Pos2::ZERO);
            crate::menu::open(origin + pos.to_vec2());
            self.persist();
        }
    }

    /// Show or hide the note window. The note itself lives in the config, so it
    /// is already loaded and saved with every other setting.
    fn toggle_note(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        if crate::note::is_open() {
            crate::note::close();
            ctx.request_repaint();
            return;
        }
        // The click that dismissed the note must not open it again.
        if crate::note::just_dismissed() {
            return;
        }
        // `rect` is viewport-local; placement needs monitor coordinates.
        let origin = ctx
            .input(|i| i.viewport().outer_rect.map(|r| r.min))
            .unwrap_or(egui::Pos2::ZERO);
        let anchor = rect.translate(origin.to_vec2());
        crate::note::open(anchor, &self.config, ctx.pixels_per_point());
    }

    /// Persist the note text edited in the note window.
    ///
    /// The text is adopted immediately so everything else already sees it; the
    /// write to disk waits for a short pause in typing, and for the window
    /// closing.
    fn poll_note_edit(&mut self) {
        if let Some(text) = crate::note::take_edit() {
            if self.config.note != text {
                self.config.note = text;
                self.note_save_at =
                    Some(Instant::now() + std::time::Duration::from_millis(600));
            }
        }
        match self.note_save_at {
            Some(at) if Instant::now() >= at => {
                self.note_save_at = None;
                self.persist();
            }
            Some(_) => {
                if let Some(ctx) = self::current_context() {
                    ctx.request_repaint_after(std::time::Duration::from_millis(120));
                }
            }
            None => {}
        }
        // Closing the window must not wait out the debounce.
        if !crate::note::is_open() && self.note_save_at.is_some() {
            self.note_save_at = None;
            self.persist();
        }
        // A resize is committed as soon as it happens, so the note reopens at
        // the size it was left at.
        if let Some((width, height)) = crate::note::take_size() {
            if self.config.note_width != width || self.config.note_height != height {
                self.config.note_width = width;
                self.config.note_height = height;
                self.persist();
            }
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
    // A second launch (double-clicking the exe again) should reveal the copy
    // that is already running instead of stacking a duplicate overlay and tray
    // icon on top of it.
    if !platform::claim_single_instance() {
        if platform::focus_existing_window(APP_NAME) {
            log::info!("Another instance is already running; focusing it");
        } else {
            log::info!("Another instance is already running; exiting");
        }
        return Ok(());
    }

    let config = config::load();
    let (x, y) = initial_position(&config);

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

/// Whether a window at `(x, y)` with this size overlaps `area` enough to be
/// usable. Split out so the off-screen recovery rule can be tested.
fn is_on_screen(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    area: (f32, f32, f32, f32),
) -> bool {
    let (left, top, screen_w, screen_h) = area;
    // Keep a comfortable band of the window on screen, not just one pixel.
    const MARGIN: f32 = 48.0;
    let visible_x = x + width > left + MARGIN && x < left + screen_w - MARGIN;
    let visible_y = y + height > top + MARGIN && y < top + screen_h - MARGIN;
    visible_x && visible_y
}

/// Where to put the overlay on launch.
///
/// A saved position is reused, but only when enough of the window would still
/// be visible: unplugging a second monitor (or changing resolution) otherwise
/// leaves the overlay stranded off-screen where it can never be dragged back.
fn initial_position(config: &Config) -> (f32, f32) {
    if config.pos_x < 0 || config.pos_y < 0 {
        return default_position(config);
    }
    let (x, y) = (config.pos_x as f32, config.pos_y as f32);
    let area = platform::primary_work_area(1.0);
    if is_on_screen(x, y, config.width as f32, config.height as f32, area) {
        (x, y)
    } else {
        log::info!("Saved window position is off-screen; moving it back into view");
        default_position(config)
    }
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

/// The window size for measured `text` ink.
///
/// `floor` is the size the user last chose, when the caller wants the automatic
/// hug kept at or above it. The result is never below the usable minimum.
fn sized_box(measured: egui::Vec2, floor: Option<egui::Vec2>, vertical: bool) -> (f32, f32) {
    let (mut width, mut height) = (measured.x, measured.y);
    if vertical {
        std::mem::swap(&mut width, &mut height);
    }
    if let Some(floor) = floor {
        // The floor is stored in window orientation, so it is applied after the
        // rotation swap.
        width = width.max(floor.x);
        height = height.max(floor.y);
    }
    (width.max(80.0), height.max(40.0))
}

#[cfg(test)]
mod tests {
    use super::{is_on_screen, sized_box};

    const SCREEN: (f32, f32, f32, f32) = (0.0, 0.0, 1920.0, 1080.0);

    #[test]
    fn window_inside_the_screen_is_kept() {
        assert!(is_on_screen(100.0, 100.0, 300.0, 120.0, SCREEN));
        assert!(is_on_screen(1600.0, 900.0, 300.0, 120.0, SCREEN));
    }

    #[test]
    fn window_on_a_removed_monitor_is_rejected() {
        // Saved when a second screen sat to the right of the primary.
        assert!(!is_on_screen(2400.0, 200.0, 300.0, 120.0, SCREEN));
        // Saved above the screen after the layout changed.
        assert!(!is_on_screen(200.0, -400.0, 300.0, 120.0, SCREEN));
    }

    #[test]
    fn tiny_sliver_at_the_edge_is_rejected() {
        // Only 20 px pokes onto the screen, less than the 48 px margin.
        assert!(!is_on_screen(-280.0, 200.0, 300.0, 120.0, SCREEN));
        assert!(!is_on_screen(200.0, 1060.0, 300.0, 120.0, SCREEN));
    }

    #[test]
    fn negative_coordinates_are_handled() {
        // A screen to the left of the primary has negative coordinates.
        let multi: (f32, f32, f32, f32) = (-1920.0, 0.0, 1920.0, 1080.0);
        assert!(is_on_screen(-500.0, 100.0, 300.0, 120.0, multi));
    }

    #[test]
    fn automatic_hug_never_shrinks_below_the_chosen_size() {
        // The user's own window is 125x53. A paused or zeroed timer measures as
        // one character (about 27x50 plus the gap). Hugging that used to drop
        // the box to the 80 px minimum and save it, which is the "tiny window".
        let chosen = egui::vec2(125.0, 53.0);
        let one_char = egui::vec2(27.0, 50.0);
        assert_eq!(sized_box(one_char, Some(chosen), false), (125.0, 53.0));
    }

    #[test]
    fn automatic_hug_still_grows_for_longer_text() {
        let chosen = egui::vec2(125.0, 53.0);
        let longer = egui::vec2(240.0, 53.0);
        assert_eq!(sized_box(longer, Some(chosen), false), (240.0, 53.0));
    }

    #[test]
    fn explicit_resize_may_shrink_to_the_measurement() {
        // Without a floor (a grip or Settings change), the measured size wins.
        let tiny = egui::vec2(27.0, 50.0);
        assert_eq!(sized_box(tiny, None, false), (80.0, 50.0));
    }

    #[test]
    fn floor_applies_after_rotation() {
        // Vertical windows swap the axes, and the floor is already in window
        // orientation, so it must be compared against the swapped size.
        let chosen = egui::vec2(125.0, 53.0);
        let measured = egui::vec2(200.0, 60.0);
        assert_eq!(sized_box(measured, Some(chosen), true), (125.0, 200.0));
    }

    /// Commands a viewport was asked for during one frame, keyed by viewport.
    fn commands_after(
        ctx: &egui::Context,
        input: egui::RawInput,
        f: impl FnOnce(&egui::Context),
    ) -> egui::ViewportIdMap<Vec<egui::ViewportCommand>> {
        let mut f = Some(f);
        let output = ctx.run(input, |ctx| {
            if let Some(f) = f.take() {
                f(ctx);
            }
        });
        output
            .viewport_output
            .into_iter()
            .map(|(id, viewport)| (id, viewport.commands))
            .collect()
    }

    fn inner_size(commands: &[egui::ViewportCommand]) -> Option<egui::Vec2> {
        commands.iter().find_map(|command| match command {
            egui::ViewportCommand::InnerSize(size) => Some(*size),
            _ => None,
        })
    }

    #[test]
    fn geometry_from_settings_targets_the_overlay_not_the_dialog() {
        // The quick-adjust buttons live in Settings, which draws as its own
        // viewport. There, a plain `send_viewport_cmd` resizes *Settings*: that
        // is what made the timer look like it had shrunk, and moved the dialog.
        let child = egui::ViewportId::from_hash_of("settings");
        let size = egui::vec2(321.0, 89.0);

        let mut child_input = egui::RawInput {
            viewport_id: child,
            ..Default::default()
        };
        child_input.viewports.insert(child, egui::ViewportInfo::default());

        // Unpinned (the old behaviour): the command follows the current window.
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let out = commands_after(&ctx, child_input.clone(), |ctx| {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        });
        assert_eq!(
            inner_size(out.get(&child).map(Vec::as_slice).unwrap_or(&[])),
            Some(size),
            "an unpinned command from Settings hits Settings"
        );

        // Pinned to ROOT (what the app now does): the overlay is targeted.
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let out = commands_after(&ctx, child_input, |ctx| {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::InnerSize(size));
        });
        assert_eq!(
            inner_size(out.get(&egui::ViewportId::ROOT).map(Vec::as_slice).unwrap_or(&[])),
            Some(size),
            "pinning to ROOT resizes the overlay"
        );
        assert_ne!(
            inner_size(out.get(&child).map(Vec::as_slice).unwrap_or(&[])),
            Some(size),
            "Settings must not be resized"
        );
    }

    #[test]
    fn root_input_reads_the_overlay_geometry_from_settings() {
        // Saving from Settings used to record the *dialog's* size into the
        // timer's config, which also shrank the overlay on the next launch.
        let child = egui::ViewportId::from_hash_of("settings");
        let ctx = egui::Context::default();

        let mut root_input = egui::RawInput::default();
        root_input.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::new(1200.0, 300.0),
            egui::vec2(125.0, 53.0),
        ));
        let _ = ctx.run(root_input, |_| {});

        let mut child_input = egui::RawInput {
            viewport_id: child,
            ..Default::default()
        };
        child_input.viewports.insert(child, egui::ViewportInfo::default());
        child_input.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::new(200.0, 100.0),
            egui::vec2(560.0, 720.0),
        ));

        let mut current = egui::Rect::NOTHING;
        let mut root_seen = egui::Rect::NOTHING;
        let _ = ctx.run(child_input, |ctx| {
            // Both reads happen *inside* the child's frame, which is exactly
            // when the settings quick-adjust buttons run.
            current = ctx.input(|i| i.screen_rect());
            root_seen = super::root_input(ctx, |i| i.screen_rect());
        });
        // The child sees its own size; `root_input` still sees the overlay's.
        assert_eq!(current.width(), 560.0);
        assert_eq!(root_seen.width(), 125.0);
        assert_eq!(root_seen.height(), 53.0);
    }
}
