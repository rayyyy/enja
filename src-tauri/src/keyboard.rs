//! Global keyboard listener (macOS).
//!
//! Uses CGEventTap directly instead of the `rdev` crate. rdev internally calls
//! TISGetInputSourceProperty (Text Services Manager) from the event-tap thread
//! to resolve key names. On macOS Sequoia+ Apple added a dispatch_assert_queue
//! assertion requiring those TSM calls to happen on the main thread, causing an
//! instant SIGTRAP crash. Enja only needs a small set of raw key codes and
//! modifier flags, so we skip TSM entirely and work with CGEvent directly.

use crate::settings::{AppSettings, ShortcutAction, ShortcutBinding};

#[derive(Debug, Clone)]
pub enum KeyboardTrigger {
    CmdCopyDouble,
    /// An Fn key tap (press + release) that did *not* form a chord with Space.
    /// Emitted on Fn release so we can wait and see whether Space arrives
    /// during the hold.
    FunctionTap,
    /// Space was pressed while Fn was held — Ask mode.
    FunctionSpace,
    /// Control was tapped by itself. Voice mode cycling decides whether it is
    /// currently meaningful.
    VoiceModeCycle,
    Escape,
    ShortcutCaptured {
        action: ShortcutAction,
        shortcut: ShortcutBinding,
    },
    ShortcutCaptureCancelled {
        action: ShortcutAction,
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct KeyboardRuntimeSettings {
    pub double_tap_threshold_ms: u64,
    pub voice_dictation_shortcut: ShortcutBinding,
    pub voice_ask_shortcut: ShortcutBinding,
}

impl From<&AppSettings> for KeyboardRuntimeSettings {
    fn from(settings: &AppSettings) -> Self {
        Self {
            double_tap_threshold_ms: settings.app.double_tap_threshold_ms,
            voice_dictation_shortcut: settings.shortcuts.voice_dictation.clone(),
            voice_ask_shortcut: settings.shortcuts.voice_ask.clone(),
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    #![allow(clippy::items_after_test_module)]

    use super::{KeyboardRuntimeSettings, KeyboardTrigger};
    use crate::settings::{ShortcutAction, ShortcutBinding, ShortcutModifiers};
    use std::os::raw::c_void;
    use std::sync::mpsc::Sender;
    use std::time::{Duration, Instant};

    /// Grace window after an Fn release during which an incoming Space press
    /// is still treated as part of an Fn+Space chord (Ask mode). This makes
    /// real-world chord typing robust against the user releasing Fn a few
    /// milliseconds before pressing Space.
    const FN_SPACE_GRACE_MS: u64 = 80;

    // --- FFI types -----------------------------------------------------------

    type CGEventRef = *mut c_void;
    type CFMachPortRef = *const c_void;
    type CFRunLoopSourceRef = *const c_void;
    type CFRunLoopRef = *const c_void;
    type CFRunLoopMode = *const c_void;
    type CFAllocatorRef = *const c_void;

    type CGEventTapCallBack = unsafe extern "C" fn(
        proxy: *const c_void,
        event_type: u32,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef;

    // --- Constants -----------------------------------------------------------

    const KCG_HID_EVENT_TAP: u32 = 0;
    const KCG_HEAD_INSERT_EVENT_TAP: u32 = 0;
    const KCG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;

    const KCG_EVENT_KEY_DOWN: u32 = 10;
    const KCG_EVENT_KEY_UP: u32 = 11;
    const KCG_EVENT_FLAGS_CHANGED: u32 = 12;
    const KCG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const KCG_EVENT_TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;

    const KCG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
    const KCG_EVENT_FLAG_MASK_SHIFT: u64 = 0x0002_0000;
    const KCG_EVENT_FLAG_MASK_CONTROL: u64 = 0x0004_0000;
    const KCG_EVENT_FLAG_MASK_ALTERNATE: u64 = 0x0008_0000;
    const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;
    const KCG_EVENT_FLAG_MASK_SECONDARY_FN: u64 = 0x0080_0000;

    /// KeyDown | KeyUp | FlagsChanged
    const EVENT_MASK: u64 = (1 << 10) | (1 << 11) | (1 << 12);

    const KEYCODE_C: i64 = 8;
    const KEYCODE_SPACE: i64 = 49;
    const KEYCODE_ESCAPE: i64 = 53;

    // --- FFI bindings --------------------------------------------------------

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        fn CGEventGetFlags(event: CGEventRef) -> u64;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFMachPortCreateRunLoopSource(
            allocator: CFAllocatorRef,
            port: CFMachPortRef,
            order: i64,
        ) -> CFRunLoopSourceRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFRunLoopMode);
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: CFRunLoopMode;
    }

    // --- Listener state (single-threaded: only accessed from the tap thread) -

    use std::sync::Mutex;

    struct ListenerState {
        tx: Sender<KeyboardTrigger>,
        threshold: Duration,
        runtime: KeyboardRuntimeSettings,
        meta_down: bool,
        control_down: bool,
        control_chord_used: bool,
        fn_down: bool,
        /// True while Fn is held and a Space press has already been registered
        /// for this hold. Used so the matching Fn release suppresses the
        /// FunctionTap event.
        fn_space_combo: bool,
        /// True when Fn has been used with any non-Fn key during this hold.
        /// This keeps a configured bare Fn tap from firing after a chord.
        fn_chord_used: bool,
        /// True while Space is being held as part of a Fn+Space chord. Used to
        /// avoid sending FunctionSpace repeatedly during auto-repeat and to
        /// swallow the matching Space up event.
        fn_space_down: bool,
        /// True after an Fn release while we're still inside the chord grace
        /// window. A Space arriving during this window upgrades the gesture
        /// from "Fn tap" to "Fn+Space".
        fn_recent_release: bool,
        /// Monotonically bumped whenever a pending FunctionTap must be
        /// invalidated (Fn re-press, Space chord in grace window, Escape,
        /// etc.). A scheduled tap only fires if its captured token still
        /// matches.
        fn_release_generation: u64,
        capture_action: Option<ShortcutAction>,
        capture_fn_down: bool,
        last_cmd_c: Option<Instant>,
        tap: CFMachPortRef,
    }

    // Safety: CFMachPortRef is only used from the tap thread's CFRunLoop.
    unsafe impl Send for ListenerState {}

    static LISTENER_STATE: Mutex<Option<Box<ListenerState>>> = Mutex::new(None);

    // --- Event tap callback --------------------------------------------------

    unsafe extern "C" fn raw_callback(
        _proxy: *const c_void,
        event_type: u32,
        event: CGEventRef,
        _user_info: *mut c_void,
    ) -> CGEventRef {
        if event_type == KCG_EVENT_TAP_DISABLED_BY_TIMEOUT
            || event_type == KCG_EVENT_TAP_DISABLED_BY_USER
        {
            eprintln!("[enja] event tap disabled (type={event_type:#X}) — re-enabling");
            if let Ok(guard) = LISTENER_STATE.lock() {
                if let Some(state) = guard.as_ref() {
                    CGEventTapEnable(state.tap, true);
                }
            }
            return event;
        }

        if event.is_null() {
            return event;
        }

        let Ok(mut guard) = LISTENER_STATE.lock() else {
            return event;
        };
        let Some(state) = guard.as_mut() else {
            return event;
        };

        match event_type {
            KCG_EVENT_FLAGS_CHANGED => {
                let flags = CGEventGetFlags(event);
                let cmd_down = (flags & KCG_EVENT_FLAG_MASK_COMMAND) != 0;
                let control_down = (flags & KCG_EVENT_FLAG_MASK_CONTROL) != 0;
                let fn_down = (flags & KCG_EVENT_FLAG_MASK_SECONDARY_FN) != 0;
                if cmd_down != state.meta_down {
                    state.meta_down = cmd_down;
                }
                if control_down != state.control_down {
                    state.control_down = control_down;
                    if state.capture_action.is_none() {
                        if control_down {
                            handle_control_pressed(state, flags);
                        } else {
                            handle_control_released(state, flags);
                        }
                    } else {
                        state.control_chord_used = false;
                    }
                } else if state.control_down && control_has_other_modifiers(flags) {
                    state.control_chord_used = true;
                }
                if fn_down != state.fn_down {
                    state.fn_down = fn_down;
                    if state.capture_action.is_some() {
                        handle_capture_fn_change(state, fn_down);
                    } else if fn_down {
                        handle_fn_pressed(state);
                    } else {
                        handle_fn_released(state);
                    }
                }
            }
            KCG_EVENT_KEY_DOWN => {
                let keycode = CGEventGetIntegerValueField(event, KCG_KEYBOARD_EVENT_KEYCODE);
                let flags = CGEventGetFlags(event);
                if state.capture_action.is_some() {
                    handle_capture_key_down(state, keycode, flags);
                    return std::ptr::null_mut();
                }
                if state.control_down {
                    state.control_chord_used = true;
                }
                if keycode == KEYCODE_ESCAPE {
                    // Any pending FunctionTap should be dropped — the user is
                    // explicitly cancelling.
                    invalidate_pending_fn_tap(state);
                    let _ = state.tx.send(KeyboardTrigger::Escape);
                    return event;
                }
                if keycode == KEYCODE_SPACE && (state.fn_down || state.fn_recent_release) {
                    // Treat as Fn+Space whether Fn is currently held *or* was
                    // released within the grace window.
                    if state.fn_recent_release {
                        invalidate_pending_fn_tap(state);
                    }
                    state.fn_chord_used = true;
                    let shortcut = ShortcutBinding::fn_space();
                    if !state.fn_space_down {
                        send_voice_shortcut_if_matched(state, &shortcut);
                    }
                    state.fn_space_down = true;
                    if state.fn_down {
                        state.fn_space_combo = true;
                    }
                    return std::ptr::null_mut();
                }
                if state.fn_down {
                    state.fn_chord_used = true;
                }
                if let Some(trigger) =
                    voice_trigger_for_shortcut(state, &shortcut_from_key_event(keycode, flags))
                {
                    let _ = state.tx.send(trigger);
                    return std::ptr::null_mut();
                }
                if keycode == KEYCODE_C && state.meta_down {
                    let now = Instant::now();
                    if let Some(prev) = state.last_cmd_c {
                        let elapsed = now.duration_since(prev);
                        if elapsed <= state.threshold {
                            let _ = state.tx.send(KeyboardTrigger::CmdCopyDouble);
                            state.last_cmd_c = None;
                        } else {
                            state.last_cmd_c = Some(now);
                        }
                    } else {
                        state.last_cmd_c = Some(now);
                    }
                }
            }
            KCG_EVENT_KEY_UP => {
                let keycode = CGEventGetIntegerValueField(event, KCG_KEYBOARD_EVENT_KEYCODE);
                if keycode == KEYCODE_SPACE && state.fn_space_down {
                    state.fn_space_down = false;
                    return std::ptr::null_mut();
                }
            }
            _ => {}
        }

        event
    }

    fn handle_fn_pressed(state: &mut ListenerState) {
        // Start a fresh chord-detection cycle. A new Fn press also cancels any
        // FunctionTap still sitting in the previous release's grace window.
        state.fn_space_combo = false;
        state.fn_chord_used = false;
        state.fn_space_down = false;
        invalidate_pending_fn_tap(state);
    }

    fn handle_fn_released(state: &mut ListenerState) {
        if state.fn_space_combo || state.fn_chord_used {
            // The Fn hold already included another key, so a bare Fn tap should
            // not fire on release.
            state.fn_space_combo = false;
            state.fn_chord_used = false;
            return;
        }

        // Defer the FunctionTap by the chord grace window. If Space arrives
        // during the window the gesture is reclassified as Fn+Space; otherwise
        // the tap fires after the window expires and Dictation toggles.
        state.fn_release_generation = state.fn_release_generation.wrapping_add(1);
        let token = state.fn_release_generation;
        state.fn_recent_release = true;

        let tx = state.tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(FN_SPACE_GRACE_MS));
            let mut trigger = None;
            if let Ok(mut guard) = LISTENER_STATE.lock() {
                if let Some(state) = guard.as_mut() {
                    if state.fn_release_generation == token && state.fn_recent_release {
                        state.fn_recent_release = false;
                        trigger = voice_trigger_for_shortcut(state, &ShortcutBinding::fn_key());
                    }
                }
            }
            if let Some(trigger) = trigger {
                let _ = tx.send(trigger);
            }
        });
    }

    fn invalidate_pending_fn_tap(state: &mut ListenerState) {
        state.fn_release_generation = state.fn_release_generation.wrapping_add(1);
        state.fn_recent_release = false;
    }

    fn handle_control_pressed(state: &mut ListenerState, flags: u64) {
        state.control_chord_used = control_has_other_modifiers(flags);
    }

    fn handle_control_released(state: &mut ListenerState, flags: u64) {
        let should_cycle = !state.control_chord_used && !control_has_other_modifiers(flags);
        state.control_chord_used = false;
        if should_cycle {
            let _ = state.tx.send(KeyboardTrigger::VoiceModeCycle);
        }
    }

    fn control_has_other_modifiers(flags: u64) -> bool {
        let other_modifiers = KCG_EVENT_FLAG_MASK_SHIFT
            | KCG_EVENT_FLAG_MASK_ALTERNATE
            | KCG_EVENT_FLAG_MASK_COMMAND
            | KCG_EVENT_FLAG_MASK_SECONDARY_FN;
        (flags & other_modifiers) != 0
    }

    fn handle_capture_fn_change(state: &mut ListenerState, fn_down: bool) {
        if fn_down {
            state.capture_fn_down = true;
            invalidate_pending_fn_tap(state);
            return;
        }

        if state.capture_fn_down {
            complete_capture(state, ShortcutBinding::fn_key());
        }
    }

    fn handle_capture_key_down(state: &mut ListenerState, keycode: i64, flags: u64) {
        if keycode == KEYCODE_ESCAPE {
            cancel_capture(state, "キャンセルしました。".to_string());
            return;
        }
        complete_capture(state, shortcut_from_key_event(keycode, flags));
    }

    fn complete_capture(state: &mut ListenerState, shortcut: ShortcutBinding) {
        let Some(action) = state.capture_action.take() else {
            return;
        };
        state.capture_fn_down = false;
        state.fn_space_down = false;
        state.fn_space_combo = false;
        state.fn_chord_used = state.fn_down;
        invalidate_pending_fn_tap(state);
        let _ = state
            .tx
            .send(KeyboardTrigger::ShortcutCaptured { action, shortcut });
    }

    fn cancel_capture(state: &mut ListenerState, reason: String) {
        let Some(action) = state.capture_action.take() else {
            return;
        };
        state.capture_fn_down = false;
        state.fn_space_down = false;
        state.fn_space_combo = false;
        state.fn_chord_used = state.fn_down;
        invalidate_pending_fn_tap(state);
        let _ = state
            .tx
            .send(KeyboardTrigger::ShortcutCaptureCancelled { action, reason });
    }

    fn send_voice_shortcut_if_matched(state: &ListenerState, shortcut: &ShortcutBinding) {
        if let Some(trigger) = voice_trigger_for_shortcut(state, shortcut) {
            let _ = state.tx.send(trigger);
        }
    }

    fn voice_trigger_for_shortcut(
        state: &ListenerState,
        shortcut: &ShortcutBinding,
    ) -> Option<KeyboardTrigger> {
        if shortcut.is_same_shortcut(&state.runtime.voice_dictation_shortcut) {
            Some(KeyboardTrigger::FunctionTap)
        } else if shortcut.is_same_shortcut(&state.runtime.voice_ask_shortcut) {
            Some(KeyboardTrigger::FunctionSpace)
        } else {
            None
        }
    }

    fn shortcut_from_key_event(keycode: i64, flags: u64) -> ShortcutBinding {
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

    fn modifiers_from_flags(flags: u64) -> ShortcutModifiers {
        ShortcutModifiers {
            command: (flags & KCG_EVENT_FLAG_MASK_COMMAND) != 0,
            option: (flags & KCG_EVENT_FLAG_MASK_ALTERNATE) != 0,
            control: (flags & KCG_EVENT_FLAG_MASK_CONTROL) != 0,
            shift: (flags & KCG_EVENT_FLAG_MASK_SHIFT) != 0,
            function: (flags & KCG_EVENT_FLAG_MASK_SECONDARY_FN) != 0,
        }
    }

    fn key_name(keycode: i64) -> (&'static str, &'static str) {
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

    #[cfg(test)]
    mod tests {
        use super::*;

        fn state_with_tx(tx: Sender<KeyboardTrigger>) -> ListenerState {
            ListenerState {
                tx,
                threshold: Duration::from_millis(400),
                runtime: KeyboardRuntimeSettings {
                    double_tap_threshold_ms: 400,
                    voice_dictation_shortcut: ShortcutBinding::fn_key(),
                    voice_ask_shortcut: ShortcutBinding::fn_space(),
                },
                meta_down: false,
                control_down: false,
                control_chord_used: false,
                fn_down: false,
                fn_space_combo: false,
                fn_chord_used: false,
                fn_space_down: false,
                fn_recent_release: false,
                fn_release_generation: 0,
                capture_action: None,
                capture_fn_down: false,
                last_cmd_c: None,
                tap: std::ptr::null(),
            }
        }

        #[test]
        fn default_voice_shortcuts_resolve_to_distinct_triggers() {
            let (tx, _rx) = std::sync::mpsc::channel();
            let state = state_with_tx(tx);

            assert!(matches!(
                voice_trigger_for_shortcut(&state, &ShortcutBinding::fn_key()),
                Some(KeyboardTrigger::FunctionTap)
            ));
            assert!(matches!(
                voice_trigger_for_shortcut(&state, &ShortcutBinding::fn_space()),
                Some(KeyboardTrigger::FunctionSpace)
            ));
        }

        #[test]
        fn shortcut_capture_emits_normalized_binding() {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut state = state_with_tx(tx);
            state.capture_action = Some(ShortcutAction::VoiceAsk);

            complete_capture(
                &mut state,
                shortcut_from_key_event(KEYCODE_SPACE, KCG_EVENT_FLAG_MASK_SECONDARY_FN),
            );

            match rx.recv_timeout(Duration::from_millis(20)).expect("trigger") {
                KeyboardTrigger::ShortcutCaptured { action, shortcut } => {
                    assert_eq!(action, ShortcutAction::VoiceAsk);
                    assert!(shortcut.is_same_shortcut(&ShortcutBinding::fn_space()));
                }
                trigger => panic!("unexpected trigger: {trigger:?}"),
            }
        }

        #[test]
        fn shortcut_capture_cancel_emits_reason() {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut state = state_with_tx(tx);
            state.capture_action = Some(ShortcutAction::VoiceDictation);

            cancel_capture(&mut state, "キャンセルしました。".to_string());

            match rx.recv_timeout(Duration::from_millis(20)).expect("trigger") {
                KeyboardTrigger::ShortcutCaptureCancelled { action, reason } => {
                    assert_eq!(action, ShortcutAction::VoiceDictation);
                    assert_eq!(reason, "キャンセルしました。");
                }
                trigger => panic!("unexpected trigger: {trigger:?}"),
            }
        }

        #[test]
        fn control_tap_emits_voice_mode_cycle() {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut state = state_with_tx(tx);

            handle_control_pressed(&mut state, KCG_EVENT_FLAG_MASK_CONTROL);
            handle_control_released(&mut state, 0);

            assert!(matches!(
                rx.recv_timeout(Duration::from_millis(20)).expect("trigger"),
                KeyboardTrigger::VoiceModeCycle
            ));
        }

        #[test]
        fn control_chord_does_not_emit_voice_mode_cycle() {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut state = state_with_tx(tx);

            handle_control_pressed(&mut state, KCG_EVENT_FLAG_MASK_CONTROL);
            state.control_chord_used = true;
            handle_control_released(&mut state, 0);

            assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
        }
    }

    // --- Public entry point --------------------------------------------------

    pub fn spawn_listener(tx: Sender<KeyboardTrigger>, runtime: KeyboardRuntimeSettings) {
        let threshold = Duration::from_millis(runtime.double_tap_threshold_ms.max(50));

        std::thread::spawn(move || unsafe {
            let tap = CGEventTapCreate(
                KCG_HID_EVENT_TAP,
                KCG_HEAD_INSERT_EVENT_TAP,
                KCG_EVENT_TAP_OPTION_DEFAULT,
                EVENT_MASK,
                raw_callback,
                std::ptr::null_mut(),
            );
            if tap.is_null() {
                eprintln!(
                    "[enja] CGEventTapCreate failed. \
                         Grant Input Monitoring / Accessibility permission for this app."
                );
                return;
            }
            if let Ok(mut guard) = LISTENER_STATE.lock() {
                *guard = Some(Box::new(ListenerState {
                    tx,
                    threshold,
                    runtime: runtime.clone(),
                    meta_down: false,
                    control_down: false,
                    control_chord_used: false,
                    fn_down: false,
                    fn_space_combo: false,
                    fn_chord_used: false,
                    fn_space_down: false,
                    fn_recent_release: false,
                    fn_release_generation: 0,
                    capture_action: None,
                    capture_fn_down: false,
                    last_cmd_c: None,
                    tap,
                }));
            }

            let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            if source.is_null() {
                eprintln!("[enja] CFMachPortCreateRunLoopSource failed");
                return;
            }

            let current_loop = CFRunLoopGetCurrent();
            CFRunLoopAddSource(current_loop, source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            CFRunLoopRun();
        });
    }

    pub fn update_runtime_settings(runtime: KeyboardRuntimeSettings) {
        if let Ok(mut guard) = LISTENER_STATE.lock() {
            if let Some(state) = guard.as_mut() {
                state.threshold = Duration::from_millis(runtime.double_tap_threshold_ms.max(50));
                state.runtime = runtime;
                invalidate_pending_fn_tap(state);
            }
        }
    }

    pub fn begin_shortcut_capture(action: ShortcutAction) -> Result<(), String> {
        let mut guard = LISTENER_STATE.lock().map_err(|e| e.to_string())?;
        let Some(state) = guard.as_mut() else {
            return Err("キーボードリスナーが起動していません。".to_string());
        };
        state.capture_action = Some(action);
        state.capture_fn_down = false;
        state.fn_space_down = false;
        state.fn_space_combo = false;
        state.fn_chord_used = false;
        invalidate_pending_fn_tap(state);
        Ok(())
    }

    pub fn cancel_shortcut_capture() -> Result<(), String> {
        let mut guard = LISTENER_STATE.lock().map_err(|e| e.to_string())?;
        if let Some(state) = guard.as_mut() {
            state.capture_action = None;
            state.capture_fn_down = false;
            invalidate_pending_fn_tap(state);
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub use macos::{
    begin_shortcut_capture, cancel_shortcut_capture, spawn_listener, update_runtime_settings,
};

#[cfg(not(target_os = "macos"))]
pub fn spawn_listener(
    _tx: std::sync::mpsc::Sender<KeyboardTrigger>,
    _runtime: KeyboardRuntimeSettings,
) {
}

#[cfg(not(target_os = "macos"))]
pub fn update_runtime_settings(_runtime: KeyboardRuntimeSettings) {}

#[cfg(not(target_os = "macos"))]
pub fn begin_shortcut_capture(_action: ShortcutAction) -> Result<(), String> {
    Err("ショートカット記録はmacOSでのみ利用できます。".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn cancel_shortcut_capture() -> Result<(), String> {
    Ok(())
}
