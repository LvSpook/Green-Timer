//! Global hotkeys: registration, dispatch and key recording.
//!
//! Bindings are stored as a display name plus the physical scan code (matching
//! the original settings format), so they survive keyboard-layout changes.
//! Registration uses `global-hotkey`, which asks Windows for a system-wide
//! hotkey per binding instead of installing a low-level hook — no anti-cheat or
//! AV flags and no per-keystroke overhead.

use crate::config::Config;
use global_hotkey::hotkey::{Code, HotKey};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use std::str::FromStr;

/// The actions a hotkey can trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Toggle,
    Reset,
    Settings,
    Lap,
    Ghost,
}

impl Action {
    #[cfg(test)]
    pub fn config_key(self) -> &'static str {
        match self {
            Action::Toggle => "hk_toggle",
            Action::Reset => "hk_reset",
            Action::Settings => "hk_settings",
            Action::Lap => "hk_lap",
            Action::Ghost => "hk_ghost",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Toggle => "Start / Pause",
            Action::Reset => "Reset",
            Action::Settings => "Open Settings",
            Action::Lap => "Trigger Lap",
            Action::Ghost => "Toggle Ghost Mode",
        }
    }

    pub const ALL: [Action; 5] = [
        Action::Toggle,
        Action::Reset,
        Action::Settings,
        Action::Lap,
        Action::Ghost,
    ];
}

pub struct HotkeyManager {
    manager: Option<GlobalHotKeyManager>,
    /// `(hotkey id, action)` for every successfully registered binding.
    bindings: Vec<(u32, Action)>,
    /// The values that are currently registered, so they can be cleaned up.
    registered: Vec<HotKey>,
    /// Bindings that could not be registered, for reporting in the UI.
    failures: Vec<(Action, String)>,
}

impl HotkeyManager {
    pub fn new() -> Self {
        HotkeyManager {
            manager: GlobalHotKeyManager::new().ok(),
            bindings: Vec::new(),
            registered: Vec::new(),
            failures: Vec::new(),
        }
    }

    pub fn failures(&self) -> &[(Action, String)] {
        &self.failures
    }

    /// Replace every registration with the current configuration.
    pub fn rebind(&mut self, config: &Config) {
        self.bindings.clear();
        self.failures.clear();

        let Some(manager) = &self.manager else {
            log::warn!("Global hotkeys unavailable on this system");
            return;
        };
        if !self.registered.is_empty() {
            let _ = manager.unregister_all(&self.registered);
            self.registered.clear();
        }

        for action in Action::ALL {
            let entry = config.hotkey(action);
            let Some(code) = code_for(&entry.name, entry.code) else {
                self.failures
                    .push((action, format!("unrecognised key {:?}", entry.name)));
                continue;
            };
            let hotkey = HotKey::new(None, code);
            match manager.register(hotkey) {
                Ok(()) => {
                    self.bindings.push((hotkey.id(), action));
                    self.registered.push(hotkey);
                }
                Err(err) => {
                    // A duplicate or reserved combination is common, so report
                    // it rather than failing the whole set.
                    self.failures.push((action, err.to_string()));
                    log::warn!("Hotkey {} not registered: {err}", entry.name);
                }
            }
        }
    }

    /// Drain pending hotkey events. Called once per frame.
    pub fn poll(&self) -> Vec<Action> {
        let mut actions = Vec::new();
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if let Some((_, action)) = self.bindings.iter().find(|(id, _)| *id == event.id()) {
                actions.push(*action);
            }
        }
        actions
    }
}

impl Default for HotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a stored name and scan code to a `Code` for registration.
///
/// `keyboard-types` uses names such as `Digit1`, `KeyA`, `Space` and `F1`, but
/// the recorder may also produce human names like `"A"`, `"1"` or `"numpad 4"`,
/// so those are normalised first. The scan code is a fallback for hand-edited
/// files.
fn code_for(name: &str, scan_code: u32) -> Option<Code> {
    name_to_code(name).or_else(|| scan_code_to_code(scan_code))
}

fn name_to_code(name: &str) -> Option<Code> {
    let lower = name.trim().to_lowercase();
    let bare = lower.strip_prefix("numpad ").unwrap_or(&lower);

    // Single characters: letters become `KeyX`, digits become `DigitN`.
    if bare.chars().count() == 1 {
        let ch = bare.chars().next()?;
        let candidate = if ch.is_ascii_alphabetic() {
            format!("Key{}", ch.to_ascii_uppercase())
        } else if ch.is_ascii_digit() {
            format!("Digit{ch}")
        } else {
            ch.to_string()
        };
        if let Ok(code) = Code::from_str(&candidate) {
            return Some(code);
        }
    }

    // Function keys.
    let upper = bare.to_uppercase();
    if upper.len() > 1
        && upper.starts_with('F')
        && upper[1..].chars().all(|c| c.is_ascii_digit())
    {
        if let Ok(code) = Code::from_str(&upper) {
            return Some(code);
        }
    }

    // Named keys, tried in the spellings `keyboard-types` accepts. Windows'
    // `GetKeyNameTextW` returns short names such as "Left", so those are
    // mapped to the arrow names the parser expects.
    let aliased = match bare {
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "pgup" => "PageUp",
        "pgdn" => "PageDown",
        "esc" => "Escape",
        "del" => "Delete",
        "ins" => "Insert",
        "return" => "Enter",
        "bksp" => "Backspace",
        "caps lock" => "CapsLock",
        "num lock" => "NumLock",
        "scroll lock" => "ScrollLock",
        _ => "",
    };
    if !aliased.is_empty() {
        if let Ok(code) = Code::from_str(aliased) {
            return Some(code);
        }
    }

    for candidate in [title_case(bare), pascal_case(bare), upper] {
        if candidate.is_empty() {
            continue;
        }
        if let Ok(code) = Code::from_str(&candidate) {
            return Some(code);
        }
    }
    None
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `"arrow up"` -> `"ArrowUp"`.
fn pascal_case(value: &str) -> String {
    value
        .split([' ', '_', '-'])
        .filter_map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        })
        .collect()
}

/// Fallback table from Windows scan codes to `Code`, used when a saved name is
/// not recognised (for example after a settings file was hand-edited).
fn scan_code_to_code(scan: u32) -> Option<Code> {
    // Scan codes follow the PC/AT set 1 layout; only the keys we expect to be
    // bound are listed, which covers every default binding.
    let code = match scan {
        0x01 => Code::Escape,
        0x02..=0x0B => return digit_code(scan, 0x02, 1),
        0x0C => Code::Minus,
        0x0D => Code::Equal,
        0x0E => Code::Backspace,
        0x0F => Code::Tab,
        0x10..=0x19 => return letter_code(scan, 0x10, 'q'),
        0x1A => Code::BracketLeft,
        0x1B => Code::BracketRight,
        0x1C => Code::Enter,
        0x1E..=0x26 => return letter_code(scan, 0x1E, 'a'),
        0x27 => Code::Semicolon,
        0x28 => Code::Quote,
        0x29 => Code::Backquote,
        0x2B => Code::Backslash,
        0x2C..=0x32 => return letter_code(scan, 0x2C, 'z'),
        0x33 => Code::Comma,
        0x34 => Code::Period,
        0x35 => Code::Slash,
        0x39 => Code::Space,
        0x3B..=0x44 => return f_key_code(scan - 0x3B + 1),
        0x47 => Code::Home,
        0x48 => Code::ArrowUp,
        0x49 => Code::PageUp,
        0x4B => Code::ArrowLeft,
        0x4D => Code::ArrowRight,
        0x4F => Code::End,
        0x50 => Code::ArrowDown,
        0x51 => Code::PageDown,
        0x52 => Code::Insert,
        0x53 => Code::Delete,
        0x57 => Code::F11,
        0x58 => Code::F12,
        _ => return None,
    };
    Some(code)
}

fn letter_code(scan: u32, base: u32, first: char) -> Option<Code> {
    let offset = (scan - base) as u8;
    let letter = (first as u8 + offset) as char;
    Code::from_str(&format!("Key{}", letter.to_ascii_uppercase())).ok()
}

fn digit_code(scan: u32, base: u32, first: u32) -> Option<Code> {
    let digit = first + (scan - base);
    Code::from_str(&format!("Digit{digit}")).ok()
}

fn f_key_code(number: u32) -> Option<Code> {
    Code::from_str(&format!("F{number}")).ok()
}

#[cfg(windows)]
mod record {
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::thread;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyNameTextW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG,
        WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
    };

    static SENDER: OnceLock<Mutex<Option<Sender<(String, u32)>>>> = OnceLock::new();

    fn sender() -> &'static Mutex<Option<Sender<(String, u32)>>> {
        SENDER.get_or_init(|| Mutex::new(None))
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // `code < 0` means the hook must be passed on untouched.
        if code >= 0 {
            let kind = wparam.0 as u32;
            if kind == WM_KEYDOWN || kind == WM_SYSKEYDOWN {
                let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
                let scan = event.scanCode;

                let mut buffer = [0u16; 64];
                let length = unsafe { GetKeyNameTextW((scan as i32) << 16, &mut buffer) };
                let name = if length > 0 {
                    String::from_utf16_lossy(&buffer[..length as usize])
                } else {
                    format!("key {scan}")
                };

                if let Some(tx) = sender().lock().ok().and_then(|mut guard| guard.take()) {
                    let _ = tx.send((name, scan));
                }
            }
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// Capture the next key press and report `(display name, scan code)`.
    ///
    /// The hook lives on its own thread with a message loop, which Windows
    /// requires for low-level hooks, and removes itself after one key.
    pub fn capture_next() -> Receiver<(String, u32)> {
        let (tx, rx) = channel();
        if let Ok(mut guard) = sender().lock() {
            *guard = Some(tx);
        }

        thread::spawn(|| {
            let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) };
            let Ok(hook) = hook else {
                log::warn!("Could not install the keyboard capture hook");
                return;
            };

            // Pump messages until the key press has been delivered.
            let mut message = MSG::default();
            loop {
                let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
                if result.0 <= 0 {
                    break;
                }
                if sender().lock().map(|guard| guard.is_none()).unwrap_or(true) {
                    break;
                }
            }
            let _ = unsafe { UnhookWindowsHookEx(hook) };
        });

        rx
    }
}

#[cfg(not(windows))]
mod record {
    pub fn capture_next() -> std::sync::mpsc::Receiver<(String, u32)> {
        let (_, rx) = std::sync::mpsc::channel();
        rx
    }
}

pub use record::capture_next;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_f1_resolves_to_a_code() {
        assert_eq!(code_for("f1", 59), Some(Code::F1));
        assert_eq!(code_for("space", 57), Some(Code::Space));
    }

    #[test]
    fn letters_resolve_from_names_and_scan_codes() {
        assert_eq!(code_for("a", 0), Some(Code::KeyA));
        // Scan code for Q, with no usable name.
        assert_eq!(code_for("?", 0x10), Some(Code::KeyQ));
    }

    #[test]
    fn digits_and_function_keys() {
        assert_eq!(code_for("5", 0), Some(Code::Digit5));
        assert_eq!(code_for("f12", 0), Some(Code::F12));
        assert_eq!(code_for("", 0x43), Some(Code::F9));
    }

    #[test]
    fn numpad_prefix_is_ignored() {
        assert_eq!(code_for("numpad 4", 0), Some(Code::Digit4));
    }

    #[test]
    fn human_names_resolve() {
        assert_eq!(code_for("arrow up", 0), Some(Code::ArrowUp));
        assert_eq!(code_for("Left", 0), Some(Code::ArrowLeft));
        assert_eq!(code_for("Escape", 0), Some(Code::Escape));
    }

    #[test]
    fn action_config_keys_are_distinct() {
        let keys: Vec<&str> = Action::ALL.iter().map(|a| a.config_key()).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len());
    }
}
