> **Work in Progress (Pre-Release)** — This build is actively in development. Features and UI may continue to evolve, and feedback or bug reports are welcome!

## What's New in v1.3.1 (WIP)

### Installer & Packaging
- **Standalone exe now has its own icon.** The icon and version details are compiled into `GreenTimer.exe`, so a single-file download shows the Green Timer icon in Explorer, on the desktop, and in Alt-Tab instead of the generic executable icon. It no longer needs the separate `.ico` beside it.

### Timer Modes & Controls
- **Interval (HIIT) Mode**: Configurable Work, Rest, and Rounds with automatic phase transitions.
- **Target Time Mode**: Count down to a specific 24-hour local clock time (e.g. `17:00`).
- **Quick Adjust**: Nudge the timer on the fly (`-1m`, `-10s`, `+10s`, `+1m`) without stopping it.
- **Hide Leading Zeros**: Compact digit display option (e.g. `5:00` instead of `05:00`).
- **Auto-Reset on Zero** (loop countdowns) and **Warning Beeps** (10s and 3-2-1 cues).
- **Lap History**: Collapsible lap log with split times, **Copy CSV**, and **Clear Laps**.

### Themes, Visuals & Polish
- **5 New Built-in Themes**: Cyberpunk Neon, Minimal Glass, Retro LCD, Warm Paper, and High Contrast.
- **Custom Theme Builder**: Create, save, and apply your own custom color/background/border themes.
- **Coherent Theming Everywhere**: Themes now style the overlay, Settings window, right-click menu, taskbar icon, and system tray icon.
- **Smart Contrast Warning**: Alerts you in Settings if text and background colors have low contrast.
- **Visual Upgrades**: Custom label above timer, gradient progress fill, warning/overdue pulse animation, text drop shadow, **Compact Mini-Mode**, **Hover-to-Fade**, and **Show Background** toggle.
- **Full Rotation & Crisp Corners**: Rotating 90°/270° now rotates the window, background, and sprite assets cleanly; themed borders no longer clip at rounded corners.

### Behavior, UX & Menu
- **Pop-Out Right-Click Menu**: Redesigned with theme-matched styling, crisp vector icons, auto-dismiss on click-outside, and a **Right-Click Menu Customizer** in Settings.
- **4-Corner Draggable Resize**: Resize smoothly from any corner when unlocked.
- **Settings Search Bar**: Quickly filter and jump to any setting across tabs.
- **Dynamic Settings**: Options now appear only when relevant to the active mode/style.
- **Saved Profiles & Per-App Auto-Switch**: Save/load named setups and bind profiles to specific windows.
- **Auto-Start on Launch**, **Magnetic Edge Snapping**, **Import/Export JSON**, and global hotkeys for **Show/Hide** (`F11`) and **Mini-Mode** (`F12`).
- **Hover Tooltips**: Short descriptions appear after hovering over non-self-explanatory option labels.

### Project & Fixes
- Fixed Settings window taskbar icon appearing as a black square.
- Updated system tray icon & menu with live theming and new quick toggles.
- Organized Rust source into `src/core/`, `src/ui/`, and `src/system/`.

---

**AI assistance disclosure:** parts of this project were written with the help of an AI coding assistant, under human direction and review. This project is not affiliated with or endorsed by any AI model provider. AI-assisted code can contain mistakes a human would not have made; see the [README](https://github.com/LvSpook/Green-Timer#-ai-assistance-disclosure) for details.

