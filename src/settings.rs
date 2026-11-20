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
}

#[derive(Default)]
struct TextFields {
    countdown: String,
    clock_format: String,
    pomo_work: String,
    pomo_break: String,
    pomo_long: String,
    threshold: String,
    sprite: String,
    sound: String,
    font_family: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileDialogKind {
    Sprite,
    Sound,
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
    TopRight,
    TopCenter,
    BottomCenter,
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
        self.text = TextFields {
            countdown: format_input(config.countdown_start),
            clock_format: config.clock_format.clone(),
            pomo_work: format!("{}", trim(config.pomo_work)),
            pomo_break: format!("{}", trim(config.pomo_break)),
            pomo_long: format!("{}", trim(config.pomo_long_break)),
            threshold: format!("{}", trim(config.threshold_time)),
            sprite: config.sprite_path.clone(),
            sound: config.custom_sound_path.clone(),
            font_family: config.font_family.clone(),
        };
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
            self.commit(app);
        }

        let mut outcome = Outcome::None;

        // The settings dialog lives in its own native window. Drawing it into
        // the overlay's viewport would clip it to the timer's small rectangle.
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("settings-window"),
            egui::ViewportBuilder::default()
                .with_title(format!("{APP_NAME} Settings \u{2014} {APP_VERSION}"))
                .with_inner_size([640.0, 720.0])
                .with_min_inner_size([520.0, 420.0])
                .with_resizable(true)
                .with_transparent(false)
                .with_always_on_top(),
            |ctx, _class| {
                let background = egui::Color32::from_rgb(27, 27, 27);
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
                        self.tab_bar(ui);
                    });

                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(background).inner_margin(egui::Margin::same(8.0)))
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let tab_outcome = match self.tab {
                                    Tab::General => {
                                        self.general_tab(ui);
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

    fn general_tab(&mut self, ui: &mut egui::Ui) {
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
                }

                if mode == Mode::Clock {
                    ui.label("Clock Format");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.text.clock_format)
                            .hint_text("%H:%M:%S"),
                    );
                    ui.end_row();
                }

                ui.label("Decimal precision");
                ui.add(egui::Slider::new(&mut self.draft.decimals, 0..=3));
                ui.end_row();

                ui.label("Always show hours");
                ui.checkbox(&mut self.draft.always_show_hours, "HH:MM:SS");
                ui.end_row();

                // The threshold only applies to the count-up and countdown modes.
                if matches!(mode, Mode::Up | Mode::Down) {
                    ui.label("Gray threshold");
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
    }

    fn appearance_tab(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("appearance")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Theme");
                egui::ComboBox::from_id_salt("theme")
                    .selected_text(theme_label(self.draft.theme))
                    .show_ui(ui, |ui| {
                        for theme in [Theme::Default, Theme::Chalkboard] {
                            ui.selectable_value(&mut self.draft.theme, theme, theme_label(theme));
                        }
                    });
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

                ui.label("Background shape");
                egui::ComboBox::from_id_salt("shape")
                    .selected_text(shape_label(self.draft.window_shape))
                    .show_ui(ui, |ui| {
                        for shape in [
                            WindowShape::Rectangular,
                            WindowShape::Rounded,
                            WindowShape::Capsule,
                        ] {
                            ui.selectable_value(&mut self.draft.window_shape, shape, shape_label(shape));
                        }
                    });
                ui.end_row();

                ui.label("Padding");
                ui.add(egui::Slider::new(&mut self.draft.padding, 0..=100).suffix(" px"));
                ui.end_row();

                if self.draft.window_shape == WindowShape::Rounded {
                    ui.label("Corner radius");
                    ui.add(
                        egui::Slider::new(&mut self.draft.corner_radius, 0..=100).suffix(" px"),
                    );
                    ui.end_row();
                }

                // The arc has no other use for this value, so it is hidden while
                // another display style is selected.
                if self.draft.display_style == DisplayStyle::Arc {
                    ui.label("Arc thickness");
                    ui.add(
                        egui::Slider::new(&mut self.draft.arc_thickness, 1.0..=50.0).suffix(" px"),
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
                            ui.selectable_value(&mut self.draft.border_style, style, border_label(style));
                        }
                    });
                ui.end_row();

                ui.label("Border thickness");
                ui.add_enabled(
                    self.draft.border_style != BorderStyle::None,
                    egui::Slider::new(&mut self.draft.border_thickness, 1.0..=20.0)
                        .suffix(" px"),
                );
                ui.end_row();

                ui.label("Show background");
                ui.checkbox(&mut self.draft.show_background, "");
                ui.end_row();

                ui.label("Native blur / acrylic");
                ui.add_enabled(
                    platform::IS_WINDOWS,
                    egui::Checkbox::new(&mut self.draft.window_blur, ""),
                );
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

                ui.label("Window opacity");
                let mut opacity = (self.draft.opacity * 100.0).round() as u8;
                if ui
                    .add(egui::Slider::new(&mut opacity, 10..=100).suffix(" %"))
                    .changed()
                {
                    self.draft.opacity = opacity as f32 / 100.0;
                }
                ui.end_row();

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
            });

        ui.add_space(8.0);
        ui.label(egui::RichText::new("Colours").strong());
        colour_row(ui, "Normal color", &mut self.draft.normal_color);
        colour_row(ui, "Threshold color", &mut self.draft.gray_color);
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

    fn behavior_tab(&mut self, ui: &mut egui::Ui, _app: &mut TimerApp) {
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

                ui.label("Start hidden");
                ui.checkbox(&mut self.draft.start_minimized, "Start in the tray");
                ui.end_row();

                ui.label("Reduce motion");
                ui.checkbox(&mut self.draft.reduce_motion, "Disable colour fades");
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
                ui.horizontal(|ui| {
                    for (target, label) in [
                        (SnapTarget::TopLeft, "Top Left"),
                        (SnapTarget::TopRight, "Top Right"),
                        (SnapTarget::TopCenter, "Top Center"),
                        (SnapTarget::BottomCenter, "Bottom Center"),
                    ] {
                        if ui.button(label).clicked() {
                            self.pending_snap = Some(target);
                        }
                    }
                });
                ui.end_row();

                ui.label("Auto check updates");
                ui.checkbox(&mut self.draft.auto_check_updates, "On launch");
                ui.end_row();
            });
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

        // The threshold applies to count-up and countdown only.
        if config.use_threshold && matches!(config.mode, Mode::Up | Mode::Down) {
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
        let (screen_w, screen_h) = primary_screen_size();
        let width = app.config.width as f32;
        let height = app.config.height as f32;
        let margin = 24.0;
        let (x, y) = match target {
            SnapTarget::TopLeft => (margin, margin),
            SnapTarget::TopRight => (screen_w - width - margin, margin),
            SnapTarget::TopCenter => ((screen_w - width) / 2.0, margin),
            SnapTarget::BottomCenter => ((screen_w - width) / 2.0, screen_h - height - margin),
        };
        app.config.pos_x = x.round() as i32;
        app.config.pos_y = y.round() as i32;
        if let Some(ctx) = crate::app::current_context() {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        }
    }

    fn poll_file_dialog(&mut self) {
        let Some(kind) = self.file_dialog.take() else {
            return;
        };
        let (title, filter) = match kind {
            FileDialogKind::Sprite => (
                "Select Sprite Image",
                "Images (*.png *.jpg *.jpeg *.bmp *.gif *.webp)",
            ),
            FileDialogKind::Sound => ("Select WAV Sound", "WAV files (*.wav)"),
        };
        if let Some(path) = pick_file(title, filter) {
            let path = path.to_string_lossy().to_string();
            match kind {
                FileDialogKind::Sprite => {
                    self.text.sprite = path.clone();
                    self.draft.sprite_path = path;
                }
                FileDialogKind::Sound => {
                    self.text.sound = path.clone();
                    self.draft.custom_sound_path = path;
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

fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Up => "Count Up",
        Mode::Down => "Countdown",
        Mode::Clock => "Clock",
        Mode::Pomodoro => "Pomodoro",
    }
}

fn theme_label(theme: Theme) -> &'static str {
    match theme {
        Theme::Default => "Default",
        Theme::Chalkboard => "Chalkboard",
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

/// Fallback primary screen size when the monitor API is unavailable.
fn primary_screen_size() -> (f32, f32) {
    (1920.0, 1080.0)
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
