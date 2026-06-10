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
    /// A bare Fn key tap (press + release) that did *not* form a chord with
    /// Space. This is the fixed voice-session stop gesture, and also the
    /// default dictation start gesture when configured that way.
    FunctionTap,
    /// The configured dictation start shortcut. This starts dictation only; it
    /// does not stop an active voice session.
    VoiceDictationStart,
    /// Space was pressed while Fn was held — Ask mode.
    FunctionSpace,
    /// The configured shortcut for polishing the currently selected text
    /// without starting a recording session.
    PolishSelection,
    ShortcutCheatSheetShow,
    ShortcutCheatSheetHide,
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
    pub polish_selection_shortcut: ShortcutBinding,
}

impl From<&AppSettings> for KeyboardRuntimeSettings {
    fn from(settings: &AppSettings) -> Self {
        Self {
            double_tap_threshold_ms: settings.app.double_tap_threshold_ms,
            voice_dictation_shortcut: settings.shortcuts.voice_dictation.clone(),
            voice_ask_shortcut: settings.shortcuts.voice_ask.clone(),
            polish_selection_shortcut: settings.shortcuts.polish_selection.clone(),
        }
    }
}
#[cfg(target_os = "macos")]
mod macos {
    use super::{KeyboardRuntimeSettings, KeyboardTrigger};
    use crate::settings::{ShortcutAction, ShortcutBinding, ShortcutModifiers};
    use std::os::raw::c_void;
    use std::sync::mpsc::Sender;
    use std::time::{Duration, Instant};

    mod capture;
    mod ffi;
    mod fn_keys;
    mod keys;
    mod state;
    mod tap;
    #[cfg(test)]
    mod tests;

    #[allow(clippy::wildcard_imports)]
    use capture::*;
    #[allow(clippy::wildcard_imports)]
    use ffi::*;
    #[allow(clippy::wildcard_imports)]
    use fn_keys::*;
    #[allow(clippy::wildcard_imports)]
    use keys::*;
    #[allow(clippy::wildcard_imports)]
    use state::*;
    pub use tap::{
        begin_shortcut_capture, cancel_shortcut_capture, set_voice_overlay_visible, spawn_listener,
        update_runtime_settings,
    };
}

#[cfg(target_os = "macos")]
pub use macos::{
    begin_shortcut_capture, cancel_shortcut_capture, set_voice_overlay_visible, spawn_listener,
    update_runtime_settings,
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

#[cfg(not(target_os = "macos"))]
pub fn set_voice_overlay_visible(_visible: bool) {}
