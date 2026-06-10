//! macOS キーボード監視の単体テスト。

#[allow(clippy::wildcard_imports)]
use super::*;

fn state_with_tx(tx: Sender<KeyboardTrigger>) -> ListenerState {
    ListenerState {
        tx,
        threshold: Duration::from_millis(400),
        runtime: KeyboardRuntimeSettings {
            double_tap_threshold_ms: 400,
            voice_dictation_shortcut: ShortcutBinding::fn_key(),
            voice_ask_shortcut: ShortcutBinding::fn_space(),
            polish_selection_shortcut: ShortcutBinding::ctrl_option_p(),
        },
        meta_down: false,
        control_down: false,
        control_chord_used: false,
        fn_down: false,
        fn_modifier_down: false,
        fn_keycode_down: false,
        suppress_fn_modifier_until_up: false,
        suppress_fn_keycode_until_up: false,
        last_fn_modifier_release_at: None,
        last_fn_keycode_release_at: None,
        fn_space_combo: false,
        fn_chord_used: false,
        fn_space_down: false,
        fn_recent_release: false,
        voice_overlay_visible: false,
        fn_hold_generation: 0,
        fn_hold_cheat_sheet_visible: false,
        fn_hold_cheat_sheet_used: false,
        fn_release_generation: 0,
        fn_recent_release_at: None,
        last_fn_tap: None,
        capture_action: None,
        capture_fn_down: false,
        capture_fn_tap_at: None,
        capture_fn_release_generation: 0,
        last_cmd_c: None,
        tap: std::ptr::null(),
    }
}

#[test]
fn default_voice_shortcuts_resolve_to_distinct_triggers() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let state = state_with_tx(tx);

    assert!(matches!(
        start_trigger_for_shortcut(&state, &ShortcutBinding::fn_key()),
        Some(KeyboardTrigger::VoiceDictationStart)
    ));
    assert!(matches!(
        start_trigger_for_shortcut(&state, &ShortcutBinding::fn_space()),
        Some(KeyboardTrigger::FunctionSpace)
    ));
    assert!(matches!(
        start_trigger_for_shortcut(&state, &ShortcutBinding::ctrl_option_p()),
        Some(KeyboardTrigger::PolishSelection)
    ));
}

#[test]
fn custom_dictation_start_does_not_replace_fixed_fn_tap() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    let custom_shortcut = shortcut_from_key_event(KEYCODE_C, KCG_EVENT_FLAG_MASK_SHIFT);
    state.runtime.voice_dictation_shortcut = custom_shortcut.clone();

    assert!(matches!(
        start_trigger_for_shortcut(&state, &custom_shortcut),
        Some(KeyboardTrigger::VoiceDictationStart)
    ));
    assert!(matches!(
        trigger_for_confirmed_fn_tap(&mut state, Instant::now(), true),
        Some(KeyboardTrigger::FunctionTap)
    ));
}

#[test]
fn fn_double_tap_resolves_to_configured_voice_shortcut() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    state.runtime.voice_dictation_shortcut = ShortcutBinding::fn_double_tap();

    let first_tap = Instant::now();
    assert!(matches!(
        trigger_for_confirmed_fn_tap(&mut state, first_tap, true),
        Some(KeyboardTrigger::FunctionTap)
    ));

    let second_tap = first_tap + Duration::from_millis(120);
    assert!(matches!(
        trigger_for_confirmed_fn_tap(&mut state, second_tap, true),
        Some(KeyboardTrigger::VoiceDictationStart)
    ));
}

#[test]
fn fn_double_tap_does_not_match_after_threshold() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    state.runtime.voice_dictation_shortcut = ShortcutBinding::fn_double_tap();

    let first_tap = Instant::now();
    assert!(matches!(
        trigger_for_confirmed_fn_tap(&mut state, first_tap, true),
        Some(KeyboardTrigger::FunctionTap)
    ));

    let second_tap = first_tap + Duration::from_millis(401);
    assert!(matches!(
        trigger_for_confirmed_fn_tap(&mut state, second_tap, true),
        Some(KeyboardTrigger::FunctionTap)
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
fn shortcut_capture_treats_globe_function_keycode_as_fn_tap() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    state.capture_action = Some(ShortcutAction::VoiceDictation);

    assert!(is_function_keycode(KEYCODE_GLOBE_FUNCTION));
    handle_function_keycode_change(&mut state, true);

    assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
    assert!(state.capture_fn_down);

    handle_function_keycode_change(&mut state, false);
    assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());

    let released_at = state.capture_fn_tap_at.expect("pending Fn tap");
    let token = state.capture_fn_release_generation;
    complete_pending_capture_fn_tap(&mut state, token, released_at);

    match rx.recv_timeout(Duration::from_millis(20)).expect("trigger") {
        KeyboardTrigger::ShortcutCaptured { action, shortcut } => {
            assert_eq!(action, ShortcutAction::VoiceDictation);
            assert!(shortcut.is_same_shortcut(&ShortcutBinding::fn_key()));
        }
        trigger => panic!("unexpected trigger: {trigger:?}"),
    }
}

#[test]
fn shortcut_capture_treats_globe_function_keycode_double_tap_as_fn_double_tap() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    state.capture_action = Some(ShortcutAction::VoiceDictation);

    handle_function_keycode_change(&mut state, true);
    handle_function_keycode_change(&mut state, false);
    handle_function_keycode_change(&mut state, true);
    handle_function_keycode_change(&mut state, false);

    match rx.recv_timeout(Duration::from_millis(20)).expect("trigger") {
        KeyboardTrigger::ShortcutCaptured { action, shortcut } => {
            assert_eq!(action, ShortcutAction::VoiceDictation);
            assert!(shortcut.is_same_shortcut(&ShortcutBinding::fn_double_tap()));
        }
        trigger => panic!("unexpected trigger: {trigger:?}"),
    }
}

#[test]
fn shortcut_capture_deduplicates_keycode_then_modifier_for_one_fn_tap() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    state.capture_action = Some(ShortcutAction::VoiceDictation);
    let start = Instant::now();

    handle_function_source_change(&mut state, FunctionSource::Keycode, true, start);
    handle_function_source_change(
        &mut state,
        FunctionSource::Keycode,
        false,
        start + Duration::from_millis(8),
    );
    handle_function_source_change(
        &mut state,
        FunctionSource::Modifier,
        true,
        start + Duration::from_millis(16),
    );
    handle_function_source_change(
        &mut state,
        FunctionSource::Modifier,
        false,
        start + Duration::from_millis(24),
    );

    assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
    let released_at = state.capture_fn_tap_at.expect("pending Fn tap");
    let token = state.capture_fn_release_generation;
    complete_pending_capture_fn_tap(&mut state, token, released_at);

    match rx.recv_timeout(Duration::from_millis(20)).expect("trigger") {
        KeyboardTrigger::ShortcutCaptured { action, shortcut } => {
            assert_eq!(action, ShortcutAction::VoiceDictation);
            assert!(shortcut.is_same_shortcut(&ShortcutBinding::fn_key()));
        }
        trigger => panic!("unexpected trigger: {trigger:?}"),
    }
}

#[test]
fn shortcut_capture_deduplicates_modifier_then_keycode_for_one_fn_tap() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    state.capture_action = Some(ShortcutAction::VoiceDictation);
    let start = Instant::now();

    handle_function_source_change(&mut state, FunctionSource::Modifier, true, start);
    handle_function_source_change(
        &mut state,
        FunctionSource::Modifier,
        false,
        start + Duration::from_millis(8),
    );
    handle_function_source_change(
        &mut state,
        FunctionSource::Keycode,
        true,
        start + Duration::from_millis(16),
    );
    handle_function_source_change(
        &mut state,
        FunctionSource::Keycode,
        false,
        start + Duration::from_millis(24),
    );

    assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
    let released_at = state.capture_fn_tap_at.expect("pending Fn tap");
    let token = state.capture_fn_release_generation;
    complete_pending_capture_fn_tap(&mut state, token, released_at);

    match rx.recv_timeout(Duration::from_millis(20)).expect("trigger") {
        KeyboardTrigger::ShortcutCaptured { action, shortcut } => {
            assert_eq!(action, ShortcutAction::VoiceDictation);
            assert!(shortcut.is_same_shortcut(&ShortcutBinding::fn_key()));
        }
        trigger => panic!("unexpected trigger: {trigger:?}"),
    }
}

#[test]
fn duplicate_fn_sources_emit_single_fixed_fn_tap() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    state.runtime.voice_dictation_shortcut = ShortcutBinding::fn_double_tap();
    let start = Instant::now();

    handle_function_source_change(&mut state, FunctionSource::Keycode, true, start);
    handle_function_source_change(
        &mut state,
        FunctionSource::Keycode,
        false,
        start + Duration::from_millis(8),
    );
    handle_function_source_change(
        &mut state,
        FunctionSource::Modifier,
        true,
        start + Duration::from_millis(16),
    );
    handle_function_source_change(
        &mut state,
        FunctionSource::Modifier,
        false,
        start + Duration::from_millis(24),
    );

    assert!(matches!(
        confirm_pending_fn_tap(&mut state, true),
        Some(KeyboardTrigger::FunctionTap)
    ));
    assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
}

#[test]
fn cheat_sheet_layer_keys_resolve_to_actions() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    state.fn_hold_cheat_sheet_visible = true;

    assert!(matches!(
        cheat_sheet_trigger_for_key(&state, KEYCODE_D),
        Some(KeyboardTrigger::VoiceDictationStart)
    ));
    assert!(matches!(
        cheat_sheet_trigger_for_key(&state, KEYCODE_P),
        Some(KeyboardTrigger::PolishSelection)
    ));
    assert!(matches!(
        cheat_sheet_trigger_for_key(&state, KEYCODE_C),
        Some(KeyboardTrigger::CmdCopyDouble)
    ));
}

#[test]
fn fn_release_after_cheat_sheet_does_not_emit_function_tap() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    state.fn_down = true;
    state.fn_hold_cheat_sheet_visible = true;
    state.fn_hold_cheat_sheet_used = true;

    handle_fn_released(&mut state);

    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(20)).expect("trigger"),
        KeyboardTrigger::ShortcutCheatSheetHide
    ));
    assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
}

#[test]
fn shortcut_capture_emits_fn_double_tap() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    state.capture_action = Some(ShortcutAction::VoiceDictation);
    state.capture_fn_tap_at = Some(Instant::now());

    handle_capture_fn_released(&mut state);

    match rx.recv_timeout(Duration::from_millis(20)).expect("trigger") {
        KeyboardTrigger::ShortcutCaptured { action, shortcut } => {
            assert_eq!(action, ShortcutAction::VoiceDictation);
            assert!(shortcut.is_same_shortcut(&ShortcutBinding::fn_double_tap()));
        }
        trigger => panic!("unexpected trigger: {trigger:?}"),
    }
}

#[test]
fn shortcut_capture_emits_fn_single_tap_after_threshold() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = state_with_tx(tx);
    state.capture_action = Some(ShortcutAction::VoiceDictation);
    let released_at = Instant::now();
    state.capture_fn_tap_at = Some(released_at);
    state.capture_fn_release_generation = 1;

    complete_pending_capture_fn_tap(&mut state, 1, released_at);

    match rx.recv_timeout(Duration::from_millis(20)).expect("trigger") {
        KeyboardTrigger::ShortcutCaptured { action, shortcut } => {
            assert_eq!(action, ShortcutAction::VoiceDictation);
            assert!(shortcut.is_same_shortcut(&ShortcutBinding::fn_key()));
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
