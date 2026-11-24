//! Global hotkeys: registration, dispatch and key recording.
//!
//! Bindings are stored as a display name plus the physical scan code (matching
//! the original settings format), so they survive keyboard-layout changes.
//!
//! Dispatch uses a low-level keyboard hook (`WH_KEYBOARD_LL`) rather than
//! `RegisterHotKey`. Like the original app's `keyboard` library, this sees keys
//! even when a full-screen game has captured the keyboard, and it matches the
//! hardware scan code rather than a layout-dependent character.

use crate::config::Config;

/// The actions a hotkey can trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Toggle,
    Reset,
    Settings,
    Lap,
    Ghost,
    ToggleVisibility,
    ToggleMiniMode,
}

impl Action {
    /// Stable identifier used in the settings file for custom bindings.
    pub fn id(self) -> &'static str {
        match self {
            Action::Toggle => "toggle",
            Action::Reset => "reset",
            Action::Settings => "settings",
            Action::Lap => "lap",
            Action::Ghost => "ghost",
            Action::ToggleVisibility => "visibility",
            Action::ToggleMiniMode => "mini_mode",
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|action| action.id() == id)
    }

    #[cfg(test)]
    pub fn config_key(self) -> &'static str {
        match self {
            Action::Toggle => "hk_toggle",
            Action::Reset => "hk_reset",
            Action::Settings => "hk_settings",
            Action::Lap => "hk_lap",
            Action::Ghost => "hk_ghost",
            Action::ToggleVisibility => "hk_visibility",
            Action::ToggleMiniMode => "hk_mini_mode",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Toggle => "Start / Pause",
            Action::Reset => "Reset",
            Action::Settings => "Open Settings",
            Action::Lap => "Trigger Lap",
            Action::Ghost => "Toggle Ghost Mode",
            Action::ToggleVisibility => "Show / Hide Timer",
            Action::ToggleMiniMode => "Toggle Mini-Mode",
        }
    }

    pub const ALL: [Action; 7] = [
        Action::Toggle,
        Action::Reset,
        Action::Settings,
        Action::Lap,
        Action::Ghost,
        Action::ToggleVisibility,
        Action::ToggleMiniMode,
    ];
}

/// Scan codes the hook compares against, plus the actions they fire.
type Bindings = Vec<(u32, Action)>;

/// Turn a config into hook bindings, reporting conflicts.
///
/// Built-in bindings are added first so a custom row can never shadow the
/// primary key for an action; unset (scan 0) rows are skipped entirely.
fn build_bindings(config: &Config) -> (Bindings, Vec<(Action, String)>) {
    let mut bindings: Bindings = Vec::new();
    let mut failures: Vec<(Action, String)> = Vec::new();

    let mut add = |code: u32, action: Action, name: &str, role: &str| {
        if code == 0 {
            return;
        }
        if bindings.iter().any(|(scan, _)| *scan == code) {
            let role = if role.is_empty() {
                String::new()
            } else {
                format!("{role} ")
            };
            failures.push((
                action,
                format!("{role}{name} is already used by another action"),
            ));
            return;
        }
        bindings.push((code, action));
    };

    for action in Action::ALL {
        let entry = config.hotkey(action);
        add(entry.code, action, &entry.name, "");
    }
    for extra in &config.hk_custom {
        let Some(action) = Action::from_id(&extra.action) else {
            continue;
        };
        add(extra.key.code, action, &extra.key.name, "extra binding");
    }
    (bindings, failures)
}

pub struct HotkeyManager {
    /// Bindings that could not be registered, for reporting in the UI.
    failures: Vec<(Action, String)>,
}

impl HotkeyManager {
    pub fn new() -> Self {
        hook::install();
        HotkeyManager {
            failures: Vec::new(),
        }
    }

    pub fn failures(&self) -> &[(Action, String)] {
        &self.failures
    }

    /// Replace every binding with the current configuration.
    pub fn rebind(&mut self, config: &Config) {
        let (bindings, failures) = build_bindings(config);
        self.failures = failures;
        hook::set_bindings(bindings);
    }

    /// Drain pending hotkey events. Called once per frame.
    pub fn poll(&self) -> Vec<Action> {
        hook::drain()
    }
}

impl Default for HotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// The low-level hook: bindings storage, dispatch channel and key capture.
#[cfg(windows)]
mod hook {
    use super::{Action, Bindings};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::thread;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, GetKeyNameTextW, MapVirtualKeyW, MAPVK_VK_TO_VSC,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, MSG,
        WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
    };

    static BINDINGS: OnceLock<Mutex<Bindings>> = OnceLock::new();
    static ACTION_TX: OnceLock<Mutex<Option<Sender<Action>>>> = OnceLock::new();
    static ACTION_RX: OnceLock<Mutex<Receiver<Action>>> = OnceLock::new();
    /// Set while the user is recording a key: dispatch is suppressed so the
    /// key being recorded does not also trigger its old action.
    static CAPTURE_PENDING: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    /// Set while the note is being typed in: every letter would otherwise fire
    /// a global hotkey while the editor has no way to swallow it.
    static SUSPENDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    fn bindings() -> &'static Mutex<Bindings> {
        BINDINGS.get_or_init(|| Mutex::new(Vec::new()))
    }

    fn action_sender() -> &'static Mutex<Option<Sender<Action>>> {
        ACTION_TX.get_or_init(|| {
            let (tx, rx) = channel();
            let _ = ACTION_RX.set(Mutex::new(rx));
            Mutex::new(Some(tx))
        })
    }

    fn action_receiver() -> &'static Mutex<Receiver<Action>> {
        let _ = action_sender();
        ACTION_RX.get().expect("action receiver initialised")
    }

    /// Install the hook once, on its own thread (Windows needs a message loop).
    pub fn install() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            thread::spawn(|| unsafe {
                let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0);
                let Ok(_hook) = hook else {
                    log::warn!("Could not install the global keyboard hook");
                    return;
                };
                // Pump messages forever; the hook is process-wide and must stay
                // alive for as long as the app runs.
                let mut message = MSG::default();
                while GetMessageW(&mut message, None, 0, 0).0 > 0 {}
            });
        });
    }

    pub fn set_bindings(next: Bindings) {
        if let Ok(mut guard) = bindings().lock() {
            *guard = next;
        }
    }

    pub fn drain() -> Vec<Action> {
        let mut out = Vec::new();
        if let Ok(rx) = action_receiver().lock() {
            while let Ok(action) = rx.try_recv() {
                out.push(action);
            }
        }
        out
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // Keep this callback cheap: Windows permanently unhooks a low-level
        // hook that is repeatedly slow (`LowLevelHooksTimeout`).
        if code >= 0 {
            let kind = wparam.0 as u32;
            if kind == WM_KEYDOWN || kind == WM_SYSKEYDOWN {
                let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
                let scan = event.scanCode;

                // Recording suppresses dispatch so the recorded key does not
                // also fire its old action.
                // Recording suppresses dispatch so the recorded key does not
                // also fire its old action; typing in the note suppresses it
                // too, so letters do not trigger shortcuts.
                if !CAPTURE_PENDING.load(std::sync::atomic::Ordering::Relaxed)
                    && !SUSPENDED.load(std::sync::atomic::Ordering::Relaxed)
                {
                    if let Ok(list) = bindings().lock() {
                        if let Some((_, action)) = list.iter().find(|(code, _)| *code == scan) {
                            if let Ok(guard) = action_sender().lock() {
                                if let Some(tx) = guard.as_ref() {
                                    let _ = tx.send(*action);
                                }
                            }
                        }
                    }
                }
            }
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// Display name for a virtual key.
    ///
    /// Lower-cased to match the original app's `event.name.lower()` and the
    /// names already stored in existing settings files ("f1", "space").
    fn key_name(vk: u16) -> String {
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) };
        // Keys on the arrow/navigation clusters and the right-hand modifiers
        // are "extended" keys. Without this bit `GetKeyNameTextW` reports the
        // numpad key that shares the scan code ("num 4" instead of "left").
        // The key's virtual-key code identifies this reliably; the scan code
        // alone cannot.
        let extended = matches!(
            vk,
            0x03 // cancel
            | 0x21..=0x28 // page up/down, end, home, arrows
            | 0x2C | 0x2D | 0x2E // print screen, insert, delete
            | 0x5B | 0x5C | 0x5D // left/right windows, menu
            | 0x6F // numpad divide
            | 0x90 // num lock
            | 0xA3 | 0xA5 // right control, right alt
        );
        let lparam = ((scan as i32) << 16) | if extended { 1 << 24 } else { 0 };

        let mut buffer = [0u16; 64];
        let length = unsafe { GetKeyNameTextW(lparam, &mut buffer) };
        if length > 0 {
            String::from_utf16_lossy(&buffer[..length as usize]).to_lowercase()
        } else {
            format!("key {scan}")
        }
    }

    /// Capture the next key press and report `(display name, scan code)`.
    ///
    /// Recording deliberately does not rely on the hook: Windows silently
    /// removes a low-level hook whose callback is slow more than a few times,
    /// and a hook that suddenly stops delivering is exactly what the user sees
    /// as "recording does nothing". Instead a helper thread polls the physical
    /// key state, which cannot affect the hook's health at all.
    pub fn capture_next() -> Receiver<(String, u32)> {
        let (tx, rx) = channel();
        // Stop dispatching while recording, so pressing a bound key does not
        // fire its old action.
        CAPTURE_PENDING.store(true, std::sync::atomic::Ordering::SeqCst);
        thread::spawn(move || {
            // Let the click that started recording go.
            thread::sleep(std::time::Duration::from_millis(250));
            // Seed the previous state so keys already held are not recorded
            // until they are released and pressed again.
            let mut previous = key_down_states();
            loop {
                if !CAPTURE_PENDING.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let current = key_down_states();
                if let Some(vk) = (8..=254u16)
                    .find(|vk| current[*vk as usize] && !previous[*vk as usize])
                {
                    CAPTURE_PENDING.store(false, std::sync::atomic::Ordering::SeqCst);
                    if let Some(scan) = scan_code(vk) {
                        let _ = tx.send((key_name(vk), scan));
                        return;
                    }
                    previous = current;
                    continue;
                }
                previous = current;
                thread::sleep(std::time::Duration::from_millis(12));
            }
        });
        rx
    }

    /// Abandon a pending capture (the dialog was closed or cancelled), so
    /// normal hotkey dispatch resumes.
    pub fn cancel_capture() {
        CAPTURE_PENDING.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Hold or release global dispatch while a text box has the keyboard, so
    /// typing a note cannot trigger shortcuts.
    pub fn set_suspended(suspended: bool) {
        SUSPENDED.store(suspended, std::sync::atomic::Ordering::SeqCst);
    }

    /// Down state of every virtual key (indexed by the key's low byte).
    ///
    /// Recording polls key state instead of using the hook: Windows silently
    /// removes a low-level hook whose callback is slow more than a few times,
    /// and a hook that stops delivering is what the user sees as "recording
    /// does nothing". `GetAsyncKeyState` cannot affect the hook's health.
    fn key_down_states() -> [bool; 256] {
        let mut states = [false; 256];
        for vk in 8..=254u16 {
            let state = unsafe { GetAsyncKeyState(vk as i32) };
            states[vk as usize] = state < 0;
        }
        states
    }

    /// Hardware scan code for a virtual key, so bindings stay layout
    /// independent (the settings file stores scan codes).
    fn scan_code(vk: u16) -> Option<u32> {
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) };
        if scan == 0 {
            None
        } else {
            Some(scan)
        }
    }
}

#[cfg(not(windows))]
mod hook {
    use super::{Action, Bindings};
    use std::sync::mpsc::{channel, Receiver};

    pub fn install() {}
    pub fn set_bindings(_next: Bindings) {}
    pub fn drain() -> Vec<Action> {
        Vec::new()
    }
    pub fn capture_next() -> Receiver<(String, u32)> {
        channel().1
    }
    pub fn cancel_capture() {}
    pub fn set_suspended(_suspended: bool) {}
}

pub use hook::capture_next;
pub use hook::cancel_capture;
pub use hook::set_suspended;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_config_keys_are_distinct() {
        let keys: Vec<&str> = Action::ALL.iter().map(|a| a.config_key()).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len());
    }

    #[test]
    fn default_hotkeys_have_scan_codes() {
        let config = Config::default();
        for action in Action::ALL {
            assert_ne!(config.hotkey(action).code, 0);
        }
    }

    #[test]
    fn hotkey_display_names_are_kept_for_the_ui() {
        assert_eq!(crate::config::Hotkey::new("f1", 59).name, "f1");
    }

    #[test]
    fn action_ids_round_trip_and_are_distinct() {
        let mut ids: Vec<&str> = Action::ALL.iter().map(|a| a.id()).collect();
        for action in Action::ALL {
            assert_eq!(Action::from_id(action.id()), Some(action));
        }
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), Action::ALL.len());
        assert_eq!(Action::from_id("nope"), None);
    }

    #[test]
    fn custom_bindings_add_extra_keys_for_an_action() {
        let mut config = Config::default();
        config.hk_custom.push(crate::config::CustomHotkey {
            action: Action::Lap.id().to_string(),
            key: crate::config::Hotkey::new("q", 16),
        });
        let (bindings, failures) = build_bindings(&config);
        assert!(failures.is_empty(), "{failures:?}");
        assert!(bindings.contains(&(16, Action::Lap)));
        // The built-in Space binding is still there.
        assert!(bindings.contains(&(57, Action::Lap)));
    }

    #[test]
    fn unset_custom_rows_are_ignored() {
        let mut config = Config::default();
        config.hk_custom.push(crate::config::CustomHotkey {
            action: Action::Reset.id().to_string(),
            key: crate::config::Hotkey::unbound(),
        });
        let (bindings, failures) = build_bindings(&config);
        assert!(failures.is_empty());
        assert_eq!(bindings.len(), Action::ALL.len());
    }

    #[test]
    fn duplicate_keys_are_reported_not_dropped_silently() {
        let mut config = Config::default();
        config.hk_custom.push(crate::config::CustomHotkey {
            action: Action::Lap.id().to_string(),
            // Already used by Start / Pause.
            key: crate::config::Hotkey::new("f1", 59),
        });
        let (bindings, failures) = build_bindings(&config);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].1.contains("already used"), "{failures:?}");
        assert_eq!(bindings.len(), Action::ALL.len());
    }

    #[test]
    fn unknown_custom_action_is_skipped() {
        let mut config = Config::default();
        config.hk_custom.push(crate::config::CustomHotkey {
            action: "does_not_exist".to_string(),
            key: crate::config::Hotkey::new("q", 16),
        });
        let (bindings, failures) = build_bindings(&config);
        assert!(failures.is_empty());
        assert_eq!(bindings.len(), Action::ALL.len());
    }
}
