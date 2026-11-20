//! System tray icon and menu.
//!
//! The tray is the fallback way to reach the app when the overlay is hidden or
//! in click-through mode. Menu items use stable ids so the click handler does
//! not depend on label text.

use crate::{resource_path, APP_NAME, APP_VERSION};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub const ID_TOGGLE: &str = "toggle";
pub const ID_RESET: &str = "reset";
pub const ID_SETTINGS: &str = "settings";
pub const ID_VISIBILITY: &str = "visibility";
pub const ID_EXIT: &str = "exit";

/// What the user picked from the tray menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Toggle,
    Reset,
    Settings,
    ToggleVisibility,
    Exit,
}

pub struct Tray {
    // Held so the icon stays alive for the process lifetime.
    _icon: Option<TrayIcon>,
}

impl Tray {
    pub fn is_available(&self) -> bool {
        self._icon.is_some()
    }

    pub fn new() -> Self {
        let menu = Menu::new();
        let toggle = MenuItem::with_id(ID_TOGGLE, "Start / Pause", true, None);
        let reset = MenuItem::with_id(ID_RESET, "Reset", true, None);
        let settings = MenuItem::with_id(ID_SETTINGS, "Settings\u{2026}", true, None);
        let visibility = MenuItem::with_id(ID_VISIBILITY, "Show / Hide Window", true, None);
        let exit = MenuItem::with_id(ID_EXIT, "Exit", true, None);
        let separator = PredefinedMenuItem::separator();

        let _ = menu.append_items(&[
            &toggle,
            &reset,
            &separator,
            &settings,
            &visibility,
            &separator,
            &exit,
        ]);

        let icon = Icon::from_path(resource_path(crate::ICON_FILENAME), Some((32, 32))).ok();
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(format!("{APP_NAME} {APP_VERSION}"))
            .with_menu_on_left_click(false);
        if let Some(icon) = icon {
            builder = builder.with_icon(icon);
        }

        let icon = match builder.build() {
            Ok(icon) => Some(icon),
            Err(err) => {
                log::warn!("Tray icon unavailable: {err}");
                None
            }
        };

        Tray { _icon: icon }
    }

    /// Drain pending menu clicks.
    pub fn poll(&self) -> Vec<TrayCommand> {
        let mut commands = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let command = match event.id.0.as_str() {
                ID_TOGGLE => Some(TrayCommand::Toggle),
                ID_RESET => Some(TrayCommand::Reset),
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
