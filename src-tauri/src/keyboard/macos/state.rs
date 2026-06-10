//! イベントタップスレッドだけが触るリスナー状態。

#[allow(clippy::wildcard_imports)]
use super::*;

/// Grace window after an Fn release during which an incoming Space press
/// is still treated as part of an Fn+Space chord (Ask mode). This makes
/// real-world chord typing robust against the user releasing Fn a few
/// milliseconds before pressing Space.
pub(crate) const FN_SPACE_GRACE_MS: u64 = 80;
pub(crate) const FN_HOLD_CHEAT_SHEET_MS: u64 = 500;

// --- Listener state (single-threaded: only accessed from the tap thread) -

use std::sync::Mutex;

pub(crate) struct ListenerState {
    pub(crate) tx: Sender<KeyboardTrigger>,
    pub(crate) threshold: Duration,
    pub(crate) runtime: KeyboardRuntimeSettings,
    pub(crate) meta_down: bool,
    pub(crate) control_down: bool,
    pub(crate) control_chord_used: bool,
    pub(crate) fn_down: bool,
    pub(crate) fn_modifier_down: bool,
    pub(crate) fn_keycode_down: bool,
    pub(crate) suppress_fn_modifier_until_up: bool,
    pub(crate) suppress_fn_keycode_until_up: bool,
    pub(crate) last_fn_modifier_release_at: Option<Instant>,
    pub(crate) last_fn_keycode_release_at: Option<Instant>,
    /// True while Fn is held and a Space press has already been registered
    /// for this hold. Used so the matching Fn release suppresses the
    /// FunctionTap event.
    pub(crate) fn_space_combo: bool,
    /// True when Fn has been used with any non-Fn key during this hold.
    /// This keeps a configured bare Fn tap from firing after a chord.
    pub(crate) fn_chord_used: bool,
    /// True while Space is being held as part of a Fn+Space chord. Used to
    /// avoid sending FunctionSpace repeatedly during auto-repeat and to
    /// swallow the matching Space up event.
    pub(crate) fn_space_down: bool,
    /// True after an Fn release while we're still inside the chord grace
    /// window. A Space arriving during this window upgrades the gesture
    /// from "Fn tap" to "Fn+Space".
    pub(crate) fn_recent_release: bool,
    pub(crate) voice_overlay_visible: bool,
    pub(crate) fn_hold_generation: u64,
    pub(crate) fn_hold_cheat_sheet_visible: bool,
    pub(crate) fn_hold_cheat_sheet_used: bool,
    /// Monotonically bumped whenever a pending FunctionTap must be
    /// invalidated (Fn re-press, Space chord in grace window, Escape,
    /// etc.). A scheduled tap only fires if its captured token still
    /// matches.
    pub(crate) fn_release_generation: u64,
    pub(crate) fn_recent_release_at: Option<Instant>,
    pub(crate) last_fn_tap: Option<Instant>,
    pub(crate) capture_action: Option<ShortcutAction>,
    pub(crate) capture_fn_down: bool,
    pub(crate) capture_fn_tap_at: Option<Instant>,
    pub(crate) capture_fn_release_generation: u64,
    pub(crate) last_cmd_c: Option<Instant>,
    pub(crate) tap: CFMachPortRef,
}

// Safety: CFMachPortRef is only used from the tap thread's CFRunLoop.
unsafe impl Send for ListenerState {}

pub(crate) static LISTENER_STATE: Mutex<Option<Box<ListenerState>>> = Mutex::new(None);
