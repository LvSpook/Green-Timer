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
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    };
    use windows::Win32::Media::Audio::{
        PlaySoundW, SND_ALIAS, SND_ASYNC, SND_FILENAME, SND_NODEFAULT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongW, IsWindowVisible, SetWindowLongW, SetWindowPos,
        SystemParametersInfoW, GWL_EXSTYLE, HWND_NOTOPMOST, HWND_TOPMOST, SPI_GETWORKAREA,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        WS_EX_TRANSPARENT,
    };

    /// Name of the per-session mutex that marks "Green Timer is running".
    ///
    /// `Local\` scopes it to the signed-in user's session, so separate users
    /// on the same machine each get their own overlay.
    const INSTANCE_MUTEX: &str = "Local\\GreenTimer.SingleInstance";

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

    /// Whether the OS currently shows this window. Used to stay in sync when
    /// something outside egui (such as a second launch) changes visibility.
    pub fn is_window_visible(window: isize) -> bool {
        unsafe { IsWindowVisible(hwnd(window)).as_bool() }
    }

    /// Try to become the one running copy. Returns `false` when another copy
    /// already holds the mutex.
    ///
    /// The handle is intentionally leaked: the OS releases it when the process
    /// ends, which is exactly the lifetime we want.
    /// Handle of the mutex we own, kept for the life of the process. Windows
    /// releases it automatically when the process exits, so it is stored as a
    /// plain integer to avoid needing a destructor or `Send`/`Sync`.
    static INSTANCE_HANDLE: std::sync::OnceLock<isize> = std::sync::OnceLock::new();

    pub fn claim_single_instance() -> bool {
        use windows::Win32::Foundation::{
            ERROR_ALREADY_EXISTS, GetLastError, SetLastError, WIN32_ERROR,
        };
        use windows::Win32::System::Threading::CreateMutexW;

        let name = wide(INSTANCE_MUTEX);
        // `CreateMutexW` reports "already exists" through the last-error slot,
        // so clear it first rather than relying on whatever was there before.
        unsafe { SetLastError(WIN32_ERROR(0)) };
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) };
        match handle {
            Ok(handle) => {
                let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
                if already {
                    // Not ours; close it so we do not hold a stray handle.
                    let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
                    false
                } else {
                    let _ = INSTANCE_HANDLE.set(handle.0 as isize);
                    true
                }
            }
            Err(err) => {
                // If the mutex cannot even be created, running is still better
                // than silently refusing to start.
                log::warn!("Could not create the single-instance mutex: {err}");
                true
            }
        }
    }

    /// Bring an already-running copy's overlay to the front.
    ///
    /// Returns `true` when a window was found, so the new process can exit
    /// quietly instead of starting a second overlay.
    pub fn focus_existing_window(title: &str) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
        };

        let title = wide(title);
        let found = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) };
        let Ok(window) = found else {
            return false;
        };
        if window.0.is_null() {
            return false;
        }
        unsafe {
            // The overlay may have been hidden to the tray, so show it rather
            // than merely raising it.
            if IsIconic(window).as_bool() {
                let _ = ShowWindow(window, SW_RESTORE);
            } else {
                let _ = ShowWindow(window, SW_SHOW);
            }
            let _ = SetForegroundWindow(window);
        }
        true
    }

    /// Native yes/no prompt. Lives here because it needs the wide-string helper.
    pub fn confirm(title: &str, text: &str) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{
            MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_YESNO,
        };
        let title = wide(title);
        let text = wide(text);
        let result = unsafe {
            MessageBoxW(
                None,
                PCWSTR(text.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
            )
        };
        result == IDYES
    }

    /// Show an informational/error message box with an OK button.
    pub fn alert(title: &str, text: &str) {
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let title = wide(title);
        let text = wide(text);
        unsafe {
            let _ = MessageBoxW(
                HWND::default(),
                PCWSTR(text.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    /// Primary monitor's usable work area (excluding taskbar) in logical points.
    pub fn primary_work_area(scale: f32) -> (f32, f32, f32, f32) {
        let mut rect = RECT::default();
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some((&mut rect as *mut RECT).cast::<c_void>()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        let scale = scale.max(0.5);
        if ok.is_ok() && rect.right > rect.left && rect.bottom > rect.top {
            (
                rect.left as f32 / scale,
                rect.top as f32 / scale,
                (rect.right - rect.left) as f32 / scale,
                (rect.bottom - rect.top) as f32 / scale,
            )
        } else {
            (0.0, 0.0, 1920.0, 1080.0)
        }
    }

    /// Open a URL or file in the user's default browser/handler.
    pub fn open_url(url: &str) {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let operation = wide("open");
        let target = wide(url);
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if result.0 as isize <= 32 {
            log::warn!("Could not open {url}");
        }
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
    pub fn primary_work_area(_scale: f32) -> (f32, f32, f32, f32) {
        (0.0, 0.0, 1920.0, 1080.0)
    }
    pub fn play_sound(_path: &str) {}
    pub fn open_url(_url: &str) {}
    pub fn claim_single_instance() -> bool {
        true
    }
    pub fn focus_existing_window(_title: &str) -> bool {
        false
    }
    pub fn is_window_visible(_window: isize) -> bool {
        true
    }
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

/// Ask the user a yes/no question in a native message box.
#[cfg(windows)]
pub fn confirm(title: &str, text: &str) -> bool {
    imp::confirm(title, text)
}

#[cfg(not(windows))]
pub fn confirm(_title: &str, _text: &str) -> bool {
    false
}

/// Show a native OK alert dialog.
#[cfg(windows)]
pub fn alert(title: &str, text: &str) {
    imp::alert(title, text);
}

#[cfg(not(windows))]
pub fn alert(_title: &str, _text: &str) {}

/// Primary monitor's usable work area `(left, top, width, height)` in logical points.
pub fn primary_work_area(scale: f32) -> (f32, f32, f32, f32) {
    imp::primary_work_area(scale)
}

/// Handle of the window that currently has keyboard focus (0 if unknown).
pub fn foreground_window() -> u64 {
    imp::foreground_window()
}

pub fn play_sound(path: &str) {
    imp::play_sound(path);
}

/// Open a URL in the default browser (no-op on non-Windows).
pub fn open_url(url: &str) {
    imp::open_url(url);
}

pub fn system_info() -> String {
    format!(
        "{} {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::FAMILY
    )
}

/// Claim the one-copy-per-session lock. `false` means another copy is running.
pub fn claim_single_instance() -> bool {
    imp::claim_single_instance()
}

/// Raise an already-running copy's window. `true` when one was found.
pub fn focus_existing_window(title: &str) -> bool {
    imp::focus_existing_window(title)
}

/// Whether the OS currently shows this window.
pub fn is_window_visible(window: isize) -> bool {
    imp::is_window_visible(window)
}
