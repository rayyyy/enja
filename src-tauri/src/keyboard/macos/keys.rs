//! キーコード・ショートカット照合・キー名変換。

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) const KEYCODE_C: i64 = 8;
pub(crate) const KEYCODE_D: i64 = 2;
pub(crate) const KEYCODE_P: i64 = 35;
pub(crate) const KEYCODE_SPACE: i64 = 49;
pub(crate) const KEYCODE_ESCAPE: i64 = 53;
pub(crate) const KEYCODE_FUNCTION: i64 = 63;
pub(crate) const KEYCODE_GLOBE_FUNCTION: i64 = 179;

pub(crate) fn cheat_sheet_trigger_for_key(
    state: &ListenerState,
    keycode: i64,
) -> Option<KeyboardTrigger> {
    if !state.fn_hold_cheat_sheet_visible {
        return None;
    }
    match keycode {
        KEYCODE_D => Some(KeyboardTrigger::VoiceDictationStart),
        KEYCODE_P => Some(KeyboardTrigger::PolishSelection),
        KEYCODE_C => Some(KeyboardTrigger::CmdCopyDouble),
        _ => None,
    }
}

pub(crate) fn is_function_keycode(keycode: i64) -> bool {
    // Some macOS keyboard layouts emit Fn/Globe as ordinary key events in
    // addition to, or instead of, the secondary-Fn modifier flag.
    matches!(keycode, KEYCODE_FUNCTION | KEYCODE_GLOBE_FUNCTION)
}

pub(crate) fn start_trigger_for_shortcut(
    state: &ListenerState,
    shortcut: &ShortcutBinding,
) -> Option<KeyboardTrigger> {
    if shortcut.is_same_shortcut(&state.runtime.voice_dictation_shortcut) {
        Some(KeyboardTrigger::VoiceDictationStart)
    } else if shortcut.is_same_shortcut(&state.runtime.voice_ask_shortcut) {
        Some(KeyboardTrigger::FunctionSpace)
    } else if shortcut.is_same_shortcut(&state.runtime.polish_selection_shortcut) {
        Some(KeyboardTrigger::PolishSelection)
    } else {
        None
    }
}

pub(crate) fn shortcut_from_key_event(keycode: i64, flags: u64) -> ShortcutBinding {
    let (key, label) = key_name(keycode);
    let label = if key == "unknown" {
        format!("Key {keycode}")
    } else {
        label.to_string()
    };
    ShortcutBinding::from_parts(
        Some(keycode),
        key.to_string(),
        label,
        modifiers_from_flags(flags),
    )
}

pub(crate) fn modifiers_from_flags(flags: u64) -> ShortcutModifiers {
    ShortcutModifiers {
        command: (flags & KCG_EVENT_FLAG_MASK_COMMAND) != 0,
        option: (flags & KCG_EVENT_FLAG_MASK_ALTERNATE) != 0,
        control: (flags & KCG_EVENT_FLAG_MASK_CONTROL) != 0,
        shift: (flags & KCG_EVENT_FLAG_MASK_SHIFT) != 0,
        function: (flags & KCG_EVENT_FLAG_MASK_SECONDARY_FN) != 0,
    }
}

pub(crate) fn key_name(keycode: i64) -> (&'static str, &'static str) {
    match keycode {
        0 => ("a", "A"),
        1 => ("s", "S"),
        2 => ("d", "D"),
        3 => ("f", "F"),
        4 => ("h", "H"),
        5 => ("g", "G"),
        6 => ("z", "Z"),
        7 => ("x", "X"),
        8 => ("c", "C"),
        9 => ("v", "V"),
        11 => ("b", "B"),
        12 => ("q", "Q"),
        13 => ("w", "W"),
        14 => ("e", "E"),
        15 => ("r", "R"),
        16 => ("y", "Y"),
        17 => ("t", "T"),
        18 => ("1", "1"),
        19 => ("2", "2"),
        20 => ("3", "3"),
        21 => ("4", "4"),
        22 => ("6", "6"),
        23 => ("5", "5"),
        24 => ("equal", "="),
        25 => ("9", "9"),
        26 => ("7", "7"),
        27 => ("minus", "-"),
        28 => ("8", "8"),
        29 => ("0", "0"),
        30 => ("rightBracket", "]"),
        31 => ("o", "O"),
        32 => ("u", "U"),
        33 => ("leftBracket", "["),
        34 => ("i", "I"),
        35 => ("p", "P"),
        36 => ("return", "Return"),
        37 => ("l", "L"),
        38 => ("j", "J"),
        39 => ("quote", "'"),
        40 => ("k", "K"),
        41 => ("semicolon", ";"),
        42 => ("backslash", "\\"),
        43 => ("comma", ","),
        44 => ("slash", "/"),
        45 => ("n", "N"),
        46 => ("m", "M"),
        47 => ("period", "."),
        48 => ("tab", "Tab"),
        49 => ("space", "Space"),
        50 => ("grave", "`"),
        51 => ("delete", "Delete"),
        53 => ("escape", "Escape"),
        65 => ("keypadDecimal", "Keypad ."),
        67 => ("keypadMultiply", "Keypad *"),
        69 => ("keypadPlus", "Keypad +"),
        71 => ("clear", "Clear"),
        75 => ("keypadDivide", "Keypad /"),
        76 => ("keypadEnter", "Keypad Enter"),
        78 => ("keypadMinus", "Keypad -"),
        81 => ("keypadEquals", "Keypad ="),
        82 => ("keypad0", "Keypad 0"),
        83 => ("keypad1", "Keypad 1"),
        84 => ("keypad2", "Keypad 2"),
        85 => ("keypad3", "Keypad 3"),
        86 => ("keypad4", "Keypad 4"),
        87 => ("keypad5", "Keypad 5"),
        88 => ("keypad6", "Keypad 6"),
        89 => ("keypad7", "Keypad 7"),
        91 => ("keypad8", "Keypad 8"),
        92 => ("keypad9", "Keypad 9"),
        96 => ("f5", "F5"),
        97 => ("f6", "F6"),
        98 => ("f7", "F7"),
        99 => ("f3", "F3"),
        100 => ("f8", "F8"),
        101 => ("f9", "F9"),
        103 => ("f11", "F11"),
        105 => ("f13", "F13"),
        106 => ("f16", "F16"),
        107 => ("f14", "F14"),
        109 => ("f10", "F10"),
        111 => ("f12", "F12"),
        113 => ("f15", "F15"),
        114 => ("help", "Help"),
        115 => ("home", "Home"),
        116 => ("pageUp", "Page Up"),
        117 => ("forwardDelete", "Forward Delete"),
        118 => ("f4", "F4"),
        119 => ("end", "End"),
        120 => ("f2", "F2"),
        121 => ("pageDown", "Page Down"),
        122 => ("f1", "F1"),
        123 => ("leftArrow", "Left"),
        124 => ("rightArrow", "Right"),
        125 => ("downArrow", "Down"),
        126 => ("upArrow", "Up"),
        _ => ("unknown", "Key"),
    }
}
