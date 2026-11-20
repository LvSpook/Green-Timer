//! Green Timer — a lightweight, always-on-top timer overlay for Windows.

// Release builds are GUI-only: no console window appears when the app is
// launched (including double-clicked from Explorer). Debug builds keep the
// console so `cargo run` still shows log output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
    init_logging();
    app::run()
}

/// Initialise logging. Release builds have no console, so everything is also
/// written to `%APPDATA%\GreenTimer\logs\green_timer.log` for troubleshooting.
fn init_logging() {
    let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let mut builder = env_logger::Builder::new();
    builder.parse_filters(&level);

    if let Some(dir) = log_dir() {
        if std::fs::create_dir_all(&dir).is_ok() {
            let path = dir.join("green_timer.log");
            // Keep the log from growing without bound across launches.
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.len() > 1_000_000 {
                    let _ = std::fs::rename(&path, dir.join("green_timer.log.old"));
                }
            }
            if let Ok(file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                builder.target(env_logger::Target::Pipe(Box::new(SharedFile(
                    std::sync::Mutex::new(file),
                ))));
                builder.init();
                log::info!("Starting {APP_NAME} {APP_VERSION}");
                log::info!("Log file: {}", path.display());
                return;
            }
        }
    }

    // No writable log location: fall back to the default (stderr).
    let _ = builder.try_init();
}

fn log_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))?;
    Some(base.join("GreenTimer").join("logs"))
}

/// A writer that can be shared by `env_logger` from any thread. Debug builds
/// also echo to stderr so `cargo run` still shows logs.
struct SharedFile(std::sync::Mutex<std::fs::File>);

impl std::io::Write for SharedFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if cfg!(debug_assertions) {
            let _ = std::io::stderr().write_all(buf);
        }
        self.0
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "log lock poisoned"))?
            .write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if cfg!(debug_assertions) {
            let _ = std::io::stderr().flush();
        }
        self.0
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "log lock poisoned"))?
            .flush()
    }
}
