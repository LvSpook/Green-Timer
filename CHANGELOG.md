> **Work in Progress (Pre-Release)** — This build is actively in development. Features and UI may continue to evolve, and feedback or bug reports are welcome!

## What's New in v1.3.3 (WIP)

### Reliability
- **Only one copy can run.** Launching `GreenTimer.exe` again now brings the running overlay back into view instead of starting a second timer and a second tray icon.
- **The window can no longer get lost.** If a saved position lands off-screen — after unplugging a monitor or changing resolution — the overlay returns to the top-right of the primary screen on the next launch.
- **Escape no longer quits.** It hides the overlay to the tray, so a stray keypress can never stop a running timer. Quit is still on the tray menu and in Settings.
- **Frame rate independent fluid animation.** The Fluid display style now moves at the same speed on a 60 Hz, 144 Hz or throttled display, and stays still when Reduce motion is on.
- **Log rotation keeps working.** The older log copy is now capped as well, so a long-running install cannot build up unbounded logs.

### First Run
- **A one-time tip on a fresh install** points at the right-click menu, which is the only way to reach Settings from the overlay. It disappears on the first click or right-click and never comes back. (It replaces the old behaviour of opening the Settings window automatically.)

### Appearance
- **The edge distance no longer jumps as the timer changes.** The panel now measures the digits in fixed-width slots that assume a `0`, so every digit sits the same distance from the background edge. When a leading zero disappears the panel shrinks toward the middle by exactly one slot instead of snapping to the digits.
- **The distance is adjustable** on the Appearance tab as "Edge distance", and it applies equally to the left/right and top/bottom edges. The outline and shadow get their own extra room, so the visible gap stays exactly what you set.

### Under the Hood
- Removed dead code, made the settings enums derive their defaults, and cleared every Clippy warning in the project.

## What's New in v1.3.2

### Global Hotkeys
- **Custom hotkeys.** Add your own extra bindings on the Hotkeys tab: pick any action from a dropdown, press Record, and that key does it too. `+` adds another row and `−` removes one. The built-in keys keep working, and a key already in use is reported instead of silently failing.

### About Tab
- **Simpler About page.** Version, a one-line description, a button that opens the GitHub repository, and the update check. The system details, settings-file path and hardcoded hotkey list are gone.
- **Pre-release updates are on the About page too**, right next to the update button, so the option sits with the thing it affects.

### Updates
- **Opt in to pre-release updates.** A new setting under Behavior ("Offer work-in-progress builds") makes the updater look at pre-release tags such as `v1.3.2-wip`, which GitHub hides from the normal "latest release" endpoint. Off by default.
- **The updater now downloads the installer, not the portable exe.** It used to take the first `.exe` it found, which was the bare `GreenTimer.exe`. It now prefers an `.msi`, then an Inno Setup `*_Setup_*.exe`, and never the portable binary.
- Update prompts and downloads keep the correct file extension, so an `.msi` opens with Windows Installer instead of being run as a program.

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

