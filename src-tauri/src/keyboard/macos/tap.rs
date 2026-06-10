//! CGEventTap のコールバックと公開エントリポイント。

#[allow(clippy::wildcard_imports)]
use super::*;

// --- Event tap callback --------------------------------------------------

pub(crate) unsafe extern "C" fn raw_callback(
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
            handle_function_modifier_change(state, fn_down);
        }
        KCG_EVENT_KEY_DOWN => {
            let keycode = CGEventGetIntegerValueField(event, KCG_KEYBOARD_EVENT_KEYCODE);
            let flags = CGEventGetFlags(event);
            if is_function_keycode(keycode) {
                handle_function_keycode_change(state, true);
                return std::ptr::null_mut();
            }
            if state.capture_action.is_some() {
                handle_capture_key_down(state, keycode, flags);
                return std::ptr::null_mut();
            }
            if state.control_down {
                state.control_chord_used = true;
            }
            if let Some(trigger) = cheat_sheet_trigger_for_key(state, keycode) {
                hide_fn_hold_cheat_sheet(state);
                let _ = state.tx.send(trigger);
                return std::ptr::null_mut();
            }
            if keycode == KEYCODE_ESCAPE && state.fn_hold_cheat_sheet_visible {
                hide_fn_hold_cheat_sheet(state);
                return std::ptr::null_mut();
            }
            if keycode == KEYCODE_ESCAPE {
                // Any pending FunctionTap should be dropped — the user is
                // explicitly cancelling.
                let should_swallow = state.voice_overlay_visible;
                invalidate_pending_fn_tap(state);
                invalidate_pending_fn_hold_cheat_sheet(state);
                reset_fn_tap_sequence(state);
                let _ = state.tx.send(KeyboardTrigger::Escape);
                if should_swallow {
                    return std::ptr::null_mut();
                }
                return event;
            }
            if keycode == KEYCODE_SPACE && (state.fn_down || state.fn_recent_release) {
                // Treat as Fn+Space whether Fn is currently held *or* was
                // released within the grace window.
                if state.fn_hold_cheat_sheet_visible {
                    hide_fn_hold_cheat_sheet(state);
                }
                if state.fn_recent_release {
                    invalidate_pending_fn_tap(state);
                }
                invalidate_pending_fn_hold_cheat_sheet(state);
                reset_fn_tap_sequence(state);
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
                if state.fn_hold_cheat_sheet_visible {
                    hide_fn_hold_cheat_sheet(state);
                }
                state.fn_chord_used = true;
                invalidate_pending_fn_hold_cheat_sheet(state);
                reset_fn_tap_sequence(state);
            }
            if let Some(trigger) =
                start_trigger_for_shortcut(state, &shortcut_from_key_event(keycode, flags))
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
            if is_function_keycode(keycode) {
                handle_function_keycode_change(state, false);
                return std::ptr::null_mut();
            }
            if keycode == KEYCODE_SPACE && state.fn_space_down {
                state.fn_space_down = false;
                return std::ptr::null_mut();
            }
        }
        _ => {}
    }

    event
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
            invalidate_pending_capture_fn_tap(state);
            invalidate_pending_fn_hold_cheat_sheet(state);
            hide_fn_hold_cheat_sheet(state);
            reset_fn_tap_sequence(state);
            reset_function_sources(state);
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
    invalidate_pending_capture_fn_tap(state);
    invalidate_pending_fn_hold_cheat_sheet(state);
    hide_fn_hold_cheat_sheet(state);
    reset_fn_tap_sequence(state);
    reset_function_sources(state);
    Ok(())
}

pub fn cancel_shortcut_capture() -> Result<(), String> {
    let mut guard = LISTENER_STATE.lock().map_err(|e| e.to_string())?;
    if let Some(state) = guard.as_mut() {
        state.capture_action = None;
        state.capture_fn_down = false;
        invalidate_pending_fn_tap(state);
        invalidate_pending_capture_fn_tap(state);
        invalidate_pending_fn_hold_cheat_sheet(state);
        hide_fn_hold_cheat_sheet(state);
        reset_fn_tap_sequence(state);
        reset_function_sources(state);
    }
    Ok(())
}

pub fn set_voice_overlay_visible(visible: bool) {
    if let Ok(mut guard) = LISTENER_STATE.lock() {
        if let Some(state) = guard.as_mut() {
            state.voice_overlay_visible = visible;
        }
    }
}
