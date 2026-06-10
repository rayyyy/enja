//! ショートカットキャプチャ(設定画面での割り当て取り込み)。

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) fn handle_capture_fn_change(state: &mut ListenerState, fn_down: bool) {
    if fn_down {
        state.capture_fn_down = true;
        cancel_pending_capture_fn_completion(state);
        invalidate_pending_fn_tap(state);
        return;
    }

    if state.capture_fn_down {
        state.capture_fn_down = false;
        handle_capture_fn_released(state);
    }
}

pub(crate) fn handle_capture_fn_released(state: &mut ListenerState) {
    let released_at = Instant::now();
    let is_double_tap = state
        .capture_fn_tap_at
        .and_then(|previous| released_at.checked_duration_since(previous))
        .is_some_and(|elapsed| elapsed <= state.threshold);

    if is_double_tap {
        invalidate_pending_capture_fn_tap(state);
        complete_capture(state, ShortcutBinding::fn_double_tap());
        return;
    }

    state.capture_fn_tap_at = Some(released_at);
    state.capture_fn_release_generation = state.capture_fn_release_generation.wrapping_add(1);
    let token = state.capture_fn_release_generation;
    let threshold = state.threshold;

    std::thread::spawn(move || {
        std::thread::sleep(threshold);
        if let Ok(mut guard) = LISTENER_STATE.lock() {
            if let Some(state) = guard.as_mut() {
                complete_pending_capture_fn_tap(state, token, released_at);
            }
        }
    });
}

pub(crate) fn complete_pending_capture_fn_tap(
    state: &mut ListenerState,
    token: u64,
    released_at: Instant,
) {
    let pending_matches = state.capture_fn_release_generation == token
        && state.capture_fn_tap_at == Some(released_at)
        && state.capture_action.is_some();
    if pending_matches {
        complete_capture(state, ShortcutBinding::fn_key());
    }
}

pub(crate) fn handle_capture_key_down(state: &mut ListenerState, keycode: i64, flags: u64) {
    if keycode == KEYCODE_ESCAPE {
        cancel_capture(state, "キャンセルしました。".to_string());
        return;
    }
    invalidate_pending_capture_fn_tap(state);
    complete_capture(state, shortcut_from_key_event(keycode, flags));
}

pub(crate) fn complete_capture(state: &mut ListenerState, shortcut: ShortcutBinding) {
    let Some(action) = state.capture_action.take() else {
        return;
    };
    state.capture_fn_down = false;
    invalidate_pending_capture_fn_tap(state);
    state.fn_space_down = false;
    state.fn_space_combo = false;
    state.fn_chord_used = state.fn_down;
    invalidate_pending_fn_tap(state);
    invalidate_pending_fn_hold_cheat_sheet(state);
    reset_fn_tap_sequence(state);
    reset_function_sources(state);
    let _ = state
        .tx
        .send(KeyboardTrigger::ShortcutCaptured { action, shortcut });
}

pub(crate) fn cancel_capture(state: &mut ListenerState, reason: String) {
    let Some(action) = state.capture_action.take() else {
        return;
    };
    state.capture_fn_down = false;
    invalidate_pending_capture_fn_tap(state);
    state.fn_space_down = false;
    state.fn_space_combo = false;
    state.fn_chord_used = state.fn_down;
    invalidate_pending_fn_tap(state);
    invalidate_pending_fn_hold_cheat_sheet(state);
    reset_fn_tap_sequence(state);
    reset_function_sources(state);
    let _ = state
        .tx
        .send(KeyboardTrigger::ShortcutCaptureCancelled { action, reason });
}

pub(crate) fn send_voice_shortcut_if_matched(state: &ListenerState, shortcut: &ShortcutBinding) {
    if let Some(trigger) = start_trigger_for_shortcut(state, shortcut) {
        let _ = state.tx.send(trigger);
    }
}

pub(crate) fn invalidate_pending_capture_fn_tap(state: &mut ListenerState) {
    cancel_pending_capture_fn_completion(state);
    state.capture_fn_tap_at = None;
}

pub(crate) fn cancel_pending_capture_fn_completion(state: &mut ListenerState) {
    state.capture_fn_release_generation = state.capture_fn_release_generation.wrapping_add(1);
}
