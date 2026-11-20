//! Green Timer — a lightweight, always-on-top timer overlay for Windows.

mod app;
mod config;
mod engine;
mod fonts;
mod formatting;
mod hotkeys;
mod platform;
mod render;
mod settings;
mod tray;
mod updater;

pub const APP_NAME: &str = "Green Timer";
pub const APP_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
pub const ICON_FILENAME: &str = "green_timer.ico";

/// Path to a bundled resource, next to the executable.
pub fn resource_path(name: &str) -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|path| path.exists())
        .unwrap_or_else(|| std::path::PathBuf::from(name))
}

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    app::run()
}
