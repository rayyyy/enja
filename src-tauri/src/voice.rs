mod aec;
mod audio;
mod cache;
mod system_tap;

use crate::dictionary::{self, DictionaryEntry};
use crate::gemini;
use crate::prompts;
use crate::secrets;
use crate::settings::{
    save_settings_to_disk, AppSettings, SettingsStore, SpeechProfile, SystemAudioHandling,
    VoiceModeProfile,
};
use crate::usage::{self, UsageService};

use aec::Aec;
use audio::{prepare_recorded_audio_for_api, samples_to_wav};
use base64::Engine;
#[cfg(target_os = "macos")]
use core_foundation::base::TCFType;
#[cfg(target_os = "macos")]
use core_foundation::boolean::CFBoolean;
#[cfg(target_os = "macos")]
use core_foundation::string::{CFString, CFStringRef};
#[cfg(target_os = "macos")]
use core_foundation_sys::array::{
    CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex, CFArrayRef,
};
#[cfg(target_os = "macos")]
use core_foundation_sys::base::{CFGetTypeID, CFTypeRef};
#[cfg(target_os = "macos")]
use core_foundation_sys::string::CFStringGetTypeID;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Sample;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
#[cfg(target_os = "macos")]
use std::os::raw::c_int;
#[cfg(target_os = "macos")]
use std::os::raw::c_void;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use system_tap::SystemTap;
use tauri::{Emitter, Manager};
use tokio::sync::oneshot;

const SPEECH_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const APPLE_SPEECH_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const APPLE_SPEECH_INSTALL_TIMEOUT: Duration = Duration::from_secs(900);
const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const RECORDING_STOP_NOTIFY_TIMEOUT: Duration = Duration::from_millis(250);
const AUDIO_INPUT_DEVICES_CHANGED_EVENT: &str = "audio-input-devices-changed";
const VOICE_WINDOW_EDGE_MARGIN: f64 = 16.0;
const VOICE_WINDOW_BOTTOM_MARGIN: f64 = 42.0;
const VOICE_WINDOW_FOLLOW_INTERVAL_MS: u64 = 180;
const DICTIONARY_LEARNING_POLL_INTERVAL_MS: u64 = 250;
const DICTIONARY_LEARNING_QUIET_MS: u64 = 2_000;
const DICTIONARY_LEARNING_MAX_WATCH_MS: u64 = 15_000;
const DICTIONARY_NOTICE_VISIBLE_MS: u64 = 6_500;
const DICTIONARY_UNDO_NOTICE_MS: u64 = 900;
const GOOGLE_SPEECH_DICTIONARY_BOOST: f32 = 8.0;
const MIN_LEARNED_CORRECTION_CHARS: usize = 2;
const MAX_LEARNED_CORRECTION_CHARS: usize = 40;
const MIN_FULL_INSERT_REWRITE_CHARS: usize = 12;
const POLISH_SELECTION_INSTRUCTION: &str = "推敲して";
const SCREEN_CONTEXT_SIDE_CHARS: usize = 1_400;
const SCREEN_CONTEXT_VISIBLE_MAX_CHARS: usize = 6_000;
const SCREEN_CONTEXT_OCR_MAX_CHARS: usize = 6_000;
const SCREEN_CONTEXT_ASR_MAX_TERMS: usize = 180;
const SCREEN_CONTEXT_ASR_MAX_TERM_CHARS: usize = 48;
const APPLE_SPEECH_CONTEXTUAL_STRINGS_MAX: usize = 180;
const SCREEN_CONTEXT_OCR_TIMEOUT: Duration = Duration::from_secs(6);
const SCREEN_CONTEXT_OCR_WAIT_TIMEOUT: Duration = Duration::from_millis(450);
#[cfg(target_os = "macos")]
const PASTE_RESTORE_DELAY_MS: u64 = 420;
#[cfg(target_os = "macos")]
const PASTE_WRITE_SETTLE_MS: u64 = 40;
#[cfg(target_os = "macos")]
const PASTE_ACTIVATE_SETTLE_MS: u64 = 80;
#[cfg(target_os = "macos")]
const MANUAL_ACCESSIBILITY_POLL_ATTEMPTS: usize = 10;
#[cfg(target_os = "macos")]
const MANUAL_ACCESSIBILITY_POLL_INTERVAL: Duration = Duration::from_millis(30);
#[cfg(target_os = "macos")]
const MANUAL_ACCESSIBILITY_FAILURE_TTL: Duration = Duration::from_secs(2);
static VOICE_STATE_SEQ: AtomicU64 = AtomicU64::new(1);
static VOICE_WINDOW_FOLLOW_SEQ: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "macos")]
static MANUAL_ACCESSIBILITY_CACHE: OnceLock<Mutex<ManualAccessibilityCache>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VoiceWindowMonitorKey {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale_bits: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoiceMode {
    Dictation,
    Ask,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechSetupCheck {
    pub ok: bool,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleSpeechStatus {
    pub helper_available: bool,
    pub supported: bool,
    pub status: String,
    pub authorization: String,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppleSpeechHelperResponse {
    ok: bool,
    status: Option<String>,
    supported: Option<bool>,
    authorization: Option<String>,
    reason: Option<String>,
    error: Option<String>,
    details: Option<Vec<String>>,
    transcript: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScreenContextHelperResponse {
    ok: bool,
    error: Option<String>,
    app_name: Option<String>,
    window_title: Option<String>,
    text: Option<String>,
    details: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceStateEvent {
    state: &'static str,
    mode: Option<VoiceMode>,
    mode_profile_id: Option<String>,
    mode_profile_name: Option<String>,
    message: Option<String>,
    seq: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceLevelEvent {
    rms: f32,
    peak: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceResultEvent {
    text: String,
    inserted: bool,
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceDictionaryLearningEvent {
    entry_id: String,
    from: String,
    to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VoiceWindowLayout {
    Compact,
    Expanded,
    Notice,
    CheatSheet,
}

impl VoiceWindowLayout {
    fn dimensions(self) -> (f64, f64) {
        match self {
            Self::Compact => (292.0, 42.0),
            Self::Expanded => (840.0, 420.0),
            Self::Notice => (460.0, 64.0),
            Self::CheatSheet => (480.0, 162.0),
        }
    }

    fn min_height(self) -> f64 {
        match self {
            Self::Expanded => 260.0,
            Self::Notice => 58.0,
            Self::CheatSheet => 148.0,
            Self::Compact => 40.0,
        }
    }

    fn focusable(self) -> bool {
        matches!(self, Self::Expanded | Self::Notice)
    }
}

#[derive(Debug, Clone)]
struct PasteTargetInfo {
    pid: Option<i32>,
    role: String,
    subrole: String,
    attributes: HashSet<String>,
}

impl PasteTargetInfo {
    fn from_osascript_output(output: &str) -> Option<Self> {
        if output.trim().is_empty() {
            return None;
        }

        let lines = output.lines().collect::<Vec<_>>();
        let first = lines.first().copied().unwrap_or_default().trim();
        let (pid, role_index) = match first.parse::<i32>() {
            Ok(pid) if pid > 0 => (Some(pid), 1),
            _ => (None, 0),
        };
        let role = lines
            .get(role_index)
            .copied()
            .unwrap_or_default()
            .trim()
            .to_string();
        let subrole = lines
            .get(role_index + 1)
            .copied()
            .unwrap_or_default()
            .trim()
            .to_string();
        let attributes = lines
            .get(role_index + 2)
            .copied()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect::<HashSet<_>>();

        if pid.is_none() && role.is_empty() && subrole.is_empty() && attributes.is_empty() {
            return None;
        }

        Some(Self {
            pid,
            role,
            subrole,
            attributes,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct VoiceScreenContext {
    app_name: Option<String>,
    window_title: Option<String>,
    element_role: Option<String>,
    focused_before: String,
    focused_selection: String,
    focused_after: String,
    visible_text: String,
    ocr_text: String,
    details: Vec<String>,
}

impl VoiceScreenContext {
    fn is_empty(&self) -> bool {
        option_text_is_empty(self.app_name.as_deref())
            && option_text_is_empty(self.window_title.as_deref())
            && self.focused_before.trim().is_empty()
            && self.focused_selection.trim().is_empty()
            && self.focused_after.trim().is_empty()
            && self.visible_text.trim().is_empty()
            && self.ocr_text.trim().is_empty()
    }

    fn merge_ocr(&mut self, ocr: VoiceScreenContextOcr) {
        if option_text_is_empty(self.app_name.as_deref()) {
            self.app_name = ocr.app_name;
        }
        if option_text_is_empty(self.window_title.as_deref()) {
            self.window_title = ocr.window_title;
        }
        self.ocr_text = clamp_chars(&ocr.text, SCREEN_CONTEXT_OCR_MAX_CHARS);
        self.details.extend(ocr.details);
    }
}

#[derive(Debug, Clone, Default)]
struct VoiceScreenContextOcr {
    app_name: Option<String>,
    window_title: Option<String>,
    text: String,
    details: Vec<String>,
}

struct VoiceScreenContextCapture {
    context: VoiceScreenContext,
    ocr_rx: Option<oneshot::Receiver<Option<VoiceScreenContextOcr>>>,
}

fn option_text_is_empty(value: Option<&str>) -> bool {
    match value {
        Some(value) => value.trim().is_empty(),
        None => true,
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
struct ManualAccessibilityCache {
    enabled_pids: HashSet<i32>,
    failed_until_by_pid: HashMap<i32, Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextRange {
    location: usize,
    length: usize,
}

impl TextRange {
    fn end(self) -> usize {
        self.location.saturating_add(self.length)
    }

    fn overlaps(self, other: TextRange) -> bool {
        self.location < other.end() && other.location < self.end()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChangedSpan {
    old_range: TextRange,
    new_range: TextRange,
    from: String,
    to: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DictionaryLearningQuiescence {
    baseline_value: String,
    last_value: String,
    stable_ms: u64,
    elapsed_ms: u64,
    last_ready_value: Option<String>,
}

impl DictionaryLearningQuiescence {
    fn new(baseline_value: &str) -> Self {
        Self {
            baseline_value: baseline_value.to_string(),
            last_value: baseline_value.to_string(),
            stable_ms: 0,
            elapsed_ms: 0,
            last_ready_value: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DictionaryLearningQuiescenceStep {
    Continue,
    Ready,
    Expired,
}

fn advance_dictionary_learning_quiescence(
    state: &mut DictionaryLearningQuiescence,
    current_value: &str,
    poll_ms: u64,
    quiet_ms: u64,
    max_watch_ms: u64,
) -> DictionaryLearningQuiescenceStep {
    state.elapsed_ms = state.elapsed_ms.saturating_add(poll_ms);

    if current_value == state.last_value {
        state.stable_ms = state.stable_ms.saturating_add(poll_ms);
    } else {
        state.last_value = current_value.to_string();
        state.stable_ms = 0;
        state.last_ready_value = None;
    }

    if state.elapsed_ms > max_watch_ms {
        return DictionaryLearningQuiescenceStep::Expired;
    }

    if current_value != state.baseline_value
        && state.stable_ms >= quiet_ms
        && state.last_ready_value.as_deref() != Some(current_value)
    {
        state.last_ready_value = Some(current_value.to_string());
        return DictionaryLearningQuiescenceStep::Ready;
    }

    DictionaryLearningQuiescenceStep::Continue
}

pub struct VoiceManager {
    active: Mutex<Option<SessionState>>,
}

enum SessionState {
    Starting {
        mode: VoiceMode,
        mode_profile_id: String,
        cancelled: Arc<Mutex<bool>>,
    },
    Active(ActiveSession),
    Processing {
        cancelled: ProcessingCancel,
    },
}

type ProcessingCancel = Arc<AtomicBool>;

struct ActiveSession {
    mode: VoiceMode,
    mode_profile_id: String,
    selected_text: String,
    paste_target: Option<PasteTargetInfo>,
    screen_context: VoiceScreenContext,
    screen_context_ocr_rx: Option<oneshot::Receiver<Option<VoiceScreenContextOcr>>>,
    recorder: Recorder,
    audio_aux: Option<AudioAux>,
}

#[derive(Debug, Clone)]
struct VoiceModeProfileSnapshot {
    id: String,
    name: String,
    formatting_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveTranscriptionProvider {
    AppleSpeechAnalyzer,
    GoogleChirp3,
}

enum AudioAux {
    Mute(SystemAudioMuteGuard),
    Isolate(#[allow(dead_code)] Arc<SystemTap>),
}

impl AudioAux {
    fn stop(self) {
        match self {
            AudioAux::Mute(guard) => guard.stop(),
            AudioAux::Isolate(_) => {}
        }
    }
}

#[derive(Clone)]
enum PipelineMode {
    Direct,
    AecIsolate(Arc<SystemTap>),
}

struct Recorder {
    control_tx: std::sync::mpsc::Sender<RecorderCommand>,
    stopped_rx: std::sync::mpsc::Receiver<()>,
    done_rx: std::sync::mpsc::Receiver<Result<AudioClip, String>>,
}

struct LiveTranscriber {
    provider: LiveTranscriptionProvider,
    sample_tx: Option<std::sync::mpsc::Sender<Vec<i16>>>,
    join: Option<std::thread::JoinHandle<Result<String, String>>>,
}

struct LiveTranscript {
    provider: LiveTranscriptionProvider,
    result: Result<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecorderCommand {
    Finish,
    Cancel,
}

struct AudioClip {
    wav: Vec<u8>,
    duration_secs: f32,
    live_transcript: Option<LiveTranscript>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy)]
struct OutputAudioSnapshot {
    volume: Option<u8>,
    muted: Option<bool>,
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
struct OutputAudioSnapshot;

struct SystemAudioMuteGuard {
    snapshot: OutputAudioSnapshot,
    stop_tx: std::sync::mpsc::Sender<()>,
    join: Option<std::thread::JoinHandle<()>>,
}

type RecorderSetup = (
    Arc<Mutex<Vec<i16>>>,
    u32,
    u16,
    cpal::Stream,
    Option<LiveTranscriber>,
);
type RecorderInitSignal = Arc<Mutex<Option<std::sync::mpsc::Sender<Result<(), String>>>>>;

impl VoiceManager {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }

    pub fn start_session(&self, app: tauri::AppHandle, mode: VoiceMode) -> Result<(), String> {
        let settings = app
            .try_state::<SettingsStore>()
            .map(|store| store.get())
            .unwrap_or_default();
        let mode_profile = if mode == VoiceMode::Dictation {
            settings
                .voice
                .active_mode_profile()
                .map(snapshot_from_profile)
        } else {
            None
        };
        let mode_profile_id = mode_profile
            .as_ref()
            .map(|profile| profile.id.clone())
            .unwrap_or_default();

        let mut guard = self.active.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Ok(());
        }

        show_voice_window(&app, false);
        emit_state(
            &app,
            "preparing",
            Some(mode),
            mode_profile.clone(),
            Some("録音を準備しています…".to_string()),
        );

        let cancelled = Arc::new(Mutex::new(false));
        *guard = Some(SessionState::Starting {
            mode,
            mode_profile_id,
            cancelled: cancelled.clone(),
        });
        drop(guard);

        let app_for_task = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(err) = finish_start_session(app_for_task, mode, cancelled).await {
                eprintln!("[enja] start_session failed: {err}");
            }
        });

        Ok(())
    }

    pub async fn stop_session(&self, app: tauri::AppHandle) -> Result<(), String> {
        let (session, cancelled) = {
            let mut guard = self.active.lock().map_err(|e| e.to_string())?;
            let Some(session) = guard.take() else {
                return Ok(());
            };

            match session {
                SessionState::Starting { cancelled, .. } => {
                    if let Ok(mut flag) = cancelled.lock() {
                        *flag = true;
                    }
                    emit_state(&app, "idle", None, None, None);
                    hide_voice_window(&app);
                    return Ok(());
                }
                SessionState::Active(session) => {
                    let cancelled = Arc::new(AtomicBool::new(false));
                    *guard = Some(SessionState::Processing {
                        cancelled: cancelled.clone(),
                    });
                    (session, cancelled)
                }
                SessionState::Processing { cancelled } => {
                    *guard = Some(SessionState::Processing { cancelled });
                    return Ok(());
                }
            }
        };

        self.stop_active_session(app, session, cancelled).await
    }

    async fn stop_active_session(
        &self,
        app: tauri::AppHandle,
        session: ActiveSession,
        cancelled: ProcessingCancel,
    ) -> Result<(), String> {
        let ActiveSession {
            mode,
            mode_profile_id,
            selected_text,
            paste_target,
            screen_context,
            screen_context_ocr_rx,
            recorder,
            audio_aux,
        } = session;
        let mode_profile = profile_snapshot_for_mode(&app, mode, &mode_profile_id);
        let processing_message = if mode == VoiceMode::Dictation
            && mode_profile
                .as_ref()
                .is_some_and(|profile| !profile.formatting_enabled)
        {
            "文字起こしを出力しています…"
        } else {
            "音声を整形しています…"
        };
        show_voice_window(&app, false);
        emit_state(
            &app,
            "stopping",
            Some(mode),
            mode_profile.clone(),
            Some("録音を終了しています…".to_string()),
        );

        let should_play_stop_sound = app
            .try_state::<SettingsStore>()
            .map(|store| store.get().voice.interaction_sounds_enabled)
            .unwrap_or(false);
        let clip = recorder.finish(move || {
            if let Some(aux) = audio_aux {
                aux.stop();
            }
            if should_play_stop_sound {
                play_interaction_sound("stop");
            }
        });

        if is_processing_cancelled(&cancelled) {
            self.clear_processing_session(&cancelled);
            return Ok(());
        }

        let clip = match clip {
            Ok(clip) => clip,
            Err(message) => {
                if is_processing_cancelled(&cancelled) || !self.clear_processing_session(&cancelled)
                {
                    return Ok(());
                }
                show_voice_window(&app, true);
                emit_state(
                    &app,
                    "error",
                    Some(mode),
                    mode_profile.clone(),
                    Some(message.clone()),
                );
                return Err(message);
            }
        };

        emit_state(
            &app,
            "processing",
            Some(mode),
            mode_profile.clone(),
            Some(processing_message.to_string()),
        );
        let screen_context =
            resolve_voice_screen_context(screen_context, screen_context_ocr_rx).await;

        let result = tokio::select! {
            result = process_clip(&app, mode, &mode_profile_id, &selected_text, &screen_context, clip) => result,
            _ = wait_processing_cancelled(cancelled.clone()) => {
                self.clear_processing_session(&cancelled);
                return Ok(());
            }
        };

        if is_processing_cancelled(&cancelled) || !self.clear_processing_session(&cancelled) {
            return Ok(());
        }

        match result {
            Ok(text) => {
                let inserted = if mode == VoiceMode::Dictation {
                    paste_text_with_dictionary_learning(&app, &text, paste_target.as_ref())
                } else {
                    paste_text(&text, paste_target.as_ref())
                };
                if inserted {
                    emit_result(
                        &app,
                        VoiceResultEvent {
                            text,
                            inserted: true,
                            reason: None,
                        },
                    );
                    hide_voice_window_after(app, Duration::from_millis(280));
                } else {
                    show_voice_window(&app, true);
                    emit_state(
                        &app,
                        "fallback",
                        Some(mode),
                        mode_profile.clone(),
                        Some(
                            "入力先が見つからなかったため、コピー用に表示しています。".to_string(),
                        ),
                    );
                    emit_result(
                        &app,
                        VoiceResultEvent {
                            text,
                            inserted: false,
                            reason: Some("入力先が見つかりませんでした。".to_string()),
                        },
                    );
                }
                Ok(())
            }
            Err(message) => {
                show_voice_window(&app, true);
                emit_state(
                    &app,
                    "error",
                    Some(mode),
                    mode_profile,
                    Some(message.clone()),
                );
                Err(message)
            }
        }
    }

    pub fn cancel_session(&self, app: tauri::AppHandle) -> Result<(), String> {
        let session = {
            let mut guard = self.active.lock().map_err(|e| e.to_string())?;
            guard.take()
        };
        match session {
            Some(SessionState::Starting { cancelled, .. }) => {
                if let Ok(mut flag) = cancelled.lock() {
                    *flag = true;
                }
            }
            Some(SessionState::Active(session)) => {
                session.recorder.cancel();
                if let Some(aux) = session.audio_aux {
                    aux.stop();
                }
            }
            Some(SessionState::Processing { cancelled }) => {
                cancel_processing(&cancelled);
            }
            None => {}
        }
        emit_state(&app, "idle", None, None, None);
        hide_voice_window(&app);
        Ok(())
    }

    pub fn cycle_mode_profile(&self, app: tauri::AppHandle) -> Result<(), String> {
        let Some(store) = app.try_state::<SettingsStore>() else {
            return Err("SettingsStore is unavailable.".to_string());
        };
        let mut settings = store.get();
        settings.sanitize();

        let current_id = {
            let guard = self.active.lock().map_err(|e| e.to_string())?;
            match guard.as_ref() {
                Some(SessionState::Starting {
                    mode,
                    mode_profile_id,
                    ..
                }) if *mode == VoiceMode::Dictation => mode_profile_id.clone(),
                Some(SessionState::Active(session)) if session.mode == VoiceMode::Dictation => {
                    session.mode_profile_id.clone()
                }
                _ => return Ok(()),
            }
        };

        let next_id = settings
            .voice
            .next_mode_profile_id(&current_id)
            .ok_or_else(|| "切り替え可能な音声モードがありません。".to_string())?;
        settings.voice.active_mode_profile_id = next_id.clone();
        settings.sanitize();
        settings.voice.validate_mode_profiles()?;
        let next_profile = settings
            .voice
            .mode_profile_by_id(&next_id)
            .map(snapshot_from_profile);

        save_settings_to_disk(&app, &settings)?;
        store.replace(settings);

        let mut guard = self.active.lock().map_err(|e| e.to_string())?;
        let next_state = match guard.as_mut() {
            Some(SessionState::Starting {
                mode,
                mode_profile_id,
                ..
            }) if *mode == VoiceMode::Dictation => {
                *mode_profile_id = next_id;
                "preparing"
            }
            Some(SessionState::Active(session)) if session.mode == VoiceMode::Dictation => {
                session.mode_profile_id = next_id;
                "recording"
            }
            _ => return Ok(()),
        };
        drop(guard);

        emit_state(
            &app,
            next_state,
            Some(VoiceMode::Dictation),
            next_profile,
            None,
        );
        Ok(())
    }

    pub fn polish_selection(&self, app: tauri::AppHandle) -> Result<(), String> {
        let mut guard = self.active.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Ok(());
        }

        let cancelled = Arc::new(AtomicBool::new(false));
        *guard = Some(SessionState::Processing {
            cancelled: cancelled.clone(),
        });
        drop(guard);

        show_voice_window(&app, false);
        emit_state(
            &app,
            "preparing",
            Some(VoiceMode::Ask),
            None,
            Some("選択テキストを取得しています…".to_string()),
        );

        tauri::async_runtime::spawn(async move {
            if let Err(err) = polish_selected_text(app, cancelled).await {
                eprintln!("[enja] polish selection failed: {err}");
            }
        });

        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.active.lock().is_ok_and(|guard| guard.is_some())
    }

    fn clear_processing_session(&self, cancelled: &ProcessingCancel) -> bool {
        let Ok(mut guard) = self.active.lock() else {
            return false;
        };
        let should_clear = matches!(
            guard.as_ref(),
            Some(SessionState::Processing {
                cancelled: current
            }) if Arc::ptr_eq(current, cancelled)
        );
        if should_clear {
            *guard = None;
        }
        should_clear
    }
}

async fn finish_start_session(
    app: tauri::AppHandle,
    mode: VoiceMode,
    cancelled: Arc<Mutex<bool>>,
) -> Result<(), String> {
    let settings = app
        .try_state::<SettingsStore>()
        .map(|store| store.get())
        .unwrap_or_default();

    if is_start_cancelled(&cancelled) {
        return Ok(());
    }

    let paste_target = capture_paste_target();

    let selected_text = if mode == VoiceMode::Ask {
        tokio::task::spawn_blocking(capture_selected_text)
            .await
            .map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let screen_context_ocr_enabled = should_capture_voice_screen_context_ocr(
        &settings,
        mode,
        &settings.voice.active_mode_profile_id,
    );
    let screen_context_capture = start_voice_screen_context_capture(
        &app,
        &settings,
        paste_target.as_ref(),
        screen_context_ocr_enabled,
    );

    if is_start_cancelled(&cancelled) {
        return Ok(());
    }

    let (audio_aux, pipeline_mode) = prepare_audio_pipeline(&settings);

    if is_start_cancelled(&cancelled) {
        if let Some(aux) = audio_aux {
            aux.stop();
        }
        return Ok(());
    }

    let app_for_recorder = app.clone();
    let microphone_id = settings.voice.selected_microphone_id.clone();
    let max_recording_seconds = settings.voice.max_recording_seconds;
    let pipeline_for_recorder = pipeline_mode.clone();
    let live_transcription_provider = live_transcription_provider_for_settings(&settings, mode);
    let screen_context_for_recorder = screen_context_capture.context.clone();
    let recorder = tokio::task::spawn_blocking(move || {
        Recorder::start(
            app_for_recorder,
            microphone_id,
            max_recording_seconds,
            pipeline_for_recorder,
            live_transcription_provider,
            screen_context_for_recorder,
        )
    })
    .await
    .map_err(|e| e.to_string())?;

    if is_start_cancelled(&cancelled) {
        if let Some(aux) = audio_aux {
            aux.stop();
        }
        if let Ok(recorder) = recorder {
            recorder.cancel();
        }
        if clear_starting_session(&app, &cancelled) {
            emit_state(&app, "idle", None, None, None);
            hide_voice_window(&app);
        }
        return Ok(());
    }

    let recorder = match recorder {
        Ok(recorder) => recorder,
        Err(err) => {
            if let Some(aux) = audio_aux {
                aux.stop();
            }
            fail_start_session(&app, mode, &cancelled, err.clone());
            return Err(err);
        }
    };

    let manager = app
        .try_state::<VoiceManager>()
        .ok_or_else(|| "VoiceManager is unavailable.".to_string())?;
    let mut guard = manager.active.lock().map_err(|e| e.to_string())?;
    let Some(SessionState::Starting {
        mode: starting_mode,
        mode_profile_id: starting_mode_profile_id,
        cancelled: starting_cancelled,
    }) = guard.as_ref()
    else {
        recorder.cancel();
        if let Some(aux) = audio_aux {
            aux.stop();
        }
        return Ok(());
    };

    if *starting_mode != mode
        || !Arc::ptr_eq(starting_cancelled, &cancelled)
        || is_start_cancelled(starting_cancelled)
    {
        recorder.cancel();
        if let Some(aux) = audio_aux {
            aux.stop();
        }
        return Ok(());
    }

    let starting_mode_profile_id = starting_mode_profile_id.clone();
    *guard = Some(SessionState::Active(ActiveSession {
        mode,
        mode_profile_id: starting_mode_profile_id.clone(),
        selected_text,
        paste_target,
        screen_context: screen_context_capture.context,
        screen_context_ocr_rx: screen_context_capture.ocr_rx,
        recorder,
        audio_aux,
    }));
    drop(guard);

    if settings.voice.interaction_sounds_enabled {
        play_interaction_sound("start");
    }

    emit_state(
        &app,
        "recording",
        Some(mode),
        profile_snapshot_for_mode(&app, mode, &starting_mode_profile_id),
        None,
    );
    Ok(())
}

fn is_start_cancelled(cancelled: &Arc<Mutex<bool>>) -> bool {
    cancelled.lock().is_ok_and(|flag| *flag)
}

fn cancel_processing(cancelled: &ProcessingCancel) {
    cancelled.store(true, Ordering::SeqCst);
}

fn is_processing_cancelled(cancelled: &ProcessingCancel) -> bool {
    cancelled.load(Ordering::SeqCst)
}

async fn wait_processing_cancelled(cancelled: ProcessingCancel) {
    while !is_processing_cancelled(&cancelled) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn polish_selected_text(
    app: tauri::AppHandle,
    cancelled: ProcessingCancel,
) -> Result<(), String> {
    let paste_target = capture_paste_target();
    let settings = app
        .try_state::<SettingsStore>()
        .map(|store| store.get())
        .unwrap_or_default();
    let screen_context_capture =
        start_voice_screen_context_capture(&app, &settings, paste_target.as_ref(), false);
    let selected_text = tokio::task::spawn_blocking(capture_selected_text)
        .await
        .map_err(|e| e.to_string())?;
    let screen_context = resolve_voice_screen_context(
        screen_context_capture.context,
        screen_context_capture.ocr_rx,
    )
    .await;

    if is_processing_cancelled(&cancelled) {
        clear_processing_session_for_app(&app, &cancelled);
        return Ok(());
    }

    if selected_text.trim().is_empty() {
        if clear_processing_session_for_app(&app, &cancelled) {
            show_voice_window(&app, true);
            emit_state(
                &app,
                "error",
                Some(VoiceMode::Ask),
                None,
                Some("テキストを選択してから実行してください。".to_string()),
            );
        }
        return Ok(());
    }

    emit_state(
        &app,
        "processing",
        Some(VoiceMode::Ask),
        None,
        Some("選択テキストを整えています…".to_string()),
    );

    let result = tokio::select! {
        result = finalize_selected_text_instruction(&app, &selected_text, &screen_context) => result,
        _ = wait_processing_cancelled(cancelled.clone()) => {
            clear_processing_session_for_app(&app, &cancelled);
            return Ok(());
        }
    };

    if is_processing_cancelled(&cancelled) || !clear_processing_session_for_app(&app, &cancelled) {
        return Ok(());
    }

    match result {
        Ok(text) => {
            let inserted = paste_text(&text, paste_target.as_ref());
            if inserted {
                emit_result(
                    &app,
                    VoiceResultEvent {
                        text,
                        inserted: true,
                        reason: None,
                    },
                );
                hide_voice_window_after(app, Duration::from_millis(280));
            } else {
                show_voice_window(&app, true);
                emit_state(
                    &app,
                    "fallback",
                    Some(VoiceMode::Ask),
                    None,
                    Some("入力先が見つからなかったため、コピー用に表示しています。".to_string()),
                );
                emit_result(
                    &app,
                    VoiceResultEvent {
                        text,
                        inserted: false,
                        reason: Some("入力先が見つかりませんでした。".to_string()),
                    },
                );
            }
            Ok(())
        }
        Err(message) => {
            show_voice_window(&app, true);
            emit_state(
                &app,
                "error",
                Some(VoiceMode::Ask),
                None,
                Some(message.clone()),
            );
            Err(message)
        }
    }
}

fn clear_processing_session_for_app(app: &tauri::AppHandle, cancelled: &ProcessingCancel) -> bool {
    app.try_state::<VoiceManager>()
        .is_some_and(|manager| manager.clear_processing_session(cancelled))
}

fn clear_starting_session(app: &tauri::AppHandle, cancelled: &Arc<Mutex<bool>>) -> bool {
    let Some(manager) = app.try_state::<VoiceManager>() else {
        return false;
    };
    let Ok(mut guard) = manager.active.lock() else {
        return false;
    };
    let should_clear = matches!(
        guard.as_ref(),
        Some(SessionState::Starting {
            cancelled: current,
            ..
        }) if Arc::ptr_eq(current, cancelled)
    );
    if should_clear {
        *guard = None;
    }
    should_clear
}

fn fail_start_session(
    app: &tauri::AppHandle,
    mode: VoiceMode,
    cancelled: &Arc<Mutex<bool>>,
    err: String,
) {
    let mode_profile = if mode == VoiceMode::Dictation {
        app.try_state::<SettingsStore>().and_then(|store| {
            let settings = store.get();
            settings
                .voice
                .active_mode_profile()
                .map(snapshot_from_profile)
        })
    } else {
        None
    };
    if !clear_starting_session(app, cancelled) {
        return;
    }
    show_voice_window(app, true);
    emit_state(app, "error", Some(mode), mode_profile, Some(err));
}

fn start_voice_screen_context_capture(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    paste_target: Option<&PasteTargetInfo>,
    capture_ocr: bool,
) -> VoiceScreenContextCapture {
    if !settings.voice.screen_context_enabled {
        return VoiceScreenContextCapture {
            context: VoiceScreenContext::default(),
            ocr_rx: None,
        };
    }

    VoiceScreenContextCapture {
        context: capture_voice_screen_context(paste_target),
        ocr_rx: if capture_ocr {
            start_screen_context_ocr_capture(app)
        } else {
            None
        },
    }
}

fn should_capture_voice_screen_context_ocr(
    settings: &AppSettings,
    mode: VoiceMode,
    mode_profile_id: &str,
) -> bool {
    if !settings.voice.screen_context_enabled || !settings.voice.screen_context_ocr_enabled {
        return false;
    }

    match mode {
        VoiceMode::Ask => true,
        VoiceMode::Dictation => {
            let formatting_enabled = settings
                .voice
                .mode_profile_or_default(mode_profile_id)
                .map(|profile| profile.formatting_enabled)
                .unwrap_or(true);
            if formatting_enabled {
                return true;
            }

            !should_use_live_transcript(settings, mode, mode_profile_id)
                && speech_profile_accepts_screen_context_hints(settings.voice.speech_profile)
        }
    }
}

fn speech_profile_accepts_screen_context_hints(profile: SpeechProfile) -> bool {
    match profile {
        SpeechProfile::GoogleChirp3
        | SpeechProfile::OpenAiGpt4oTranscribe
        | SpeechProfile::OpenAiGpt4oMiniTranscribe
        | SpeechProfile::GeminiAudio
        | SpeechProfile::AppleSpeechAnalyzer => true,
    }
}

async fn resolve_voice_screen_context(
    mut context: VoiceScreenContext,
    ocr_rx: Option<oneshot::Receiver<Option<VoiceScreenContextOcr>>>,
) -> VoiceScreenContext {
    let Some(ocr_rx) = ocr_rx else {
        return context;
    };
    match tokio::time::timeout(SCREEN_CONTEXT_OCR_WAIT_TIMEOUT, ocr_rx).await {
        Ok(Ok(Some(ocr))) => context.merge_ocr(ocr),
        Ok(Ok(None)) | Ok(Err(_)) | Err(_) => {}
    }
    context
}

#[cfg(target_os = "macos")]
fn start_screen_context_ocr_capture(
    app: &tauri::AppHandle,
) -> Option<oneshot::Receiver<Option<VoiceScreenContextOcr>>> {
    let (tx, rx) = oneshot::channel();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = match run_screen_context_ocr_helper(&app) {
            Ok(context) => Some(context),
            Err(err) => {
                eprintln!("[enja] screen OCR context unavailable: {err}");
                None
            }
        };
        let _ = tx.send(result);
    });
    Some(rx)
}

#[cfg(not(target_os = "macos"))]
fn start_screen_context_ocr_capture(
    _app: &tauri::AppHandle,
) -> Option<oneshot::Receiver<Option<VoiceScreenContextOcr>>> {
    None
}

#[cfg(target_os = "macos")]
fn run_screen_context_ocr_helper(app: &tauri::AppHandle) -> Result<VoiceScreenContextOcr, String> {
    let helper = resolve_screen_context_helper(app)?;
    let mut command = std::process::Command::new(&helper);
    command.arg("ocr-screen");
    let output = command_output_with_timeout(
        command,
        SCREEN_CONTEXT_OCR_TIMEOUT,
        &format!("Screen context helper（path: {}）", helper.display()),
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        return Err(if stderr.is_empty() {
            format!("screen context helper failed: {}", output.status)
        } else {
            stderr
        });
    }
    let response: ScreenContextHelperResponse = serde_json::from_str(&stdout)
        .map_err(|err| format!("screen context helper JSON parse failed: {err}: {stdout}"))?;
    if !response.ok {
        return Err(response
            .error
            .unwrap_or_else(|| "screen context helper failed.".to_string()));
    }
    let text = response.text.unwrap_or_default();
    if text.trim().is_empty() {
        return Err("screen OCR text is empty.".to_string());
    }
    Ok(VoiceScreenContextOcr {
        app_name: response.app_name,
        window_title: response.window_title,
        text,
        details: response.details.unwrap_or_default(),
    })
}

#[cfg(target_os = "macos")]
fn resolve_screen_context_helper(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let executable_name = "enja-screen-context-helper";
    let target_name = format!("enja-screen-context-helper-{}", env!("ENJA_TARGET_TRIPLE"));
    let mut candidates = Vec::<PathBuf>::new();
    if let Ok(path) = std::env::var("ENJA_SCREEN_CONTEXT_HELPER_PATH") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(executable_name));
        }
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join(executable_name));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bin")
            .join(target_name),
    );

    for path in &candidates {
        if path.is_file() {
            return Ok(path.clone());
        }
    }
    Err(format!(
        "screen context helperが見つかりません。探した場所: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Default)]
struct FrontAppMetadata {
    pid: Option<i32>,
    app_name: Option<String>,
    window_title: Option<String>,
}

#[cfg(target_os = "macos")]
fn capture_voice_screen_context(paste_target: Option<&PasteTargetInfo>) -> VoiceScreenContext {
    let preferred_pid = paste_target.and_then(|target| target.pid);
    let metadata = current_front_app_metadata(preferred_pid).unwrap_or_default();
    let metadata_pid = metadata.pid;
    let mut context = VoiceScreenContext {
        app_name: metadata.app_name,
        window_title: metadata.window_title,
        element_role: paste_target.and_then(paste_target_role_label),
        ..VoiceScreenContext::default()
    };

    if let Some(focused) = AxFocusedText::capture() {
        let matches_target = preferred_pid
            .map(|pid| focused.snapshot.pid == pid)
            .unwrap_or(true);
        if matches_target {
            let (before, selection, after) =
                focused_text_context(&focused.snapshot.value, focused.snapshot.selected_range);
            context.focused_before = before;
            context.focused_selection = selection;
            context.focused_after = after;
        }
    }

    let context_pid = preferred_pid.or(metadata_pid);
    if let Some(pid) = context_pid {
        context.visible_text = collect_accessibility_window_text(pid);
    }

    context
}

#[cfg(not(target_os = "macos"))]
fn capture_voice_screen_context(_paste_target: Option<&PasteTargetInfo>) -> VoiceScreenContext {
    VoiceScreenContext::default()
}

#[cfg(target_os = "macos")]
fn current_front_app_metadata(preferred_pid: Option<i32>) -> Option<FrontAppMetadata> {
    let process_selector = if let Some(pid) = preferred_pid {
        format!("first application process whose unix id is {pid}")
    } else {
        "first application process whose frontmost is true".to_string()
    };
    let script = format!(
        r#"
tell application "System Events"
  try
    set frontApp to {process_selector}
    set pidValue to unix id of frontApp as text
    set appName to name of frontApp as text
    set windowTitle to ""
    try
      set windowTitle to name of front window of frontApp as text
    end try
    return pidValue & linefeed & appName & linefeed & windowTitle
  on error
    return ""
  end try
end tell
"#
    );
    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    if lines.is_empty() || stdout.trim().is_empty() {
        return None;
    }
    let pid = lines
        .first()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .filter(|pid| *pid > 0);
    Some(FrontAppMetadata {
        pid,
        app_name: lines
            .get(1)
            .map(|value| normalize_context_line(value))
            .filter(|value| !value.is_empty()),
        window_title: lines
            .get(2)
            .map(|value| normalize_context_line(value))
            .filter(|value| !value.is_empty()),
    })
}

fn paste_target_role_label(target: &PasteTargetInfo) -> Option<String> {
    let mut parts = Vec::new();
    if !target.role.trim().is_empty() {
        parts.push(target.role.trim());
    }
    if !target.subrole.trim().is_empty() && target.subrole.trim() != target.role.trim() {
        parts.push(target.subrole.trim());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" / "))
    }
}

#[cfg(target_os = "macos")]
fn focused_text_context(value: &str, selected_range: TextRange) -> (String, String, String) {
    let selection_start = utf16_offset_to_byte_index(value, selected_range.location).unwrap_or(0);
    let selection_end =
        utf16_offset_to_byte_index(value, selected_range.end()).unwrap_or(selection_start);
    let before = clamp_chars_from_end(&value[..selection_start], SCREEN_CONTEXT_SIDE_CHARS);
    let selection = if selection_end > selection_start {
        clamp_chars(
            &value[selection_start..selection_end],
            SCREEN_CONTEXT_SIDE_CHARS,
        )
    } else {
        String::new()
    };
    let after = if selection_end < value.len() {
        clamp_chars(&value[selection_end..], SCREEN_CONTEXT_SIDE_CHARS)
    } else {
        String::new()
    };
    (before, selection, after)
}

#[cfg(target_os = "macos")]
fn collect_accessibility_window_text(pid: i32) -> String {
    unsafe {
        let app = AXUIElementCreateApplication(pid as c_int);
        if app.is_null() {
            return String::new();
        }
        let app_element = AxElementRef { raw: app };
        let mut collector = AxVisibleTextCollector::default();
        if let Some(window) =
            copy_ax_attribute_raw(app_element.raw, "AXFocusedWindow").map(|raw| AxElementRef {
                raw: raw as AXUIElementRef,
            })
        {
            collect_ax_visible_text(window.raw, 0, &mut collector);
        } else {
            collect_ax_visible_text(app_element.raw, 0, &mut collector);
        }
        collector.finish()
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct AxVisibleTextCollector {
    lines: Vec<String>,
    seen: HashSet<String>,
    chars: usize,
    nodes: usize,
}

#[cfg(target_os = "macos")]
impl AxVisibleTextCollector {
    fn can_continue(&self) -> bool {
        self.chars < SCREEN_CONTEXT_VISIBLE_MAX_CHARS && self.nodes < 450
    }

    fn push(&mut self, value: &str) {
        if !self.can_continue() {
            return;
        }
        let line = normalize_context_line(value);
        if line.chars().count() < 2 {
            return;
        }
        let key = line.to_lowercase();
        if !self.seen.insert(key) {
            return;
        }
        let remaining = SCREEN_CONTEXT_VISIBLE_MAX_CHARS.saturating_sub(self.chars);
        if remaining == 0 {
            return;
        }
        let clipped = clamp_chars(&line, remaining.min(500));
        self.chars = self.chars.saturating_add(clipped.chars().count() + 1);
        self.lines.push(clipped);
    }

    fn finish(self) -> String {
        self.lines.join("\n")
    }
}

#[cfg(target_os = "macos")]
fn collect_ax_visible_text(
    element: AXUIElementRef,
    depth: usize,
    collector: &mut AxVisibleTextCollector,
) {
    if element.is_null() || depth > 8 || !collector.can_continue() {
        return;
    }
    collector.nodes = collector.nodes.saturating_add(1);

    let role = copy_ax_string_attribute(element, "AXRole").unwrap_or_default();
    let subrole = copy_ax_string_attribute(element, "AXSubrole").unwrap_or_default();
    if is_sensitive_ax_role(&role) || is_sensitive_ax_role(&subrole) {
        return;
    }

    for attribute in ["AXTitle", "AXValue", "AXDescription", "AXHelp"] {
        if let Some(value) = copy_ax_string_attribute(element, attribute) {
            collector.push(&value);
        }
    }

    if depth >= 8 || !collector.can_continue() {
        return;
    }

    let mut visited_children = false;
    for attribute in ["AXVisibleChildren", "AXChildren", "AXRows", "AXColumns"] {
        if for_each_ax_child(element, attribute, |child| {
            visited_children = true;
            collect_ax_visible_text(child, depth + 1, collector);
        }) && visited_children
        {
            break;
        }
    }
}

#[cfg(target_os = "macos")]
fn for_each_ax_child(
    element: AXUIElementRef,
    attribute: &str,
    mut visit: impl FnMut(AXUIElementRef),
) -> bool {
    let Some(value) = copy_ax_attribute_raw(element, attribute) else {
        return false;
    };
    unsafe {
        if CFGetTypeID(value) != CFArrayGetTypeID() {
            CFRelease(value);
            return false;
        }
        let count = CFArrayGetCount(value as CFArrayRef);
        for index in 0..count {
            let child = CFArrayGetValueAtIndex(value as CFArrayRef, index);
            if !child.is_null() {
                visit(child as AXUIElementRef);
            }
        }
        CFRelease(value);
    }
    true
}

fn is_sensitive_ax_role(role: &str) -> bool {
    matches!(role, "AXSecureTextField")
}

fn transcription_prompt_context(
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
) -> String {
    let mut sections = Vec::new();
    let dictionary_context = dictionary::prompt_lines(entries);
    if !dictionary_context.trim().is_empty() {
        sections.push(dictionary_context);
    }
    let terms = screen_context_terms(screen_context);
    if !terms.is_empty() {
        sections.push(format!(
            "画面文脈から抽出したヒント語（聞こえた場合だけ使用）:\n{}",
            terms
                .into_iter()
                .map(|value| format!("- {value}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    sections.join("\n")
}

fn transcription_contextual_phrases(
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    limit: usize,
) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut values = Vec::new();
    for value in dictionary::enabled_phrases(entries)
        .into_iter()
        .chain(screen_context_terms(screen_context))
    {
        if values.len() >= limit {
            break;
        }
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_lowercase();
        if seen.insert(key) {
            values.push(trimmed.to_string());
        }
    }
    values
}

fn screen_context_terms(screen_context: &VoiceScreenContext) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut out = Vec::<String>::new();
    for value in [
        screen_context.app_name.as_deref().unwrap_or_default(),
        screen_context.window_title.as_deref().unwrap_or_default(),
        &screen_context.focused_before,
        &screen_context.focused_selection,
        &screen_context.focused_after,
        &screen_context.visible_text,
        &screen_context.ocr_text,
    ] {
        extract_screen_context_terms(value, &mut seen, &mut out);
        if out.len() >= SCREEN_CONTEXT_ASR_MAX_TERMS {
            break;
        }
    }
    out
}

fn extract_screen_context_terms(text: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    let mut current = String::new();
    for ch in text.chars() {
        if is_context_term_char(ch) {
            current.push(ch);
        } else {
            push_context_term(&current, seen, out);
            current.clear();
        }
        if out.len() >= SCREEN_CONTEXT_ASR_MAX_TERMS {
            return;
        }
    }
    push_context_term(&current, seen, out);
}

fn push_context_term(value: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    if out.len() >= SCREEN_CONTEXT_ASR_MAX_TERMS {
        return;
    }
    let value = value
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '.' | ',' | ':' | ';' | '/' | '\\' | '-' | '_' | '@' | '#' | '$' | '%' | '&' | '*'
            )
        })
        .trim();
    let char_count = value.chars().count();
    if !(2..=SCREEN_CONTEXT_ASR_MAX_TERM_CHARS).contains(&char_count) {
        return;
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        return;
    }
    let key = value.to_lowercase();
    if seen.insert(key) {
        out.push(value.to_string());
    }
}

fn is_context_term_char(ch: char) -> bool {
    ch.is_alphanumeric()
        || matches!(
            ch,
            '_' | '-' | '.' | '/' | '\\' | '@' | '#' | '$' | '%' | '&' | '+' | ':' | '*'
        )
        || ('\u{3040}'..='\u{30ff}').contains(&ch)
        || ('\u{3400}'..='\u{9fff}').contains(&ch)
}

fn finalization_screen_context_section(screen_context: &VoiceScreenContext) -> String {
    if screen_context.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    if let Some(app_name) = screen_context.app_name.as_deref() {
        if !app_name.trim().is_empty() {
            lines.push(format!("アプリ: {}", normalize_context_line(app_name)));
        }
    }
    if let Some(window_title) = screen_context.window_title.as_deref() {
        if !window_title.trim().is_empty() {
            lines.push(format!(
                "ウィンドウ: {}",
                normalize_context_line(window_title)
            ));
        }
    }
    if let Some(role) = screen_context.element_role.as_deref() {
        if !role.trim().is_empty() {
            lines.push(format!("入力先: {}", normalize_context_line(role)));
        }
    }
    push_context_block(
        &mut lines,
        "カーソル前",
        &screen_context.focused_before,
        SCREEN_CONTEXT_SIDE_CHARS,
    );
    push_context_block(
        &mut lines,
        "選択中",
        &screen_context.focused_selection,
        SCREEN_CONTEXT_SIDE_CHARS,
    );
    push_context_block(
        &mut lines,
        "カーソル後",
        &screen_context.focused_after,
        SCREEN_CONTEXT_SIDE_CHARS,
    );
    push_context_block(
        &mut lines,
        "表示テキスト",
        &screen_context.visible_text,
        SCREEN_CONTEXT_VISIBLE_MAX_CHARS,
    );
    push_context_block(
        &mut lines,
        "OCRテキスト",
        &screen_context.ocr_text,
        SCREEN_CONTEXT_OCR_MAX_CHARS,
    );

    if lines.is_empty() {
        String::new()
    } else {
        format!(
            "画面文脈（貼り付け先と周辺表示。音声と矛盾する内容は使わない）:\n{}",
            lines.join("\n")
        )
    }
}

fn push_context_block(lines: &mut Vec<String>, label: &str, value: &str, max_chars: usize) {
    let value = normalize_context_block(value);
    if value.is_empty() {
        return;
    }
    lines.push(format!("{label}:\n{}", clamp_chars(&value, max_chars)));
}

fn normalize_context_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_context_block(value: &str) -> String {
    value
        .lines()
        .map(normalize_context_line)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn clamp_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

fn clamp_chars_from_end(value: &str, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return value.to_string();
    }
    chars[chars.len() - max_chars..].iter().collect()
}

fn prepare_audio_pipeline(settings: &AppSettings) -> (Option<AudioAux>, PipelineMode) {
    match settings.voice.system_audio_handling {
        SystemAudioHandling::Mute => (
            Some(AudioAux::Mute(SystemAudioMuteGuard::start())),
            PipelineMode::Direct,
        ),
        SystemAudioHandling::Isolate => match SystemTap::start() {
            Ok(capture) => {
                let shared = Arc::new(capture);
                (
                    Some(AudioAux::Isolate(shared.clone())),
                    PipelineMode::AecIsolate(shared),
                )
            }
            Err(err) => {
                eprintln!("[enja] システム音声分離の開始に失敗しました: {err}");
                (
                    Some(AudioAux::Mute(SystemAudioMuteGuard::start())),
                    PipelineMode::Direct,
                )
            }
        },
        SystemAudioHandling::Off => (None, PipelineMode::Direct),
    }
}

pub fn prewarm_microphone() {
    let host = cpal::default_host();
    if let Some(device) = host.default_input_device() {
        let _ = device.default_input_config();
    }
    let _ = list_audio_input_devices();
}

pub fn spawn_audio_input_device_watcher(app: tauri::AppHandle) {
    audio_input_device_watcher::spawn(app);
}

impl Recorder {
    fn start(
        app: tauri::AppHandle,
        selected_device_id: Option<String>,
        max_recording_seconds: u64,
        pipeline: PipelineMode,
        live_transcription_provider: Option<LiveTranscriptionProvider>,
        screen_context: VoiceScreenContext,
    ) -> Result<Self, String> {
        let (control_tx, control_rx) = std::sync::mpsc::channel::<RecorderCommand>();
        let (stopped_tx, stopped_rx) = std::sync::mpsc::channel::<()>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<AudioClip, String>>();
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        std::thread::spawn(move || {
            let result = run_recording_thread(
                app.clone(),
                selected_device_id,
                max_recording_seconds,
                control_rx,
                stopped_tx,
                init_tx,
                pipeline,
                live_transcription_provider,
                screen_context,
            );
            let _ = done_tx.send(result);
        });
        init_rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "マイクの初期化がタイムアウトしました。".to_string())??;
        Ok(Self {
            control_tx,
            stopped_rx,
            done_rx,
        })
    }

    fn finish(self, after_recording_stopped: impl FnOnce()) -> Result<AudioClip, String> {
        let _ = self.control_tx.send(RecorderCommand::Finish);
        let _ = self.stopped_rx.recv_timeout(RECORDING_STOP_NOTIFY_TIMEOUT);
        after_recording_stopped();
        let result = match self.done_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(result) => result,
            Err(_) => Err("録音停止処理がタイムアウトしました。".to_string()),
        };
        result
    }

    fn cancel(self) {
        let _ = self.control_tx.send(RecorderCommand::Cancel);
        let _ = self.done_rx.recv_timeout(Duration::from_secs(2));
    }
}

impl LiveTranscriber {
    fn sample_sender(&self) -> Option<std::sync::mpsc::Sender<Vec<i16>>> {
        self.sample_tx.as_ref().cloned()
    }

    fn finish(mut self) -> Result<String, String> {
        self.sample_tx.take();
        match self.join.take() {
            Some(join) => join
                .join()
                .unwrap_or_else(|_| Err("ライブ文字起こしスレッドが停止しました。".to_string())),
            None => Err("ライブ文字起こしが開始されていません。".to_string()),
        }
    }

    fn cancel(mut self) {
        self.sample_tx.take();
        self.join.take();
    }
}

fn live_transcription_provider_for_settings(
    settings: &AppSettings,
    mode: VoiceMode,
) -> Option<LiveTranscriptionProvider> {
    if mode != VoiceMode::Dictation {
        return None;
    }
    let profile = settings.voice.active_mode_profile()?;
    if !profile.live_transcription_enabled {
        return None;
    }
    live_transcription_provider_for_speech_profile(settings.voice.speech_profile)
}

fn live_transcription_provider_for_speech_profile(
    profile: SpeechProfile,
) -> Option<LiveTranscriptionProvider> {
    match profile {
        SpeechProfile::AppleSpeechAnalyzer => Some(LiveTranscriptionProvider::AppleSpeechAnalyzer),
        SpeechProfile::GoogleChirp3 => Some(LiveTranscriptionProvider::GoogleChirp3),
        SpeechProfile::OpenAiGpt4oTranscribe
        | SpeechProfile::OpenAiGpt4oMiniTranscribe
        | SpeechProfile::GeminiAudio => None,
    }
}

fn should_use_live_transcript(
    settings: &AppSettings,
    mode: VoiceMode,
    mode_profile_id: &str,
) -> bool {
    if mode != VoiceMode::Dictation {
        return false;
    }
    let Some(profile) = settings.voice.mode_profile_or_default(mode_profile_id) else {
        return false;
    };
    profile.live_transcription_enabled
        && live_transcription_provider_for_speech_profile(settings.voice.speech_profile).is_some()
}

fn start_live_transcriber(
    app: &tauri::AppHandle,
    provider: LiveTranscriptionProvider,
    sample_rate: u32,
    channels: u16,
    screen_context: &VoiceScreenContext,
) -> Result<LiveTranscriber, String> {
    match provider {
        LiveTranscriptionProvider::AppleSpeechAnalyzer => {
            start_apple_live_transcriber(app, sample_rate, channels, screen_context)
        }
        LiveTranscriptionProvider::GoogleChirp3 => {
            start_google_live_transcriber(app, sample_rate, channels, screen_context)
        }
    }
}

fn start_apple_live_transcriber(
    app: &tauri::AppHandle,
    sample_rate: u32,
    channels: u16,
    screen_context: &VoiceScreenContext,
) -> Result<LiveTranscriber, String> {
    let helper = resolve_apple_speech_helper(app)?;
    let entries = dictionary::load_dictionary(app).unwrap_or_default();
    let context_path = temp_voice_file_path("apple-speech-live-context", "json");
    let contextual_strings = apple_speech_contextual_strings(&entries, screen_context);
    let context = serde_json::json!({
        "contextualStrings": contextual_strings,
    });
    fs::write(&context_path, context.to_string()).map_err(|e| e.to_string())?;

    let mut command = std::process::Command::new(&helper);
    command
        .arg("stream-transcribe")
        .arg(sample_rate.to_string())
        .arg(channels.to_string())
        .arg("ja-JP")
        .arg(context_path.display().to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            let _ = fs::remove_file(&context_path);
            return Err(format!(
                "Apple SpeechAnalyzer helper（path: {}）を開始できませんでした: {err}",
                helper.display()
            ));
        }
    };

    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(&context_path);
        return Err("Apple SpeechAnalyzer helperのstdinを取得できませんでした。".to_string());
    };

    let (sample_tx, sample_rx) = std::sync::mpsc::channel::<Vec<i16>>();
    let writer_join = std::thread::spawn(move || -> Result<(), String> {
        for samples in sample_rx {
            write_i16_samples(&mut stdin, &samples)?;
        }
        stdin.flush().map_err(|e| e.to_string())
    });

    let join = std::thread::spawn(move || -> Result<String, String> {
        let writer_result = writer_join
            .join()
            .unwrap_or_else(|_| Err("ライブ音声送信スレッドが停止しました。".to_string()));
        let output = child
            .wait_with_output()
            .map_err(|e| format!("Apple SpeechAnalyzer helperの出力を取得できませんでした: {e}"));
        let _ = fs::remove_file(&context_path);
        writer_result?;
        let output = output?;
        parse_apple_speech_transcript_output(output)
    });

    Ok(LiveTranscriber {
        provider: LiveTranscriptionProvider::AppleSpeechAnalyzer,
        sample_tx: Some(sample_tx),
        join: Some(join),
    })
}

fn write_i16_samples(writer: &mut impl Write, samples: &[i16]) -> Result<(), String> {
    let bytes = i16_samples_to_bytes(samples);
    writer.write_all(&bytes).map_err(|e| e.to_string())
}

fn i16_samples_to_bytes(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * std::mem::size_of::<i16>());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn parse_apple_speech_transcript_output(output: std::process::Output) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(if detail.trim().is_empty() {
            format!(
                "Apple SpeechAnalyzer helperが失敗しました: {}",
                output.status
            )
        } else {
            detail
        });
    }
    let response: AppleSpeechHelperResponse = serde_json::from_str(&stdout).map_err(|err| {
        if stderr.is_empty() {
            format!("Apple SpeechAnalyzer helperからJSON応答が返りませんでした: {err}")
        } else {
            format!("Apple SpeechAnalyzer helperからJSON応答が返りませんでした: {err}: {stderr}")
        }
    })?;
    if !response.ok {
        return Err(response
            .error
            .or(response.reason)
            .unwrap_or_else(|| "Apple SpeechAnalyzer helperが失敗しました。".to_string()));
    }
    response
        .transcript
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Apple SpeechAnalyzerのライブ文字起こし結果が空でした。".to_string())
}

fn start_google_live_transcriber(
    app: &tauri::AppHandle,
    sample_rate: u32,
    channels: u16,
    screen_context: &VoiceScreenContext,
) -> Result<LiveTranscriber, String> {
    let settings = app
        .try_state::<SettingsStore>()
        .map(|store| store.get())
        .unwrap_or_default();
    let project = settings.voice.google_cloud_project_id.trim().to_string();
    if project.is_empty() {
        return Err("Google Cloud Project IDを設定してください。".to_string());
    }
    let region = settings.voice.google_cloud_region.trim().to_string();
    if region.is_empty() {
        return Err("Google Cloudリージョンを設定してください。".to_string());
    }
    let entries = dictionary::load_dictionary(app).unwrap_or_default();
    let screen_context = screen_context.clone();
    let (sample_tx, sample_rx) = std::sync::mpsc::channel::<Vec<i16>>();
    let join = std::thread::spawn(move || -> Result<String, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(google_streaming_transcribe(
            settings,
            entries,
            sample_rx,
            sample_rate,
            channels,
            project,
            region,
            screen_context,
        ))
    });

    Ok(LiveTranscriber {
        provider: LiveTranscriptionProvider::GoogleChirp3,
        sample_tx: Some(sample_tx),
        join: Some(join),
    })
}

async fn google_streaming_transcribe(
    settings: AppSettings,
    entries: Vec<DictionaryEntry>,
    sample_rx: std::sync::mpsc::Receiver<Vec<i16>>,
    sample_rate: u32,
    channels: u16,
    project: String,
    region: String,
    screen_context: VoiceScreenContext,
) -> Result<String, String> {
    use googleapis_tonic_google_cloud_speech_v2::google::cloud::speech::v2::{
        explicit_decoding_config, phrase_set, recognition_config, speech_adaptation,
        speech_client::SpeechClient, streaming_recognize_request, ExplicitDecodingConfig,
        PhraseSet, RecognitionConfig, RecognitionFeatures, SpeechAdaptation,
        StreamingRecognitionConfig, StreamingRecognitionFeatures, StreamingRecognizeRequest,
    };
    use tonic::metadata::MetadataValue;
    use tonic::service::Interceptor;
    use tonic::transport::Channel;

    #[derive(Clone)]
    struct GoogleAuthInterceptor {
        authorization: MetadataValue<tonic::metadata::Ascii>,
    }

    impl Interceptor for GoogleAuthInterceptor {
        fn call(
            &mut self,
            mut request: tonic::Request<()>,
        ) -> Result<tonic::Request<()>, tonic::Status> {
            request
                .metadata_mut()
                .insert("authorization", self.authorization.clone());
            Ok(request)
        }
    }

    let token = google_access_token(&settings).await?;
    let endpoint = format!("https://{region}-speech.googleapis.com");
    let channel = Channel::from_shared(endpoint.clone())
        .map_err(|e| e.to_string())?
        .connect()
        .await
        .map_err(|e| format!("Google Speech-to-Text gRPCへ接続できませんでした: {e}"))?;
    let authorization = MetadataValue::try_from(format!("Bearer {token}"))
        .map_err(|e| format!("Google認証メタデータを作成できませんでした: {e}"))?;
    let mut client =
        SpeechClient::with_interceptor(channel, GoogleAuthInterceptor { authorization });

    let recognizer = format!("projects/{project}/locations/{region}/recognizers/_");
    let phrases = transcription_contextual_phrases(&entries, &screen_context, 1000);
    let phrase_values = phrases
        .iter()
        .take(1000)
        .map(|value| phrase_set::Phrase {
            value: value.clone(),
            boost: GOOGLE_SPEECH_DICTIONARY_BOOST,
        })
        .collect::<Vec<_>>();
    let adaptation = if phrase_values.is_empty() {
        None
    } else {
        Some(SpeechAdaptation {
            phrase_sets: vec![speech_adaptation::AdaptationPhraseSet {
                value: Some(
                    speech_adaptation::adaptation_phrase_set::Value::InlinePhraseSet(PhraseSet {
                        phrases: phrase_values,
                        boost: GOOGLE_SPEECH_DICTIONARY_BOOST,
                        ..Default::default()
                    }),
                ),
            }],
            custom_classes: Vec::new(),
        })
    };
    let config = RecognitionConfig {
        model: "chirp_3".to_string(),
        language_codes: vec!["ja-JP".to_string()],
        features: Some(RecognitionFeatures {
            enable_automatic_punctuation: true,
            ..Default::default()
        }),
        adaptation,
        decoding_config: Some(recognition_config::DecodingConfig::ExplicitDecodingConfig(
            ExplicitDecodingConfig {
                encoding: explicit_decoding_config::AudioEncoding::Linear16 as i32,
                sample_rate_hertz: sample_rate as i32,
                audio_channel_count: channels as i32,
            },
        )),
        ..Default::default()
    };
    let streaming_config = StreamingRecognitionConfig {
        config: Some(config),
        streaming_features: Some(StreamingRecognitionFeatures {
            interim_results: true,
            ..Default::default()
        }),
        ..Default::default()
    };

    let (request_tx, request_rx) = tokio::sync::mpsc::channel::<StreamingRecognizeRequest>(16);
    request_tx
        .send(StreamingRecognizeRequest {
            recognizer: recognizer.clone(),
            streaming_request: Some(
                streaming_recognize_request::StreamingRequest::StreamingConfig(streaming_config),
            ),
        })
        .await
        .map_err(|_| "Google Speech-to-Text gRPCの送信開始に失敗しました。".to_string())?;

    let bridge_join = std::thread::spawn(move || -> Result<(), String> {
        for samples in sample_rx {
            let bytes = i16_samples_to_bytes(&samples);
            for chunk in bytes.chunks(14 * 1024) {
                request_tx
                    .blocking_send(StreamingRecognizeRequest {
                        recognizer: recognizer.clone(),
                        streaming_request: Some(
                            streaming_recognize_request::StreamingRequest::Audio(chunk.to_vec()),
                        ),
                    })
                    .map_err(|_| {
                        "Google Speech-to-Text gRPCへの音声送信が停止しました。".to_string()
                    })?;
            }
        }
        Ok(())
    });

    let mut response_stream = client
        .streaming_recognize(tokio_stream::wrappers::ReceiverStream::new(request_rx))
        .await
        .map_err(|e| format!("Google Speech-to-Text streamingRecognizeが失敗しました: {e}"))?
        .into_inner();
    let mut final_parts = Vec::new();
    let mut latest_interim = String::new();

    while let Some(response) = response_stream
        .message()
        .await
        .map_err(|e| format!("Google Speech-to-Text streaming応答の取得に失敗しました: {e}"))?
    {
        for result in response.results {
            let transcript = result
                .alternatives
                .first()
                .map(|alternative| alternative.transcript.trim().to_string())
                .unwrap_or_default();
            if transcript.is_empty() {
                continue;
            }
            if result.is_final {
                if final_parts.last() != Some(&transcript) {
                    final_parts.push(transcript);
                }
                latest_interim.clear();
            } else {
                latest_interim = transcript;
            }
        }
    }

    let bridge_result = bridge_join.join().unwrap_or_else(|_| {
        Err("Google Speech-to-Text音声送信スレッドが停止しました。".to_string())
    });
    bridge_result?;

    if final_parts.is_empty() && !latest_interim.trim().is_empty() {
        final_parts.push(latest_interim);
    }
    let transcript = final_parts.join("\n").trim().to_string();
    if transcript.is_empty() {
        Err("Google Speech-to-Textのライブ文字起こし結果が空でした。".to_string())
    } else {
        Ok(transcript)
    }
}

impl SystemAudioMuteGuard {
    fn start() -> Self {
        let snapshot = current_output_audio_snapshot();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let join = std::thread::spawn(move || {
            mute_system_output();
            loop {
                match stop_rx.recv_timeout(Duration::from_millis(450)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => mute_system_output(),
                }
            }
        });
        Self {
            snapshot,
            stop_tx,
            join: Some(join),
        }
    }

    fn stop(mut self) {
        let _ = self.stop_tx.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        restore_system_output(self.snapshot);
    }
}

impl Drop for SystemAudioMuteGuard {
    fn drop(&mut self) {
        if self.join.is_some() {
            let _ = self.stop_tx.send(());
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
            restore_system_output(self.snapshot);
        }
    }
}

fn run_recording_thread(
    app: tauri::AppHandle,
    selected_device_id: Option<String>,
    max_recording_seconds: u64,
    control_rx: std::sync::mpsc::Receiver<RecorderCommand>,
    stopped_tx: std::sync::mpsc::Sender<()>,
    init_tx: std::sync::mpsc::Sender<Result<(), String>>,
    pipeline: PipelineMode,
    live_transcription_provider: Option<LiveTranscriptionProvider>,
    screen_context: VoiceScreenContext,
) -> Result<AudioClip, String> {
    let init_signal: RecorderInitSignal = Arc::new(Mutex::new(Some(init_tx)));
    let setup: Result<RecorderSetup, String> = (|| {
        let host = cpal::default_host();
        let device = input_device_by_id(&host, selected_device_id.as_deref())?
            .or_else(|| host.default_input_device())
            .ok_or_else(|| "利用できるマイクが見つかりません。".to_string())?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let device_sample_rate = supported.sample_rate().0;
        let device_channels = supported.channels();
        let config: cpal::StreamConfig = supported.clone().into();
        let samples = Arc::new(Mutex::new(Vec::<i16>::new()));
        let (output_sample_rate, output_channels) = match &pipeline {
            PipelineMode::Direct => (device_sample_rate, device_channels),
            PipelineMode::AecIsolate(_) => (aec::SAMPLE_RATE, 1u16),
        };
        let max_samples = (output_sample_rate as usize)
            * (output_channels as usize)
            * max_recording_seconds.clamp(5, 600) as usize;
        let live_transcriber = live_transcription_provider.and_then(|provider| {
            match start_live_transcriber(
                &app,
                provider,
                output_sample_rate,
                output_channels,
                &screen_context,
            ) {
                Ok(transcriber) => Some(transcriber),
                Err(err) => {
                    eprintln!("[enja] live transcription unavailable: {err}");
                    None
                }
            }
        });
        let live_sample_tx = live_transcriber
            .as_ref()
            .and_then(LiveTranscriber::sample_sender);
        let last_emit = Arc::new(Mutex::new(Instant::now()));
        let err_fn = |err| eprintln!("[enja] audio input stream error: {err}");

        let aec_pipeline = match &pipeline {
            PipelineMode::Direct => None,
            PipelineMode::AecIsolate(system) => Some(AecPipeline::new(
                system.clone(),
                device_sample_rate,
                device_channels,
            )?),
        };

        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build_input_stream::<f32>(
                &device,
                &config,
                samples.clone(),
                last_emit,
                max_samples,
                app.clone(),
                device_channels,
                aec_pipeline,
                live_sample_tx.clone(),
                init_signal.clone(),
                err_fn,
            ),
            cpal::SampleFormat::I16 => build_input_stream::<i16>(
                &device,
                &config,
                samples.clone(),
                last_emit,
                max_samples,
                app.clone(),
                device_channels,
                aec_pipeline,
                live_sample_tx.clone(),
                init_signal.clone(),
                err_fn,
            ),
            cpal::SampleFormat::U16 => build_input_stream::<u16>(
                &device,
                &config,
                samples.clone(),
                last_emit,
                max_samples,
                app.clone(),
                device_channels,
                aec_pipeline,
                live_sample_tx.clone(),
                init_signal.clone(),
                err_fn,
            ),
            _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
        }
        .map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;
        Ok((
            samples,
            output_sample_rate,
            output_channels,
            stream,
            live_transcriber,
        ))
    })();

    let (samples, sample_rate, channels, stream, live_transcriber) = match setup {
        Ok(values) => values,
        Err(err) => {
            send_recorder_init(&init_signal, Err(err.clone()));
            return Err(err);
        }
    };

    let max_wait = Duration::from_secs(max_recording_seconds.clamp(5, 600));
    let command = match control_rx.recv_timeout(max_wait) {
        Ok(command) => command,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => RecorderCommand::Finish,
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => RecorderCommand::Cancel,
    };
    drop(stream);
    let _ = stopped_tx.send(());

    if command == RecorderCommand::Cancel {
        if let Some(transcriber) = live_transcriber {
            transcriber.cancel();
        }
        return Err("録音をキャンセルしました。".to_string());
    }

    let live_transcript = live_transcriber.map(|transcriber| {
        let provider = transcriber.provider;
        LiveTranscript {
            provider,
            result: transcriber.finish(),
        }
    });

    let samples = samples.lock().map_err(|e| e.to_string())?.clone();
    if samples.is_empty() {
        return Err("音声が録音されていません。".to_string());
    }

    let prepared = prepare_recorded_audio_for_api(&samples, sample_rate, channels)?;
    let wav = samples_to_wav(&prepared.samples, sample_rate, channels)?;
    Ok(AudioClip {
        wav,
        duration_secs: prepared.analysis.duration_secs,
        live_transcript,
    })
}

fn send_recorder_init(signal: &RecorderInitSignal, result: Result<(), String>) {
    if let Ok(mut guard) = signal.lock() {
        if let Some(tx) = guard.take() {
            let _ = tx.send(result);
        }
    }
}

struct AecPipeline {
    aec: Aec,
    system: Arc<SystemTap>,
    step: f64,
    next_read: f64,
    input_count: u64,
    prev_in: f32,
    mic_frame: Vec<f32>,
}

impl AecPipeline {
    fn new(
        system: Arc<SystemTap>,
        device_sample_rate: u32,
        _device_channels: u16,
    ) -> Result<Self, String> {
        Ok(Self {
            aec: Aec::new()?,
            system,
            step: device_sample_rate as f64 / aec::SAMPLE_RATE as f64,
            next_read: 0.0,
            input_count: 0,
            prev_in: 0.0,
            mic_frame: Vec::with_capacity(aec::FRAME_SAMPLES * 4),
        })
    }

    fn push_mono(&mut self, sample: f32) {
        let cur = self.input_count as f64;
        let prev = cur - 1.0;
        while self.next_read <= cur {
            let frac = (self.next_read - prev) as f32;
            let value = self.prev_in + (sample - self.prev_in) * frac;
            self.mic_frame.push(value.clamp(-1.0, 1.0));
            self.next_read += self.step;
        }
        self.prev_in = sample;
        self.input_count += 1;
    }

    fn drain_frames<F: FnMut(&[f32])>(&mut self, mut emit: F) {
        while self.mic_frame.len() >= aec::FRAME_SAMPLES {
            let mut frame: Vec<f32> = self.mic_frame.drain(..aec::FRAME_SAMPLES).collect();
            let reference = self.system.take_reference(aec::FRAME_SAMPLES);
            if let Err(err) = self.aec.process(&mut frame, &reference) {
                eprintln!("[enja] AEC処理に失敗: {err}");
            }
            emit(&frame);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Mutex<Vec<i16>>>,
    last_emit: Arc<Mutex<Instant>>,
    max_samples: usize,
    app: tauri::AppHandle,
    device_channels: u16,
    mut aec_pipeline: Option<AecPipeline>,
    live_sample_tx: Option<std::sync::mpsc::Sender<Vec<i16>>>,
    init_signal: RecorderInitSignal,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::Sample + cpal::SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let chan = device_channels.max(1) as usize;
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            send_recorder_init(&init_signal, Ok(()));
            let mut peak = 0.0f32;
            let mut sum = 0.0f32;
            let mut count = 0usize;

            if let Some(pipeline) = aec_pipeline.as_mut() {
                for chunk in data.chunks(chan) {
                    let mut mono = 0.0_f32;
                    for sample in chunk {
                        mono += f32::from_sample(*sample);
                    }
                    let mono = (mono / chunk.len().max(1) as f32).clamp(-1.0, 1.0);
                    peak = peak.max(mono.abs());
                    sum += mono * mono;
                    count += 1;
                    pipeline.push_mono(mono);
                }
                let samples_buf = samples.clone();
                let live_tx = live_sample_tx.clone();
                pipeline.drain_frames(|frame| {
                    let mut live_samples = Vec::new();
                    if let Ok(mut guard) = samples_buf.lock() {
                        let remaining = max_samples.saturating_sub(guard.len());
                        for value in frame.iter().take(remaining) {
                            let sample = (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                            guard.push(sample);
                            live_samples.push(sample);
                        }
                    }
                    if !live_samples.is_empty() {
                        if let Some(tx) = live_tx.as_ref() {
                            let _ = tx.send(live_samples);
                        }
                    }
                });
            } else if let Ok(mut guard) = samples.lock() {
                let remaining = max_samples.saturating_sub(guard.len());
                let mut live_samples = Vec::new();
                for sample in data.iter().take(remaining) {
                    let value = f32::from_sample(*sample).clamp(-1.0, 1.0);
                    peak = peak.max(value.abs());
                    sum += value * value;
                    count += 1;
                    let pcm = (value * i16::MAX as f32) as i16;
                    guard.push(pcm);
                    live_samples.push(pcm);
                }
                if !live_samples.is_empty() {
                    if let Some(tx) = live_sample_tx.as_ref() {
                        let _ = tx.send(live_samples);
                    }
                }
            }

            if count > 0 {
                let rms = (sum / count as f32).sqrt().clamp(0.0, 1.0);
                if let Ok(mut last) = last_emit.lock() {
                    if last.elapsed() >= Duration::from_millis(45) {
                        *last = Instant::now();
                        let _ = app.emit(
                            "voice-level",
                            VoiceLevelEvent {
                                rms,
                                peak: peak.clamp(0.0, 1.0),
                            },
                        );
                    }
                }
            }
        },
        err_fn,
        None,
    )
}

pub fn list_audio_input_devices() -> Result<Vec<AudioInputDevice>, String> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let mut name_counts = std::collections::HashMap::<String, usize>::new();
    let mut entries = Vec::new();

    for (idx, device) in host.input_devices().map_err(|e| e.to_string())?.enumerate() {
        let name = device
            .name()
            .unwrap_or_else(|_| "名称未取得のマイク".to_string());
        *name_counts.entry(name.clone()).or_insert(0) += 1;
        entries.push((idx, name));
    }

    let mut out = Vec::with_capacity(entries.len());
    for (idx, name) in entries {
        let is_default = default_name
            .as_deref()
            .is_some_and(|default| default == name && name_counts.get(&name) == Some(&1));
        out.push(AudioInputDevice {
            id: format!("{name}#{idx}"),
            is_default,
            name,
        });
    }
    Ok(out)
}

fn parse_audio_input_device_id(selected_id: &str) -> Option<&str> {
    let hash_index = selected_id.rfind('#')?;
    if hash_index == 0 {
        return None;
    }
    selected_id[hash_index + 1..]
        .parse::<usize>()
        .ok()
        .map(|_| &selected_id[..hash_index])
}

fn input_device_by_id(
    host: &cpal::Host,
    selected_id: Option<&str>,
) -> Result<Option<cpal::Device>, String> {
    let Some(selected_id) = selected_id else {
        return Ok(None);
    };

    let selected_name = parse_audio_input_device_id(selected_id);
    let mut same_name_match = None;
    let mut same_name_count = 0usize;

    for (idx, device) in host.input_devices().map_err(|e| e.to_string())?.enumerate() {
        let name = device.name().unwrap_or_default();
        if selected_id == format!("{name}#{idx}") {
            return Ok(Some(device));
        }
        if selected_name == Some(name.as_str()) {
            same_name_count += 1;
            same_name_match = Some(device);
        }
    }
    if same_name_count == 1 {
        return Ok(same_name_match);
    }
    Ok(None)
}

fn audio_input_devices_signature(devices: &[AudioInputDevice]) -> Vec<(String, String, bool)> {
    devices
        .iter()
        .map(|device| (device.id.clone(), device.name.clone(), device.is_default))
        .collect()
}

fn poll_audio_input_devices(app: tauri::AppHandle, interval: Duration) {
    let mut last_signature = list_audio_input_devices()
        .map(|devices| audio_input_devices_signature(&devices))
        .unwrap_or_default();

    loop {
        std::thread::sleep(interval);
        let devices = match list_audio_input_devices() {
            Ok(devices) => devices,
            Err(err) => {
                eprintln!("[enja] audio input device refresh failed: {err}");
                continue;
            }
        };
        let signature = audio_input_devices_signature(&devices);
        if signature != last_signature {
            last_signature = signature;
            let _ = app.emit(AUDIO_INPUT_DEVICES_CHANGED_EVENT, devices);
        }
    }
}

#[cfg(target_os = "macos")]
mod audio_input_device_watcher {
    use super::{
        audio_input_devices_signature, list_audio_input_devices, poll_audio_input_devices,
        AUDIO_INPUT_DEVICES_CHANGED_EVENT,
    };
    use coreaudio_sys::{
        kAudioHardwareNoError, kAudioHardwarePropertyDefaultInputDevice,
        kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMaster,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioObjectAddPropertyListener,
        AudioObjectPropertyAddress, AudioObjectRemovePropertyListener, OSStatus,
    };
    use std::ffi::c_void;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::time::Duration;
    use tauri::Emitter;

    const REFRESH_DEBOUNCE: Duration = Duration::from_millis(300);

    pub fn spawn(app: tauri::AppHandle) {
        std::thread::spawn(move || {
            let (tx, rx) = mpsc::channel();
            let _listeners = match CoreAudioDeviceListeners::register(tx) {
                Ok(listeners) => listeners,
                Err(err) => {
                    eprintln!("[enja] CoreAudio device watcher unavailable: {err}");
                    poll_audio_input_devices(app, Duration::from_secs(2));
                    return;
                }
            };

            run_event_loop(app, rx);
        });
    }

    fn run_event_loop(app: tauri::AppHandle, rx: Receiver<()>) {
        let mut last_signature = list_audio_input_devices()
            .map(|devices| audio_input_devices_signature(&devices))
            .unwrap_or_default();

        while rx.recv().is_ok() {
            std::thread::sleep(REFRESH_DEBOUNCE);
            while rx.try_recv().is_ok() {}

            let devices = match list_audio_input_devices() {
                Ok(devices) => devices,
                Err(err) => {
                    eprintln!("[enja] audio input device refresh failed: {err}");
                    continue;
                }
            };
            let signature = audio_input_devices_signature(&devices);
            if signature != last_signature {
                last_signature = signature;
                let _ = app.emit(AUDIO_INPUT_DEVICES_CHANGED_EVENT, devices);
            }
        }
    }

    struct CoreAudioDeviceListeners {
        client_data: *mut Sender<()>,
        devices_registered: bool,
        default_input_registered: bool,
    }

    impl CoreAudioDeviceListeners {
        fn register(tx: Sender<()>) -> Result<Self, String> {
            let mut listeners = Self {
                client_data: Box::into_raw(Box::new(tx)),
                devices_registered: false,
                default_input_registered: false,
            };

            unsafe {
                listeners.add_listener(kAudioHardwarePropertyDevices)?;
                listeners.devices_registered = true;
                listeners.add_listener(kAudioHardwarePropertyDefaultInputDevice)?;
                listeners.default_input_registered = true;
            }

            Ok(listeners)
        }

        unsafe fn add_listener(&self, selector: u32) -> Result<(), String> {
            let address = listener_address(selector);
            let status = AudioObjectAddPropertyListener(
                kAudioObjectSystemObject,
                &address,
                Some(audio_device_listener),
                self.client_data.cast::<c_void>(),
            );
            if status == kAudioHardwareNoError as OSStatus {
                Ok(())
            } else {
                Err(format!(
                    "AudioObjectAddPropertyListener({selector}) returned {status}"
                ))
            }
        }

        unsafe fn remove_listener(&self, selector: u32) {
            let address = listener_address(selector);
            let _ = AudioObjectRemovePropertyListener(
                kAudioObjectSystemObject,
                &address,
                Some(audio_device_listener),
                self.client_data.cast::<c_void>(),
            );
        }
    }

    impl Drop for CoreAudioDeviceListeners {
        fn drop(&mut self) {
            unsafe {
                if self.default_input_registered {
                    self.remove_listener(kAudioHardwarePropertyDefaultInputDevice);
                }
                if self.devices_registered {
                    self.remove_listener(kAudioHardwarePropertyDevices);
                }
                drop(Box::from_raw(self.client_data));
            }
        }
    }

    fn listener_address(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        }
    }

    unsafe extern "C" fn audio_device_listener(
        _object_id: u32,
        _address_count: u32,
        _addresses: *const AudioObjectPropertyAddress,
        client_data: *mut c_void,
    ) -> OSStatus {
        if client_data.is_null() {
            return kAudioHardwareNoError as OSStatus;
        }
        let tx = &*(client_data as *const Sender<()>);
        let _ = tx.send(());
        kAudioHardwareNoError as OSStatus
    }
}

#[cfg(not(target_os = "macos"))]
mod audio_input_device_watcher {
    use super::poll_audio_input_devices;
    use std::time::Duration;

    pub fn spawn(app: tauri::AppHandle) {
        std::thread::spawn(move || poll_audio_input_devices(app, Duration::from_secs(2)));
    }
}

pub async fn check_speech_profile_setup(
    app: &tauri::AppHandle,
    profile: SpeechProfile,
    settings: AppSettings,
) -> Result<SpeechSetupCheck, String> {
    match profile {
        SpeechProfile::GoogleChirp3 => check_google_chirp3_setup(&settings).await,
        SpeechProfile::OpenAiGpt4oTranscribe | SpeechProfile::OpenAiGpt4oMiniTranscribe => {
            Ok(check_secret_setup(
                "OpenAI APIキー",
                "openai",
                "OpenAI APIキーが保存されています。",
                "OpenAI APIキーを保存してください。",
            ))
        }
        SpeechProfile::GeminiAudio => Ok(check_secret_setup(
            "Gemini APIキー",
            "gemini",
            "Gemini APIキーが保存されています。",
            "Gemini APIキーを保存してください。",
        )),
        SpeechProfile::AppleSpeechAnalyzer => {
            let status = apple_speech_status(app, true)?;
            Ok(apple_speech_setup_check(&status))
        }
    }
}

async fn check_google_chirp3_setup(settings: &AppSettings) -> Result<SpeechSetupCheck, String> {
    let mut missing = Vec::new();
    if settings.voice.google_cloud_project_id.trim().is_empty() {
        missing.push("Google Cloud Project ID");
    }
    if settings.voice.google_cloud_region.trim().is_empty() {
        missing.push("Google Cloudリージョン");
    }
    if !missing.is_empty() {
        return Ok(SpeechSetupCheck {
            ok: false,
            message: format!("未入力の設定があります: {}", missing.join(", ")),
            details: Vec::new(),
        });
    }

    match google_access_token_with_details(settings).await {
        Ok((_token, mut details)) => {
            details.insert(
                0,
                format!(
                    "Project ID: {} / リージョン: {}",
                    settings.voice.google_cloud_project_id.trim(),
                    settings.voice.google_cloud_region.trim()
                ),
            );
            details.push(
                "認証トークン取得まで確認しました。Speech-to-Text API有効化と権限は実際の認識リクエスト時に検証されます。"
                    .to_string(),
            );
            Ok(SpeechSetupCheck {
                ok: true,
                message: "Google Chirp 3の認証設定は利用可能です。".to_string(),
                details,
            })
        }
        Err(message) => Ok(SpeechSetupCheck {
            ok: false,
            message,
            details: Vec::new(),
        }),
    }
}

fn check_secret_setup(
    label: &str,
    provider: &str,
    ok_message: &str,
    missing_message: &str,
) -> SpeechSetupCheck {
    match secrets::get_secret(provider) {
        Ok(value) if !value.trim().is_empty() => SpeechSetupCheck {
            ok: true,
            message: ok_message.to_string(),
            details: vec![format!("{label}: 保存済み")],
        },
        _ => SpeechSetupCheck {
            ok: false,
            message: missing_message.to_string(),
            details: vec![format!("{label}: 未保存")],
        },
    }
}

async fn process_clip(
    app: &tauri::AppHandle,
    mode: VoiceMode,
    mode_profile_id: &str,
    selected_text: &str,
    screen_context: &VoiceScreenContext,
    clip: AudioClip,
) -> Result<String, String> {
    let settings = crate::settings::load_settings(app)?;
    let entries = dictionary::load_dictionary(app)?;
    let transcript = if should_use_live_transcript(&settings, mode, mode_profile_id) {
        match clip.live_transcript.as_ref() {
            Some(live)
                if live
                    .result
                    .as_ref()
                    .is_ok_and(|value| !value.trim().is_empty()) =>
            {
                match live.provider {
                    LiveTranscriptionProvider::GoogleChirp3 => {
                        if let Err(err) =
                            usage::record_google_speech_to_text(app, clip.duration_secs)
                        {
                            eprintln!("[enja] usage tracking failed: {err}");
                        }
                    }
                    LiveTranscriptionProvider::AppleSpeechAnalyzer => {}
                }
                live.result.as_ref().unwrap().clone()
            }
            Some(live) if live.result.is_ok() => {
                transcribe(app, &settings, &entries, screen_context, &clip).await?
            }
            Some(live) => {
                let err = live
                    .result
                    .as_ref()
                    .err()
                    .cloned()
                    .unwrap_or_else(|| "ライブ文字起こしに失敗しました。".to_string());
                eprintln!("[enja] live transcription failed; falling back to batch: {err}");
                transcribe(app, &settings, &entries, screen_context, &clip).await?
            }
            None => transcribe(app, &settings, &entries, screen_context, &clip).await?,
        }
    } else {
        transcribe(app, &settings, &entries, screen_context, &clip).await?
    };
    finalize_text(
        app,
        &settings,
        &entries,
        mode,
        mode_profile_id,
        selected_text,
        screen_context,
        &transcript,
    )
    .await
}

async fn finalize_selected_text_instruction(
    app: &tauri::AppHandle,
    selected_text: &str,
    screen_context: &VoiceScreenContext,
) -> Result<String, String> {
    let settings = crate::settings::load_settings(app)?;
    let entries = dictionary::load_dictionary(app)?;
    finalize_text(
        app,
        &settings,
        &entries,
        VoiceMode::Ask,
        "",
        selected_text,
        screen_context,
        POLISH_SELECTION_INSTRUCTION,
    )
    .await
}

async fn transcribe(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    match settings.voice.speech_profile {
        SpeechProfile::GoogleChirp3 => {
            if clip.duration_secs > 60.0 || clip.wav.len() > 10 * 1024 * 1024 {
                transcribe_long_audio_fallback(app, settings, entries, screen_context, clip).await
            } else {
                transcribe_google_chirp3(app, settings, entries, screen_context, clip).await
            }
        }
        SpeechProfile::OpenAiGpt4oTranscribe => {
            transcribe_openai(
                app,
                "gpt-4o-transcribe",
                settings,
                entries,
                screen_context,
                clip,
            )
            .await
        }
        SpeechProfile::OpenAiGpt4oMiniTranscribe => {
            transcribe_openai(
                app,
                "gpt-4o-mini-transcribe",
                settings,
                entries,
                screen_context,
                clip,
            )
            .await
        }
        SpeechProfile::GeminiAudio => {
            transcribe_gemini_audio(app, settings, entries, screen_context, clip).await
        }
        SpeechProfile::AppleSpeechAnalyzer => {
            transcribe_apple_speech(app, entries, screen_context, clip).await
        }
    }
}

async fn transcribe_long_audio_fallback(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    if secrets::get_secret("openai").is_ok_and(|key| !key.trim().is_empty()) {
        return transcribe_openai(
            app,
            "gpt-4o-transcribe",
            settings,
            entries,
            screen_context,
            clip,
        )
        .await;
    }
    transcribe_gemini_audio(app, settings, entries, screen_context, clip).await
}

fn http_client(timeout: Duration) -> Result<reqwest::Client, String> {
    cache::http_client(timeout, TOKEN_REQUEST_TIMEOUT)
}

fn speech_request_error(provider: &str, err: reqwest::Error) -> String {
    if err.is_timeout() {
        format!("{provider}の応答がタイムアウトしました。短く録音するか、別の音声認識モデルを試してください。")
    } else {
        err.to_string()
    }
}

pub fn apple_speech_status(
    app: &tauri::AppHandle,
    request_authorization: bool,
) -> Result<AppleSpeechStatus, String> {
    match run_apple_speech_helper(
        app,
        &[
            "status".to_string(),
            "ja-JP".to_string(),
            if request_authorization {
                "--request-authorization".to_string()
            } else {
                "--no-request-authorization".to_string()
            },
        ],
        APPLE_SPEECH_REQUEST_TIMEOUT,
    ) {
        Ok(response) => Ok(apple_status_from_helper(response)),
        Err(err) => Ok(AppleSpeechStatus {
            helper_available: false,
            supported: false,
            status: "unknown".to_string(),
            authorization: "unknown".to_string(),
            message: "Apple SpeechAnalyzer helperを利用できません。".to_string(),
            details: vec![err],
        }),
    }
}

pub fn install_apple_speech_model(app: &tauri::AppHandle) -> Result<AppleSpeechStatus, String> {
    let response = run_apple_speech_helper(
        app,
        &["install".to_string(), "ja-JP".to_string()],
        APPLE_SPEECH_INSTALL_TIMEOUT,
    )?;
    Ok(apple_status_from_helper(response))
}

fn apple_status_from_helper(response: AppleSpeechHelperResponse) -> AppleSpeechStatus {
    let status = response.status.unwrap_or_else(|| "unknown".to_string());
    let authorization = response
        .authorization
        .unwrap_or_else(|| "unknown".to_string());
    let supported = response.supported.unwrap_or(status != "unsupported");
    let reason = response.reason.or(response.error);
    let mut details = response.details.unwrap_or_default();
    if let Some(reason) = reason.as_ref() {
        if !reason.trim().is_empty() {
            details.insert(0, reason.clone());
        }
    }
    let message = if response.ok {
        match (supported, status.as_str(), authorization.as_str()) {
            (false, _, _) => "このMacではApple SpeechAnalyzerを利用できません。".to_string(),
            (_, "installed", "authorized") => {
                "Apple SpeechAnalyzer日本語モデルは利用可能です。".to_string()
            }
            (_, "installed", "notDetermined") => {
                "Apple SpeechAnalyzer日本語モデルはインストール済みです。音声認識権限の確認が必要です。"
                    .to_string()
            }
            (_, "installed", "denied" | "restricted") => {
                "音声認識権限が許可されていません。macOSの設定で許可してください。".to_string()
            }
            (_, "downloading", _) => {
                "Apple SpeechAnalyzer日本語モデルをインストール中です。".to_string()
            }
            (_, "supported", _) => {
                "Apple SpeechAnalyzer日本語モデルは未インストールです。".to_string()
            }
            _ => "Apple SpeechAnalyzerの状態を確認しました。".to_string(),
        }
    } else {
        "Apple SpeechAnalyzerの状態確認に失敗しました。".to_string()
    };

    AppleSpeechStatus {
        helper_available: true,
        supported,
        status,
        authorization,
        message,
        details,
    }
}

fn apple_speech_setup_check(status: &AppleSpeechStatus) -> SpeechSetupCheck {
    let ok = status.helper_available
        && status.supported
        && status.status == "installed"
        && status.authorization == "authorized";
    let mut details = vec![
        format!("モデル状態: {}", status.status),
        format!("音声認識権限: {}", status.authorization),
    ];
    details.extend(status.details.clone());
    SpeechSetupCheck {
        ok,
        message: status.message.clone(),
        details,
    }
}

fn run_apple_speech_helper(
    app: &tauri::AppHandle,
    args: &[String],
    timeout: Duration,
) -> Result<AppleSpeechHelperResponse, String> {
    let helper = resolve_apple_speech_helper(app)?;
    let mut command = std::process::Command::new(&helper);
    command.args(args);
    let output = command_output_with_timeout(
        command,
        timeout,
        &format!("Apple SpeechAnalyzer helper（path: {}）", helper.display()),
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let response = if stdout.is_empty() {
        None
    } else {
        serde_json::from_str::<AppleSpeechHelperResponse>(&stdout).ok()
    };
    if let Some(response) = response {
        if response.ok || response.status.is_some() {
            return Ok(response);
        }
        return Err(response
            .error
            .or(response.reason)
            .unwrap_or_else(|| "Apple SpeechAnalyzer helperが失敗しました。".to_string()));
    }
    let detail = if stderr.is_empty() {
        stdout
    } else if stdout.is_empty() {
        stderr
    } else {
        format!("{stdout}\n{stderr}")
    };
    Err(if detail.trim().is_empty() {
        "Apple SpeechAnalyzer helperからJSON応答が返りませんでした。".to_string()
    } else {
        detail
    })
}

fn resolve_apple_speech_helper(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let executable_name = "enja-speech-helper";
    let target_name = format!("enja-speech-helper-{}", env!("ENJA_TARGET_TRIPLE"));
    let mut candidates = Vec::<PathBuf>::new();
    if let Ok(path) = std::env::var("ENJA_SPEECH_HELPER_PATH") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(executable_name));
        }
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join(executable_name));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bin")
            .join(target_name),
    );

    for path in &candidates {
        if path.is_file() {
            return Ok(path.clone());
        }
    }
    Err(format!(
        "Apple SpeechAnalyzer helperが見つかりません。探した場所: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

async fn transcribe_apple_speech(
    app: &tauri::AppHandle,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    let wav_path = temp_voice_file_path("apple-speech", "wav");
    let context_path = temp_voice_file_path("apple-speech-context", "json");
    fs::write(&wav_path, &clip.wav).map_err(|e| e.to_string())?;
    let contextual_strings = apple_speech_contextual_strings(entries, screen_context);
    let context = serde_json::json!({
        "contextualStrings": contextual_strings,
    });
    fs::write(&context_path, context.to_string()).map_err(|e| e.to_string())?;

    let args = vec![
        "transcribe".to_string(),
        wav_path.display().to_string(),
        "ja-JP".to_string(),
        context_path.display().to_string(),
    ];
    let result =
        run_apple_speech_helper(app, &args, APPLE_SPEECH_REQUEST_TIMEOUT).and_then(|response| {
            response
                .transcript
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    response.error.or(response.reason).unwrap_or_else(|| {
                        "Apple SpeechAnalyzerの文字起こし結果が空でした。".to_string()
                    })
                })
        });

    let _ = fs::remove_file(&wav_path);
    let _ = fs::remove_file(&context_path);
    result
}

fn apple_speech_contextual_strings(
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut values = Vec::<String>::new();
    for entry in entries.iter().filter(|entry| entry.enabled) {
        let value = entry.preferred.trim();
        if value.is_empty() || value.chars().count() > 40 {
            continue;
        }
        let key = value.to_lowercase();
        if seen.insert(key) {
            values.push(value.to_string());
            if values.len() >= APPLE_SPEECH_CONTEXTUAL_STRINGS_MAX {
                return values;
            }
        }
    }
    for value in screen_context_terms(screen_context) {
        if values.len() >= APPLE_SPEECH_CONTEXTUAL_STRINGS_MAX {
            break;
        }
        let key = value.to_lowercase();
        if seen.insert(key) {
            values.push(value);
        }
    }
    values
}

fn temp_voice_file_path(label: &str, extension: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "enja-{label}-{}-{nonce}.{extension}",
        std::process::id()
    ))
}

async fn transcribe_google_chirp3(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    if clip.duration_secs > 60.0 || clip.wav.len() > 10 * 1024 * 1024 {
        return Err(
            "Google Chirp 3の同期認識は1分/10MBまでです。長い録音はOpenAIまたはGeminiへ自動フォールバックします。"
                .to_string(),
        );
    }
    let project = settings.voice.google_cloud_project_id.trim();
    if project.is_empty() {
        return Err("Google Cloud Project IDを設定してください。".to_string());
    }
    let token = google_access_token(settings).await?;
    let phrases = transcription_contextual_phrases(entries, screen_context, 1000);
    // chirp_3 は最大1,000フレーズの適応辞書に対応。高い boost は false positive も
    // 増やすため、既定は中程度に留める。
    let phrase_values = phrases
        .iter()
        .take(1000)
        .map(|value| {
            serde_json::json!({
                "value": value,
                "boost": GOOGLE_SPEECH_DICTIONARY_BOOST,
            })
        })
        .collect::<Vec<_>>();
    let mut config = serde_json::json!({
        "autoDecodingConfig": {},
        "languageCodes": ["ja-JP"],
        "model": "chirp_3",
        "features": {
            "enableAutomaticPunctuation": true
        }
    });
    if !phrase_values.is_empty() {
        config["adaptation"] = serde_json::json!({
            "phraseSets": [{
                "inlinePhraseSet": {
                    "phrases": phrase_values
                }
            }]
        });
    }
    let body = serde_json::json!({
        "config": config,
        "content": base64::engine::general_purpose::STANDARD.encode(&clip.wav)
    });
    let region = settings.voice.google_cloud_region.trim();
    let url = format!(
        "https://{region}-speech.googleapis.com/v2/projects/{project}/locations/{region}/recognizers/_:recognize"
    );
    let response = http_client(SPEECH_REQUEST_TIMEOUT)?
        .post(url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| speech_request_error("Google Speech-to-Text", e))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Google Speech-to-Text HTTP {status}: {text}"));
    }
    if let Err(err) = usage::record_google_speech_to_text(app, clip.duration_secs) {
        eprintln!("[enja] usage tracking failed: {err}");
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let out = v
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|result| {
            result
                .get("alternatives")
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
                .and_then(|a| a.get("transcript"))
                .and_then(|t| t.as_str())
        })
        .collect::<Vec<_>>()
        .join("\n");
    if out.trim().is_empty() {
        Err("文字起こし結果が空でした。".to_string())
    } else {
        Ok(out)
    }
}

async fn google_access_token(settings: &AppSettings) -> Result<String, String> {
    google_access_token_with_details(settings)
        .await
        .map(|(token, _details)| token)
}

async fn google_access_token_with_details(
    settings: &AppSettings,
) -> Result<(String, Vec<String>), String> {
    if settings.voice.google_cloud_use_adc {
        let cache_key = "adc".to_string();
        if let Some(cached) = cache::cached_google_token(&cache_key) {
            return Ok(cached);
        }
        let gcloud = resolve_gcloud_path()?;
        let mut command = std::process::Command::new(&gcloud);
        command.args(["auth", "application-default", "print-access-token"]);
        let output = command_output_with_timeout(
            command,
            TOKEN_REQUEST_TIMEOUT,
            &format!("gcloud（path: {}）", gcloud.display()),
        )?;
        if output.status.success() {
            let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !token.is_empty() {
                let details = vec![
                    "認証方式: ADC".to_string(),
                    format!("gcloud: {}", gcloud.display()),
                ];
                return Ok(cache::store_google_token(cache_key, token, details));
            }
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!(
                "gcloudからアクセストークンが返りませんでした。ターミナルで `{} auth application-default login` を実行してください。",
                gcloud.display()
            )
        } else {
            stderr
        });
    }

    #[derive(Deserialize)]
    struct ServiceAccount {
        client_email: String,
        private_key: String,
        token_uri: String,
    }
    #[derive(Serialize)]
    struct Claims<'a> {
        iss: &'a str,
        scope: &'a str,
        aud: &'a str,
        exp: usize,
        iat: usize,
    }
    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let secret = secrets::get_secret("googleServiceAccount")
        .map_err(|_| "Google CloudサービスアカウントJSONを保存してください。".to_string())?;
    let cache_key = format!("service:{}", cache::hash_cache_key(&secret));
    if let Some(cached) = cache::cached_google_token(&cache_key) {
        return Ok(cached);
    }
    let account: ServiceAccount = serde_json::from_str(&secret).map_err(|e| e.to_string())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as usize;
    let claims = Claims {
        iss: &account.client_email,
        scope: "https://www.googleapis.com/auth/cloud-platform",
        aud: &account.token_uri,
        exp: now + 3600,
        iat: now,
    };
    let assertion = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_rsa_pem(account.private_key.as_bytes())
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let response = http_client(TOKEN_REQUEST_TIMEOUT)?
        .post(account.token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", assertion.as_str()),
        ])
        .send()
        .await
        .map_err(|e| speech_request_error("Google OAuth", e))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Google OAuth HTTP {status}: {text}"));
    }
    let token: TokenResponse = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(cache::store_google_token(
        cache_key,
        token.access_token,
        vec!["認証方式: サービスアカウントJSON".to_string()],
    ))
}

fn command_output_with_timeout(
    mut command: std::process::Command,
    timeout: Duration,
    label: &str,
) -> Result<std::process::Output, String> {
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("{label}を実行できませんでした: {e}"))?;
    let start = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|e| format!("{label}の出力を取得できませんでした: {e}"));
            }
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{label}がタイムアウトしました。"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("{label}の終了状態を確認できませんでした: {e}")),
        }
    }
}

fn resolve_gcloud_path() -> Result<PathBuf, String> {
    let mut searched = Vec::<String>::new();
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let path = dir.join("gcloud");
            searched.push(path.display().to_string());
            if path.exists() {
                return Ok(path);
            }
        }
    }

    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/gcloud"),
        PathBuf::from("/usr/local/bin/gcloud"),
        PathBuf::from("/opt/google-cloud-sdk/bin/gcloud"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join("google-cloud-sdk/bin/gcloud"));
        candidates.push(home.join("Downloads/google-cloud-sdk/bin/gcloud"));
    }
    if let Some(root) = std::env::var_os("CLOUDSDK_ROOT_DIR") {
        candidates.push(PathBuf::from(root).join("bin/gcloud"));
    }

    for path in candidates {
        searched.push(path.display().to_string());
        if path.exists() {
            return Ok(path);
        }
    }

    let mut command = std::process::Command::new("/bin/zsh");
    command.args(["-lc", "command -v gcloud"]);
    if let Ok(output) = command_output_with_timeout(command, Duration::from_secs(3), "gcloud検索")
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                let path = PathBuf::from(path);
                searched.push(path.display().to_string());
                if path.exists() {
                    return Ok(path);
                }
            }
        }
    }

    Err(format!(
        "gcloudが見つかりません。ターミナルではログイン済みでも、Spotlight/Dockから起動したEnjaではPATHが異なることがあります。Google Cloud SDKをHomebrewなど通常の場所に入れるか、ADCをオフにしてサービスアカウントJSONを保存してください。探した場所: {}",
        searched.join(", ")
    ))
}

async fn transcribe_openai(
    app: &tauri::AppHandle,
    model: &str,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    let key = secrets::get_secret("openai")
        .map_err(|_| "OpenAI APIキーを保存してください。".to_string())?;
    let dictionary_context = transcription_prompt_context(entries, screen_context);
    let prompt =
        prompts::openai_transcription_prompt(&settings.prompts.overrides, &dictionary_context);
    let file = reqwest::multipart::Part::bytes(clip.wav.clone())
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new()
        .part("file", file)
        .text("model", model.to_string())
        .text("language", "ja")
        .text("response_format", "json")
        .text("prompt", prompt);
    let response = http_client(SPEECH_REQUEST_TIMEOUT)?
        .post("https://api.openai.com/v1/audio/transcriptions")
        .bearer_auth(key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| speech_request_error("OpenAI", e))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("OpenAI HTTP {status}: {text}"));
    }
    if let Err(err) = usage::record_openai_transcription(app, model, clip.duration_secs) {
        eprintln!("[enja] usage tracking failed: {err}");
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let out = v
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if out.is_empty() {
        Err("OpenAIの文字起こし結果が空でした。".to_string())
    } else {
        Ok(out)
    }
}

async fn transcribe_gemini_audio(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    let key = gemini_api_key(app)?;
    let dictionary_context = transcription_prompt_context(entries, screen_context);
    let prompt = prompts::gemini_audio_user(&settings.prompts.overrides, &dictionary_context);
    let system = prompts::gemini_audio_system(&settings.prompts.overrides);
    let model = settings.voice.finalization_model.model_id();
    let output = gemini::generate_from_audio_with_usage(
        &key,
        model,
        settings.voice.finalization_model.thinking_level(),
        system.as_ref(),
        &prompt,
        &clip.wav,
        0.1,
    )
    .await?;
    if let Err(err) =
        usage::record_gemini_usage(app, UsageService::GeminiAudioInput, model, output.usage)
    {
        eprintln!("[enja] usage tracking failed: {err}");
    }
    Ok(output.text)
}

async fn finalize_text(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    mode: VoiceMode,
    mode_profile_id: &str,
    selected_text: &str,
    screen_context: &VoiceScreenContext,
    transcript: &str,
) -> Result<String, String> {
    let dictation_profile = if mode == VoiceMode::Dictation {
        Some(
            settings
                .voice
                .mode_profile_or_default(mode_profile_id)
                .ok_or_else(|| "音声モードが見つかりません。".to_string())?,
        )
    } else {
        None
    };
    if dictation_profile.is_some_and(|profile| !profile.formatting_enabled) {
        return Ok(dictionary::apply_transcript_corrections(
            transcript.trim(),
            entries,
        ));
    }

    let key = gemini_api_key(app)?;
    let dictionary_context = dictionary::prompt_lines(entries);
    let dictionary_section = if dictionary_context.trim().is_empty() {
        "優先表記辞書は空です。".to_string()
    } else {
        format!("優先表記辞書（該当語だと判断できる場合のみ使用）:\n{dictionary_context}")
    };
    let screen_context_section = finalization_screen_context_section(screen_context);
    let (system, user) = match mode {
        VoiceMode::Dictation => {
            let profile = dictation_profile.expect("dictation profile");
            (
                profile.system_prompt.clone(),
                prompts::voice_mode_user_with_context(
                    &profile.user_prompt,
                    &dictionary_section,
                    &screen_context_section,
                    transcript,
                ),
            )
        }
        VoiceMode::Ask if selected_text.trim().is_empty() => (
            prompts::ask_without_selection_system(&settings.prompts.overrides).to_string(),
            prompts::ask_without_selection_user(
                &settings.prompts.overrides,
                &dictionary_section,
                &screen_context_section,
                transcript,
            ),
        ),
        VoiceMode::Ask => (
            prompts::ask_with_selection_system(&settings.prompts.overrides).to_string(),
            prompts::ask_with_selection_user(
                &settings.prompts.overrides,
                &dictionary_section,
                &screen_context_section,
                selected_text,
                transcript,
            ),
        ),
    };
    let model = settings.voice.finalization_model.model_id();
    let output = gemini::generate_text_with_usage(
        &key,
        model,
        settings.voice.finalization_model.thinking_level(),
        &system,
        &user,
        0.2,
    )
    .await?;
    if let Err(err) =
        usage::record_gemini_usage(app, UsageService::GeminiFinalization, model, output.usage)
    {
        eprintln!("[enja] usage tracking failed: {err}");
    }
    Ok(output.text.trim().to_string())
}

fn gemini_api_key(_app: &tauri::AppHandle) -> Result<String, String> {
    if let Ok(key) = secrets::get_secret("gemini") {
        if !key.trim().is_empty() {
            return Ok(key);
        }
    }
    Err("Gemini APIキーを保存してください。".to_string())
}

fn snapshot_from_profile(profile: &VoiceModeProfile) -> VoiceModeProfileSnapshot {
    VoiceModeProfileSnapshot {
        id: profile.id.clone(),
        name: profile.name.clone(),
        formatting_enabled: profile.formatting_enabled,
    }
}

fn profile_snapshot_for_id(
    app: &tauri::AppHandle,
    mode_profile_id: &str,
) -> Option<VoiceModeProfileSnapshot> {
    app.try_state::<SettingsStore>().and_then(|store| {
        let settings = store.get();
        settings
            .voice
            .mode_profile_or_default(mode_profile_id)
            .map(snapshot_from_profile)
    })
}

fn profile_snapshot_for_mode(
    app: &tauri::AppHandle,
    mode: VoiceMode,
    mode_profile_id: &str,
) -> Option<VoiceModeProfileSnapshot> {
    if mode == VoiceMode::Dictation {
        profile_snapshot_for_id(app, mode_profile_id)
    } else {
        None
    }
}

fn emit_state(
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

fn state_profile_fields(
    mode_profile: Option<VoiceModeProfileSnapshot>,
) -> (Option<String>, Option<String>) {
    match mode_profile {
        Some(profile) => (Some(profile.id), Some(profile.name)),
        None => (None, None),
    }
}

fn emit_result(app: &tauri::AppHandle, event: VoiceResultEvent) {
    let _ = app.emit("voice-result", event.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for delay_ms in [120_u64, 360] {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            let _ = app.emit("voice-result", event.clone());
        }
    });
}

fn show_dictionary_learning_notice(app: &tauri::AppHandle, learned: dictionary::LearnedCorrection) {
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

pub fn show_shortcut_cheat_sheet(app: &tauri::AppHandle) {
    show_voice_window_with_layout(app, VoiceWindowLayout::CheatSheet);
    emit_state(app, "cheatSheet", None, None, None);
}

pub fn hide_shortcut_cheat_sheet(app: &tauri::AppHandle) {
    emit_state(app, "idle", None, None, None);
    hide_voice_window(app);
}

fn next_voice_state_seq() -> u64 {
    VOICE_STATE_SEQ.fetch_add(1, Ordering::SeqCst)
}

fn show_voice_window(app: &tauri::AppHandle, expanded: bool) {
    let layout = if expanded {
        VoiceWindowLayout::Expanded
    } else {
        VoiceWindowLayout::Compact
    };
    show_voice_window_with_layout(app, layout);
}

fn show_voice_notice_window(app: &tauri::AppHandle) {
    show_voice_window_with_layout(app, VoiceWindowLayout::Notice);
}

fn show_voice_window_with_layout(app: &tauri::AppHandle, layout: VoiceWindowLayout) {
    let Some(window) = app.get_webview_window("voice") else {
        crate::keyboard::set_voice_overlay_visible(false);
        return;
    };
    let monitor_key = configure_voice_window(app, &window, layout);
    let _ = window.set_always_on_top(true);
    if window.show().is_ok() {
        crate::keyboard::set_voice_overlay_visible(true);
    }
    start_voice_window_follow(app, layout, monitor_key);
}

fn configure_voice_window(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    layout: VoiceWindowLayout,
) -> Option<VoiceWindowMonitorKey> {
    let target_monitor = voice_window_target_monitor(app);
    let monitor_key = target_monitor.as_ref().map(voice_window_monitor_key);
    let scale = target_monitor
        .as_ref()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or_else(|| window.scale_factor().unwrap_or(1.0))
        .max(1.0);
    let (mut width, mut height) = layout.dimensions();
    if let Some(monitor) = target_monitor.as_ref() {
        let size = monitor.size();
        let logical_width = size.width as f64 / scale;
        let logical_height = size.height as f64 / scale;
        width = width.min((logical_width - 40.0).max(260.0));
        height = height.min((logical_height - 88.0).max(layout.min_height()));
    }
    let _ = window.set_focusable(layout.focusable());
    let _ = window.set_shadow(layout.focusable());
    let _ = window.set_size(tauri::LogicalSize::new(width, height));
    if let Some(monitor) = target_monitor.as_ref() {
        let pos = monitor.position();
        let size = monitor.size();
        let screen_pos = pos.to_logical::<f64>(scale);
        let screen_size = size.to_logical::<f64>(scale);
        let screen_x = screen_pos.x;
        let screen_y = screen_pos.y;
        let screen_width = screen_size.width;
        let screen_height = screen_size.height;
        let edge_margin = VOICE_WINDOW_EDGE_MARGIN;
        let bottom_margin = VOICE_WINDOW_BOTTOM_MARGIN;
        let window_width = width;
        let window_height = height;
        let min_x = screen_x + edge_margin;
        let max_x = (screen_x + screen_width - window_width - edge_margin).max(min_x);
        let x = (screen_x + (screen_width - window_width) / 2.0).clamp(min_x, max_x);
        let min_y = screen_y + edge_margin;
        let max_y = (screen_y + screen_height - window_height - edge_margin).max(min_y);
        let y = (screen_y + screen_height - window_height - bottom_margin).clamp(min_y, max_y);
        let _ = window.set_position(tauri::LogicalPosition::new(x, y));
    }

    monitor_key
}

fn start_voice_window_follow(
    app: &tauri::AppHandle,
    layout: VoiceWindowLayout,
    monitor_key: Option<VoiceWindowMonitorKey>,
) {
    let token = VOICE_WINDOW_FOLLOW_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut current_monitor = monitor_key;
        loop {
            tokio::time::sleep(Duration::from_millis(VOICE_WINDOW_FOLLOW_INTERVAL_MS)).await;
            if VOICE_WINDOW_FOLLOW_SEQ.load(Ordering::SeqCst) != token {
                return;
            }

            let Some(window) = app.get_webview_window("voice") else {
                return;
            };
            if !window.is_visible().unwrap_or(false) {
                return;
            }

            let next_monitor = voice_window_target_monitor(&app)
                .as_ref()
                .map(voice_window_monitor_key);
            if next_monitor != current_monitor {
                current_monitor = configure_voice_window(&app, &window, layout);
            }
        }
    });
}

fn stop_voice_window_follow() {
    VOICE_WINDOW_FOLLOW_SEQ.fetch_add(1, Ordering::SeqCst);
}

fn voice_window_target_monitor(app: &tauri::AppHandle) -> Option<tauri::window::Monitor> {
    if let Ok(cursor) = app.cursor_position() {
        if let Ok(Some(monitor)) = app.monitor_from_point(cursor.x, cursor.y) {
            return Some(monitor);
        }

        if let Ok(monitors) = app.available_monitors() {
            if let Some(monitor) = monitors
                .into_iter()
                .find(|monitor| monitor_contains_physical_point(monitor, cursor.x, cursor.y))
            {
                return Some(monitor);
            }
        }
    }

    app.primary_monitor().ok().flatten()
}

fn voice_window_monitor_key(monitor: &tauri::window::Monitor) -> VoiceWindowMonitorKey {
    let pos = monitor.position();
    let size = monitor.size();
    VoiceWindowMonitorKey {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
        scale_bits: monitor.scale_factor().to_bits(),
    }
}

fn monitor_contains_physical_point(monitor: &tauri::window::Monitor, x: f64, y: f64) -> bool {
    let pos = monitor.position();
    let size = monitor.size();
    let left = pos.x as f64;
    let top = pos.y as f64;
    x >= left && x < left + size.width as f64 && y >= top && y < top + size.height as f64
}

fn hide_voice_window(app: &tauri::AppHandle) {
    stop_voice_window_follow();
    crate::keyboard::set_voice_overlay_visible(false);
    if let Some(window) = app.get_webview_window("voice") {
        let _ = window.hide();
    }
}

fn hide_voice_window_after(app: tauri::AppHandle, delay: Duration) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        hide_voice_window(&app);
    });
}

fn play_interaction_sound(kind: &str) {
    #[cfg(target_os = "macos")]
    {
        let sound = if kind == "start" { "Pop" } else { "Tink" };
        let path = format!("/System/Library/Sounds/{sound}.aiff");
        let _ = std::process::Command::new("afplay").arg(path).spawn();
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = kind;
    }
}

#[cfg(target_os = "macos")]
fn current_output_audio_snapshot() -> OutputAudioSnapshot {
    OutputAudioSnapshot {
        volume: read_output_volume(),
        muted: read_output_muted(),
    }
}

#[cfg(not(target_os = "macos"))]
fn current_output_audio_snapshot() -> OutputAudioSnapshot {
    OutputAudioSnapshot
}

#[cfg(target_os = "macos")]
fn read_osascript_value(script: &str) -> Option<String> {
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn read_output_volume() -> Option<u8> {
    read_osascript_value("output volume of (get volume settings)")?
        .parse()
        .ok()
}

#[cfg(target_os = "macos")]
fn read_output_muted() -> Option<bool> {
    match read_osascript_value("output muted of (get volume settings)")?
        .to_ascii_lowercase()
        .as_str()
    {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn mute_system_output() {
    let _ = std::process::Command::new("osascript")
        .args([
            "-e",
            "set volume with output muted",
            "-e",
            "set volume output volume 0",
            "-e",
            "set volume with output muted",
        ])
        .output();
}

#[cfg(not(target_os = "macos"))]
fn mute_system_output() {}

#[cfg(target_os = "macos")]
fn restore_system_output(snapshot: OutputAudioSnapshot) {
    if let Some(volume) = snapshot.volume {
        set_output_volume(volume);
    }
    if let Some(muted) = snapshot.muted {
        set_output_muted(muted);
    }
}

#[cfg(not(target_os = "macos"))]
fn restore_system_output(_snapshot: OutputAudioSnapshot) {}

#[cfg(target_os = "macos")]
fn set_output_muted(muted: bool) {
    let script = if muted {
        "set volume with output muted"
    } else {
        "set volume without output muted"
    };
    let _ = std::process::Command::new("osascript")
        .args(["-e", script])
        .output();
}

#[cfg(target_os = "macos")]
fn set_output_volume(volume: u8) {
    let script = format!("set volume output volume {}", volume.min(100));
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output();
}

#[cfg(not(target_os = "macos"))]
fn set_output_volume(_volume: u8) {}

#[cfg(target_os = "macos")]
fn capture_paste_target() -> Option<PasteTargetInfo> {
    current_paste_target_info()
}

#[cfg(not(target_os = "macos"))]
fn capture_paste_target() -> Option<PasteTargetInfo> {
    None
}

#[cfg(target_os = "macos")]
fn capture_selected_text() -> String {
    if let Some(selected) = read_accessibility_selected_text() {
        return selected;
    }

    let original = read_clipboard_text();
    let sentinel = new_clipboard_sentinel();
    if !write_clipboard_text(&sentinel) {
        return String::new();
    }

    if !run_keystroke("c") {
        restore_clipboard(original);
        return String::new();
    }

    let selected =
        wait_for_copied_selection(&sentinel, Duration::from_millis(700)).unwrap_or_default();
    restore_clipboard(original);
    selected
}

#[cfg(not(target_os = "macos"))]
fn capture_selected_text() -> String {
    String::new()
}

#[cfg(target_os = "macos")]
fn read_accessibility_selected_text() -> Option<String> {
    let script = r#"
tell application "System Events"
  try
    set frontApp to first application process whose frontmost is true
    set focusedElement to value of attribute "AXFocusedUIElement" of frontApp
    set selectedText to value of attribute "AXSelectedText" of focusedElement
    if selectedText is missing value then return ""
    return selectedText as text
  on error
    return ""
  end try
end tell
"#;
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(target_os = "macos")]
fn new_clipboard_sentinel() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("__ENJA_SELECTED_TEXT_SENTINEL_{nanos}__")
}

#[cfg(target_os = "macos")]
fn wait_for_copied_selection(sentinel: &str, timeout: Duration) -> Option<String> {
    let start = Instant::now();
    loop {
        if let Some(value) = read_clipboard_text() {
            if value != sentinel {
                return Some(value);
            }
        }
        if start.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "macos")]
type AXUIElementRef = *const c_void;
#[cfg(target_os = "macos")]
type AXValueRef = *const c_void;
#[cfg(target_os = "macos")]
type AXError = c_int;
#[cfg(target_os = "macos")]
type Boolean = u8;

#[cfg(target_os = "macos")]
const KAX_ERROR_SUCCESS: AXError = 0;
#[cfg(target_os = "macos")]
const KAX_VALUE_CF_RANGE_TYPE: c_int = 4;

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct AxCfRange {
    location: isize,
    length: isize,
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: c_int) -> AXUIElementRef;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementCopyAttributeNames(element: AXUIElementRef, names: *mut CFArrayRef) -> AXError;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut c_int) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXValueGetType(value: AXValueRef) -> c_int;
    fn AXValueGetValue(value: AXValueRef, value_type: c_int, value_ptr: *mut c_void) -> Boolean;
}

#[cfg(target_os = "macos")]
struct AxElementRef {
    raw: AXUIElementRef,
}

#[cfg(target_os = "macos")]
unsafe impl Send for AxElementRef {}

#[cfg(target_os = "macos")]
impl Drop for AxElementRef {
    fn drop(&mut self) {
        unsafe {
            if !self.raw.is_null() {
                CFRelease(self.raw.cast());
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
struct AxTextSnapshot {
    pid: c_int,
    value: String,
    selected_range: TextRange,
}

#[cfg(target_os = "macos")]
struct AxFocusedElement {
    element: AxElementRef,
}

#[cfg(target_os = "macos")]
impl AxFocusedElement {
    fn capture() -> Option<Self> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return None;
            }
            let focused =
                copy_ax_attribute_raw(system, "AXFocusedUIElement").map(|raw| AxElementRef {
                    raw: raw as AXUIElementRef,
                });
            CFRelease(system.cast());

            focused.map(|element| Self { element })
        }
    }

    fn read_paste_target_info(&self) -> Option<PasteTargetInfo> {
        let pid = self.element.pid()?;
        Some(PasteTargetInfo {
            pid: Some(pid),
            role: copy_ax_string_attribute(self.element.raw, "AXRole").unwrap_or_default(),
            subrole: copy_ax_string_attribute(self.element.raw, "AXSubrole").unwrap_or_default(),
            attributes: copy_ax_attribute_names(self.element.raw).unwrap_or_default(),
        })
    }
}

#[cfg(target_os = "macos")]
struct AxFocusedText {
    element: AxElementRef,
    snapshot: AxTextSnapshot,
}

#[cfg(target_os = "macos")]
struct VerifiedPaste {
    target: AxFocusedText,
    after_paste: AxTextSnapshot,
    insertion: VerifiedPasteInsertion,
}

#[cfg(target_os = "macos")]
enum VerifiedPasteInsertion {
    Changed(TextRange),
    SameTextReplacement,
}

#[cfg(target_os = "macos")]
impl AxFocusedText {
    fn capture() -> Option<Self> {
        let focused = AxFocusedElement::capture()?;
        let snapshot = focused.element.read_text_snapshot()?;
        Some(Self {
            element: focused.element,
            snapshot,
        })
    }

    fn capture_for_paste_target(target: &PasteTargetInfo) -> Option<Self> {
        let focused = Self::capture()?;
        if target.pid.is_some_and(|pid| focused.snapshot.pid != pid) {
            return None;
        }
        Some(focused)
    }
}

#[cfg(target_os = "macos")]
impl AxElementRef {
    fn pid(&self) -> Option<c_int> {
        if self.raw.is_null() {
            return None;
        }
        let mut pid: c_int = 0;
        unsafe {
            if AXUIElementGetPid(self.raw, &mut pid) != KAX_ERROR_SUCCESS {
                return None;
            }
        }
        Some(pid)
    }

    fn read_text_snapshot(&self) -> Option<AxTextSnapshot> {
        let pid = self.pid()?;
        let raw_value = copy_ax_string_attribute(self.raw, "AXValue")?;
        let placeholder = copy_ax_string_attribute(self.raw, "AXPlaceholderValue");
        let value = value_without_placeholder(raw_value, placeholder.as_deref());
        let selected_range = copy_ax_range_attribute(self.raw, "AXSelectedTextRange")?;
        Some(AxTextSnapshot {
            pid,
            value,
            selected_range,
        })
    }
}

#[cfg(target_os = "macos")]
fn copy_ax_attribute_raw(element: AXUIElementRef, attribute: &str) -> Option<CFTypeRef> {
    let attribute = CFString::new(attribute);
    let mut value: CFTypeRef = std::ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value)
    };
    if status == KAX_ERROR_SUCCESS && !value.is_null() {
        Some(value)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn copy_ax_string_attribute(element: AXUIElementRef, attribute: &str) -> Option<String> {
    let value = copy_ax_attribute_raw(element, attribute)?;
    unsafe {
        if CFGetTypeID(value) != CFStringGetTypeID() {
            CFRelease(value);
            return None;
        }
        let text = CFString::wrap_under_create_rule(value as CFStringRef).to_string();
        Some(text)
    }
}

#[cfg(target_os = "macos")]
fn copy_ax_range_attribute(element: AXUIElementRef, attribute: &str) -> Option<TextRange> {
    let value = copy_ax_attribute_raw(element, attribute)?;
    unsafe {
        if AXValueGetType(value as AXValueRef) != KAX_VALUE_CF_RANGE_TYPE {
            CFRelease(value);
            return None;
        }
        let mut range = AxCfRange {
            location: 0,
            length: 0,
        };
        let ok = AXValueGetValue(
            value as AXValueRef,
            KAX_VALUE_CF_RANGE_TYPE,
            &mut range as *mut _ as *mut c_void,
        ) != 0;
        CFRelease(value);
        if !ok || range.location < 0 || range.length < 0 {
            return None;
        }
        Some(TextRange {
            location: range.location as usize,
            length: range.length as usize,
        })
    }
}

#[cfg(target_os = "macos")]
fn copy_ax_attribute_names(element: AXUIElementRef) -> Option<HashSet<String>> {
    let mut names: CFArrayRef = std::ptr::null();
    let status = unsafe { AXUIElementCopyAttributeNames(element, &mut names) };
    if status != KAX_ERROR_SUCCESS || names.is_null() {
        return None;
    }

    let mut out = HashSet::new();
    unsafe {
        let count = CFArrayGetCount(names);
        for index in 0..count {
            let value = CFArrayGetValueAtIndex(names, index);
            if !value.is_null() && CFGetTypeID(value.cast()) == CFStringGetTypeID() {
                let text = CFString::wrap_under_get_rule(value as CFStringRef).to_string();
                out.insert(text);
            }
        }
        CFRelease(names.cast());
    }

    Some(out)
}

#[cfg(target_os = "macos")]
fn paste_text_with_dictionary_learning(
    app: &tauri::AppHandle,
    text: &str,
    preferred_target: Option<&PasteTargetInfo>,
) -> bool {
    let Some(paste) = perform_verified_clipboard_paste(text, preferred_target) else {
        return false;
    };

    if let VerifiedPasteInsertion::Changed(inserted_range) = paste.insertion {
        start_dictionary_learning_watch(
            app.clone(),
            paste.target,
            paste.after_paste,
            inserted_range,
        );
    }
    true
}

#[cfg(not(target_os = "macos"))]
fn paste_text_with_dictionary_learning(
    _app: &tauri::AppHandle,
    text: &str,
    _preferred_target: Option<&PasteTargetInfo>,
) -> bool {
    paste_text(text, None)
}

#[cfg(target_os = "macos")]
fn start_dictionary_learning_watch(
    app: tauri::AppHandle,
    target: AxFocusedText,
    after_paste: AxTextSnapshot,
    inserted_range: TextRange,
) {
    std::thread::spawn(move || {
        let baseline = after_paste;
        let mut quiescence = DictionaryLearningQuiescence::new(&baseline.value);
        loop {
            std::thread::sleep(Duration::from_millis(DICTIONARY_LEARNING_POLL_INTERVAL_MS));
            let Some(current) = target.element.read_text_snapshot() else {
                return;
            };
            if current.pid != baseline.pid {
                return;
            }
            match advance_dictionary_learning_quiescence(
                &mut quiescence,
                &current.value,
                DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                DICTIONARY_LEARNING_QUIET_MS,
                DICTIONARY_LEARNING_MAX_WATCH_MS,
            ) {
                DictionaryLearningQuiescenceStep::Continue => {}
                DictionaryLearningQuiescenceStep::Expired => return,
                DictionaryLearningQuiescenceStep::Ready => {
                    let Some((from, to)) = learned_correction_from_values(
                        &baseline.value,
                        &current.value,
                        inserted_range,
                    ) else {
                        continue;
                    };
                    match dictionary::upsert_learned_correction(&app, &from, &to) {
                        Ok(Some(learned)) => {
                            show_dictionary_learning_notice(&app, learned);
                        }
                        Ok(None) => {}
                        Err(err) => eprintln!("[enja] dictionary learning failed: {err}"),
                    }
                    return;
                }
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn inserted_range_from_snapshots(
    before: &AxTextSnapshot,
    after: &AxTextSnapshot,
) -> Option<TextRange> {
    let span = changed_span(&before.value, &after.value)?;
    if span.to.is_empty() {
        return None;
    }
    let selection = before.selected_range;
    let changed_replaced_selection =
        span.old_range.overlaps(selection) || span.old_range.location == selection.location;
    if !changed_replaced_selection {
        return None;
    }
    Some(span.new_range)
}

fn learned_correction_from_values(
    baseline: &str,
    current: &str,
    inserted_range: TextRange,
) -> Option<(String, String)> {
    let span = changed_span(baseline, current)?;
    if !span.old_range.overlaps(inserted_range) {
        return None;
    }
    let from = span.from.trim().to_string();
    let to = span.to.trim().to_string();
    if from.is_empty() || to.is_empty() || from == to {
        return None;
    }
    if !is_learnable_correction(&from, &to, span.old_range, inserted_range) {
        return None;
    }
    Some((from, to))
}

fn value_without_placeholder(value: String, placeholder: Option<&str>) -> String {
    let Some(placeholder) = placeholder else {
        return value;
    };
    if !placeholder.trim().is_empty() && value.trim() == placeholder.trim() {
        String::new()
    } else {
        value
    }
}

fn is_learnable_correction(
    from: &str,
    to: &str,
    changed_range: TextRange,
    inserted_range: TextRange,
) -> bool {
    let from_chars = from.chars().count();
    let to_chars = to.chars().count();
    if from_chars < MIN_LEARNED_CORRECTION_CHARS || to_chars < MIN_LEARNED_CORRECTION_CHARS {
        return false;
    }
    if from_chars > MAX_LEARNED_CORRECTION_CHARS || to_chars > MAX_LEARNED_CORRECTION_CHARS {
        return false;
    }
    if from.is_ascii() && to.is_ascii() && from.eq_ignore_ascii_case(to) {
        return false;
    }
    if is_sentence_like_correction_value(from) || is_sentence_like_correction_value(to) {
        return false;
    }
    if covers_most_inserted_range(changed_range, inserted_range)
        && (from_chars >= MIN_FULL_INSERT_REWRITE_CHARS
            || to_chars >= MIN_FULL_INSERT_REWRITE_CHARS)
    {
        return false;
    }
    true
}

fn is_sentence_like_correction_value(value: &str) -> bool {
    let value = value.trim();
    let char_count = value.chars().count();
    if value.chars().any(is_sentence_punctuation) {
        return true;
    }
    if value.split_whitespace().count() > 3 {
        return true;
    }
    if char_count >= 6
        && [
            "です",
            "ます",
            "でした",
            "ました",
            "ですね",
            "ですよ",
            "でしょう",
            "ください",
            "ません",
            "だよ",
            "だね",
        ]
        .iter()
        .any(|ending| value.ends_with(ending))
    {
        return true;
    }
    if char_count >= 6
        && value.chars().any(|ch| matches!(ch, 'を' | 'が' | 'は'))
        && value
            .chars()
            .last()
            .is_some_and(is_japanese_predicate_ending)
    {
        return true;
    }
    false
}

fn is_sentence_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '。' | '、' | '，' | ',' | '！' | '!' | '？' | '?' | '；' | ';' | '：' | ':' | '\n' | '\r'
    )
}

fn is_japanese_predicate_ending(ch: char) -> bool {
    matches!(
        ch,
        'う' | 'く' | 'ぐ' | 'す' | 'つ' | 'ぬ' | 'ぶ' | 'む' | 'る' | 'た' | 'だ' | 'い'
    )
}

fn covers_most_inserted_range(changed_range: TextRange, inserted_range: TextRange) -> bool {
    if inserted_range.length == 0 {
        return false;
    }
    let overlap_start = changed_range.location.max(inserted_range.location);
    let overlap_end = changed_range.end().min(inserted_range.end());
    if overlap_end <= overlap_start {
        return false;
    }
    let overlap = overlap_end - overlap_start;
    overlap * 100 >= inserted_range.length * 80
}

fn changed_span(before: &str, after: &str) -> Option<ChangedSpan> {
    if before == after {
        return None;
    }

    let before_chars = before.chars().collect::<Vec<_>>();
    let after_chars = after.chars().collect::<Vec<_>>();
    let mut prefix = 0usize;
    while prefix < before_chars.len()
        && prefix < after_chars.len()
        && before_chars[prefix] == after_chars[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while suffix + prefix < before_chars.len()
        && suffix + prefix < after_chars.len()
        && before_chars[before_chars.len() - 1 - suffix]
            == after_chars[after_chars.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let before_end = before_chars.len().saturating_sub(suffix);
    let after_end = after_chars.len().saturating_sub(suffix);
    let from = before_chars[prefix..before_end].iter().collect::<String>();
    let to = after_chars[prefix..after_end].iter().collect::<String>();
    let prefix_utf16 = utf16_len_chars(&before_chars[..prefix]);
    Some(ChangedSpan {
        old_range: TextRange {
            location: prefix_utf16,
            length: utf16_len(&from),
        },
        new_range: TextRange {
            location: prefix_utf16,
            length: utf16_len(&to),
        },
        from,
        to,
    })
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn utf16_len_chars(chars: &[char]) -> usize {
    chars.iter().map(|ch| ch.len_utf16()).sum()
}

fn utf16_range_text(value: &str, range: TextRange) -> Option<String> {
    let start = utf16_offset_to_byte_index(value, range.location)?;
    let end = utf16_offset_to_byte_index(value, range.end())?;
    if start > end {
        return None;
    }
    Some(value[start..end].to_string())
}

fn utf16_offset_to_byte_index(value: &str, offset: usize) -> Option<usize> {
    let mut utf16_offset = 0usize;
    for (byte_index, ch) in value.char_indices() {
        if utf16_offset == offset {
            return Some(byte_index);
        }
        utf16_offset = utf16_offset.saturating_add(ch.len_utf16());
        if utf16_offset > offset {
            return None;
        }
    }
    if utf16_offset == offset {
        Some(value.len())
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn paste_text(text: &str, preferred_target: Option<&PasteTargetInfo>) -> bool {
    perform_verified_clipboard_paste(text, preferred_target).is_some()
}

#[cfg(target_os = "macos")]
fn perform_verified_clipboard_paste(
    text: &str,
    preferred_target: Option<&PasteTargetInfo>,
) -> Option<VerifiedPaste> {
    let target = resolve_paste_target_info(preferred_target)?;
    // If macOS cannot expose the target text, fall back to manual copy instead
    // of treating a posted Cmd+V event as a successful insertion.
    let focused = AxFocusedText::capture_for_paste_target(&target)?;
    if !perform_clipboard_paste(text) {
        return None;
    }
    let after_paste = focused.element.read_text_snapshot()?;
    let insertion = verify_paste_insertion(&focused.snapshot, &after_paste, text)?;
    Some(VerifiedPaste {
        target: focused,
        after_paste,
        insertion,
    })
}

#[cfg(target_os = "macos")]
fn verify_paste_insertion(
    before: &AxTextSnapshot,
    after: &AxTextSnapshot,
    text: &str,
) -> Option<VerifiedPasteInsertion> {
    if after.pid != before.pid {
        return None;
    }
    if let Some(range) = inserted_range_from_snapshots(before, after) {
        return Some(VerifiedPasteInsertion::Changed(range));
    }
    if unchanged_selection_replacement_matches_text(before, after, text) {
        return Some(VerifiedPasteInsertion::SameTextReplacement);
    }
    None
}

#[cfg(target_os = "macos")]
fn unchanged_selection_replacement_matches_text(
    before: &AxTextSnapshot,
    after: &AxTextSnapshot,
    text: &str,
) -> bool {
    if text.is_empty() || before.value != after.value || before.selected_range.length == 0 {
        return false;
    }
    if before.selected_range == after.selected_range {
        return false;
    }
    utf16_range_text(&before.value, before.selected_range).is_some_and(|selected| selected == text)
}

#[cfg(target_os = "macos")]
fn perform_clipboard_paste(text: &str) -> bool {
    let original = read_clipboard_text();
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if clipboard.set_text(text.to_string()).is_err() {
            return false;
        }
    } else {
        return false;
    }
    std::thread::sleep(Duration::from_millis(PASTE_WRITE_SETTLE_MS));
    let ok = run_keystroke("v");
    std::thread::sleep(Duration::from_millis(PASTE_RESTORE_DELAY_MS));
    restore_clipboard(original);
    ok
}

#[cfg(not(target_os = "macos"))]
fn paste_text(_text: &str, _preferred_target: Option<&PasteTargetInfo>) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn run_keystroke(key: &str) -> bool {
    let Some(keycode) = command_keycode(key) else {
        return false;
    };
    post_command_key(keycode)
}

#[cfg(target_os = "macos")]
fn command_keycode(key: &str) -> Option<u16> {
    match key {
        "c" => Some(8),
        "v" => Some(9),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
type CGEventRef = *mut c_void;
#[cfg(target_os = "macos")]
type CGEventSourceRef = *mut c_void;

#[cfg(target_os = "macos")]
const KCG_HID_EVENT_TAP: u32 = 0;
#[cfg(target_os = "macos")]
const KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: u32 = 1;
#[cfg(target_os = "macos")]
const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceCreate(state_id: u32) -> CGEventSourceRef;
    fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        virtual_key: u16,
        key_down: bool,
    ) -> CGEventRef;
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventPost(tap: u32, event: CGEventRef);
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

#[cfg(target_os = "macos")]
fn post_command_key(keycode: u16) -> bool {
    unsafe {
        let source = CGEventSourceCreate(KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
        if source.is_null() {
            return false;
        }

        let key_down = CGEventCreateKeyboardEvent(source, keycode, true);
        let key_up = CGEventCreateKeyboardEvent(source, keycode, false);
        if key_down.is_null() || key_up.is_null() {
            if !key_down.is_null() {
                CFRelease(key_down.cast_const());
            }
            if !key_up.is_null() {
                CFRelease(key_up.cast_const());
            }
            CFRelease(source.cast_const());
            return false;
        }

        CGEventSetFlags(key_down, KCG_EVENT_FLAG_MASK_COMMAND);
        CGEventSetFlags(key_up, KCG_EVENT_FLAG_MASK_COMMAND);
        CGEventPost(KCG_HID_EVENT_TAP, key_down);
        std::thread::sleep(Duration::from_millis(12));
        CGEventPost(KCG_HID_EVENT_TAP, key_up);

        CFRelease(key_down.cast_const());
        CFRelease(key_up.cast_const());
        CFRelease(source.cast_const());
        true
    }
}

#[cfg(target_os = "macos")]
fn resolve_paste_target_info(
    preferred_target: Option<&PasteTargetInfo>,
) -> Option<PasteTargetInfo> {
    let own_pid = std::process::id() as i32;
    let target = current_paste_target_info();
    let current_missing = target.is_none();
    let current_is_own = target
        .as_ref()
        .is_some_and(|target| target.pid == Some(own_pid));
    if target
        .as_ref()
        .is_some_and(|target| is_verified_paste_candidate(target, own_pid))
    {
        return target;
    }

    let fallback = target
        .as_ref()
        .filter(|target| is_fallback_paste_candidate(target, own_pid))
        .cloned();

    if let Some(pid) = manual_accessibility_retry_pid(target.as_ref(), own_pid) {
        if ensure_manual_accessibility_for_pid(pid) {
            for _ in 0..MANUAL_ACCESSIBILITY_POLL_ATTEMPTS {
                std::thread::sleep(MANUAL_ACCESSIBILITY_POLL_INTERVAL);
                let target = current_paste_target_info();
                if target
                    .as_ref()
                    .is_some_and(|target| is_verified_paste_candidate(target, own_pid))
                {
                    return target;
                }
                if target
                    .as_ref()
                    .is_some_and(|target| is_fallback_paste_candidate(target, own_pid))
                {
                    return target;
                }
            }
        }
    }

    fallback.or_else(|| {
        let preferred =
            preferred_target.filter(|target| is_attemptable_paste_target(target, own_pid))?;

        if current_is_own {
            let pid = preferred.pid?;
            if !activate_application_pid(pid) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(PASTE_ACTIVATE_SETTLE_MS));
            let target = current_paste_target_info();
            if target
                .as_ref()
                .is_some_and(|target| is_verified_paste_candidate(target, own_pid))
            {
                return target;
            }
            if target
                .as_ref()
                .is_some_and(|target| is_fallback_paste_candidate(target, own_pid))
            {
                return target;
            }
            return Some(preferred.clone());
        }

        if current_missing {
            Some(preferred.clone())
        } else {
            None
        }
    })
}

#[cfg(target_os = "macos")]
fn current_paste_target_info() -> Option<PasteTargetInfo> {
    current_ax_focused_target_info().or_else(current_system_events_paste_target_info)
}

#[cfg(target_os = "macos")]
fn current_ax_focused_target_info() -> Option<PasteTargetInfo> {
    let focused = AxFocusedElement::capture()?;
    focused.read_paste_target_info()
}

#[cfg(target_os = "macos")]
fn current_system_events_paste_target_info() -> Option<PasteTargetInfo> {
    let script = r#"
tell application "System Events"
  try
    set frontApp to first application process whose frontmost is true
    set pidValue to unix id of frontApp as text
    set roleValue to ""
    set subroleValue to ""
    set attributeNames to {}
    try
      set focusedElement to value of attribute "AXFocusedUIElement" of frontApp
      try
        set roleValue to value of attribute "AXRole" of focusedElement as text
      end try
      try
        set subroleValue to value of attribute "AXSubrole" of focusedElement as text
      end try
      try
        set attributeNames to name of every attribute of focusedElement
      end try
    end try
    set oldDelimiters to AppleScript's text item delimiters
    set AppleScript's text item delimiters to ","
    set attributeNamesText to attributeNames as text
    set AppleScript's text item delimiters to oldDelimiters
    return pidValue & linefeed & roleValue & linefeed & subroleValue & linefeed & attributeNamesText
  on error
    return ""
  end try
end tell
"#;
    std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| PasteTargetInfo::from_osascript_output(&String::from_utf8_lossy(&o.stdout)))
}

#[cfg(target_os = "macos")]
fn activate_application_pid(pid: i32) -> bool {
    let script = format!(
        r#"
tell application "System Events"
  try
    set frontmost of first application process whose unix id is {pid} to true
    return "ok"
  on error
    return ""
  end try
end tell
"#
    );
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "ok")
}

fn manual_accessibility_retry_pid(target: Option<&PasteTargetInfo>, own_pid: i32) -> Option<i32> {
    let target = target?;
    if is_pasteable_target(target) {
        return None;
    }
    let pid = target.pid?;
    if pid == own_pid {
        return None;
    }
    Some(pid)
}

#[cfg(target_os = "macos")]
fn ensure_manual_accessibility_for_pid(pid: i32) -> bool {
    if recently_failed_manual_accessibility(pid) {
        return false;
    }
    if manual_accessibility_is_enabled(pid) {
        return true;
    }

    if enable_manual_accessibility_for_pid(pid) {
        remember_manual_accessibility_enabled(pid);
        true
    } else {
        remember_manual_accessibility_failure(pid);
        false
    }
}

#[cfg(target_os = "macos")]
fn enable_manual_accessibility_for_pid(pid: i32) -> bool {
    unsafe {
        let app = AXUIElementCreateApplication(pid as c_int);
        if app.is_null() {
            return false;
        }

        let attribute = CFString::new("AXManualAccessibility");
        let enabled = CFBoolean::true_value();
        let status = AXUIElementSetAttributeValue(
            app,
            attribute.as_concrete_TypeRef(),
            enabled.as_CFTypeRef(),
        );
        CFRelease(app.cast());
        status == KAX_ERROR_SUCCESS
    }
}

#[cfg(target_os = "macos")]
fn manual_accessibility_cache() -> &'static Mutex<ManualAccessibilityCache> {
    MANUAL_ACCESSIBILITY_CACHE.get_or_init(|| Mutex::new(ManualAccessibilityCache::default()))
}

#[cfg(target_os = "macos")]
fn manual_accessibility_is_enabled(pid: i32) -> bool {
    manual_accessibility_cache()
        .lock()
        .is_ok_and(|cache| cache.enabled_pids.contains(&pid))
}

#[cfg(target_os = "macos")]
fn remember_manual_accessibility_enabled(pid: i32) {
    if let Ok(mut cache) = manual_accessibility_cache().lock() {
        cache.enabled_pids.insert(pid);
        cache.failed_until_by_pid.remove(&pid);
    }
}

#[cfg(target_os = "macos")]
fn recently_failed_manual_accessibility(pid: i32) -> bool {
    let Ok(mut cache) = manual_accessibility_cache().lock() else {
        return false;
    };
    let Some(until) = cache.failed_until_by_pid.get(&pid).copied() else {
        return false;
    };
    if Instant::now() < until {
        return true;
    }
    cache.failed_until_by_pid.remove(&pid);
    false
}

#[cfg(target_os = "macos")]
fn remember_manual_accessibility_failure(pid: i32) {
    if let Ok(mut cache) = manual_accessibility_cache().lock() {
        cache
            .failed_until_by_pid
            .insert(pid, Instant::now() + MANUAL_ACCESSIBILITY_FAILURE_TTL);
    }
}

fn is_pasteable_target(target: &PasteTargetInfo) -> bool {
    if is_text_input_role(&target.role) || is_text_input_role(&target.subrole) {
        return true;
    }

    // CGEventPost only tells us the shortcut was emitted, not that any app inserted
    // text. Unknown accessibility roles must fall back unless they expose a cursor.
    target.attributes.contains("AXSelectedTextRange")
        || target.attributes.contains("AXInsertionPointLineNumber")
        || target.attributes.contains("AXSelectedTextMarkerRange")
        || target.attributes.contains("AXEditableAncestor")
}

fn is_verified_paste_candidate(target: &PasteTargetInfo, own_pid: i32) -> bool {
    target.pid != Some(own_pid) && is_pasteable_target(target)
}

fn is_attemptable_paste_target(target: &PasteTargetInfo, own_pid: i32) -> bool {
    if target.pid == Some(own_pid) {
        return false;
    }

    is_pasteable_target(target) || is_fallback_paste_candidate(target, own_pid)
}

fn is_fallback_paste_candidate(target: &PasteTargetInfo, own_pid: i32) -> bool {
    is_web_content_paste_candidate(target, own_pid)
        || is_ambiguous_external_paste_candidate(target, own_pid)
}

fn is_web_content_paste_candidate(target: &PasteTargetInfo, own_pid: i32) -> bool {
    if target.pid == Some(own_pid) {
        return false;
    }

    is_web_content_role(&target.role) || is_web_content_role(&target.subrole)
}

fn is_ambiguous_external_paste_candidate(target: &PasteTargetInfo, own_pid: i32) -> bool {
    target.pid.is_some_and(|pid| pid != own_pid)
        && target.role.is_empty()
        && target.subrole.is_empty()
        && target.attributes.is_empty()
}

fn is_text_input_role(role: &str) -> bool {
    matches!(
        role,
        "AXTextArea" | "AXTextField" | "AXComboBox" | "AXSearchField" | "AXTextView"
    )
}

fn is_web_content_role(role: &str) -> bool {
    role == "AXWebArea"
}

fn read_clipboard_text() -> Option<String> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok())
}

fn write_clipboard_text(value: &str) -> bool {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(value.to_string()))
        .is_ok()
}

fn restore_clipboard(value: Option<String>) {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        match value {
            Some(value) => {
                let _ = clipboard.set_text(value);
            }
            None => {
                let _ = clipboard.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_audio_input_device_id_reads_trailing_index_only() {
        assert_eq!(
            parse_audio_input_device_id("MacBook Pro Microphone#0"),
            Some("MacBook Pro Microphone")
        );
        assert_eq!(
            parse_audio_input_device_id("Studio #1#2"),
            Some("Studio #1")
        );
        assert_eq!(parse_audio_input_device_id("invalid"), None);
        assert_eq!(parse_audio_input_device_id("Mic#abc"), None);
    }

    #[test]
    fn paste_target_accepts_text_input_roles() {
        assert!(is_pasteable_target(&paste_target("AXTextArea", "", &[])));
        assert!(is_pasteable_target(&paste_target("", "AXTextField", &[])));
    }

    #[test]
    fn paste_target_attempt_skips_own_text_input() {
        let target = PasteTargetInfo {
            pid: Some(100),
            role: "AXTextArea".to_string(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(is_pasteable_target(&target));
        assert!(!is_verified_paste_candidate(&target, 100));
        assert!(!is_attemptable_paste_target(&target, 100));
    }

    #[test]
    fn paste_target_accepts_editor_cursor_attributes() {
        assert!(is_pasteable_target(&paste_target(
            "AXGroup",
            "",
            &["AXRole", "AXSelectedTextRange"]
        )));
        assert!(is_pasteable_target(&paste_target(
            "AXWebArea",
            "",
            &["AXInsertionPointLineNumber"]
        )));
        assert!(is_pasteable_target(&paste_target(
            "AXGroup",
            "",
            &["AXSelectedTextMarkerRange"]
        )));
        assert!(is_pasteable_target(&paste_target(
            "AXStaticText",
            "",
            &["AXEditableAncestor"]
        )));
    }

    #[test]
    fn paste_target_rejects_unclear_roles_without_cursor_attributes() {
        assert!(!is_pasteable_target(&paste_target(
            "AXGroup",
            "",
            &["AXRole", "AXValue"]
        )));
        assert!(!is_pasteable_target(&paste_target(
            "AXWebArea",
            "",
            &["AXRole"]
        )));
        assert!(!is_pasteable_target(&paste_target("AXUnknown", "", &[])));
    }

    #[test]
    fn paste_target_treats_web_area_as_fallback_candidate() {
        assert!(is_web_content_paste_candidate(
            &paste_target("AXWebArea", "", &["AXRole"]),
            100
        ));
        assert!(is_web_content_paste_candidate(
            &paste_target("", "AXWebArea", &["AXRole"]),
            100
        ));
    }

    #[test]
    fn paste_target_fallback_skips_own_web_area() {
        let target = PasteTargetInfo {
            pid: Some(100),
            role: "AXWebArea".to_string(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(!is_web_content_paste_candidate(&target, 100));
    }

    #[test]
    fn paste_target_treats_pid_only_external_target_as_fallback_candidate() {
        let target = PasteTargetInfo {
            pid: Some(4242),
            role: String::new(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(is_ambiguous_external_paste_candidate(&target, 100));
        assert!(is_attemptable_paste_target(&target, 100));
    }

    #[test]
    fn paste_target_rejects_known_non_text_external_target_as_fallback_candidate() {
        let target = PasteTargetInfo {
            pid: Some(4242),
            role: "AXButton".to_string(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(!is_fallback_paste_candidate(&target, 100));
        assert!(!is_attemptable_paste_target(&target, 100));
    }

    #[test]
    fn paste_target_info_parses_osascript_output() {
        let target = PasteTargetInfo::from_osascript_output(
            "AXGroup\nAXTextArea\nAXRole, AXSelectedTextRange\n",
        )
        .expect("target");

        assert_eq!(target.pid, None);
        assert_eq!(target.role, "AXGroup");
        assert_eq!(target.subrole, "AXTextArea");
        assert!(target.attributes.contains("AXSelectedTextRange"));
    }

    #[test]
    fn paste_target_info_parses_pid_osascript_output() {
        let target = PasteTargetInfo::from_osascript_output(
            "4242\nAXGroup\nAXTextArea\nAXRole, AXSelectedTextRange\n",
        )
        .expect("target");

        assert_eq!(target.pid, Some(4242));
        assert_eq!(target.role, "AXGroup");
        assert_eq!(target.subrole, "AXTextArea");
        assert!(target.attributes.contains("AXSelectedTextRange"));
    }

    #[test]
    fn paste_target_info_keeps_pid_when_focus_is_unavailable() {
        let target = PasteTargetInfo::from_osascript_output("4242\n\n\n\n").expect("target");

        assert_eq!(target.pid, Some(4242));
        assert_eq!(target.role, "");
        assert_eq!(target.subrole, "");
        assert!(target.attributes.is_empty());
    }

    #[test]
    fn paste_target_info_rejects_empty_osascript_output() {
        assert!(PasteTargetInfo::from_osascript_output("").is_none());
        assert!(PasteTargetInfo::from_osascript_output("\n\n").is_none());
    }

    #[test]
    fn manual_accessibility_retry_uses_pid_for_unclear_target() {
        let target = PasteTargetInfo {
            pid: Some(4242),
            role: String::new(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert_eq!(
            manual_accessibility_retry_pid(Some(&target), 100),
            Some(4242)
        );
    }

    #[test]
    fn manual_accessibility_retry_skips_existing_pasteable_target() {
        let target = paste_target("AXTextArea", "", &[]);

        assert_eq!(manual_accessibility_retry_pid(Some(&target), 100), None);
    }

    #[test]
    fn manual_accessibility_retry_skips_own_process() {
        let target = PasteTargetInfo {
            pid: Some(4242),
            role: String::new(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert_eq!(manual_accessibility_retry_pid(Some(&target), 4242), None);
    }

    #[test]
    fn changed_span_tracks_utf16_ranges() {
        let span = changed_span("絵文字🙂タイプレスです", "絵文字🙂Typelessです").expect("span");

        assert_eq!(span.from, "タイプレス");
        assert_eq!(span.to, "Typeless");
        assert_eq!(span.old_range.location, "絵文字🙂".encode_utf16().count());
        assert_eq!(span.old_range.length, "タイプレス".encode_utf16().count());
    }

    #[test]
    fn utf16_range_text_reads_ranges_with_surrogates() {
        let value = "絵文字🙂タイプレス";
        let range = TextRange {
            location: "絵文字🙂".encode_utf16().count(),
            length: "タイプレス".encode_utf16().count(),
        };

        assert_eq!(
            utf16_range_text(value, range).as_deref(),
            Some("タイプレス")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn verified_paste_accepts_changed_value() {
        let before = ax_text_snapshot("hello ", 6, 0);
        let after = ax_text_snapshot("hello world", 11, 0);

        let Some(VerifiedPasteInsertion::Changed(range)) =
            verify_paste_insertion(&before, &after, "world")
        else {
            panic!("expected changed insertion");
        };

        assert_eq!(range.location, 6);
        assert_eq!(range.length, "world".encode_utf16().count());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn verified_paste_accepts_identical_selected_text_replacement() {
        let selected = "タイプレス";
        let before = ax_text_snapshot(selected, 0, selected.encode_utf16().count());
        let after = ax_text_snapshot(selected, selected.encode_utf16().count(), 0);

        assert!(matches!(
            verify_paste_insertion(&before, &after, selected),
            Some(VerifiedPasteInsertion::SameTextReplacement)
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn verified_paste_rejects_unchanged_cursor_value() {
        let before = ax_text_snapshot("hello", 5, 0);
        let after = ax_text_snapshot("hello", 5, 0);

        assert!(verify_paste_insertion(&before, &after, "world").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn verified_paste_rejects_same_text_when_selection_did_not_move() {
        let selected = "タイプレス";
        let before = ax_text_snapshot(selected, 0, selected.encode_utf16().count());
        let after = ax_text_snapshot(selected, 0, selected.encode_utf16().count());

        assert!(verify_paste_insertion(&before, &after, selected).is_none());
    }

    #[test]
    fn dictionary_learning_quiescence_waits_while_value_changes() {
        let mut state = DictionaryLearningQuiescence::new("タイプレス");

        for value in ["t", "ty", "typ", "type", "typel"] {
            assert_eq!(
                advance_dictionary_learning_quiescence(
                    &mut state,
                    value,
                    DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                    DICTIONARY_LEARNING_QUIET_MS,
                    DICTIONARY_LEARNING_MAX_WATCH_MS,
                ),
                DictionaryLearningQuiescenceStep::Continue
            );
        }
    }

    #[test]
    fn dictionary_learning_quiescence_requires_quiet_period() {
        let mut state = DictionaryLearningQuiescence::new("タイプレス");
        let stable_polls = DICTIONARY_LEARNING_QUIET_MS / DICTIONARY_LEARNING_POLL_INTERVAL_MS;

        for _ in 0..stable_polls {
            assert_eq!(
                advance_dictionary_learning_quiescence(
                    &mut state,
                    "Typeless",
                    DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                    DICTIONARY_LEARNING_QUIET_MS,
                    DICTIONARY_LEARNING_MAX_WATCH_MS,
                ),
                DictionaryLearningQuiescenceStep::Continue
            );
        }
        assert_eq!(
            advance_dictionary_learning_quiescence(
                &mut state,
                "Typeless",
                DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                DICTIONARY_LEARNING_QUIET_MS,
                DICTIONARY_LEARNING_MAX_WATCH_MS,
            ),
            DictionaryLearningQuiescenceStep::Ready
        );
    }

    #[test]
    fn dictionary_learning_quiescence_ignores_baseline_value() {
        let mut state = DictionaryLearningQuiescence::new("タイプレス");

        for _ in 0..10 {
            assert_eq!(
                advance_dictionary_learning_quiescence(
                    &mut state,
                    "タイプレス",
                    DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                    DICTIONARY_LEARNING_QUIET_MS,
                    DICTIONARY_LEARNING_MAX_WATCH_MS,
                ),
                DictionaryLearningQuiescenceStep::Continue
            );
        }
    }

    #[test]
    fn dictionary_learning_quiescence_can_recover_after_ineligible_candidate() {
        let mut state = DictionaryLearningQuiescence::new("タイプレス");
        let stable_polls = DICTIONARY_LEARNING_QUIET_MS / DICTIONARY_LEARNING_POLL_INTERVAL_MS;

        for _ in 0..stable_polls {
            assert_eq!(
                advance_dictionary_learning_quiescence(
                    &mut state,
                    "T",
                    DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                    DICTIONARY_LEARNING_QUIET_MS,
                    DICTIONARY_LEARNING_MAX_WATCH_MS,
                ),
                DictionaryLearningQuiescenceStep::Continue
            );
        }
        assert_eq!(
            advance_dictionary_learning_quiescence(
                &mut state,
                "T",
                DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                DICTIONARY_LEARNING_QUIET_MS,
                DICTIONARY_LEARNING_MAX_WATCH_MS,
            ),
            DictionaryLearningQuiescenceStep::Ready
        );
        assert_eq!(
            advance_dictionary_learning_quiescence(
                &mut state,
                "T",
                DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                DICTIONARY_LEARNING_QUIET_MS,
                DICTIONARY_LEARNING_MAX_WATCH_MS,
            ),
            DictionaryLearningQuiescenceStep::Continue
        );

        for _ in 0..stable_polls {
            assert_eq!(
                advance_dictionary_learning_quiescence(
                    &mut state,
                    "Typeless",
                    DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                    DICTIONARY_LEARNING_QUIET_MS,
                    DICTIONARY_LEARNING_MAX_WATCH_MS,
                ),
                DictionaryLearningQuiescenceStep::Continue
            );
        }
        assert_eq!(
            advance_dictionary_learning_quiescence(
                &mut state,
                "Typeless",
                DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                DICTIONARY_LEARNING_QUIET_MS,
                DICTIONARY_LEARNING_MAX_WATCH_MS,
            ),
            DictionaryLearningQuiescenceStep::Ready
        );
    }

    #[test]
    fn learned_correction_uses_changes_inside_inserted_range() {
        let inserted_range = TextRange {
            location: 3,
            length: "タイプレスを使う".encode_utf16().count(),
        };

        let correction = learned_correction_from_values(
            "今日はタイプレスを使う",
            "今日はTypelessを使う",
            inserted_range,
        )
        .expect("correction");

        assert_eq!(
            correction,
            ("タイプレス".to_string(), "Typeless".to_string())
        );
    }

    #[test]
    fn learned_correction_ignores_changes_outside_inserted_range() {
        let inserted_range = TextRange {
            location: 3,
            length: "タイプレス".encode_utf16().count(),
        };

        let correction = learned_correction_from_values(
            "今日はタイプレス。明日も。",
            "今日はタイプレス。昨日も。",
            inserted_range,
        );

        assert!(correction.is_none());
    }

    #[test]
    fn learned_correction_ignores_single_character_edits() {
        let inserted_range = TextRange {
            location: 0,
            length: "hello".encode_utf16().count(),
        };

        let correction = learned_correction_from_values("hello", "Hello", inserted_range);

        assert!(correction.is_none());
    }

    #[test]
    fn learned_correction_accepts_multi_character_terms() {
        let inserted_range = TextRange {
            location: 0,
            length: "タイプレス".encode_utf16().count(),
        };

        let correction = learned_correction_from_values("タイプレス", "Typeless", inserted_range)
            .expect("correction");

        assert_eq!(
            correction,
            ("タイプレス".to_string(), "Typeless".to_string())
        );
    }

    #[test]
    fn learned_correction_ignores_sentence_like_values() {
        let inserted = "皆さん、ご飯が美味しいですね！";
        let inserted_range = TextRange {
            location: 0,
            length: inserted.encode_utf16().count(),
        };

        let correction = learned_correction_from_values(
            inserted,
            "フォローアップの変更を求める",
            inserted_range,
        );

        assert!(correction.is_none());
    }

    #[test]
    fn placeholder_value_is_treated_as_empty_text() {
        assert_eq!(
            value_without_placeholder(
                "フォローアップの変更を求める".to_string(),
                Some("フォローアップの変更を求める"),
            ),
            ""
        );
    }

    #[test]
    fn learned_correction_ignores_deleted_text_when_placeholder_is_exposed() {
        let inserted = "皆さん、ご飯が美味しいですね！";
        let inserted_range = TextRange {
            location: 0,
            length: inserted.encode_utf16().count(),
        };
        let current = value_without_placeholder(
            "フォローアップの変更を求める".to_string(),
            Some("フォローアップの変更を求める"),
        );

        let correction = learned_correction_from_values(inserted, &current, inserted_range);

        assert!(correction.is_none());
    }

    #[test]
    fn learned_correction_ignores_long_full_insert_rewrites() {
        let inserted = "ご飯が美味しいですね";
        let inserted_range = TextRange {
            location: 0,
            length: inserted.encode_utf16().count(),
        };

        let correction =
            learned_correction_from_values(inserted, "ランチが最高ですね", inserted_range);

        assert!(correction.is_none());
    }

    #[test]
    fn learned_correction_allows_sentence_local_term_change() {
        let inserted_range = TextRange {
            location: 0,
            length: "今日はタイプレスを使います".encode_utf16().count(),
        };

        let correction = learned_correction_from_values(
            "今日はタイプレスを使います",
            "今日はTypelessを使います",
            inserted_range,
        )
        .expect("correction");

        assert_eq!(
            correction,
            ("タイプレス".to_string(), "Typeless".to_string())
        );
    }

    #[test]
    fn screen_context_terms_extracts_code_and_product_terms() {
        let context = VoiceScreenContext {
            app_name: Some("Cursor".to_string()),
            window_title: Some("main.ts - enja".to_string()),
            visible_text: "GPT-4o kubectl src-tauri/voice.rs Typeless AquaVoice 12345".to_string(),
            ..VoiceScreenContext::default()
        };

        let terms = screen_context_terms(&context);

        assert!(terms.contains(&"Cursor".to_string()));
        assert!(terms.contains(&"main.ts".to_string()));
        assert!(terms.contains(&"GPT-4o".to_string()));
        assert!(terms.contains(&"kubectl".to_string()));
        assert!(terms.contains(&"src-tauri/voice.rs".to_string()));
        assert!(!terms.contains(&"12345".to_string()));
    }

    #[test]
    fn finalization_screen_context_section_labels_context_sources() {
        let context = VoiceScreenContext {
            app_name: Some("Slack".to_string()),
            window_title: Some("Acme".to_string()),
            focused_before: "了解しました。".to_string(),
            focused_after: "よろしくお願いします。".to_string(),
            ..VoiceScreenContext::default()
        };

        let section = finalization_screen_context_section(&context);

        assert!(section.contains("アプリ: Slack"));
        assert!(section.contains("ウィンドウ: Acme"));
        assert!(section.contains("カーソル前"));
        assert!(section.contains("カーソル後"));
    }

    #[test]
    fn screen_context_ocr_policy_skips_speed_mode_when_live_transcript_is_used() {
        let mut settings = AppSettings::default();
        settings.voice.active_mode_profile_id = "speed".to_string();

        assert!(!should_capture_voice_screen_context_ocr(
            &settings,
            VoiceMode::Dictation,
            "speed"
        ));
    }

    #[test]
    fn screen_context_ocr_policy_keeps_speed_mode_for_batch_asr_hints() {
        let mut settings = AppSettings::default();
        settings.voice.active_mode_profile_id = "speed".to_string();
        settings.voice.speech_profile = SpeechProfile::OpenAiGpt4oTranscribe;

        assert!(should_capture_voice_screen_context_ocr(
            &settings,
            VoiceMode::Dictation,
            "speed"
        ));
    }

    #[test]
    fn screen_context_ocr_policy_keeps_finalized_voice_flows() {
        let settings = AppSettings::default();

        assert!(should_capture_voice_screen_context_ocr(
            &settings,
            VoiceMode::Dictation,
            "default"
        ));
        assert!(should_capture_voice_screen_context_ocr(
            &settings,
            VoiceMode::Ask,
            ""
        ));
    }

    #[cfg(target_os = "macos")]
    fn ax_text_snapshot(
        value: &str,
        selection_location: usize,
        selection_length: usize,
    ) -> AxTextSnapshot {
        AxTextSnapshot {
            pid: 4242,
            value: value.to_string(),
            selected_range: TextRange {
                location: selection_location,
                length: selection_length,
            },
        }
    }

    fn paste_target(role: &str, subrole: &str, attributes: &[&str]) -> PasteTargetInfo {
        PasteTargetInfo {
            pid: None,
            role: role.to_string(),
            subrole: subrole.to_string(),
            attributes: attributes.iter().map(|value| value.to_string()).collect(),
        }
    }
}
