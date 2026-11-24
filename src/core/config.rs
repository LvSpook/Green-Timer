//! Configuration: defaults, validation, persistence and migration.
//!
//! Settings live in `%APPDATA%\GreenTimer\timer_pro_settings.json`. The legacy
//! location (next to the executable) is migrated once on first run. Writes are
//! atomic so a crash can never leave a truncated file behind.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const CONFIG_FILENAME: &str = "timer_pro_settings.json";

/// Longest note the editor accepts, in characters.
pub const NOTE_MAX_CHARS: usize = 2_000;
/// Smallest side the note window can be dragged to, in points.
pub const NOTE_MIN_SIDE: u32 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Up,
    Down,
    Clock,
    Pomodoro,
    Interval,
    TargetTime,
}

impl Mode {
    pub const ALL: [Mode; 6] = [
        Mode::Up,
        Mode::Down,
        Mode::Clock,
        Mode::Pomodoro,
        Mode::Interval,
        Mode::TargetTime,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DisplayStyle {
    #[default]
    Text,
    Arc,
    Bar,
    Fluid,
    Sprite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WindowShape {
    #[default]
    Rectangular,
    Rounded,
    Capsule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BorderStyle {
    #[default]
    None,
    Thin,
    Neon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Rotation {
    #[serde(rename = "0°")]
    #[default]
    None,
    #[serde(rename = "90°")]
    Cw90,
    #[serde(rename = "270°")]
    Cw270,
}

impl Rotation {
    pub fn degrees(self) -> f32 {
        match self {
            Rotation::None => 0.0,
            Rotation::Cw90 => 90.0,
            Rotation::Cw270 => 270.0,
        }
    }

    pub fn is_vertical(self) -> bool {
        !matches!(self, Rotation::None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    Default,
    Chalkboard,
    Cyberpunk,
    MinimalGlass,
    RetroLcd,
    WarmPaper,
    HighContrast,
}

impl Theme {
    pub const ALL: [Theme; 7] = [
        Theme::Default,
        Theme::Chalkboard,
        Theme::Cyberpunk,
        Theme::MinimalGlass,
        Theme::RetroLcd,
        Theme::WarmPaper,
        Theme::HighContrast,
    ];
}

/// A user-created theme saved in Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomTheme {
    pub name: String,
    pub background: Rgb,
    pub text_color: Rgb,
    pub warning_color: Rgb,
    pub border_color: Rgb,
    pub dark_mode: bool,
}

impl Default for CustomTheme {
    fn default() -> Self {
        CustomTheme {
            name: String::new(),
            background: Rgb(24, 28, 36),
            text_color: Rgb(0, 230, 140),
            warning_color: Rgb(255, 90, 90),
            border_color: Rgb(90, 110, 140),
            dark_mode: true,
        }
    }
}

/// Which items appear in the right-click context menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextMenuConfig {
    pub start_pause: bool,
    pub reset: bool,
    pub lap: bool,
    pub quick_adjust: bool,
    pub copy_time: bool,
    pub mini_mode: bool,
    pub ghost_mode: bool,
    pub profiles: bool,
    pub settings: bool,
    pub exit: bool,
}

impl Default for ContextMenuConfig {
    fn default() -> Self {
        ContextMenuConfig {
            start_pause: true,
            reset: true,
            lap: true,
            quick_adjust: true,
            copy_time: true,
            mini_mode: true,
            ghost_mode: true,
            profiles: true,
            settings: true,
            exit: true,
        }
    }
}

/// A named snapshot of settings that can be switched to in one click.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedProfile {
    pub name: String,
    pub settings: serde_json::Value,
}

/// Automatic profile switch when a specific window (HWND) gains focus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppProfileRule {
    pub target_hwnd: u64,
    pub profile_name: String,
}

/// A colour stored as an `#RRGGBB` string, matching the Python file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(value: &str, fallback: Rgb) -> Rgb {
        let trimmed = value.trim().trim_start_matches('#');
        if trimmed.len() != 6 {
            return fallback;
        }
        match (
            u8::from_str_radix(&trimmed[0..2], 16),
            u8::from_str_radix(&trimmed[2..4], 16),
            u8::from_str_radix(&trimmed[4..6], 16),
        ) {
            (Ok(r), Ok(g), Ok(b)) => Rgb(r, g, b),
            _ => fallback,
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }
}

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(Rgb::parse(&raw, Rgb(0, 0, 0)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    pub name: String,
    pub code: u32, // scan code
}

impl Hotkey {
    pub fn new(name: &str, code: u32) -> Self {
        Hotkey { name: name.to_string(), code }
    }

    /// No key assigned yet.
    pub fn unbound() -> Self {
        Hotkey {
            name: String::new(),
            code: 0,
        }
    }

    pub fn is_bound(&self) -> bool {
        self.code != 0
    }
}

/// A user-added extra binding for one of the built-in actions.
///
/// The action is stored by its stable id rather than an index, so reordering
/// the action list can never silently rebind someone's keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomHotkey {
    pub action: String,
    pub key: Hotkey,
}

impl Default for CustomHotkey {
    fn default() -> Self {
        CustomHotkey {
            action: "toggle".to_string(),
            key: Hotkey::unbound(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(deserialize_with = "lenient")]
    pub mode: Mode,
    pub countdown_start: f64,
    pub pomo_work: f64,
    pub pomo_break: f64,
    pub pomo_long_break: f64,
    pub pomo_cycles: u32,
    pub clock_format: String,
    pub threshold_time: f64,
    pub use_threshold: bool,
    pub font_size: u32,
    pub font_family: String,
    pub normal_color: Rgb,
    pub gray_color: Rgb,
    pub decimals: u8,
    pub always_show_hours: bool,
    pub show_text: bool,
    pub hk_toggle: Hotkey,
    pub hk_reset: Hotkey,
    pub hk_settings: Hotkey,
    pub hk_lap: Hotkey,
    pub hk_ghost: Hotkey,
    pub ghost_mode: bool,
    pub sound_cue: bool,
    pub custom_sound_path: String,
    pub opacity: f32,
    pub text_outline: bool,
    pub text_outline_color: Rgb,
    pub text_outline_thickness: f32,
    #[serde(deserialize_with = "lenient")]
    pub theme: Theme,
    #[serde(deserialize_with = "lenient")]
    pub border_style: BorderStyle,
    pub border_thickness: f32,
    #[serde(deserialize_with = "lenient")]
    pub window_shape: WindowShape,
    #[serde(deserialize_with = "lenient")]
    pub display_style: DisplayStyle,
    pub sprite_path: String,
    pub arc_thickness: f32,
    pub window_locked: bool,
    pub pos_x: i32,
    pub pos_y: i32,
    pub width: u32,
    pub height: u32,
    pub target_hwnd: u64,
    pub padding: u32,
    pub corner_radius: u32,
    pub window_blur: bool,
    pub show_background: bool,
    pub thin_border_color: Rgb,
    pub bar_text_color: Rgb,
    #[serde(deserialize_with = "lenient")]
    pub rotation: Rotation,
    pub auto_check_updates: bool,
    /// Also offer pre-release (work in progress) builds when checking updates.
    pub include_prereleases: bool,
    pub reduce_motion: bool,
    pub start_minimized: bool,
    pub hide_leading_zeros: bool,
    /// The right-click tip is still to be shown. Enabled only on a brand-new
    /// install, so it never appears on an update.
    pub show_first_run_tip: bool,
    pub interval_work: f64,
    pub interval_rest: f64,
    pub interval_rounds: u32,
    pub target_time: String,
    pub auto_start_on_launch: bool,
    pub auto_reset_on_zero: bool,
    pub warning_beeps: bool,
    pub use_gradient: bool,
    pub gradient_end_color: Rgb,
    pub pulse_on_warning: bool,
    pub custom_label: String,
    pub mini_mode: bool,
    pub use_text_shadow: bool,
    pub hover_to_fade: bool,
    pub hover_fade_opacity: f32,
    pub edge_snapping: bool,
    pub hk_visibility: Hotkey,
    pub hk_mini_mode: Hotkey,
    /// Extra user-added bindings, in addition to the seven built-in ones.
    pub hk_custom: Vec<CustomHotkey>,
    /// The single free-form note. Empty when no note is saved.
    pub note: String,
    /// Note text colour. `None` follows the timer's numbers, so the two stay in
    /// step until the user picks something else.
    pub note_text_color: Option<Rgb>,
    /// Note window size in points. The note is not forced square, so this is
    /// kept as its own width and height.
    pub note_width: u32,
    pub note_height: u32,
    /// Whether the note window's edges can be dragged to resize it.
    pub note_resizable: bool,
    pub context_menu: ContextMenuConfig,
    pub profiles: Vec<SavedProfile>,
    pub app_profile_rules: Vec<AppProfileRule>,
    pub custom_themes: Vec<CustomTheme>,
    pub active_custom_theme: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: Mode::Up,
            countdown_start: 60.0,
            pomo_work: 25.0,
            pomo_break: 5.0,
            pomo_long_break: 15.0,
            pomo_cycles: 4,
            clock_format: "%H:%M:%S".to_string(),
            threshold_time: 60.0,
            use_threshold: true,
            font_size: 42,
            font_family: "Segoe UI".to_string(),
            normal_color: Rgb(0x00, 0xFF, 0x66),
            gray_color: Rgb(0x80, 0x80, 0x80),
            decimals: 2,
            always_show_hours: false,
            show_text: true,
            hk_toggle: Hotkey::new("f1", 59),
            hk_reset: Hotkey::new("f2", 60),
            hk_settings: Hotkey::new("f3", 61),
            hk_lap: Hotkey::new("space", 57),
            hk_ghost: Hotkey::new("f4", 62),
            ghost_mode: false,
            sound_cue: true,
            custom_sound_path: String::new(),
            opacity: 1.0,
            text_outline: true,
            text_outline_color: Rgb(0, 0, 0),
            text_outline_thickness: 2.0,
            theme: Theme::Default,
            border_style: BorderStyle::None,
            border_thickness: 1.0,
            window_shape: WindowShape::Rectangular,
            display_style: DisplayStyle::Text,
            sprite_path: String::new(),
            arc_thickness: 12.0,
            window_locked: false,
            pos_x: -1,
            pos_y: -1,
            width: 300,
            height: 120,
            target_hwnd: 0,
            padding: 20,
            corner_radius: 15,
            window_blur: false,
            show_background: true,
            thin_border_color: Rgb(0xFF, 0xFF, 0xFF),
            bar_text_color: Rgb(0xFF, 0xFF, 0xFF),
            rotation: Rotation::None,
            auto_check_updates: true,
            include_prereleases: false,
            reduce_motion: false,
            start_minimized: false,
            hide_leading_zeros: false,
            show_first_run_tip: false,
            interval_work: 40.0,
            interval_rest: 20.0,
            interval_rounds: 8,
            target_time: "17:00".to_string(),
            auto_start_on_launch: false,
            auto_reset_on_zero: false,
            warning_beeps: false,
            use_gradient: false,
            gradient_end_color: Rgb(0xFF, 0x50, 0x3C),
            pulse_on_warning: false,
            custom_label: String::new(),
            mini_mode: false,
            use_text_shadow: false,
            hover_to_fade: false,
            hover_fade_opacity: 0.2,
            edge_snapping: true,
            hk_visibility: Hotkey::new("f11", 87),
            hk_mini_mode: Hotkey::new("f12", 88),
            hk_custom: Vec::new(),
            note: String::new(),
            note_text_color: None,
            note_width: 280,
            note_height: 280,
            note_resizable: true,
            context_menu: ContextMenuConfig::default(),
            profiles: Vec::new(),
            app_profile_rules: Vec::new(),
            custom_themes: Vec::new(),
            active_custom_theme: None,
        }
    }
}

impl Config {
    /// Returns the currently active custom theme, if one is selected and exists.
    pub fn active_custom_theme_data(&self) -> Option<&CustomTheme> {
        let name = self.active_custom_theme.as_deref()?;
        self.custom_themes.iter().find(|t| t.name == name)
    }
    /// The binding for `action`.
    pub fn hotkey(&self, action: crate::hotkeys::Action) -> &Hotkey {
        use crate::hotkeys::Action;
        match action {
            Action::Toggle => &self.hk_toggle,
            Action::Reset => &self.hk_reset,
            Action::Settings => &self.hk_settings,
            Action::Lap => &self.hk_lap,
            Action::Ghost => &self.hk_ghost,
            Action::ToggleVisibility => &self.hk_visibility,
            Action::ToggleMiniMode => &self.hk_mini_mode,
        }
    }

    /// The binding for `action`, mutably.
    pub fn hotkey_mut(&mut self, action: crate::hotkeys::Action) -> &mut Hotkey {
        use crate::hotkeys::Action;
        match action {
            Action::Toggle => &mut self.hk_toggle,
            Action::Reset => &mut self.hk_reset,
            Action::Settings => &mut self.hk_settings,
            Action::Lap => &mut self.hk_lap,
            Action::Ghost => &mut self.hk_ghost,
            Action::ToggleVisibility => &mut self.hk_visibility,
            Action::ToggleMiniMode => &mut self.hk_mini_mode,
        }
    }

    /// Capture a JSON snapshot of this config (excluding nested profiles/rules)
    /// suitable for storing as a named profile.
    pub fn snapshot_for_profile(&self) -> serde_json::Value {
        let mut clone = self.clone();
        clone.profiles.clear();
        clone.app_profile_rules.clear();
        serde_json::to_value(clone).unwrap_or(serde_json::Value::Null)
    }

    /// Apply a saved profile onto this config while preserving the profile list
    /// and per-app rules.
    pub fn apply_profile(&mut self, name: &str) -> bool {
        let Some(profile) = self.profiles.iter().find(|p| p.name == name) else {
            return false;
        };
        let Ok(mut loaded) = serde_json::from_value::<Config>(profile.settings.clone()) else {
            return false;
        };
        loaded.profiles = self.profiles.clone();
        loaded.app_profile_rules = self.app_profile_rules.clone();
        *self = loaded.validated();
        true
    }

    /// Clamp every value into a sane range, replacing anything nonsensical.
    /// Mirrors the validation the Python version performed so a hand-edited
    /// file can never crash the app.
    pub fn validated(mut self) -> Config {
        self.countdown_start = clamp_f(self.countdown_start, 1.0, 359_999.0, 60.0);
        self.threshold_time = clamp_f(self.threshold_time, 0.0, 359_999.0, 60.0);
        self.pomo_work = clamp_f(self.pomo_work, 0.1, 600.0, 25.0);
        self.pomo_break = clamp_f(self.pomo_break, 0.1, 600.0, 5.0);
        self.pomo_long_break = clamp_f(self.pomo_long_break, 0.1, 600.0, 15.0);
        self.pomo_cycles = self.pomo_cycles.clamp(1, 100);
        self.interval_work = clamp_f(self.interval_work, 1.0, 3600.0, 40.0);
        self.interval_rest = clamp_f(self.interval_rest, 0.0, 3600.0, 20.0);
        self.interval_rounds = self.interval_rounds.clamp(1, 200);
        if self.target_time.trim().is_empty() {
            self.target_time = "17:00".to_string();
        }
        self.hover_fade_opacity = self.hover_fade_opacity.clamp(0.05, 1.0);
        self.decimals = self.decimals.min(3);
        self.font_size = self.font_size.clamp(4, 500);
        self.opacity = self.opacity.clamp(0.1, 1.0);
        self.padding = self.padding.clamp(0, 100);
        self.corner_radius = self.corner_radius.clamp(0, 100);
        self.border_thickness = self.border_thickness.clamp(1.0, 20.0);
        self.text_outline_thickness = self.text_outline_thickness.clamp(1.0, 15.0);
        self.arc_thickness = self.arc_thickness.clamp(1.0, 50.0);
        self.width = self.width.clamp(80, 10_000);
        self.height = self.height.clamp(40, 10_000);
        if self.clock_format.trim().is_empty() {
            self.clock_format = "%H:%M:%S".to_string();
        }
        if self.font_family.trim().is_empty() {
            self.font_family = "Segoe UI".to_string();
        }
        // A note is one short reminder, not a document; the cap also keeps a
        // corrupted file from bloating the settings.
        self.note = self.note.chars().take(NOTE_MAX_CHARS).collect();
        self.note_width = self.note_width.clamp(NOTE_MIN_SIDE, 2_000);
        self.note_height = self.note_height.clamp(NOTE_MIN_SIDE, 2_000);
        self
    }
}

fn clamp_f(value: f64, low: f64, high: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        fallback
    }
}

/// Deserialize a value, falling back to its default when it is unparsable.
///
/// This keeps a hand-edited or forward-compatible settings file useful: one
/// bad enum string degrades to a sensible default instead of discarding every
/// other preference in the file.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// `%APPDATA%\GreenTimer` on Windows, with sensible fallbacks elsewhere.
pub fn config_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }
    .unwrap_or_else(|| PathBuf::from("."));
    base.join("GreenTimer")
}

pub fn config_path() -> PathBuf {
    config_dir().join(CONFIG_FILENAME)
}

/// The pre-1.2 location: alongside the executable.
pub fn legacy_config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(CONFIG_FILENAME)))
        .unwrap_or_else(|| PathBuf::from(CONFIG_FILENAME))
}

fn read_config(path: &Path) -> Option<Config> {
    let text = fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Config>(&text) {
        Ok(cfg) => Some(cfg.validated()),
        Err(err) => {
            log::warn!("Could not parse {}: {err}", path.display());
            None
        }
    }
}

/// Whether this is a brand-new install: no settings and no legacy file.
///
/// Checked before [`load`], which writes a settings file out on first launch.
pub fn is_first_run() -> bool {
    !config_path().exists() && !legacy_config_path().exists()
}

/// Load settings, migrating the legacy file on first run.
pub fn load() -> Config {
    let path = config_path();
    if !path.exists() {
        let legacy = legacy_config_path();
        if legacy.exists() && legacy != path {
            if let Some(cfg) = read_config(&legacy) {
                if save(&cfg).is_ok() {
                    log::info!("Migrated settings from {}", legacy.display());
                }
                return cfg;
            }
        }
    }
    read_config(&path).unwrap_or_default()
}

/// Export settings to an arbitrary JSON file.
pub fn export_to_path(config: &Config, path: &Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

/// Import settings from an arbitrary JSON file.
pub fn import_from_path(path: &Path) -> Result<Config, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let parsed: Config = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(parsed.validated())
}

/// Atomically write settings.
pub fn save(config: &Config) -> std::io::Result<()> {
    let path = config_path();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;

    let json = serde_json::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let temp = dir.join(format!(".settings-{}.tmp", std::process::id()));
    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
    }
    // `rename` replaces the destination atomically on Windows and Unix.
    match fs::rename(&temp, &path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&temp);
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let cfg = Config::default();
        let again = cfg.clone().validated();
        assert_eq!(cfg.pomo_work, again.pomo_work);
    }

    #[test]
    fn colour_parsing() {
        assert_eq!(Rgb::parse("#00FF66", Rgb(1, 2, 3)), Rgb(0, 255, 102));
        assert_eq!(Rgb::parse("00ff66", Rgb(1, 2, 3)), Rgb(0, 255, 102));
        assert_eq!(Rgb::parse("nonsense", Rgb(1, 2, 3)), Rgb(1, 2, 3));
        assert_eq!(Rgb::parse("#GGGGGG", Rgb(1, 2, 3)), Rgb(1, 2, 3));
        assert_eq!(Rgb(0, 255, 102).to_hex(), "#00FF66");
    }

    #[test]
    fn round_trips_through_json() {
        let cfg = Config::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.normal_color, cfg.normal_color);
        assert_eq!(back.mode, cfg.mode);
        assert_eq!(back.rotation, cfg.rotation);
        assert_eq!(back.hk_toggle.code, 59);
    }

    #[test]
    fn the_note_survives_a_save_and_reload() {
        let mut cfg = Config::default();
        cfg.note = "Buy milk\nCall back \u{2014} 09:30".to_string();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.note, cfg.note, "newlines and accents are preserved");
    }

    #[test]
    fn an_overlong_note_is_truncated_on_load() {
        let cfg = Config::default();
        let mut too_long = cfg.clone();
        too_long.note = "x".repeat(NOTE_MAX_CHARS + 500);
        let cfg = too_long.validated();
        assert_eq!(cfg.note.chars().count(), NOTE_MAX_CHARS);
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        let mut cfg = Config::default();
        // Three bytes each, so a byte-based cut would split one in half.
        cfg.note = "\u{2713}".repeat(NOTE_MAX_CHARS + 3);
        let cfg = cfg.validated();
        assert_eq!(cfg.note.chars().count(), NOTE_MAX_CHARS);
    }

    #[test]
    fn the_note_is_not_shared_between_profiles() {
        let mut cfg = Config::default();
        cfg.note = "profile-specific".to_string();
        cfg.profiles.push(SavedProfile {
            name: "Work".to_string(),
            settings: serde_json::to_value(Config::default()).unwrap(),
        });
        let restored = cfg
            .profiles
            .iter()
            .find(|p| p.name == "Work")
            .and_then(|p| serde_json::from_value::<Config>(p.settings.clone()).ok())
            .map(|c| c.validated());
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(
            json.contains("profile-specific"),
            "the note is written to the settings file"
        );
        assert!(
            restored.is_some_and(|c| c.note.is_empty()),
            "loading a profile does not carry the current note over"
        );
    }

    #[test]
    fn missing_fields_use_defaults() {
        // Simulates an old settings file that predates newer options.
        let json = r##"{"mode":"down","pomo_work":30.5,"normal_color":"#123456"}"##;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.mode, Mode::Down);
        assert_eq!(cfg.pomo_work, 30.5);
        assert_eq!(cfg.normal_color, Rgb(0x12, 0x34, 0x56));
        assert_eq!(cfg.font_family, "Segoe UI");
        assert!(!cfg.reduce_motion);
        // Newer than the file: no note and pre-release updates off.
        assert!(cfg.note.is_empty());
        assert!(!cfg.include_prereleases);
        assert!(cfg.auto_check_updates);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{"mode":"up","future_option":true}"#;
        assert!(serde_json::from_str::<Config>(json).is_ok());
    }

    #[test]
    fn rotation_uses_degree_strings() {
        let json = r#"{"rotation":"90\u00b0"}"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.rotation, Rotation::Cw90);
        let encoded = serde_json::to_string(&Rotation::Cw270).unwrap();
        assert!(encoded.contains("270"), "{encoded}");
    }

    #[test]
    fn validation_clamps_bad_values() {
        let cfg = Config {
            opacity: 5.0,
            decimals: 99,
            padding: 100_000,
            countdown_start: f64::NAN,
            ..Config::default()
        };
        let cfg = cfg.validated();
        assert_eq!(cfg.opacity, 1.0);
        assert_eq!(cfg.decimals, 3);
        assert_eq!(cfg.padding, 100);
        assert_eq!(cfg.countdown_start, 60.0);
    }

    #[test]
    fn custom_hotkeys_are_optional_and_round_trip() {
        // Old settings files have no custom bindings at all.
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.hk_custom.is_empty());

        let cfg = Config {
            hk_custom: vec![CustomHotkey {
                action: "lap".to_string(),
                key: Hotkey::new("f9", 67),
            }],
            ..Config::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.hk_custom, cfg.hk_custom);
    }

    #[test]
    fn invalid_enum_value_falls_back_per_field() {
        let json = r##"{"mode":"nonsense","pomo_work":12.0,"normal_color":"#00FF00"}"##;
        let cfg: Config = serde_json::from_str(json).unwrap();
        // The bad enum degrades to its default...
        assert_eq!(cfg.mode, Mode::Up);
        // ...but every valid preference in the file is preserved.
        assert_eq!(cfg.pomo_work, 12.0);
        assert_eq!(cfg.normal_color, Rgb(0, 255, 0));
    }
}
