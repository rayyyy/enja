//! フロントエンドへ送る音声系 Tauri イベントの定義と発行。

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) const DICTIONARY_NOTICE_VISIBLE_MS: u64 = 6_500;

pub(crate) const DICTIONARY_UNDO_NOTICE_MS: u64 = 900;

pub(crate) static VOICE_STATE_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VoiceStateEvent {
    pub(crate) state: &'static str,
    pub(crate) mode: Option<VoiceMode>,
    pub(crate) mode_profile_id: Option<String>,
    pub(crate) mode_profile_name: Option<String>,
    pub(crate) message: Option<String>,
    pub(crate) seq: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VoiceLevelEvent {
    pub(crate) rms: f32,
    pub(crate) peak: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VoiceResultEvent {
    pub(crate) text: String,
    pub(crate) inserted: bool,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VoiceDictionaryLearningEvent {
    pub(crate) entry_id: String,
    pub(crate) from: String,
    pub(crate) to: String,
}

pub(crate) fn emit_state(
    app: &tauri::AppHandle,
    state: &'static str,
    mode: Option<VoiceMode>,
    mode_profile: Option<VoiceModeProfileSnapshot>,
    message: Option<String>,
) {
    let seq = next_voice_state_seq();
    let (mode_profile_id, mode_profile_name) = state_profile_fields(mode_profile);
    let event = VoiceStateEvent {
        state,
        mode,
        mode_profile_id,
        mode_profile_name,
        message,
        seq,
    };
    let _ = app.emit("voice-state", event.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for delay_ms in [120_u64, 360] {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            let _ = app.emit("voice-state", event.clone());
        }
    });
}

pub(crate) fn state_profile_fields(
    mode_profile: Option<VoiceModeProfileSnapshot>,
) -> (Option<String>, Option<String>) {
    match mode_profile {
        Some(profile) => (Some(profile.id), Some(profile.name)),
        None => (None, None),
    }
}

pub(crate) fn emit_result(app: &tauri::AppHandle, event: VoiceResultEvent) {
    let _ = app.emit("voice-result", event.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for delay_ms in [120_u64, 360] {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            let _ = app.emit("voice-result", event.clone());
        }
    });
}

pub(crate) fn show_dictionary_learning_notice(
    app: &tauri::AppHandle,
    learned: dictionary::LearnedCorrection,
) {
    show_voice_notice_window(app);
    let event = VoiceDictionaryLearningEvent {
        entry_id: learned.entry_id,
        from: learned.from,
        to: learned.to,
    };
    let _ = app.emit("voice-dictionary-learning", event.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(DICTIONARY_NOTICE_VISIBLE_MS)).await;
        if !app
            .try_state::<VoiceManager>()
            .is_some_and(|manager| manager.is_active())
        {
            hide_voice_window(&app);
        }
    });
}

pub fn hide_voice_notice_after_undo(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(DICTIONARY_UNDO_NOTICE_MS)).await;
        if !app
            .try_state::<VoiceManager>()
            .is_some_and(|manager| manager.is_active())
        {
            hide_voice_window(&app);
        }
    });
}

pub(crate) fn next_voice_state_seq() -> u64 {
    VOICE_STATE_SEQ.fetch_add(1, Ordering::SeqCst)
}
