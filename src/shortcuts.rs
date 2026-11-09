use global_hotkey::hotkey::HotKey;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub sup: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Record,
    Play,
    Loop,
}

#[cfg(target_os = "macos")]
pub const DEFAULT_RECORD: &str = "alt+shift+super+F1";
#[cfg(target_os = "macos")]
pub const DEFAULT_PLAY: &str = "alt+shift+super+F2";
#[cfg(target_os = "macos")]
pub const DEFAULT_LOOP: &str = "alt+shift+super+F3";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_RECORD: &str = "ctrl+alt+shift+F1";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_PLAY: &str = "ctrl+alt+shift+F2";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_LOOP: &str = "ctrl+alt+shift+F3";

#[cfg(target_os = "macos")]
const SPEED_MODS: &str = "alt+shift+super+";
#[cfg(not(target_os = "macos"))]
const SPEED_MODS: &str = "ctrl+alt+shift+";
/// Fixed speed shortcuts, one per speed::LABELS entry, left to right.
pub const SPEED_KEYS: [&str; crate::speed::COUNT] = ["F5", "F6", "F7", "F8", "F9", "F10"];

pub fn speed_shortcut(step: usize) -> String {
    format!("{SPEED_MODS}{}", SPEED_KEYS[step])
}

pub fn speed_shortcuts() -> Vec<String> {
    (0..SPEED_KEYS.len()).map(speed_shortcut).collect()
}

/// Canonical text form, for example "ctrl+alt+shift+F1". The key is a W3C
/// `KeyboardEvent.code` name such as "KeyA", "Digit1" or "F5".
pub fn compose(m: Mods, code: &str) -> String {
    let mut s = String::new();
    for (on, name) in [(m.ctrl, "ctrl"), (m.alt, "alt"), (m.shift, "shift"), (m.sup, "super")] {
        if on {
            s.push_str(name);
            s.push('+');
        }
    }
    s.push_str(code);
    s
}

pub fn parse(s: &str) -> Option<(Mods, String)> {
    let mut m = Mods::default();
    let mut key = None;
    for t in s.split('+') {
        match t.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => m.ctrl = true,
            "alt" | "option" => m.alt = true,
            "shift" => m.shift = true,
            "super" | "cmd" | "command" | "win" | "meta" => m.sup = true,
            _ if key.is_none() && !t.is_empty() => key = Some(t.to_string()),
            _ => return None,
        }
    }
    key.map(|k| (m, k))
}

pub fn hotkey(s: &str) -> Result<HotKey, String> {
    HotKey::from_str(s).map_err(|_| "This key cannot be used in a shortcut.".to_string())
}

/// Number of physical keys in the shortcut, used to cut it from a recording.
pub fn chord_len(s: &str) -> usize {
    parse(s)
        .map(|(m, _)| 1 + [m.ctrl, m.alt, m.shift, m.sup].iter().filter(|b| **b).count())
        .unwrap_or(1)
}

pub fn key_label(code: &str) -> String {
    let named = match code {
        "Backquote" => "`",
        "Minus" => "-",
        "Equal" => "=",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "Semicolon" => ";",
        "Quote" => "'",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "PageUp" => "Page Up",
        "PageDown" => "Page Down",
        "Escape" => "Esc",
        "Backspace" => "Backspace",
        "PrintScreen" => "Print Screen",
        "ScrollLock" => "Scroll Lock",
        "NumpadAdd" => "Num +",
        "NumpadSubtract" => "Num -",
        "NumpadMultiply" => "Num *",
        "NumpadDivide" => "Num /",
        "NumpadDecimal" => "Num .",
        "NumpadEnter" => "Num Enter",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_string();
    }
    if let Some(k) = code.strip_prefix("Key") {
        return k.to_string();
    }
    if let Some(d) = code.strip_prefix("Digit") {
        return d.to_string();
    }
    if let Some(n) = code.strip_prefix("Numpad") {
        return format!("Num {n}");
    }
    code.to_string()
}

/// Display text: "Ctrl + Alt + Shift + F1" on Windows and Linux, the
/// standard modifier symbols on macOS.
pub fn label(s: &str) -> String {
    let Some((m, code)) = parse(s) else {
        return s.to_string();
    };
    let key = key_label(&code);
    if cfg!(target_os = "macos") {
        let mut out = String::new();
        for (on, sym) in [(m.ctrl, "\u{2303}"), (m.alt, "\u{2325}"), (m.shift, "\u{21E7}"), (m.sup, "\u{2318}")] {
            if on {
                out.push_str(sym);
            }
        }
        out.push_str(&key);
        out
    } else {
        let sup = if cfg!(windows) { "Win" } else { "Super" };
        let mut parts: Vec<&str> = Vec::new();
        for (on, name) in [(m.ctrl, "Ctrl"), (m.alt, "Alt"), (m.shift, "Shift"), (m.sup, sup)] {
            if on {
                parts.push(name);
            }
        }
        parts.push(&key);
        parts.join(" + ")
    }
}

pub fn is_modifier_code(code: &str) -> bool {
    matches!(
        code,
        "ControlLeft" | "ControlRight" | "AltLeft" | "AltRight" | "ShiftLeft" | "ShiftRight" | "MetaLeft" | "MetaRight" | "OSLeft" | "OSRight" | "Fn" | "CapsLock"
    )
}

fn is_fkey(code: &str) -> bool {
    code.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n))
}

fn system_name() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "your desktop"
    }
}

#[cfg(windows)]
const RESERVED: &[(&str, &str)] = &[
    ("ctrl+alt+Delete", "security screen"),
    ("ctrl+shift+Escape", "Task Manager"),
    ("alt+F4", "close window"),
    ("alt+Tab", "switch windows"),
    ("alt+Escape", "switch windows"),
    ("alt+Space", "window menu"),
    ("ctrl+Escape", "Start menu"),
    ("super+KeyL", "lock screen"),
    ("super+KeyD", "show desktop"),
    ("super+KeyE", "File Explorer"),
    ("super+KeyR", "Run"),
    ("super+KeyI", "Settings"),
    ("super+KeyS", "search"),
    ("super+KeyV", "clipboard history"),
    ("super+KeyX", "quick link menu"),
    ("super+KeyA", "quick settings"),
    ("super+KeyN", "notifications"),
    ("super+KeyG", "Game Bar"),
    ("super+KeyH", "voice typing"),
    ("super+KeyK", "cast"),
    ("super+KeyP", "project screen"),
    ("super+KeyW", "widgets"),
    ("super+KeyM", "minimize all"),
    ("super+Tab", "task view"),
    ("super+Period", "emoji panel"),
    ("super+shift+KeyS", "screen snip"),
    ("super+alt+KeyR", "Game Bar recording"),
    ("ctrl+super+KeyD", "new desktop"),
    ("super+ArrowUp", "maximize window"),
    ("super+ArrowDown", "minimize window"),
    ("super+ArrowLeft", "snap window"),
    ("super+ArrowRight", "snap window"),
];

#[cfg(target_os = "macos")]
const RESERVED: &[(&str, &str)] = &[
    ("super+Space", "Spotlight"),
    ("alt+super+Space", "Finder search"),
    ("ctrl+Space", "input source"),
    ("super+Tab", "app switcher"),
    ("super+Backquote", "window switcher"),
    ("super+Comma", "app settings"),
    ("super+KeyQ", "quit app"),
    ("super+KeyW", "close window"),
    ("super+KeyH", "hide app"),
    ("alt+super+KeyH", "hide others"),
    ("super+KeyM", "minimize"),
    ("ctrl+super+KeyQ", "lock screen"),
    ("ctrl+super+KeyF", "full screen"),
    ("ctrl+super+Space", "character viewer"),
    ("alt+super+Escape", "force quit"),
    ("shift+super+Digit3", "screenshot"),
    ("shift+super+Digit4", "screenshot"),
    ("shift+super+Digit5", "screenshot"),
    ("shift+super+KeyQ", "log out"),
    ("ctrl+ArrowUp", "Mission Control"),
    ("ctrl+ArrowDown", "app windows"),
    ("ctrl+ArrowLeft", "switch space"),
    ("ctrl+ArrowRight", "switch space"),
];

#[cfg(target_os = "linux")]
const RESERVED: &[(&str, &str)] = &[
    ("ctrl+alt+Delete", "log out"),
    ("ctrl+alt+KeyT", "terminal"),
    ("ctrl+alt+KeyL", "lock screen"),
    ("super+KeyL", "lock screen"),
    ("super+KeyD", "show desktop"),
    ("super+KeyA", "app grid"),
    ("super+KeyV", "notifications"),
    ("super+Tab", "switch apps"),
    ("alt+Tab", "switch windows"),
    ("alt+F4", "close window"),
    ("alt+F2", "run command"),
    ("alt+Space", "window menu"),
    ("ctrl+alt+ArrowLeft", "switch workspace"),
    ("ctrl+alt+ArrowRight", "switch workspace"),
    ("ctrl+alt+ArrowUp", "switch workspace"),
    ("ctrl+alt+ArrowDown", "switch workspace"),
];

/// Everyday editing shortcuts every app relies on.
const EDITING_KEYS: &[(&str, &str)] = &[
    ("KeyC", "copy"),
    ("KeyV", "paste"),
    ("KeyX", "cut"),
    ("KeyZ", "undo"),
    ("KeyY", "redo"),
    ("KeyA", "select all"),
    ("KeyS", "save"),
    ("KeyP", "print"),
    ("KeyF", "find"),
    ("KeyN", "new"),
    ("KeyO", "open"),
    ("KeyW", "close"),
    ("KeyT", "new tab"),
    ("KeyQ", "quit"),
    ("Tab", "switch tabs"),
];

/// Checks a shortcut against the rules this app enforces and the shortcuts
/// the operating system keeps for itself. Shortcuts held by other apps are
/// found when the shortcut is registered.
pub fn validate(s: &str, others: &[&str]) -> Result<(), String> {
    let (m, code) = parse(s).ok_or("This shortcut could not be read. Try again.")?;
    hotkey(s)?;
    if is_modifier_code(&code) {
        return Err("Add a regular key after the modifier keys.".into());
    }
    let cmd = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
    if !(m.ctrl || m.alt || m.sup) && !is_fkey(&code) {
        return Err(format!("Add {cmd} or Alt so this shortcut does not get in the way of typing."));
    }
    for other in others {
        if let Some((om, ocode)) = parse(other) {
            if om == m && ocode == code {
                return Err("Another action already uses this shortcut. Choose another one.".into());
            }
        }
    }
    for (r, what) in RESERVED {
        if let Some((rm, rcode)) = parse(r) {
            if rm == m && rcode == code {
                return Err(format!("{} uses this shortcut for {what}. Choose another one.", system_name()));
            }
        }
    }
    let primary = if cfg!(target_os = "macos") { m.sup } else { m.ctrl };
    let only_primary = primary && !m.alt && !m.shift && (cfg!(target_os = "macos") && !m.ctrl || !cfg!(target_os = "macos") && !m.sup);
    if only_primary {
        if let Some((_, what)) = EDITING_KEYS.iter().find(|(k, _)| *k == code) {
            return Err(format!("Most apps use this shortcut for {what}. Choose another one."));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_distinct() {
        assert!(validate(DEFAULT_RECORD, &[DEFAULT_PLAY, DEFAULT_LOOP]).is_ok());
        assert!(validate(DEFAULT_PLAY, &[DEFAULT_RECORD, DEFAULT_LOOP]).is_ok());
        assert!(validate(DEFAULT_LOOP, &[DEFAULT_RECORD, DEFAULT_PLAY]).is_ok());
        assert_eq!(chord_len(DEFAULT_RECORD), 4);
    }

    #[test]
    fn rejects_plain_typing_keys() {
        assert!(validate("shift+KeyA", &[DEFAULT_PLAY]).is_err());
        assert!(validate("KeyA", &[DEFAULT_PLAY]).is_err());
        assert!(validate("F9", &[DEFAULT_PLAY]).is_ok());
    }

    #[test]
    fn rejects_same_as_other() {
        assert!(validate(DEFAULT_PLAY, &[DEFAULT_RECORD, DEFAULT_PLAY]).is_err());
        assert!(validate(DEFAULT_LOOP, &[DEFAULT_LOOP]).is_err());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn rejects_system_and_editing_shortcuts() {
        assert!(validate("alt+F4", &[DEFAULT_PLAY]).is_err());
        assert!(validate("ctrl+KeyC", &[DEFAULT_PLAY]).is_err());
        assert!(validate("ctrl+alt+KeyC", &[DEFAULT_PLAY]).is_ok());
    }

    #[test]
    fn compose_parse_roundtrip() {
        let m = Mods { ctrl: true, alt: false, shift: true, sup: false };
        let s = compose(m, "KeyK");
        assert_eq!(s, "ctrl+shift+KeyK");
        assert_eq!(parse(&s), Some((m, "KeyK".into())));
    }

    #[cfg(windows)]
    #[test]
    fn labels_read_naturally() {
        assert_eq!(label("ctrl+alt+shift+F1"), "Ctrl + Alt + Shift + F1");
        assert_eq!(label("ctrl+Digit7"), "Ctrl + 7");
    }

    #[test]
    fn speed_shortcuts_map_to_steps_in_order() {
        let all = speed_shortcuts();
        let want = [0.25, 0.5, 1.0, 2.0, 4.0, 100.0];
        for (i, s) in all.iter().enumerate() {
            let (m, code) = parse(s).unwrap();
            assert_eq!(code, format!("F{}", i + 5));
            assert_eq!(m, parse(DEFAULT_RECORD).unwrap().0);
            assert!(hotkey(s).is_ok());
            assert_eq!(crate::speed::factor(i, 100.0), want[i]);
        }
        let mut ids: Vec<u32> = all.iter().map(|s| hotkey(s).unwrap().id()).collect();
        ids.extend([DEFAULT_RECORD, DEFAULT_PLAY, DEFAULT_LOOP].map(|s| hotkey(s).unwrap().id()));
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 9);
    }

    #[test]
    fn user_shortcut_cannot_take_a_speed_key() {
        let speeds = speed_shortcuts();
        let others: Vec<&str> = speeds.iter().map(|s| s.as_str()).collect();
        assert!(validate(&speed_shortcut(3), &others).is_err());
        assert!(validate(DEFAULT_RECORD, &others).is_ok());
    }
}
