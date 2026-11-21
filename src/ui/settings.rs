//! Settings window.
//!
//! Edits a working copy of the configuration so a partially typed value can
//! never leave the live overlay half-updated. `Save & Apply` commits; `Cancel`
//! discards. Every committed change re-applies appearance immediately, matching
//! the live preview the original app had.

use crate::app::TimerApp;
use crate::config::{
    BorderStyle, Config, DisplayStyle, Hotkey, Mode, Rgb, Rotation, Theme, WindowShape,
};
use crate::engine::format_clock;
use crate::fonts;
use crate::formatting::{format_input, parse_duration};
use crate::hotkeys::{capture_next, Action};
use crate::platform;
use crate::updater;
use crate::{APP_NAME, APP_VERSION};
use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Appearance,
    Behavior,
    Hotkeys,
    About,
}

pub struct SettingsState {
    pub open: bool,
    tab: Tab,
    /// Working copy, initialised when the window opens.
    draft: Config,
    /// Live configuration as it was when the dialog opened, restored on
    /// Cancel so live previews are undone.
    original: Config,
    /// The last configuration pushed to the running overlay, so unchanged
    /// frames do not re-apply (and fight the resize grip).
    live: Option<Config>,
    /// Text fields that hold unparsed user input.
    text: TextFields,
    /// Key capture receiver, when recording a hotkey.
    recording: Option<(Action, std::sync::mpsc::Receiver<(String, u32)>)>,
    /// Pending file-dialog kind, consumed on the next frame.
    file_dialog: Option<FileDialogKind>,
    /// Pending snap request, consumed on the next frame.
    pending_snap: Option<SnapTarget>,
    /// Prompt shown before restoring defaults.
    confirm_defaults: bool,
    hotkey_message: Option<String>,
    /// Typed input is not valid yet; the last valid preview is kept.
    invalid_hint: bool,
    update_busy: bool,
    /// Update found by the about tab.
    pending_update: Option<updater::UpdateInfo>,
    /// Installer URL the user asked to download.
    install_requested: Option<String>,
    /// Draft has been initialised from the live config.
    initialised: bool,
    /// Result of the delayed foreground-window capture.
    foreground_rx: Option<std::sync::mpsc::Receiver<u64>>,
    /// Name being typed for saving a new profile.
    new_profile_name: String,
    /// Brief feedback message for profile or import/export actions.
    io_status: Option<String>,
    /// Search filter for jumping to settings across tabs.
    search_query: String,
    /// Draft being edited in the Custom Theme Builder.
    theme_builder: crate::config::CustomTheme,
}

#[derive(Default)]
struct TextFields {
    countdown: String,
    clock_format: String,
    pomo_work: String,
    pomo_break: String,
    pomo_long: String,
    interval_work: String,
    interval_rest: String,
    target_time: String,
    threshold: String,
    sprite: String,
    sound: String,
    font_family: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileDialogKind {
    Sprite,
    Sound,
    ImportConfig,
    ExportConfig,
}

impl Default for SettingsState {
    fn default() -> Self {
        SettingsState {
            open: false,
            tab: Tab::General,
            draft: Config::default(),
            original: Config::default(),
            live: None,
            text: TextFields::default(),
            recording: None,
            file_dialog: None,
            pending_snap: None,
            confirm_defaults: false,
            hotkey_message: None,
            invalid_hint: false,
            update_busy: false,
            pending_update: None,
            install_requested: None,
            initialised: false,
            foreground_rx: None,
            new_profile_name: String::new(),
            io_status: None,
            search_query: String::new(),
            theme_builder: crate::config::CustomTheme::default(),
        }
    }
}

/// Outcome of drawing one frame of the settings window.
enum Outcome {
    None,
    Close,
    Apply,
    RestoreDefaults,
    Quit,
    CheckUpdates,
    Install(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SnapTarget {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// Draw the settings window for one frame.
///
/// A free function (rather than a method on the app) so the settings state can
/// be borrowed mutably alongside the app itself.
pub fn show_window(ctx: &egui::Context, state: &mut SettingsState, app: &mut TimerApp) {
    state.show(ctx, app);
}

impl SettingsState {
    pub fn open(&mut self, config: &Config) {
        self.open = true;
        self.draft = config.clone();
        self.original = config.clone();
        self.live = Some(config.clone());
        self.sync_text();
        self.stop_recording();
        self.confirm_defaults = false;
        self.hotkey_message = None;
        self.invalid_hint = false;
        self.pending_snap = None;
        self.install_requested = None;
        self.initialised = true;
    }

    pub fn show(&mut self, ctx: &egui::Context, app: &mut TimerApp) {
        if !self.open {
            return;
        }
        if !self.initialised {
            self.open(&app.config);
        }

        // Pick up the result of a background update check.
        if app.update_available.is_some() {
            self.pending_update = app.update_available.clone();
            self.update_busy = false;
        }

        self.poll_recording();
        self.poll_file_dialog();
        self.poll_install();
        self.poll_foreground();

        if let Some(target) = self.pending_snap.take() {
            self.snap(app, target);
        }

        let mut outcome = Outcome::None;

        // The settings dialog lives in its own native window. Drawing it into
        // the overlay's viewport would clip it to the timer's small rectangle.
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("settings-window"),
            egui::ViewportBuilder::default()
                .with_title(format!("{APP_NAME} Settings \u{2014} {APP_VERSION}"))
                .with_icon(std::sync::Arc::new(crate::render::themed_window_icon(
                    &self.draft,
                )))
                .with_inner_size([640.0, 720.0])
                .with_min_inner_size([520.0, 420.0])
                .with_resizable(true)
                .with_transparent(false)
                .with_always_on_top(),
            |ctx, _class| {
                ctx.style_mut(|s| s.interaction.tooltip_delay = 0.45);
                crate::render::apply_theme_visuals_for_config(ctx, &self.draft);
                let background = crate::render::ui_palette_for_config(&self.draft).window_bg;
                // Footer first so it is always visible, then the tab bar, and
                // the scroll area takes whatever height is left.
                egui::TopBottomPanel::bottom("settings-footer")
                    .frame(egui::Frame::none().fill(background).inner_margin(egui::Margin::same(8.0)))
                    .show(ctx, |ui| {
                        if let Some(message) = &self.hotkey_message {
                            ui.colored_label(egui::Color32::from_rgb(0xE0, 0xA0, 0x00), message);
                        }
                        if self.invalid_hint {
                            ui.colored_label(
                                egui::Color32::from_rgb(0xE0, 0xA0, 0x00),
                                "Some values are not valid yet and are being ignored.",
                            );
                        }
                        if self.confirm_defaults {
                            ui.horizontal(|ui| {
                                ui.label("Reset every setting to its default value?");
                                if ui.button("Yes, reset").clicked() {
                                    outcome = Outcome::RestoreDefaults;
                                    self.confirm_defaults = false;
                                }
                                if ui.button("Cancel").clicked() {
                                    self.confirm_defaults = false;
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Restore Defaults").clicked() {
                                self.confirm_defaults = true;
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Quit Timer").clicked() {
                                        outcome = Outcome::Quit;
                                    }
                                    let save = ui.button("Save & Apply");
                                    if save.clicked() {
                                        outcome = Outcome::Apply;
                                    }
                                    let cancel = ui.button("Cancel");
                                    if cancel.clicked() {
                                        outcome = Outcome::Close;
                                    }
                                },
                            );
                        });
                    });

                egui::TopBottomPanel::top("settings-tabs")
                    .frame(egui::Frame::none().fill(background).inner_margin(egui::Margin::same(8.0)))
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            self.tab_bar(ui);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if !self.search_query.is_empty() && ui.small_button("X").clicked() {
                                        self.search_query.clear();
                                    }
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.search_query)
                                            .desired_width(150.0)
                                            .hint_text("Search settings\u{2026}"),
                                    );
                                },
                            );
                        });
                    });

                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(background).inner_margin(egui::Margin::same(8.0)))
                    .show(ctx, |ui| {
                        if !self.search_query.trim().is_empty() {
                            self.search_results(ui);
                            return;
                        }
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let tab_outcome = match self.tab {
                                    Tab::General => {
                                        self.general_tab(ui, app);
                                        Outcome::None
                                    }
                                    Tab::Appearance => {
                                        self.appearance_tab(ui);
                                        Outcome::None
                                    }
                                    Tab::Behavior => {
                                        self.behavior_tab(ui, app);
                                        Outcome::None
                                    }
                                    Tab::Hotkeys => {
                                        self.hotkeys_tab(ui);
                                        Outcome::None
                                    }
                                    Tab::About => self.about_tab(ui),
                                };
                                if !matches!(tab_outcome, Outcome::None) {
                                    outcome = tab_outcome;
                                }
                            });
                    });

                // The title bar's close button behaves like Cancel.
                if ctx.input(|i| i.viewport().close_requested())
                    && matches!(outcome, Outcome::None)
                {
                    outcome = Outcome::Close;
                }
            },
        );

        // Reflect edits on the live overlay as they are made, before Save.
        if matches!(outcome, Outcome::None) {
            self.live_update(app);
        }

        match outcome {
            Outcome::None => {}
            Outcome::Close => {
                self.cancel(app);
                self.open = false;
                self.initialised = false;
                self.stop_recording();
            }
            // `Save & Apply` persists and closes the dialog, like the original's
            // `QDialog.accept()`. Restore Defaults deliberately stays open.
            Outcome::Apply => {
                self.commit(app);
                if self.hotkey_message.is_none() {
                    self.open = false;
                    self.initialised = false;
                    self.stop_recording();
                }
            }
            Outcome::RestoreDefaults => {
                self.draft = Config::default();
                self.sync_text();
                self.commit(app);
                app.reset();
            }
            Outcome::Quit => {
                app.request_quit(ctx);
                self.open = false;
            }
            Outcome::CheckUpdates => {
                self.update_busy = true;
                app.update_available = None;
                app.update_rx = Some(updater::spawn_check(APP_VERSION.to_string()));
            }
            Outcome::Install(url) => self.install_requested = Some(url),
        }
    }

    // -- tabs ------------------------------------------------------------
    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (tab, label) in [
                (Tab::General, "General"),
                (Tab::Appearance, "Appearance"),
                (Tab::Behavior, "Behavior"),
                (Tab::Hotkeys, "Hotkeys"),
                (Tab::About, "About"),
            ] {
                if ui.selectable_label(self.tab == tab, label).clicked() {
                    self.tab = tab;
                }
            }
        });
    }

    fn search_results(&mut self, ui: &mut egui::Ui) {
        let q = self.search_query.trim().to_lowercase();
        const ENTRIES: &[(&str, &str, Tab)] = &[
            ("Mode (Count Up, Countdown, Clock, Pomodoro, Interval, Target Time)", "Timer", Tab::General),
            ("Quick Adjust (+1m, -1m, +10s, -10s)", "Timer", Tab::General),
            ("Decimal precision", "Timer", Tab::General),
            ("Always show hours (HH:MM:SS)", "Timer", Tab::General),
            ("Hide leading zeros (compact digits)", "Timer", Tab::General),
            ("Auto-reset on zero (loop countdown)", "Timer", Tab::General),
            ("Warning beeps (10s, 3-2-1)", "Timer", Tab::General),
            ("Gray threshold & threshold time", "Timer", Tab::General),
            ("Pomodoro (work, break, cycles)", "Timer", Tab::General),
            ("Interval / HIIT (work, rest, rounds)", "Timer", Tab::General),
            ("Target Time (countdown to clock)", "Timer", Tab::General),
            ("Lap History & CSV Export", "Timer", Tab::General),
            ("Theme & Custom Theme Builder", "Appearance", Tab::Appearance),
            ("Custom label above timer", "Appearance", Tab::Appearance),
            ("Display style (Text, Arc, Bar, Fluid, Sprite)", "Appearance", Tab::Appearance),
            ("Show timer text", "Appearance", Tab::Appearance),
            ("Show background & background shape", "Appearance", Tab::Appearance),
            ("Corner radius & padding", "Appearance", Tab::Appearance),
            ("Border style & border thickness", "Appearance", Tab::Appearance),
            ("Native blur / acrylic", "Appearance", Tab::Appearance),
            ("Window opacity", "Appearance", Tab::Appearance),
            ("Font family & font size", "Appearance", Tab::Appearance),
            ("Text outline & text drop shadow", "Appearance", Tab::Appearance),
            ("Gradient fill & gradient end color", "Appearance", Tab::Appearance),
            ("Pulse on warning", "Appearance", Tab::Appearance),
            ("Compact mini-mode", "Appearance", Tab::Appearance),
            ("Hover-to-fade & fade opacity", "Appearance", Tab::Appearance),
            ("Normal & threshold colours", "Appearance", Tab::Appearance),
            ("Rotation (0 / 90 / 270)", "Behavior", Tab::Behavior),
            ("Lock position (prevent dragging)", "Behavior", Tab::Behavior),
            ("Auto-start on launch", "Behavior", Tab::Behavior),
            ("Edge snapping (magnetic screen edges)", "Behavior", Tab::Behavior),
            ("Start hidden (system tray)", "Behavior", Tab::Behavior),
            ("Reduce motion (disable animations)", "Behavior", Tab::Behavior),
            ("Ghost mode (click-through)", "Behavior", Tab::Behavior),
            ("Sound cue & custom WAV", "Behavior", Tab::Behavior),
            ("Auto-pause target app", "Behavior", Tab::Behavior),
            ("Snap position (6 presets)", "Behavior", Tab::Behavior),
            ("Right-Click Menu Items customizer", "Behavior", Tab::Behavior),
            ("Saved Profiles & Per-App Auto-Switch", "Behavior", Tab::Behavior),
            ("Import / Export Settings (JSON)", "Behavior", Tab::Behavior),
            ("Global Hotkeys (Start/Pause, Reset, Lap, Ghost, Show/Hide, Mini-Mode)", "Hotkeys", Tab::Hotkeys),
            ("Updates & About", "About", Tab::About),
        ];

        ui.label(egui::RichText::new(format!("Matches for \"{}\":", self.search_query.trim())).strong());
        ui.add_space(6.0);
        let mut found = 0;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (name, tab_name, tab) in ENTRIES {
                if name.to_lowercase().contains(&q) || tab_name.to_lowercase().contains(&q) {
                    found += 1;
                    ui.horizontal(|ui| {
                        if ui.button(format!("Open {tab_name}")).clicked() {
                            self.tab = *tab;
                            self.search_query.clear();
                        }
                        ui.label(*name);
                    });
                }
            }
            if found == 0 {
                ui.small("No matching settings found.");
            }
        });
    }

    fn general_tab(&mut self, ui: &mut egui::Ui, app: &mut TimerApp) {
        let mode = self.draft.mode;
        // Only the fields that apply to the current mode are shown, so the tab
        // stays focused on what the timer actually uses.
        egui::Grid::new("general")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Timer Mode");
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(mode_label(self.draft.mode))
                    .show_ui(ui, |ui| {
                        for option in Mode::ALL {
                            ui.selectable_value(&mut self.draft.mode, option, mode_label(option));
                        }
                    });
                ui.end_row();

                if mode == Mode::Down {
                    ui.label("Countdown Start");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.text.countdown)
                            .hint_text("e.g. 5:00, 90, or 0:01:30"),
                    );
                    ui.end_row();

                    ui.label("Quick presets");
                    ui.horizontal_wrapped(|ui| {
                        for (label, value) in [
                            ("1m", "1:00"),
                            ("5m", "5:00"),
                            ("10m", "10:00"),
                            ("15m", "15:00"),
                            ("25m", "25:00"),
                            ("30m", "30:00"),
                            ("1h", "1:00:00"),
                        ] {
                            if ui.small_button(label).clicked() {
                                self.text.countdown = value.to_string();
                            }
                        }
                    });
                    ui.end_row();
                }

                if mode == Mode::Clock {
                    tip_label(
                        ui,
                        "Clock Format",
                        "Time format string (e.g. %H:%M:%S for 24h or %I:%M %p for 12h).",
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.text.clock_format)
                            .hint_text("%H:%M:%S"),
                    );
                    ui.end_row();
                }

                if mode == Mode::TargetTime {
                    tip_label(
                        ui,
                        "Target Time",
                        "24-hour local time to count down to (e.g. 17:00 or 20:30:00).",
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.text.target_time)
                            .hint_text("17:00"),
                    );
                    ui.end_row();
                }

                if !matches!(mode, Mode::Clock | Mode::TargetTime) {
                    tip_label(
                        ui,
                        "Quick adjust",
                        "Add or subtract time on the fly without stopping the timer.",
                    );
                    ui.horizontal(|ui| {
                        for (label, delta) in [
                            ("-1m", -60.0),
                            ("-10s", -10.0),
                            ("+10s", 10.0),
                            ("+1m", 60.0),
                        ] {
                            if ui.small_button(label).clicked() {
                                app.adjust_time(delta);
                            }
                        }
                    });
                    ui.end_row();
                }

                if mode != Mode::Clock {
                    ui.label("Decimal precision");
                    ui.add(egui::Slider::new(&mut self.draft.decimals, 0..=3));
                    ui.end_row();

                    tip_label(
                        ui,
                        "Always show hours",
                        "Keep an 00: hours prefix visible even when the timer is under 60 minutes.",
                    );
                    ui.checkbox(&mut self.draft.always_show_hours, "HH:MM:SS");
                    ui.end_row();

                    tip_label(
                        ui,
                        "Hide leading zeros",
                        "Omit leading zeros (e.g. show 5:00 instead of 05:00, or 9.50 instead of 00:09.50).",
                    );
                    ui.checkbox(&mut self.draft.hide_leading_zeros, "Compact digits");
                    ui.end_row();
                }

                if mode == Mode::Down {
                    tip_label(
                        ui,
                        "Auto-reset on zero",
                        "Automatically loop back to the starting time instead of counting overdue.",
                    );
                    ui.checkbox(&mut self.draft.auto_reset_on_zero, "Loop countdown");
                    ui.end_row();
                }

                if matches!(
                    mode,
                    Mode::Down | Mode::Pomodoro | Mode::Interval | Mode::TargetTime
                ) {
                    tip_label(
                        ui,
                        "Warning beeps",
                        "Play a short cue at 10 seconds and 3-2-1 before reaching zero.",
                    );
                    ui.checkbox(&mut self.draft.warning_beeps, "Beep at 10s, 3-2-1");
                    ui.end_row();
                }

                if matches!(
                    mode,
                    Mode::Up | Mode::Down | Mode::Interval | Mode::TargetTime
                ) {
                    tip_label(
                        ui,
                        "Gray threshold",
                        "Automatically switch to the threshold color once the timer crosses the limit below.",
                    );
                    ui.checkbox(
                        &mut self.draft.use_threshold,
                        "Change color past the threshold",
                    );
                    ui.end_row();

                    if self.draft.use_threshold {
                        ui.label("Threshold (seconds)");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.text.threshold)
                                .desired_width(80.0),
                        );
                        ui.end_row();
                    }
                }
            });

        if mode == Mode::Interval {
            ui.add_space(8.0);
            ui.group(|ui| {
                ui.label(egui::RichText::new("Interval (HIIT)").strong());
                egui::Grid::new("interval")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Work (seconds)");
                        ui.add(egui::TextEdit::singleline(&mut self.text.interval_work));
                        ui.end_row();
                        ui.label("Rest (seconds)");
                        ui.add(egui::TextEdit::singleline(&mut self.text.interval_rest));
                        ui.end_row();
                        ui.label("Total rounds");
                        ui.add(egui::DragValue::new(&mut self.draft.interval_rounds).range(1..=200));
                        ui.end_row();
                    });
            });
        }

        if mode == Mode::Pomodoro {
            ui.add_space(8.0);
            ui.group(|ui| {
                ui.label(egui::RichText::new("Pomodoro").strong());
                egui::Grid::new("pomodoro")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Work length (min)");
                        ui.add(egui::TextEdit::singleline(&mut self.text.pomo_work));
                        ui.end_row();
                        ui.label("Short break (min)");
                        ui.add(egui::TextEdit::singleline(&mut self.text.pomo_break));
                        ui.end_row();
                        ui.label("Long break (min)");
                        ui.add(egui::TextEdit::singleline(&mut self.text.pomo_long));
                        ui.end_row();
                        ui.label("Sessions per long break");
                        ui.add(egui::DragValue::new(&mut self.draft.pomo_cycles).range(1..=100));
                        ui.end_row();
                    });
            });
        }

        if matches!(mode, Mode::Up | Mode::Down) && self.draft.use_threshold {
            ui.add_space(4.0);
            ui.small("Count-up grays out after this long; countdown grays out below it.");
        }

        if !matches!(mode, Mode::Clock | Mode::TargetTime) {
            ui.add_space(8.0);
            ui.collapsing(
                format!("Lap History ({})", app.engine.lap_history().len()),
                |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Copy CSV").clicked() {
                        ui.ctx().copy_text(app.engine.export_laps_csv());
                    }
                    if ui.button("Clear Laps").clicked() {
                        app.engine.clear_laps();
                    }
                });
                if app.engine.lap_history().is_empty() {
                    ui.small("No laps recorded yet. Trigger a lap while the timer is running.");
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(140.0)
                        .show(ui, |ui| {
                            egui::Grid::new("lap-history-grid")
                                .num_columns(3)
                                .spacing([16.0, 4.0])
                                .show(ui, |ui| {
                                    ui.strong("#");
                                    ui.strong("Total");
                                    ui.strong("Split");
                                    ui.end_row();
                                    for entry in app.engine.lap_history().iter().rev() {
                                        ui.label(format!("{}", entry.index));
                                        ui.monospace(&entry.total_text);
                                        ui.monospace(&entry.split_text);
                                        ui.end_row();
                                    }
                                });
                        });
                    }
                },
            );
        }
    }

    fn appearance_tab(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("appearance")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                tip_label(
                    ui,
                    "Theme",
                    "Applies a curated or custom color palette to the overlay, Settings, menu, and tray icon.",
                );
                let current_theme_text = self
                    .draft
                    .active_custom_theme
                    .clone()
                    .unwrap_or_else(|| theme_label(self.draft.theme).to_string());
                egui::ComboBox::from_id_salt("theme")
                    .selected_text(current_theme_text)
                    .show_ui(ui, |ui| {
                        for theme in Theme::ALL {
                            let selected = self.draft.active_custom_theme.is_none()
                                && self.draft.theme == theme;
                            if ui.selectable_label(selected, theme_label(theme)).clicked() {
                                self.draft.theme = theme;
                                self.draft.active_custom_theme = None;
                            }
                        }
                        if !self.draft.custom_themes.is_empty() {
                            ui.separator();
                            let names: Vec<String> = self
                                .draft
                                .custom_themes
                                .iter()
                                .map(|t| t.name.clone())
                                .collect();
                            for name in names {
                                let selected =
                                    self.draft.active_custom_theme.as_deref() == Some(name.as_str());
                                if ui.selectable_label(selected, format!("\u{2605} {name}")).clicked() {
                                    self.draft.active_custom_theme = Some(name);
                                }
                            }
                        }
                    });
                ui.end_row();

                tip_label(
                    ui,
                    "Custom label",
                    "Optional short title shown above the timer numbers (e.g. WORK, BRB, BOSS).",
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft.custom_label)
                        .hint_text("Optional label above timer"),
                );
                ui.end_row();

                ui.label("Display style");
                egui::ComboBox::from_id_salt("style")
                    .selected_text(style_label(self.draft.display_style))
                    .show_ui(ui, |ui| {
                        for style in [
                            DisplayStyle::Text,
                            DisplayStyle::Arc,
                            DisplayStyle::Bar,
                            DisplayStyle::Fluid,
                            DisplayStyle::Sprite,
                        ] {
                            ui.selectable_value(
                                &mut self.draft.display_style,
                                style,
                                style_label(style),
                            );
                        }
                    });
                ui.end_row();

                ui.label("Show timer text");
                ui.checkbox(&mut self.draft.show_text, "");
                ui.end_row();

                if self.draft.display_style == DisplayStyle::Sprite {
                    ui.label("Sprite image");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.text.sprite).desired_width(260.0),
                        );
                        if ui.button("Browse\u{2026}").clicked() {
                            self.file_dialog = Some(FileDialogKind::Sprite);
                        }
                    });
                    ui.end_row();
                }

                tip_label(
                    ui,
                    "Show background",
                    "Draw the background panel behind the timer. Turn off for a floating transparent look.",
                );
                ui.checkbox(&mut self.draft.show_background, "Enable background panel");
                ui.end_row();

                if self.draft.show_background {
                    ui.label("Background shape");
                    egui::ComboBox::from_id_salt("shape")
                        .selected_text(shape_label(self.draft.window_shape))
                        .show_ui(ui, |ui| {
                            for shape in [
                                WindowShape::Rectangular,
                                WindowShape::Rounded,
                                WindowShape::Capsule,
                            ] {
                                ui.selectable_value(
                                    &mut self.draft.window_shape,
                                    shape,
                                    shape_label(shape),
                                );
                            }
                        });
                    ui.end_row();

                    if self.draft.window_shape == WindowShape::Rounded {
                        ui.label("Corner radius");
                        ui.add(
                            egui::Slider::new(&mut self.draft.corner_radius, 0..=100).suffix(" px"),
                        );
                        ui.end_row();
                    }

                    ui.label("Border style");
                    egui::ComboBox::from_id_salt("border")
                        .selected_text(border_label(self.draft.border_style))
                        .show_ui(ui, |ui| {
                            for style in [
                                BorderStyle::None,
                                BorderStyle::Thin,
                                BorderStyle::Neon,
                            ] {
                                ui.selectable_value(
                                    &mut self.draft.border_style,
                                    style,
                                    border_label(style),
                                );
                            }
                        });
                    ui.end_row();

                    if self.draft.border_style != BorderStyle::None {
                        ui.label("Border thickness");
                        ui.add(
                            egui::Slider::new(&mut self.draft.border_thickness, 1.0..=20.0)
                                .suffix(" px"),
                        );
                        ui.end_row();
                    }

                    if platform::IS_WINDOWS {
                        tip_label(
                            ui,
                            "Native blur / acrylic",
                            "Apply Windows translucent blur behind the timer panel.",
                        );
                        ui.checkbox(&mut self.draft.window_blur, "Blur behind panel");
                        ui.end_row();
                    }
                }

                ui.label("Padding");
                ui.add(egui::Slider::new(&mut self.draft.padding, 0..=100).suffix(" px"));
                ui.end_row();

                // The arc has no other use for this value, so it is hidden while
                // another display style is selected.
                if self.draft.display_style == DisplayStyle::Arc {
                    ui.label("Arc thickness");
                    ui.add(
                        egui::Slider::new(&mut self.draft.arc_thickness, 1.0..=50.0).suffix(" px"),
                    );
                    ui.end_row();
                }

                ui.label("Window opacity");
                let mut opacity = (self.draft.opacity * 100.0).round() as u8;
                if ui
                    .add(egui::Slider::new(&mut opacity, 10..=100).suffix(" %"))
                    .changed()
                {
                    self.draft.opacity = opacity as f32 / 100.0;
                }
                ui.end_row();

                if self.draft.show_text {
                    ui.label("Font family");
                    egui::ComboBox::from_id_salt("font")
                        .selected_text(self.text.font_family.clone())
                        .show_ui(ui, |ui| {
                            for family in fonts::available_families() {
                                ui.selectable_value(
                                    &mut self.text.font_family,
                                    family.clone(),
                                    family,
                                );
                            }
                        });
                    ui.end_row();

                    ui.label("Font size");
                    ui.add(egui::Slider::new(&mut self.draft.font_size, 8..=300).suffix(" pt"));
                    ui.end_row();

                    ui.label("Text outline");
                    ui.checkbox(&mut self.draft.text_outline, "");
                    ui.end_row();

                    if self.draft.text_outline {
                        ui.label("Outline thickness");
                        ui.add(
                            egui::Slider::new(&mut self.draft.text_outline_thickness, 1.0..=15.0)
                                .suffix(" px"),
                        );
                        ui.end_row();
                    }
                }

                if matches!(
                    self.draft.display_style,
                    DisplayStyle::Bar | DisplayStyle::Fluid | DisplayStyle::Arc
                ) {
                    tip_label(
                        ui,
                        "Gradient fill",
                        "Smoothly blends the progress fill toward the gradient end color as time runs low.",
                    );
                    ui.checkbox(&mut self.draft.use_gradient, "Blend progress color");
                    ui.end_row();
                }

                tip_label(
                    ui,
                    "Pulse on warning",
                    "Gently pulses the overlay border and glow when overdue or past the threshold.",
                );
                ui.checkbox(&mut self.draft.pulse_on_warning, "Animate warning state");
                ui.end_row();

                tip_label(
                    ui,
                    "Text drop shadow",
                    "Adds a soft dark shadow behind the timer text for readability over bright scenes.",
                );
                ui.checkbox(&mut self.draft.use_text_shadow, "Enable shadow");
                ui.end_row();

                tip_label(
                    ui,
                    "Compact mini-mode",
                    "Shrinks the overlay down to a small corner badge (also toggleable via hotkey or menu).",
                );
                ui.checkbox(&mut self.draft.mini_mode, "Mini overlay");
                ui.end_row();

                tip_label(
                    ui,
                    "Hover-to-fade",
                    "Automatically fades the overlay when your mouse hovers over it so you can see behind it.",
                );
                ui.checkbox(&mut self.draft.hover_to_fade, "Fade on mouse hover");
                ui.end_row();

                if self.draft.hover_to_fade {
                    ui.label("Hover fade opacity");
                    let mut fade = (self.draft.hover_fade_opacity * 100.0).round() as u8;
                    if ui
                        .add(egui::Slider::new(&mut fade, 5..=100).suffix(" %"))
                        .changed()
                    {
                        self.draft.hover_fade_opacity = fade as f32 / 100.0;
                    }
                    ui.end_row();
                }
            });

        ui.add_space(8.0);
        ui.label(egui::RichText::new("Colours").strong());
        colour_row(ui, "Normal color", &mut self.draft.normal_color);
        colour_row(ui, "Threshold color", &mut self.draft.gray_color);
        if self.draft.use_gradient {
            colour_row(ui, "Gradient end color", &mut self.draft.gradient_end_color);
        }

        // Smart Contrast Warning: alert the user if text and background are too close.
        if self.draft.show_background && self.draft.show_text {
            let text_col = crate::render::target_color(&self.draft, crate::engine::ColorRole::Normal);
            let bg_col = crate::render::effective_background_color(&self.draft);
            let ratio = crate::render::contrast_ratio(text_col, bg_col);
            if ratio < 3.0 {
                ui.add_space(4.0);
                ui.colored_label(
                    egui::Color32::from_rgb(0xE0, 0xA0, 0x00),
                    format!(
                        "\u{26A0} Low contrast ({ratio:.1}:1) between timer text and background \u{2014} it may be hard to read."
                    ),
                );
            }
        }

        ui.add_space(8.0);
        ui.collapsing("Custom Theme Builder", |ui| {
            ui.small("Create your own named theme and apply it across the timer, Settings, menu, and tray.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(
                    egui::TextEdit::singleline(&mut self.theme_builder.name)
                        .desired_width(160.0)
                        .hint_text("My Theme"),
                );
                ui.checkbox(&mut self.theme_builder.dark_mode, "Dark UI");
            });
            colour_row(ui, "Background", &mut self.theme_builder.background);
            colour_row(ui, "Timer text / accent", &mut self.theme_builder.text_color);
            colour_row(ui, "Warning / overdue", &mut self.theme_builder.warning_color);
            colour_row(ui, "Border", &mut self.theme_builder.border_color);
            ui.add_space(4.0);
            if ui.button("Save & Apply Theme").clicked() {
                let name = self.theme_builder.name.trim().to_string();
                if !name.is_empty() {
                    let mut saved = self.theme_builder.clone();
                    saved.name = name.clone();
                    if let Some(existing) = self
                        .draft
                        .custom_themes
                        .iter_mut()
                        .find(|t| t.name == name)
                    {
                        *existing = saved;
                    } else {
                        self.draft.custom_themes.push(saved);
                    }
                    self.draft.active_custom_theme = Some(name);
                }
            }
            if !self.draft.custom_themes.is_empty() {
                ui.add_space(6.0);
                let mut delete_name: Option<String> = None;
                for t in &self.draft.custom_themes {
                    ui.horizontal(|ui| {
                        ui.label(&t.name);
                        if ui.small_button("Delete").clicked() {
                            delete_name = Some(t.name.clone());
                        }
                    });
                }
                if let Some(name) = delete_name {
                    self.draft.custom_themes.retain(|t| t.name != name);
                    if self.draft.active_custom_theme.as_deref() == Some(name.as_str()) {
                        self.draft.active_custom_theme = None;
                    }
                }
            }
        });
        if self.draft.text_outline {
            colour_row(ui, "Outline color", &mut self.draft.text_outline_color);
        }
        if self.draft.border_style == BorderStyle::Thin {
            colour_row(ui, "Border color", &mut self.draft.thin_border_color);
        }
        if self.draft.display_style == DisplayStyle::Bar {
            colour_row(ui, "Bar text color", &mut self.draft.bar_text_color);
        }
    }

    fn behavior_tab(&mut self, ui: &mut egui::Ui, app: &mut TimerApp) {
        egui::Grid::new("behavior")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Rotation");
                egui::ComboBox::from_id_salt("rotation")
                    .selected_text(rotation_label(self.draft.rotation))
                    .show_ui(ui, |ui| {
                        for rotation in [Rotation::None, Rotation::Cw90, Rotation::Cw270] {
                            ui.selectable_value(
                                &mut self.draft.rotation,
                                rotation,
                                rotation_label(rotation),
                            );
                        }
                    });
                ui.end_row();

                ui.label("Lock position");
                ui.checkbox(&mut self.draft.window_locked, "Prevent dragging");
                ui.end_row();

                tip_label(
                    ui,
                    "Auto-start on launch",
                    "Begin counting immediately as soon as Green Timer opens.",
                );
                ui.checkbox(&mut self.draft.auto_start_on_launch, "Start counting on launch");
                ui.end_row();

                tip_label(
                    ui,
                    "Edge snapping",
                    "Magnetically snap the timer to screen edges and corners when dragged nearby.",
                );
                ui.checkbox(&mut self.draft.edge_snapping, "Snap to screen edges");
                ui.end_row();

                tip_label(
                    ui,
                    "Start hidden",
                    "Launch minimized to the system tray instead of showing the overlay immediately.",
                );
                ui.checkbox(&mut self.draft.start_minimized, "Start in the tray");
                ui.end_row();

                tip_label(
                    ui,
                    "Reduce motion",
                    "Disable smooth color fades and warning pulse animations.",
                );
                ui.checkbox(&mut self.draft.reduce_motion, "Disable animations");
                ui.end_row();

                ui.label("Sound cue");
                ui.checkbox(&mut self.draft.sound_cue, "On zero / phase change");
                ui.end_row();

                ui.label("Custom sound (.wav)");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.text.sound).desired_width(240.0));
                    if ui.button("Browse\u{2026}").clicked() {
                        self.file_dialog = Some(FileDialogKind::Sound);
                    }
                    if ui.button("Test").clicked() {
                        platform::play_sound(&self.text.sound);
                    }
                });
                ui.end_row();

                ui.label("Auto-pause target");
                ui.horizontal(|ui| {
                    let captured = self.draft.target_hwnd;
                    if ui
                        .button(if captured != 0 {
                            format!("Target captured (HWND {captured})")
                        } else {
                            "Pick target app\u{2026}".to_string()
                        })
                        .on_hover_text("Pauses the timer whenever another window has focus.")
                        .clicked()
                    {
                        self.capture_foreground_soon(ui.ctx());
                    }
                    if ui.button("Clear").clicked() {
                        self.draft.target_hwnd = 0;
                    }
                });
                ui.end_row();

                ui.label("Snap position");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        for (target, label) in [
                            (SnapTarget::TopLeft, "Top Left"),
                            (SnapTarget::TopCenter, "Top Center"),
                            (SnapTarget::TopRight, "Top Right"),
                        ] {
                            if ui.button(label).clicked() {
                                self.pending_snap = Some(target);
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        for (target, label) in [
                            (SnapTarget::BottomLeft, "Bottom Left"),
                            (SnapTarget::BottomCenter, "Bottom Center"),
                            (SnapTarget::BottomRight, "Bottom Right"),
                        ] {
                            if ui.button(label).clicked() {
                                self.pending_snap = Some(target);
                            }
                        }
                    });
                });
                ui.end_row();

                ui.label("Auto check updates");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.draft.auto_check_updates, "On launch");
                    if ui
                        .add_enabled(!self.update_busy, egui::Button::new("Check now"))
                        .clicked()
                    {
                        self.update_busy = true;
                        app.update_available = None;
                        app.update_rx = Some(updater::spawn_check(APP_VERSION.to_string()));
                    }
                });
                ui.end_row();
            });

        ui.add_space(10.0);
        ui.collapsing("Right-Click Menu Items", |ui| {
            ui.small("Choose which options appear when you right-click the timer.");
            ui.add_space(4.0);
            let menu = &mut self.draft.context_menu;
            egui::Grid::new("context-menu-customizer")
                .num_columns(2)
                .spacing([20.0, 4.0])
                .show(ui, |ui| {
                    ui.checkbox(&mut menu.start_pause, "Start / Pause");
                    ui.checkbox(&mut menu.reset, "Reset");
                    ui.end_row();
                    ui.checkbox(&mut menu.lap, "Lap");
                    ui.checkbox(&mut menu.quick_adjust, "Quick Adjust (+1m / -1m)");
                    ui.end_row();
                    ui.checkbox(&mut menu.copy_time, "Copy Time");
                    ui.checkbox(&mut menu.mini_mode, "Mini Mode");
                    ui.end_row();
                    ui.checkbox(&mut menu.ghost_mode, "Ghost Mode");
                    ui.checkbox(&mut menu.profiles, "Saved Profiles");
                    ui.end_row();
                    ui.checkbox(&mut menu.settings, "Settings");
                    ui.checkbox(&mut menu.exit, "Exit");
                    ui.end_row();
                });
        });

        ui.add_space(8.0);
        ui.collapsing(
            format!("Saved Profiles ({})", self.draft.profiles.len()),
            |ui| {
                ui.small("Save the current timer setup as a reusable profile or auto-switch by focused app.");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_profile_name)
                            .desired_width(170.0)
                            .hint_text("Profile name"),
                    );
                    if ui.button("Save Current").clicked() {
                        let name = self.new_profile_name.trim().to_string();
                        if !name.is_empty() {
                            let snap = self.draft.snapshot_for_profile();
                            if let Some(existing) =
                                self.draft.profiles.iter_mut().find(|p| p.name == name)
                            {
                                existing.settings = snap;
                            } else {
                                self.draft.profiles.push(crate::config::SavedProfile {
                                    name: name.clone(),
                                    settings: snap,
                                });
                            }
                            self.new_profile_name.clear();
                            self.io_status = Some(format!("Saved profile \"{name}\"."));
                        }
                    }
                });

                if !self.draft.profiles.is_empty() {
                    ui.add_space(4.0);
                    let mut load_name: Option<String> = None;
                    let mut delete_name: Option<String> = None;
                    let mut bind_app_name: Option<String> = None;

                    for profile in &self.draft.profiles {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&profile.name).strong());
                            if ui.small_button("Load").clicked() {
                                load_name = Some(profile.name.clone());
                            }
                            if self.draft.target_hwnd != 0
                                && ui
                                    .small_button("Bind to Target App")
                                    .on_hover_text(
                                        "Automatically switch to this profile whenever the captured target window is focused.",
                                    )
                                    .clicked()
                            {
                                bind_app_name = Some(profile.name.clone());
                            }
                            if ui.small_button("Delete").clicked() {
                                delete_name = Some(profile.name.clone());
                            }
                        });
                    }

                    if let Some(name) = load_name {
                        if self.draft.apply_profile(&name) {
                            self.sync_text();
                            self.io_status = Some(format!("Loaded profile \"{name}\"."));
                        }
                    }
                    if let Some(name) = bind_app_name {
                        let hwnd = self.draft.target_hwnd;
                        self.draft
                            .app_profile_rules
                            .retain(|r| r.target_hwnd != hwnd);
                        self.draft
                            .app_profile_rules
                            .push(crate::config::AppProfileRule {
                                target_hwnd: hwnd,
                                profile_name: name.clone(),
                            });
                        self.io_status = Some(format!(
                            "Bound window {hwnd} to profile \"{name}\"."
                        ));
                    }
                    if let Some(name) = delete_name {
                        self.draft.profiles.retain(|p| p.name != name);
                        self.draft
                            .app_profile_rules
                            .retain(|r| r.profile_name != name);
                        self.io_status = Some(format!("Deleted profile \"{name}\"."));
                    }
                }

                if !self.draft.app_profile_rules.is_empty() {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Per-App Auto-Switch Rules").small().strong());
                    let mut remove_hwnd: Option<u64> = None;
                    for rule in &self.draft.app_profile_rules {
                        ui.horizontal(|ui| {
                            ui.small(format!(
                                "Window {} \u{2192} {}",
                                rule.target_hwnd, rule.profile_name
                            ));
                            if ui.small_button("Remove").clicked() {
                                remove_hwnd = Some(rule.target_hwnd);
                            }
                        });
                    }
                    if let Some(hwnd) = remove_hwnd {
                        self.draft
                            .app_profile_rules
                            .retain(|r| r.target_hwnd != hwnd);
                    }
                }
            },
        );

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            tip_label(
                ui,
                "Backup & Share",
                "Export all settings to a JSON file or import a previously saved configuration.",
            );
            if ui.button("Export Settings\u{2026}").clicked() {
                self.file_dialog = Some(FileDialogKind::ExportConfig);
            }
            if ui.button("Import Settings\u{2026}").clicked() {
                self.file_dialog = Some(FileDialogKind::ImportConfig);
            }
        });
        if let Some(msg) = &self.io_status {
            ui.small(msg);
        }
    }

    fn hotkeys_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(
            "Hotkeys are global. Click Record, then press the key you want to use.\n\
             Bindings are matched by physical key, so they work with any keyboard layout.",
        );
        ui.add_space(8.0);

        egui::Grid::new("hotkeys")
            .num_columns(3)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Action").strong());
                ui.label(egui::RichText::new("Binding").strong());
                ui.end_row();

                for action in Action::ALL {
                    let entry = self.draft.hotkey_mut(action);
                    ui.label(action.label());
                    ui.monospace(format!("{}  (scan {})", entry.name, entry.code));

                    let recording_this = self
                        .recording
                        .as_ref()
                        .map(|(a, _)| *a == action)
                        .unwrap_or(false);
                    if recording_this {
                        ui.label("Press a key\u{2026}");
                    } else if ui.button("Record").clicked() {
                        self.start_recording(action);
                    }
                    ui.end_row();
                }
            });
    }

    fn about_tab(&mut self, ui: &mut egui::Ui) -> Outcome {
        let mut outcome = Outcome::None;
        ui.heading(format!("{APP_NAME} {APP_VERSION}"));
        ui.label(
            "A lightweight, always-on-top overlay timer with count-up, countdown, \
             clock and pomodoro modes.",
        );
        ui.add_space(6.0);
        ui.label(format!("System: {}", platform::system_info()));
        ui.label(format!(
            "Settings file: {}",
            crate::config::config_path().display()
        ));
        ui.label(format!("Repository: {}", updater::GITHUB_REPO));
        ui.add_space(6.0);
        ui.label("Hotkeys: F1 start/pause \u{00b7} F2 reset \u{00b7} F3 settings \u{00b7} F4 ghost mode \u{00b7} Space lap");

        ui.add_space(12.0);
        if self.update_busy {
            ui.label("Checking for updates\u{2026}");
        } else if ui.button("Check for updates now").clicked() {
            outcome = Outcome::CheckUpdates;
        }

        if let Some(info) = &self.pending_update {
            ui.add_space(8.0);
            ui.label(format!(
                "{} {} is available (you have {}).",
                APP_NAME, info.latest, APP_VERSION
            ));
            if ui.button("Download and run installer").clicked() {
                outcome = Outcome::Install(info.url.clone());
            }
        }

        ui.add_space(8.0);
        ui.label(format!("Current time: {}", format_clock("%H:%M:%S")));
        outcome
    }

    // -- helpers ---------------------------------------------------------
    fn sync_text(&mut self) {
        self.text = TextFields {
            countdown: format_input(self.draft.countdown_start),
            clock_format: self.draft.clock_format.clone(),
            pomo_work: format!("{}", trim(self.draft.pomo_work)),
            pomo_break: format!("{}", trim(self.draft.pomo_break)),
            pomo_long: format!("{}", trim(self.draft.pomo_long_break)),
            interval_work: format!("{}", trim(self.draft.interval_work)),
            interval_rest: format!("{}", trim(self.draft.interval_rest)),
            target_time: self.draft.target_time.clone(),
            threshold: format!("{}", trim(self.draft.threshold_time)),
            sprite: self.draft.sprite_path.clone(),
            sound: self.draft.custom_sound_path.clone(),
            font_family: self.draft.font_family.clone(),
        };
    }

    /// Read the draft and text fields into a complete configuration.
    ///
    /// Only fields that apply to the selected mode are parsed, so a stale or
    /// half-typed value in a hidden field can never block saving or preview.
    /// Returns `None` while the typed input is not valid yet; the caller then
    /// keeps the last valid configuration.
    fn collect(&self) -> Option<Config> {
        let mut config = self.draft.clone();

        if config.mode == Mode::Down {
            config.countdown_start = parse_duration(&self.text.countdown).ok()?;
        }

        if config.mode == Mode::Pomodoro {
            for (value, target) in [
                (&self.text.pomo_work, &mut config.pomo_work),
                (&self.text.pomo_break, &mut config.pomo_break),
                (&self.text.pomo_long, &mut config.pomo_long_break),
            ] {
                let parsed: f64 = value.trim().parse().ok()?;
                if !parsed.is_finite() {
                    return None;
                }
                *target = parsed;
            }
        }

        if config.mode == Mode::Interval {
            for (value, target) in [
                (&self.text.interval_work, &mut config.interval_work),
                (&self.text.interval_rest, &mut config.interval_rest),
            ] {
                let parsed: f64 = value.trim().parse().ok()?;
                if !parsed.is_finite() {
                    return None;
                }
                *target = parsed;
            }
        }

        if config.mode == Mode::TargetTime {
            let trimmed = self.text.target_time.trim();
            if trimmed.is_empty() {
                return None;
            }
            config.target_time = trimmed.to_string();
        }

        // The threshold applies to count-up, countdown, interval, and target time.
        if config.use_threshold
            && matches!(
                config.mode,
                Mode::Up | Mode::Down | Mode::Interval | Mode::TargetTime
            )
        {
            let parsed: f64 = self.text.threshold.trim().parse().ok()?;
            if !parsed.is_finite() {
                return None;
            }
            config.threshold_time = parsed;
        }

        config.clock_format = if self.text.clock_format.trim().is_empty() {
            "%H:%M:%S".to_string()
        } else {
            self.text.clock_format.trim().to_string()
        };
        config.sprite_path = self.text.sprite.trim().to_string();
        config.custom_sound_path = self.text.sound.trim().to_string();
        config.font_family = if self.text.font_family.trim().is_empty() {
            "Segoe UI".to_string()
        } else {
            self.text.font_family.trim().to_string()
        };

        Some(config.validated())
    }

    /// Controls in the dialog never change geometry or the grip's font size,
    /// so the draft adopts whatever the live window currently has instead of
    /// overwriting a grip resize with stale values.
    ///
    /// The font size is only taken from the live config when the slider was
    /// not moved since the last frame; otherwise the user's slider edit would
    /// be immediately undone by the size the grip (or the text) chose.
    fn adopt_live_state(&mut self, live: &Config) {
        self.draft.pos_x = live.pos_x;
        self.draft.pos_y = live.pos_y;
        self.draft.width = live.width;
        self.draft.height = live.height;

        let slider_touched = self
            .live
            .as_ref()
            .is_some_and(|previous| previous.font_size != self.draft.font_size);
        if !slider_touched {
            self.draft.font_size = live.font_size;
        }
    }

    /// Push the current draft to the running overlay so changes are visible
    /// immediately, before Save. Never persists: Cancel still restores the
    /// configuration the dialog opened with.
    fn live_update(&mut self, app: &mut TimerApp) {
        // There is no UI action here that changes geometry, so carry the live
        // window's position and size into the drafted config first; otherwise
        // pressing any control would snap a dragged or grip-resized window
        // back to the geometry from when the dialog opened.
        self.adopt_live_state(&app.config);

        let Some(config) = self.collect() else {
            self.invalid_hint = true;
            return;
        };
        self.invalid_hint = false;

        if self.live.as_ref() == Some(&config) {
            return;
        }
        self.live = Some(config.clone());

        let font_changed = app.config.font_family != config.font_family;
        let sprite_changed = app.config.sprite_path != config.sprite_path;

        app.config = config;
        app.font_size = app.config.font_size as f32;

        if font_changed {
            if let Some(ctx) = crate::app::current_context() {
                fonts::install(&ctx, &app.config.font_family);
            }
        }
        if sprite_changed {
            if let Some(ctx) = crate::app::current_context() {
                app.reload_assets(&ctx);
            }
        }
        app.apply_appearance();
        app.refresh();
    }

    /// Undo any live preview and restore the configuration the dialog opened
    /// with (Cancel / title-bar close).
    fn cancel(&mut self, app: &mut TimerApp) {
        let original = self.original.clone();
        let font_changed = app.config.font_family != original.font_family;
        let sprite_changed = app.config.sprite_path != original.sprite_path;

        app.config = original;
        app.font_size = app.config.font_size as f32;
        if font_changed {
            if let Some(ctx) = crate::app::current_context() {
                fonts::install(&ctx, &app.config.font_family);
            }
        }
        if sprite_changed {
            if let Some(ctx) = crate::app::current_context() {
                app.reload_assets(&ctx);
            }
        }
        app.apply_appearance();
        app.refresh();

        self.draft = app.config.clone();
        self.live = Some(app.config.clone());
        self.sync_text();
        self.invalid_hint = false;
    }

    /// Validate the draft, apply it and persist.
    fn commit(&mut self, app: &mut TimerApp) {
        self.adopt_live_state(&app.config);
        let Some(draft) = self.collect() else {
            self.hotkey_message = Some(
                "Some values are not valid yet; fix the highlighted field and try again.".into(),
            );
            return;
        };

        let mode_or_duration_changed = self.original.mode != draft.mode
            || (self.original.countdown_start - draft.countdown_start).abs() > f64::EPSILON
            || (self.original.pomo_work - draft.pomo_work).abs() > f64::EPSILON;
        let sprite_changed = app.config.sprite_path != draft.sprite_path;
        let font_changed = app.config.font_family != draft.font_family;

        app.config = draft;
        self.draft = app.config.clone();
        self.live = Some(app.config.clone());
        app.font_size = app.config.font_size as f32;
        self.sync_text();
        self.hotkey_message = None;
        self.invalid_hint = false;

        if font_changed {
            if let Some(ctx) = crate::app::current_context() {
                fonts::install(&ctx, &app.config.font_family);
            }
        }
        if sprite_changed {
            if let Some(ctx) = crate::app::current_context() {
                app.reload_assets(&ctx);
            }
        }
        app.apply_appearance();
        if mode_or_duration_changed {
            app.reset();
        } else {
            app.refresh();
        }

        match app.hotkeys_rebind() {
            Ok(()) => {}
            Err(failures) => {
                self.hotkey_message = Some(format!("Some hotkeys could not be registered: {failures}"));
            }
        }
        app.persist();
    }

    fn start_recording(&mut self, action: Action) {
        self.hotkey_message = None;
        self.recording = Some((action, capture_next()));
    }

    /// Abandon a pending capture and resume normal hotkey dispatch.
    fn stop_recording(&mut self) {
        if self.recording.take().is_some() {
            crate::hotkeys::cancel_capture();
        }
    }

    fn poll_recording(&mut self) {
        let Some((action, rx)) = &self.recording else {
            return;
        };
        let action = *action;
        let result = rx.try_recv();
        match result {
            Ok((name, code)) => {
                let entry = self.draft.hotkey_mut(action);
                *entry = Hotkey::new(&name, code);
                self.recording = None;
                // Rebinding only takes effect on Apply; tell the user so.
                self.hotkey_message =
                    Some(format!("{} is now {name} (scan {code}). Save to apply.", action.label()));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.recording = None;
                self.hotkey_message = Some("Key capture failed.".into());
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Ask the user to focus the target app, then capture it after a short
    /// delay so the click on the settings window is not what gets recorded.
    fn capture_foreground_soon(&mut self, ctx: &egui::Context) {
        self.hotkey_message = Some(
            "Switch to the target app; the timer will capture focus in 5 seconds.".into(),
        );
        ctx.request_repaint_after(std::time::Duration::from_secs(5));

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let _ = tx.send(platform::foreground_window());
        });
        self.foreground_rx = Some(rx);
    }

    fn snap(&mut self, app: &mut TimerApp, target: SnapTarget) {
        let scale = crate::app::current_context()
            .map(|ctx| ctx.pixels_per_point())
            .unwrap_or(1.0);
        let (left, top, screen_w, screen_h) = platform::primary_work_area(scale);
        let width = app.config.width as f32;
        let height = app.config.height as f32;
        let margin = 24.0;
        let (x, y) = match target {
            SnapTarget::TopLeft => (left + margin, top + margin),
            SnapTarget::TopCenter => (left + (screen_w - width) / 2.0, top + margin),
            SnapTarget::TopRight => (left + screen_w - width - margin, top + margin),
            SnapTarget::BottomLeft => (left + margin, top + screen_h - height - margin),
            SnapTarget::BottomCenter => {
                (left + (screen_w - width) / 2.0, top + screen_h - height - margin)
            }
            SnapTarget::BottomRight => (
                left + screen_w - width - margin,
                top + screen_h - height - margin,
            ),
        };
        let pos_x = x.round() as i32;
        let pos_y = y.round() as i32;
        app.config.pos_x = pos_x;
        app.config.pos_y = pos_y;
        self.draft.pos_x = pos_x;
        self.draft.pos_y = pos_y;
        self.original.pos_x = pos_x;
        self.original.pos_y = pos_y;
        if let Some(ctx) = crate::app::current_context() {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        }
        app.persist();
    }

    fn poll_file_dialog(&mut self) {
        let Some(kind) = self.file_dialog.take() else {
            return;
        };
        match kind {
            FileDialogKind::Sprite => {
                if let Some(path) = pick_file(
                    "Select Sprite Image",
                    "Images (*.png *.jpg *.jpeg *.bmp *.gif *.webp)",
                ) {
                    let path = path.to_string_lossy().to_string();
                    self.text.sprite = path.clone();
                    self.draft.sprite_path = path;
                }
            }
            FileDialogKind::Sound => {
                if let Some(path) = pick_file("Select WAV Sound", "WAV files (*.wav)") {
                    let path = path.to_string_lossy().to_string();
                    self.text.sound = path.clone();
                    self.draft.custom_sound_path = path;
                }
            }
            FileDialogKind::ImportConfig => {
                if let Some(path) = pick_file("Import Settings", "JSON (*.json)") {
                    match crate::config::import_from_path(&path) {
                        Ok(loaded) => {
                            self.draft = loaded;
                            self.sync_text();
                            self.io_status = Some(format!(
                                "Imported settings from {}",
                                path.display()
                            ));
                        }
                        Err(err) => {
                            self.io_status = Some(format!("Import failed: {err}"));
                        }
                    }
                }
            }
            FileDialogKind::ExportConfig => {
                if let Some(path) = save_file(
                    "Export Settings",
                    "JSON (*.json)",
                    "green_timer_settings.json",
                ) {
                    let to_save = self.collect().unwrap_or_else(|| self.draft.clone());
                    match crate::config::export_to_path(&to_save, &path) {
                        Ok(()) => {
                            self.io_status = Some(format!(
                                "Exported settings to {}",
                                path.display()
                            ));
                        }
                        Err(err) => {
                            self.io_status = Some(format!("Export failed: {err}"));
                        }
                    }
                }
            }
        }
    }

    /// Read the delayed focus capture, if one is pending.
    fn poll_foreground(&mut self) {
        let Some(rx) = &self.foreground_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(hwnd) => {
                self.foreground_rx = None;
                if hwnd != 0 {
                    self.draft.target_hwnd = hwnd;
                    self.hotkey_message =
                        Some(format!("Auto-pause target set to HWND {hwnd}. Save to apply."));
                } else {
                    self.hotkey_message = Some("No focused window was found.".into());
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.foreground_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Hand an installer download to the app, which runs it in the background
    /// and exits once it starts.
    fn poll_install(&mut self) {
        let Some(url) = self.install_requested.take() else {
            return;
        };
        self.open = false;
        crate::app::request_install(&url);
    }
}

fn colour_row(ui: &mut egui::Ui, label: &str, colour: &mut Rgb) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut rgb = [colour.0, colour.1, colour.2];
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            *colour = Rgb(rgb[0], rgb[1], rgb[2]);
        }
        ui.monospace(colour.to_hex());
    });
}

fn trim(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        let text = format!("{value:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Label that shows a concise explanation after hovering for a moment.
/// Used only on options whose purpose is not immediately obvious from their name.
fn tip_label(ui: &mut egui::Ui, text: &str, tip: &str) {
    ui.label(text).on_hover_text(tip);
}

fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Up => "Count Up",
        Mode::Down => "Countdown",
        Mode::Clock => "Clock",
        Mode::Pomodoro => "Pomodoro",
        Mode::Interval => "Interval (HIIT)",
        Mode::TargetTime => "Target Time",
    }
}

fn theme_label(theme: Theme) -> &'static str {
    match theme {
        Theme::Default => "Default",
        Theme::Chalkboard => "Chalkboard",
        Theme::Cyberpunk => "Cyberpunk Neon",
        Theme::MinimalGlass => "Minimal Glass",
        Theme::RetroLcd => "Retro LCD",
        Theme::WarmPaper => "Warm Paper",
        Theme::HighContrast => "High Contrast",
    }
}

fn style_label(style: DisplayStyle) -> &'static str {
    match style {
        DisplayStyle::Text => "Text",
        DisplayStyle::Arc => "Arc",
        DisplayStyle::Bar => "Bar",
        DisplayStyle::Fluid => "Fluid",
        DisplayStyle::Sprite => "Sprite",
    }
}

fn shape_label(shape: WindowShape) -> &'static str {
    match shape {
        WindowShape::Rectangular => "Rectangular",
        WindowShape::Rounded => "Rounded",
        WindowShape::Capsule => "Capsule",
    }
}

fn border_label(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::None => "None",
        BorderStyle::Thin => "Thin",
        BorderStyle::Neon => "Neon",
    }
}

fn rotation_label(rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::None => "0\u{00b0}",
        Rotation::Cw90 => "90\u{00b0}",
        Rotation::Cw270 => "270\u{00b0}",
    }
}

/// Native file picker via PowerShell's dialogs is undesirable; use a tiny
/// Win32 `GetOpenFileNameW` dialog instead.
#[cfg(windows)]
fn pick_file(title: &str, filter: &str) -> Option<std::path::PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    // The filter is a double-NUL terminated list of "label\0pattern\0" pairs.
    let mut filter_wide: Vec<u16> = Vec::new();
    let (label, pattern) = filter
        .split_once(' ')
        .unwrap_or(("Files", "*.*"));
    for part in [label, pattern] {
        filter_wide.extend(wide(part));
    }
    filter_wide.push(0);
    filter_wide.push(0);

    let mut buffer = vec![0u16; 1024];
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: windows::core::PCWSTR(filter_wide.as_ptr()),
        lpstrFile: windows::core::PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: windows::core::PCWSTR(wide(title).as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };

    let opened = unsafe { GetOpenFileNameW(&mut dialog) };
    if !opened.as_bool() {
        return None;
    }
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let path = OsString::from_wide(&buffer[..end]);
    Some(std::path::PathBuf::from(path))
}

#[cfg(not(windows))]
fn pick_file(_title: &str, _filter: &str) -> Option<std::path::PathBuf> {
    None
}

#[cfg(windows)]
fn save_file(title: &str, filter: &str, default_name: &str) -> Option<std::path::PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows::Win32::UI::Controls::Dialogs::{
        GetSaveFileNameW, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let mut filter_wide: Vec<u16> = Vec::new();
    let (label, pattern) = filter.split_once(' ').unwrap_or(("Files", "*.*"));
    for part in [label, pattern] {
        filter_wide.extend(wide(part));
    }
    filter_wide.push(0);
    filter_wide.push(0);

    let mut buffer = vec![0u16; 1024];
    for (i, ch) in default_name.encode_utf16().enumerate() {
        if i + 1 < buffer.len() {
            buffer[i] = ch;
        }
    }
    let title_wide = wide(title);
    let def_ext = wide("json");
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: windows::core::PCWSTR(filter_wide.as_ptr()),
        lpstrFile: windows::core::PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: windows::core::PCWSTR(title_wide.as_ptr()),
        lpstrDefExt: windows::core::PCWSTR(def_ext.as_ptr()),
        Flags: OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        ..Default::default()
    };

    let saved = unsafe { GetSaveFileNameW(&mut dialog) };
    if !saved.as_bool() {
        return None;
    }
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let path = OsString::from_wide(&buffer[..end]);
    Some(std::path::PathBuf::from(path))
}

#[cfg(not(windows))]
fn save_file(_title: &str, _filter: &str, _default_name: &str) -> Option<std::path::PathBuf> {
    None
}
