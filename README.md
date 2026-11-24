<div align="center">

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/banner-light.svg">
  <source media="(prefers-color-scheme: dark)" srcset="docs/banner.svg">
  <img src="docs/banner.svg" alt="Green Timer — a tiny, always-on-top timer overlay for Windows" width="100%">
</picture>

<br>

[![Latest release](https://img.shields.io/github/v/release/LvSpook/Green-Timer?include_prereleases&label=release&color=00d250)](https://github.com/LvSpook/Green-Timer/releases)
[![Status](https://img.shields.io/badge/status-work%20in%20progress-f5a623)](#-work-in-progress)
[![Platform](https://img.shields.io/badge/platform-Windows-00d250)](https://github.com/LvSpook/Green-Timer/releases)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-00d250)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-00d250)](LICENSE)

**A lightweight, always-on-top timer overlay for Windows.**
Count up, count down, run a Pomodoro, or count to a specific time — over any window, without stealing clicks.

[Download](#-download) · [Features](#-features) · [Hotkeys](#-default-hotkeys) · [Docs site](https://lvspook.github.io/Green-Timer/) · [AI disclosure](#-ai-assistance-disclosure)

</div>

---

> [!WARNING]
> ## 🚧 Work in Progress
> Green Timer is **actively in development**. Builds are published as pre-releases and the UI, settings and file formats may still change between versions. Expect rough edges, and please report anything broken via [Issues](https://github.com/LvSpook/Green-Timer/issues).

---

## ✨ Features

<table>
<tr>
<td width="50%" valign="top">

**Timer modes**
- ⏱️ Count Up
- ⏳ Countdown
- 🕐 Clock
- 🍅 Pomodoro
- 🏃 Interval / HIIT
- 🎯 Target Time

**Display styles**
- Text
- Arc
- Bar
- Fluid
- Sprite

</td>
<td width="50%" valign="top">

**Look and feel**
- 7 built-in themes + custom theme builder
- Colours, fonts, opacity, padding, corner radius
- Gradient fills, text shadows, warning pulses
- Optional Windows blur/acrylic backdrop
- Rotation, with background and assets rotating too

**Behaviour**
- Global hotkeys (layout independent, by scan code)
- System tray with quick actions
- Customisable right-click menu
- Saved profiles and per-app auto-switch
- Auto-pause when a target app loses focus
- Lap history with CSV export
- Import/export settings as JSON

</td>
</tr>
</table>

The overlay is a frameless, per-pixel-transparent window that stays above other
applications, so it can sit over a game or any other full-screen window without
stealing clicks.

## 📥 Download

Grab the installer from the [latest release](https://github.com/LvSpook/Green-Timer/releases):

| File | What it is |
| --- | --- |
| `GreenTimer_Setup_v<version>.exe` | Windows installer (per-user, no admin required) |
| `GreenTimer.exe` | Standalone portable executable (icon included, nothing else needed) |

> Pre-release builds are **work in progress** and may be rough around the edges.

## ⌨️ Default hotkeys

| Action | Key |
| --- | --- |
| Start / pause | `F1` |
| Reset | `F2` |
| Open settings | `F3` |
| Ghost mode (click-through) | `F4` |
| Lap | `Space` |
| Show / hide timer | `F11` |
| Toggle mini mode | `F12` |

Rebind them under **Settings → Hotkeys**. Right-click the overlay for a quick
menu; double-click it to start or pause. Drag any corner to scale the text to
fit.

## 🔨 Running from source

Requires a Rust toolchain (1.80 or newer).

```powershell
cargo run --release
```

Set `RUST_LOG=GreenTimer=debug` (or `info`) for verbose logging.

### Building the installer

```powershell
powershell -ExecutionPolicy Bypass -File build.ps1
```

This runs the tests, produces `target\release\GreenTimer.exe`, stages it (with
the icon) into `dist\GreenTimer`, and, if [Inno Setup](https://jrsoftware.org/isinfo.php)
is installed, compiles `Output\GreenTimer_Setup_v<version>.exe`.

Release builds are GUI-only, so no console window appears when the app starts.
Logs are written to the file below (rolled over to `green_timer.log.old` once it
passes 1 MB); `cargo run` in a debug build also prints them to the terminal.

## 📁 Settings file locations

| Item | Path |
| --- | --- |
| Settings | `%APPDATA%\GreenTimer\timer_pro_settings.json` |
| Logs | `%APPDATA%\GreenTimer\logs\green_timer.log` |

On first launch an older `timer_pro_settings.json` next to the executable is
migrated automatically. A corrupted settings file falls back to defaults field
by field instead of crashing the app.

## 🧪 Tests

```powershell
cargo test
```

The suite covers time formatting/parsing, config validation and migration, the
timer/Pomodoro state machine, hotkey key-name mapping and update-version
comparison.

## 🗂 Project layout

```
build.rs              Embeds the window icon and version info into the exe
src/
  main.rs               Entry point and shared constants
  app.rs                Overlay window, event loop, hotkey/tray dispatch
  core/
    config.rs           Defaults, validation, atomic save, migration
    formatting.rs       Duration formatting and input parsing
    engine.rs           Framework-independent timer state machine
  ui/
    render.rs           All painting (styles, outline, rotation, themes)
    settings.rs         Settings dialog (its own native window)
    menu.rs             Right-click context menu
    fonts.rs            Font discovery and fallback
  system/
    platform.rs         Topmost, click-through, DWM blur, sounds
    hotkeys.rs          Global hotkey registration and key recording
    tray.rs             Tray icon and menu
    updater.rs          Release check and safe installer download
installer.iss           Inno Setup installer
legacy-python/          The previous Python implementation, kept for reference
```

## 🤖 AI assistance disclosure

**Parts of this project were written with the help of an AI coding assistant.**
This is disclosed openly because you deserve to know how the software you install
was made.

- **What this means:** design decisions, code, docs, tests and release automation
  have been produced or reviewed with AI assistance, under human direction and
  review.
- **What it does not mean:** the project is **not** an official product of any AI
  company, and it is not endorsed by or affiliated with any model provider.
- **Your risk:** AI-assisted code can contain mistakes a human would not have
  made. The test suite and manual testing reduce that risk but cannot eliminate
  it. Treat pre-release builds accordingly.
- **Licence:** the code is provided under the MIT licence, without warranty, as
  described in [LICENSE](LICENSE).

If AI-assisted code is a problem for your use case, please don't install it — no
hard feelings.

## 🙏 Contributing

Bug reports and suggestions are welcome via
[Issues](https://github.com/LvSpook/Green-Timer/issues). This is a hobby project
in active development, so please be patient with response times.

---

<div align="center">
<sub>Built with 🦀 Rust · Windows only · MIT licensed</sub>
</div>
