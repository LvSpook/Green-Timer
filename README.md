# Green Timer

A lightweight, always-on-top timer overlay for Windows, written in Rust. It
supports count-up, countdown, a clock and a Pomodoro workflow, with Text, Arc,
Bar, Fluid and Sprite display styles.

The overlay is a frameless, per-pixel-transparent window that stays above other
applications, so it can sit over a game or any other full-screen window without
stealing clicks.

## Features

- **Four timer modes:** Count Up, Countdown, Clock and Pomodoro
- **Five display styles:** Text, Arc, Bar, Fluid and Sprite
- **Full customisation:** colours, fonts, opacity, padding, corner radius,
  borders, rotation and an optional native Windows blur/acrylic backdrop
- **Global hotkeys** (matched by physical key, so they are layout independent)
- **System tray** with quick actions
- **Auto-pause** when a chosen target application loses focus
- **Automatic update checks** against the GitHub releases API
- Settings are saved per-user in `%APPDATA%\GreenTimer`, so the app works when
  installed under `Program Files`

## Default hotkeys

| Action | Key |
| --- | --- |
| Start / pause | F1 |
| Reset | F2 |
| Open settings | F3 |
| Ghost mode (click-through) | F4 |
| Lap | Space |

Rebind them under **Settings → Hotkeys**. Right-click the overlay for a quick
menu; double-click it to start or pause. Drag the bottom-right corner to scale
the text to fit.

## Running from source

Requires a Rust toolchain (1.80 or newer).

```powershell
cargo run --release
```

Set `RUST_LOG=GreenTimer=debug` (or `info`) for verbose logging.

## Settings file locations

| Item | Path |
| --- | --- |
| Settings | `%APPDATA%\GreenTimer\timer_pro_settings.json` |
| Logs | `%APPDATA%\GreenTimer\logs\green_timer.log` |

Release builds are GUI-only, so no console window appears when the app starts.
Logs are written to the file above (rolled over to `green_timer.log.old` once it
passes 1 MB); `cargo run` in a debug build also prints them to the terminal.

On first launch an older `timer_pro_settings.json` next to the executable is
migrated automatically. A corrupted settings file falls back to defaults field
by field instead of crashing the app.

## Tests

```powershell
cargo test
```

The suite covers time formatting/parsing, config validation and migration, the
timer/Pomodoro state machine, hotkey key-name mapping and update-version
comparison.

## Building

```powershell
powershell -ExecutionPolicy Bypass -File build.ps1
```

This runs the tests, produces `target\release\GreenTimer.exe`, stages it (with
the icon) into `dist\GreenTimer`, and, if [Inno Setup](https://jrsoftware.org/isinfo.php)
is installed, compiles `Output\GreenTimer_Setup_v<version>.exe`.

## Project layout

```
src/
  main.rs               Entry point and shared constants
  app.rs                Overlay window, event loop, hotkey/tray dispatch
  config.rs             Defaults, validation, atomic save, migration
  formatting.rs         Duration formatting and input parsing
  engine.rs             Framework-independent timer state machine
  hotkeys.rs            Global hotkey registration and key recording
  fonts.rs              Font discovery and fallback
  platform.rs           Topmost, click-through, DWM blur, sounds
  render.rs             All painting (styles, outline, rotation)
  settings.rs           Settings dialog (its own native window)
  tray.rs               Tray icon and menu
  updater.rs            Release check and safe installer download
installer.iss           Inno Setup installer
legacy-python/          The previous Python implementation, kept for reference
```
