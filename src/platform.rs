//! Thin, defensive wrappers around OS-specific behaviour.
//!
//! Every function degrades gracefully (no-op or safe default) when a feature is
//! unavailable, so the rest of the app never has to special-case platform
//! checks and the module stays unit-testable off Windows.

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    };
    use windows::Win32::Media::Audio::{
        PlaySoundW, SND_ALIAS, SND_ASYNC, SND_FILENAME, SND_NODEFAULT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_EXSTYLE,
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_TRANSPARENT,
    };

    fn hwnd(value: isize) -> HWND {
        HWND(value as *mut c_void)
    }

    fn wide(path: &str) -> Vec<u16> {
        std::ffi::OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn set_topmost(window: isize, topmost: bool) {
        let after = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
        unsafe {
            let _ = SetWindowPos(
                hwnd(window),
                after,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    /// Toggle click-through. When `enabled`, the window ignores all mouse input
    /// while remaining visible — useful for a passive overlay.
    pub fn set_click_through(window: isize, enabled: bool) {
        unsafe {
            let current = GetWindowLongW(hwnd(window), GWL_EXSTYLE);
            let mut style = current as u32;
            if enabled {
                style |= WS_EX_TRANSPARENT.0;
            } else {
                style &= !WS_EX_TRANSPARENT.0;
            }
            if style != current as u32 {
                SetWindowLongW(hwnd(window), GWL_EXSTYLE, style as i32);
            }
        }
    }

    /// Enable or disable the native Windows 11 acrylic backdrop.
    pub fn apply_blur(window: isize, enable: bool) {
        // 3 = acrylic, 1 = none (DWMSBT_TRANSIENTWINDOW / DWMSBT_NONE).
        let value: i32 = if enable { 3 } else { 1 };
        let result = unsafe {
            DwmSetWindowAttribute(
                hwnd(window),
                DWMWA_SYSTEMBACKDROP_TYPE,
                &value as *const i32 as *const c_void,
                std::mem::size_of::<i32>() as u32,
            )
        };
        if result.is_err() {
            log::debug!("DwmSetWindowAttribute(SYSTEMBACKDROP_TYPE) failed: {result:?}");
        }
    }

    pub fn set_dark_title_bar(window: isize, enabled: bool) {
        let value: i32 = i32::from(enabled);
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd(window),
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &value as *const i32 as *const c_void,
                std::mem::size_of::<i32>() as u32,
            );
        }
    }

    pub fn foreground_window() -> u64 {
        unsafe { GetForegroundWindow().0 as u64 }
    }

    /// Play a WAV file asynchronously, or the OS alert when no file is usable.
    pub fn play_sound(path: &str) {
        if !path.is_empty() && std::path::Path::new(path).is_file() {
            let sound = wide(path);
            let played = unsafe {
                PlaySoundW(
                    PCWSTR(sound.as_ptr()),
                    None,
                    SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
                )
            };
            if played.as_bool() {
                return;
            }
            log::debug!("Could not play {path}; falling back to the system alert");
        }
        // "SystemAsterisk" is always registered, so this works on every install.
        let alias = wide("SystemAsterisk");
        unsafe {
            let _ = PlaySoundW(PCWSTR(alias.as_ptr()), None, SND_ALIAS | SND_ASYNC);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn set_topmost(_window: isize, _topmost: bool) {}
    pub fn set_click_through(_window: isize, _enabled: bool) {}
    pub fn apply_blur(_window: isize, _enable: bool) {}
    pub fn set_dark_title_bar(_window: isize, _enabled: bool) {}
    pub fn foreground_window() -> u64 {
        0
    }
    pub fn play_sound(_path: &str) {}
}

pub const IS_WINDOWS: bool = cfg!(windows);

pub fn set_topmost(window: isize, topmost: bool) {
    imp::set_topmost(window, topmost);
}

pub fn set_click_through(window: isize, enabled: bool) {
    imp::set_click_through(window, enabled);
}

pub fn apply_blur(window: isize, enable: bool) {
    if IS_WINDOWS {
        imp::apply_blur(window, enable);
    }
}

pub fn set_dark_title_bar(window: isize, enabled: bool) {
    imp::set_dark_title_bar(window, enabled);
}

/// Handle of the window that currently has keyboard focus (0 if unknown).
pub fn foreground_window() -> u64 {
    imp::foreground_window()
}

pub fn play_sound(path: &str) {
    imp::play_sound(path);
}

pub fn system_info() -> String {
    format!(
        "{} {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::FAMILY
    )
}
