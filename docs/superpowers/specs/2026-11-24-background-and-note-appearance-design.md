# Background opacity, note border and note corners

## Intent

Give the timer's background its own opacity control, and give the note window
the appearance controls it is missing: an opacity that follows the timer's
*background* rather than its whole-window fade, an on/off border, and rounded
corners that can match the timer's.

## What exists today

- `src/ui/render.rs:165` `background_alpha()` is hardcoded: `140`, or `100`
  when `window_blur` is on. It is not a setting.
- `src/ui/render.rs:177` `panel_color()` further overrides the alpha per theme
  (custom 235, Chalkboard/RetroLcd/WarmPaper/HighContrast 255, Cyberpunk 215,
  MinimalGlass 115). Only the Default theme uses `background_alpha`.
- `src/app.rs:784` `painter.set_opacity(config.opacity)` fades the whole timer,
  background included. Settings exposes it as "Window opacity" (10–100%).
- `src/ui/note.rs` paints its paper fully opaque and fades the entire note
  window through the OS compositor (`set_window_alpha`). Its frame is a fixed
  2 pt stroke and its corners are square.
- The note's window cannot be per-pixel transparent through OpenGL on this
  machine: the GL config reports no transparency support, so eframe logs
  "Cannot create transparent window" and the surface stays opaque. The overlay
  is see-through anyway because winit asks the desktop compositor for a
  blur-behind region with no area when it creates the window — a step child
  viewports never get.

## Goals

1. A "Background opacity" slider in Appearance, defaulting to today's look.
2. The note's opacity follows that background value by default, with a
   "Same as timer background" button beside it.
3. A "Show border" toggle for the note, on by default.
4. A corner-radius control for the note, with a "Same as timer" button.
5. Every existing appearance keeps working; the default appearance is unchanged
   except where the user opts in.
6. Only the note's paper fades. The text stays fully solid (user, m00840:
   "Only the background is supposed to match the timer. the text is supposed to
   stay 100% visible").

## Non-goals

- Switching the renderer. Tried and reverted: `wgpu-hal-22.0.0/src/dx12/
  adapter.rs:747` hardcodes `composite_alpha_modes: vec![Opaque]`, so wgpu 22
  cannot give a transparent window on Windows at all, and Vulkan had no
  transparent alpha mode here either.
- Fading the whole note window with `SetLayeredWindowAttributes`. Rejected: it
  fades the writing too.

## Approaches

**Transparent window surface via the desktop compositor.** Keep the OpenGL
backend and the window opaque to the GL driver, then call
`DwmEnableBlurBehindWindow` with an empty region on the note window, the same
thing winit does for the overlay at creation. The surface then honours its own
alpha: the paper fades, the text does not, and rounded corners are just
transparent pixels. *Chosen — verified at runtime.*

**Shape the note's window with a round-rect region** (`SetWindowRgn`). Would
have worked for corners only, and could not fade the paper. Superseded.

**Switch eframe to the wgpu backend** so the note window can carry per-pixel
alpha. Dead end on Windows with wgpu 22 (see non-goals).

**For the two opacity sliders:** window opacity keeps fading everything,
background included, so the two multiply. This keeps today's appearance at
default settings and keeps the existing slider behaving as it does now.

## Design

### Config (`src/core/config.rs`)

- `background_opacity: Option<f32>` — `None` keeps today's behaviour
  (`140`, or `100` with blur). Dragging the slider stores `Some(value)`,
  clamped `0.0..=1.0` in `validated()`, mirroring `note_bg_opacity`.
- `note_bg_opacity: Option<f32>` — unchanged field, new meaning: `None` means
  "same as the timer's background" instead of "same as the whole-window fade".
- `note_border: bool`, default `true`.
- `note_corner_radius: Option<u32>` — `None` means "same as the timer's
  corners". `Some` is clamped to half the note's shorter side in `validated()`.

### Rendering (`src/ui/render.rs`)

- `pub fn background_alpha(config) -> u8` returns
  `(background_opacity(config) * 255).round()`, where `background_opacity()`
  resolves `None` to the legacy 1.0-or-blur value.
- Everything else (`panel_color`, `draw_background`) is unchanged, so themes
  keep their own overrides and only the Default theme follows the slider.

### Note (`src/ui/note.rs`)

- `paper_color()` uses the theme's panel colour and replaces only its alpha
  with the note's opacity.
- `note_opacity()` resolves `None` to the panel's real alpha, so a theme that
  makes the timer solid makes the note solid too.
- `frame_stroke()` returns `Stroke::NONE` when `note_border` is off, so the
  content margin collapses to `MARGIN` and the text gains the border's space.
- `note_radius(config, rect)` resolves `None` to
  `render::corner_radius(config, rect)`, clamped to half the shorter side, and
  the paper is painted with `Rounding::same(radius)`.

### Platform (`src/system/platform.rs`)

`pub fn set_window_transparent(title: &str) -> bool` — finds the window by
title, calls `DwmEnableBlurBehindWindow` with an empty `CreateRectRgn(0, 0, -1,
-1)` region, frees the region afterwards (the compositor copies it), and
reports whether it was accepted. A no-op stub returning `false` off Windows.
Called once per open, with a 50 ms repaint retry while the window does not
exist yet.

### Settings (`src/ui/settings.rs`)

- Appearance tab, under "Window opacity": "Background opacity" slider with a
  **Reset** back to the automatic value.
- Notes tab: "Paper opacity" slider reading the background value; "Same as
  timer background" resets `note_bg_opacity` to `None`; "Show a border"
  checkbox; "Corner radius" slider with "Same as timer".

### Tests

- `background_alpha` default equals the legacy 140/100 values, and a stored
  value wins; out-of-range values clamp.
- The note follows the background value, not `config.opacity`, and an explicit
  value wins.
- Corner radius resolves to the timer's radius when unset, and clamps to half
  the shorter side.
- Border off produces an empty stroke and a smaller content inset.
- Existing note, menu and config tests keep passing.

## Verified

- `note transparency: true` logged on every open, so the compositor accepted the
  call.
- `PrintWindow` of the note returns its own pixels; the timer and note both keep
  their geometry and stay centred.
- 112 tests pass, `cargo clippy --release -- -D warnings` is clean, and the
  release build is warning-free.

## Left to the builder

Exact slider ranges and labels, and whether "Show border" sits above or below
the opacity slider.
