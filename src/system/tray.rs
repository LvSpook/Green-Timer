//! System tray icon and menu.
//!
//! The tray is the fallback way to reach the app when the overlay is hidden or
//! in click-through mode. Menu items use stable ids so the click handler does
//! not depend on label text, and the tray icon updates to match the active
//! [`Theme`].

use crate::config::Config;
use crate::render::{themed_icon_rgba, ui_palette_for_config};
use crate::{APP_NAME, APP_VERSION};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub const ID_TOGGLE: &str = "toggle";
pub const ID_RESET: &str = "reset";
pub const ID_LAP: &str = "lap";
pub const ID_MINI: &str = "mini";
pub const ID_GHOST: &str = "ghost";
pub const ID_SETTINGS: &str = "settings";
pub const ID_VISIBILITY: &str = "visibility";
pub const ID_EXIT: &str = "exit";

/// What the user picked from the tray menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Toggle,
    Reset,
    Lap,
    ToggleMiniMode,
    ToggleGhost,
    Settings,
    ToggleVisibility,
    Exit,
}

pub struct Tray {
    icon: Option<TrayIcon>,
    last_key: (crate::config::Theme, Option<String>),
}

/// Generate a crisp 32x32 themed timer icon so the system tray stays coherent
/// with the user's active theme (built-in or custom).
fn themed_icon(config: &Config) -> Option<Icon> {
    let (rgba, w, h) = themed_icon_rgba(ui_palette_for_config(config));
    Icon::from_rgba(rgba, w, h).ok()
}

impl Tray {
    pub fn is_available(&self) -> bool {
        self.icon.is_some()
    }

    pub fn new(config: &Config) -> Self {
        let menu = Menu::new();
        let toggle = MenuItem::with_id(ID_TOGGLE, "Start / Pause", true, None);
        let reset = MenuItem::with_id(ID_RESET, "Reset", true, None);
        let lap = MenuItem::with_id(ID_LAP, "Lap", true, None);
        let sep1 = PredefinedMenuItem::separator();
        let mini = MenuItem::with_id(ID_MINI, "Toggle Mini Mode", true, None);
        let ghost = MenuItem::with_id(ID_GHOST, "Toggle Ghost Mode", true, None);
        let visibility = MenuItem::with_id(ID_VISIBILITY, "Show / Hide Window", true, None);
        let sep2 = PredefinedMenuItem::separator();
        let settings = MenuItem::with_id(ID_SETTINGS, "Settings\u{2026}", true, None);
        let exit = MenuItem::with_id(ID_EXIT, "Exit", true, None);

        let _ = menu.append_items(&[
            &toggle,
            &reset,
            &lap,
            &sep1,
            &mini,
            &ghost,
            &visibility,
            &sep2,
            &settings,
            &exit,
        ]);

        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(format!("{APP_NAME} {APP_VERSION}"))
            .with_menu_on_left_click(false);
        if let Some(icon) = themed_icon(config) {
            builder = builder.with_icon(icon);
        }

        let icon = match builder.build() {
            Ok(icon) => Some(icon),
            Err(err) => {
                log::warn!("Tray icon unavailable: {err}");
                None
            }
        };

        Tray {
            icon,
            last_key: (config.theme, config.active_custom_theme.clone()),
        }
    }

    /// Update the tray icon whenever the user switches themes.
    pub fn set_theme(&mut self, config: &Config) {
        let key = (config.theme, config.active_custom_theme.clone());
        if self.last_key == key {
            return;
        }
        self.last_key = key;
        if let (Some(tray), Some(icon)) = (&self.icon, themed_icon(config)) {
            let _ = tray.set_icon(Some(icon));
        }
    }

    /// Drain pending menu clicks and tray icon left-clicks.
    pub fn poll(&self) -> Vec<TrayCommand> {
        let mut commands = Vec::new();
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                commands.push(TrayCommand::ToggleVisibility);
            }
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let command = match event.id.0.as_str() {
                ID_TOGGLE => Some(TrayCommand::Toggle),
                ID_RESET => Some(TrayCommand::Reset),
                ID_LAP => Some(TrayCommand::Lap),
                ID_MINI => Some(TrayCommand::ToggleMiniMode),
                ID_GHOST => Some(TrayCommand::ToggleGhost),
                ID_SETTINGS => Some(TrayCommand::Settings),
                ID_VISIBILITY => Some(TrayCommand::ToggleVisibility),
                ID_EXIT => Some(TrayCommand::Exit),
                _ => None,
            };
            if let Some(command) = command {
                commands.push(command);
            }
        }
        commands
    }
}
