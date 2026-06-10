//! Fn キーのソース重複排除・連打/長押し判定・Control タップ。

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) const FN_SOURCE_DEDUP_MS: u64 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FunctionSource {
    Modifier,
    Keycode,
}

pub(crate) fn handle_function_modifier_change(state: &mut ListenerState, fn_down: bool) {
    handle_function_source_change(state, FunctionSource::Modifier, fn_down, Instant::now());
}

pub(crate) fn handle_function_keycode_change(state: &mut ListenerState, fn_down: bool) {
    handle_function_source_change(state, FunctionSource::Keycode, fn_down, Instant::now());
}

pub(crate) fn handle_function_source_change(
    state: &mut ListenerState,
    source: FunctionSource,
    source_down: bool,
    now: Instant,
) {
    let was_active = state.fn_down;

    if source_down {
        if is_function_source_down(state, source) {
            return;
        }
        if !was_active && recently_released_other_function_source(state, source, now) {
            set_function_source_suppressed(state, source, true);
            return;
        }
        set_function_source_down(state, source, true);
    } else {
        if take_function_source_suppressed(state, source) {
            return;
        }
        if !is_function_source_down(state, source) {
            return;
        }
        set_function_source_down(state, source, false);
        set_function_source_release_at(state, source, now);
    }

    let is_active = state.fn_modifier_down || state.fn_keycode_down;
    if was_active == is_active {
        return;
    }

    state.fn_down = is_active;
    if state.capture_action.is_some() {
        handle_capture_fn_change(state, is_active);
    } else if is_active {
        handle_fn_pressed(state);
    } else {
        handle_fn_released(state);
    }
}

pub(crate) fn is_function_source_down(state: &ListenerState, source: FunctionSource) -> bool {
    match source {
        FunctionSource::Modifier => state.fn_modifier_down,
        FunctionSource::Keycode => state.fn_keycode_down,
    }
}

pub(crate) fn set_function_source_down(
    state: &mut ListenerState,
    source: FunctionSource,
    source_down: bool,
) {
    match source {
        FunctionSource::Modifier => state.fn_modifier_down = source_down,
        FunctionSource::Keycode => state.fn_keycode_down = source_down,
    }
}

pub(crate) fn set_function_source_suppressed(
    state: &mut ListenerState,
    source: FunctionSource,
    suppressed: bool,
) {
    match source {
        FunctionSource::Modifier => state.suppress_fn_modifier_until_up = suppressed,
        FunctionSource::Keycode => state.suppress_fn_keycode_until_up = suppressed,
    }
}

pub(crate) fn take_function_source_suppressed(
    state: &mut ListenerState,
    source: FunctionSource,
) -> bool {
    let suppressed = match source {
        FunctionSource::Modifier => &mut state.suppress_fn_modifier_until_up,
        FunctionSource::Keycode => &mut state.suppress_fn_keycode_until_up,
    };
    let was_suppressed = *suppressed;
    *suppressed = false;
    was_suppressed
}

pub(crate) fn set_function_source_release_at(
    state: &mut ListenerState,
    source: FunctionSource,
    released_at: Instant,
) {
    match source {
        FunctionSource::Modifier => state.last_fn_modifier_release_at = Some(released_at),
        FunctionSource::Keycode => state.last_fn_keycode_release_at = Some(released_at),
    }
}

pub(crate) fn recently_released_other_function_source(
    state: &ListenerState,
    source: FunctionSource,
    now: Instant,
) -> bool {
    let other_release_at = match source {
        FunctionSource::Modifier => state.last_fn_keycode_release_at,
        FunctionSource::Keycode => state.last_fn_modifier_release_at,
    };
    other_release_at
        .and_then(|released_at| now.checked_duration_since(released_at))
        .is_some_and(|elapsed| elapsed <= Duration::from_millis(FN_SOURCE_DEDUP_MS))
}

pub(crate) fn reset_function_sources(state: &mut ListenerState) {
    state.fn_down = false;
    state.fn_modifier_down = false;
    state.fn_keycode_down = false;
    state.suppress_fn_modifier_until_up = false;
    state.suppress_fn_keycode_until_up = false;
    state.last_fn_modifier_release_at = None;
    state.last_fn_keycode_release_at = None;
}

pub(crate) fn handle_fn_pressed(state: &mut ListenerState) {
    if state.fn_recent_release {
        if let Some(trigger) = confirm_pending_fn_tap(state, false) {
            let _ = state.tx.send(trigger);
        }
    }
    // Start a fresh chord-detection cycle. A new Fn press also cancels any
    // FunctionTap still sitting in the previous release's grace window.
    state.fn_space_combo = false;
    state.fn_chord_used = false;
    state.fn_space_down = false;
    invalidate_pending_fn_tap(state);
    state.fn_hold_cheat_sheet_used = false;
    schedule_fn_hold_cheat_sheet(state);
}

pub(crate) fn handle_fn_released(state: &mut ListenerState) {
    invalidate_pending_fn_hold_cheat_sheet(state);
    if state.fn_hold_cheat_sheet_visible || state.fn_hold_cheat_sheet_used {
        hide_fn_hold_cheat_sheet(state);
        state.fn_hold_cheat_sheet_used = false;
        state.fn_space_combo = false;
        state.fn_chord_used = false;
        return;
    }

    if state.fn_space_combo || state.fn_chord_used {
        // The Fn hold already included another key, so a bare Fn tap should
        // not fire on release.
        state.fn_space_combo = false;
        state.fn_chord_used = false;
        return;
    }

    // Defer the FunctionTap by the chord grace window. If Space arrives
    // during the window the gesture is reclassified as Fn+Space; otherwise
    // the bare Fn tap fires after the window expires.
    state.fn_release_generation = state.fn_release_generation.wrapping_add(1);
    let token = state.fn_release_generation;
    state.fn_recent_release = true;
    state.fn_recent_release_at = Some(Instant::now());

    let tx = state.tx.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(FN_SPACE_GRACE_MS));
        let mut trigger = None;
        if let Ok(mut guard) = LISTENER_STATE.lock() {
            if let Some(state) = guard.as_mut() {
                if state.fn_release_generation == token && state.fn_recent_release {
                    trigger = confirm_pending_fn_tap(state, true);
                }
            }
        }
        if let Some(trigger) = trigger {
            let _ = tx.send(trigger);
        }
    });
}

pub(crate) fn schedule_fn_hold_cheat_sheet(state: &mut ListenerState) {
    if state.voice_overlay_visible {
        return;
    }
    state.fn_hold_generation = state.fn_hold_generation.wrapping_add(1);
    let token = state.fn_hold_generation;
    let tx = state.tx.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(FN_HOLD_CHEAT_SHEET_MS));
        let mut should_show = false;
        if let Ok(mut guard) = LISTENER_STATE.lock() {
            if let Some(state) = guard.as_mut() {
                should_show = state.fn_hold_generation == token
                    && state.fn_down
                    && !state.fn_chord_used
                    && !state.fn_space_combo
                    && !state.voice_overlay_visible
                    && state.capture_action.is_none();
                if should_show {
                    state.fn_hold_cheat_sheet_visible = true;
                    state.fn_hold_cheat_sheet_used = true;
                    state.voice_overlay_visible = true;
                }
            }
        }
        if should_show {
            let _ = tx.send(KeyboardTrigger::ShortcutCheatSheetShow);
        }
    });
}

pub(crate) fn invalidate_pending_fn_hold_cheat_sheet(state: &mut ListenerState) {
    state.fn_hold_generation = state.fn_hold_generation.wrapping_add(1);
}

pub(crate) fn hide_fn_hold_cheat_sheet(state: &mut ListenerState) {
    invalidate_pending_fn_hold_cheat_sheet(state);
    if state.fn_hold_cheat_sheet_visible {
        state.fn_hold_cheat_sheet_visible = false;
        state.voice_overlay_visible = false;
        let _ = state.tx.send(KeyboardTrigger::ShortcutCheatSheetHide);
    }
}

pub(crate) fn invalidate_pending_fn_tap(state: &mut ListenerState) {
    state.fn_release_generation = state.fn_release_generation.wrapping_add(1);
    state.fn_recent_release = false;
    state.fn_recent_release_at = None;
}

pub(crate) fn confirm_pending_fn_tap(
    state: &mut ListenerState,
    emit_single_tap: bool,
) -> Option<KeyboardTrigger> {
    let tapped_at = state.fn_recent_release_at.unwrap_or_else(Instant::now);
    state.fn_recent_release = false;
    state.fn_recent_release_at = None;
    trigger_for_confirmed_fn_tap(state, tapped_at, emit_single_tap)
}

pub(crate) fn trigger_for_confirmed_fn_tap(
    state: &mut ListenerState,
    tapped_at: Instant,
    emit_single_tap: bool,
) -> Option<KeyboardTrigger> {
    let is_double_tap = state
        .last_fn_tap
        .and_then(|previous| tapped_at.checked_duration_since(previous))
        .is_some_and(|elapsed| elapsed <= state.threshold);

    if is_double_tap {
        state.last_fn_tap = None;
        if let Some(trigger) = start_trigger_for_shortcut(state, &ShortcutBinding::fn_double_tap())
        {
            return Some(trigger);
        }
    }

    state.last_fn_tap = Some(tapped_at);
    if emit_single_tap {
        Some(KeyboardTrigger::FunctionTap)
    } else {
        None
    }
}

pub(crate) fn reset_fn_tap_sequence(state: &mut ListenerState) {
    state.last_fn_tap = None;
}

pub(crate) fn handle_control_pressed(state: &mut ListenerState, flags: u64) {
    state.control_chord_used = control_has_other_modifiers(flags);
}

pub(crate) fn handle_control_released(state: &mut ListenerState, flags: u64) {
    let should_cycle = !state.control_chord_used && !control_has_other_modifiers(flags);
    state.control_chord_used = false;
    if should_cycle {
        let _ = state.tx.send(KeyboardTrigger::VoiceModeCycle);
    }
}

pub(crate) fn control_has_other_modifiers(flags: u64) -> bool {
    let other_modifiers = KCG_EVENT_FLAG_MASK_SHIFT
        | KCG_EVENT_FLAG_MASK_ALTERNATE
        | KCG_EVENT_FLAG_MASK_COMMAND
        | KCG_EVENT_FLAG_MASK_SECONDARY_FN;
    (flags & other_modifiers) != 0
}
