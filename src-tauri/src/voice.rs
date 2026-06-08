mod aec;
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
use base64::Engine;
#[cfg(target_os = "macos")]
use core_foundation::base::TCFType;
#[cfg(target_os = "macos")]
use core_foundation::boolean::CFBoolean;
#[cfg(target_os = "macos")]
use core_foundation::string::{CFString, CFStringRef};
#[cfg(target_os = "macos")]
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
#[cfg(target_os = "macos")]
use core_foundation_sys::base::{CFGetTypeID, CFTypeRef};
#[cfg(target_os = "macos")]
use core_foundation_sys::string::CFStringGetTypeID;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Sample;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Write};
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

const SPEECH_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const APPLE_SPEECH_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const APPLE_SPEECH_INSTALL_TIMEOUT: Duration = Duration::from_secs(900);
const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const OPENAI_REALTIME_TRANSCRIPTION_MODEL: &str = "gpt-realtime-whisper";
const OPENAI_REALTIME_TRANSCRIPTION_SAMPLE_RATE: u32 = 24_000;
const OPENAI_REALTIME_TRANSCRIPTION_TIMEOUT: Duration = Duration::from_secs(20);
const AUDIO_INPUT_DEVICES_CHANGED_EVENT: &str = "audio-input-devices-changed";
const VOICE_WINDOW_EDGE_MARGIN: f64 = 16.0;
const VOICE_WINDOW_BOTTOM_MARGIN: f64 = 42.0;
const VOICE_WINDOW_FOLLOW_INTERVAL_MS: u64 = 180;
const MIN_API_RECORDING_SECS: f32 = 0.7;
const VOICE_FRAME_MS: u32 = 20;
const MIN_ACTIVE_AUDIO_SECS: f32 = 0.08;
const VAD_NOISE_RMS_FLOOR: f32 = 0.0003;
const VAD_NOISE_PEAK_FLOOR: f32 = 0.001;
const VAD_MIN_CONTINUATION_RMS_THRESHOLD: f32 = 0.0008;
const VAD_MIN_WEAK_RMS_THRESHOLD: f32 = 0.0012;
const VAD_MIN_STRONG_RMS_THRESHOLD: f32 = 0.0024;
const VAD_MIN_CONTINUATION_PEAK_THRESHOLD: f32 = 0.004;
const VAD_MIN_WEAK_PEAK_THRESHOLD: f32 = 0.006;
const VAD_MIN_STRONG_PEAK_THRESHOLD: f32 = 0.012;
const VAD_AMBIGUOUS_DYNAMIC_RANGE: f32 = 2.5;
const VAD_MIN_START_MS: u32 = 60;
const VAD_MIN_SEGMENT_MS: u32 = 80;
const VAD_PREFIX_PADDING_MS: u32 = 300;
const VAD_POST_PADDING_MS: u32 = 600;
const VAD_END_SILENCE_MS: u32 = 1_200;
const VAD_SHORT_GAP_MERGE_MS: u32 = 900;
const VAD_AMBIGUOUS_PREFIX_PADDING_MS: u32 = 420;
const VAD_AMBIGUOUS_POST_PADDING_MS: u32 = 900;
const VAD_AMBIGUOUS_END_SILENCE_MS: u32 = 1_800;
const VAD_AMBIGUOUS_SHORT_GAP_MERGE_MS: u32 = 1_400;
const VAD_TERMINAL_PROTECTION_MS: u32 = 2_600;
const DICTIONARY_LEARNING_POLL_INTERVAL_MS: u64 = 250;
const DICTIONARY_LEARNING_QUIET_MS: u64 = 2_000;
const DICTIONARY_LEARNING_MAX_WATCH_MS: u64 = 15_000;
const DICTIONARY_NOTICE_VISIBLE_MS: u64 = 6_500;
const DICTIONARY_UNDO_NOTICE_MS: u64 = 900;
const GOOGLE_SPEECH_DICTIONARY_BOOST: f32 = 8.0;
const MIN_LEARNED_CORRECTION_CHARS: usize = 2;
const MAX_LEARNED_CORRECTION_CHARS: usize = 40;
const MIN_FULL_INSERT_REWRITE_CHARS: usize = 12;
const STOP_RECORDING_BUFFER: Duration = Duration::from_millis(500);
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
}

impl VoiceWindowLayout {
    fn dimensions(self) -> (f64, f64) {
        match self {
            Self::Compact => (292.0, 42.0),
            Self::Expanded => (840.0, 420.0),
            Self::Notice => (460.0, 64.0),
        }
    }

    fn min_height(self) -> f64 {
        match self {
            Self::Expanded => 260.0,
            Self::Notice => 58.0,
            Self::Compact => 40.0,
        }
    }

    fn focusable(self) -> bool {
        self != Self::Compact
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
    OpenAiRealtimeWhisper,
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
    Finish { include_stop_buffer: bool },
    Cancel,
}

struct AudioClip {
    wav: Vec<u8>,
    duration_secs: f32,
    live_transcript: Option<LiveTranscript>,
}

#[derive(Debug, Clone, Copy)]
struct PreparedAudioAnalysis {
    duration_secs: f32,
    active_audio_secs: f32,
}

#[derive(Debug)]
struct PreparedAudio {
    samples: Vec<i16>,
    analysis: PreparedAudioAnalysis,
}

#[derive(Debug, Clone, Copy)]
struct VoiceFrameStats {
    rms: f32,
    peak: f32,
}

#[derive(Debug, Clone, Copy)]
struct VoiceSegment {
    start_frame: usize,
    end_frame: usize,
}

#[derive(Debug, Clone, Copy)]
struct VoiceVadConfig {
    continuation_rms_threshold: f32,
    weak_rms_threshold: f32,
    strong_rms_threshold: f32,
    continuation_peak_threshold: f32,
    weak_peak_threshold: f32,
    strong_peak_threshold: f32,
    min_start_frames: usize,
    min_segment_frames: usize,
    prefix_padding_frames: usize,
    post_padding_frames: usize,
    end_silence_frames: usize,
    short_gap_merge_frames: usize,
    terminal_protection_frames: usize,
}

#[derive(Debug)]
struct VoiceVadResult {
    segments: Vec<VoiceSegment>,
    speech_frames: usize,
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

        let clip = recorder.finish_with_stop_buffer(move || {
            if let Some(aux) = audio_aux {
                aux.stop();
            }
        });

        if is_processing_cancelled(&cancelled) {
            self.clear_processing_session(&cancelled);
            return Ok(());
        }

        if app
            .try_state::<SettingsStore>()
            .map(|store| store.get().voice.interaction_sounds_enabled)
            .unwrap_or(false)
        {
            play_interaction_sound("stop");
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

        let result = tokio::select! {
            result = process_clip(&app, mode, &mode_profile_id, &selected_text, clip) => result,
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
    let recorder = tokio::task::spawn_blocking(move || {
        Recorder::start(
            app_for_recorder,
            microphone_id,
            max_recording_seconds,
            pipeline_for_recorder,
            live_transcription_provider,
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
    ) -> Result<Self, String> {
        let (control_tx, control_rx) = std::sync::mpsc::channel::<RecorderCommand>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<AudioClip, String>>();
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        std::thread::spawn(move || {
            let result = run_recording_thread(
                app.clone(),
                selected_device_id,
                max_recording_seconds,
                control_rx,
                init_tx,
                pipeline,
                live_transcription_provider,
            );
            let _ = done_tx.send(result);
        });
        init_rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "マイクの初期化がタイムアウトしました。".to_string())??;
        Ok(Self {
            control_tx,
            done_rx,
        })
    }

    fn finish_with_stop_buffer(
        self,
        after_recording_stopped: impl FnOnce(),
    ) -> Result<AudioClip, String> {
        let _ = self.control_tx.send(RecorderCommand::Finish {
            include_stop_buffer: true,
        });
        let result = match self.done_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(result) => result,
            Err(_) => Err("録音停止処理がタイムアウトしました。".to_string()),
        };
        after_recording_stopped();
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
        SpeechProfile::OpenAiGpt4oTranscribe | SpeechProfile::OpenAiGpt4oMiniTranscribe => {
            Some(LiveTranscriptionProvider::OpenAiRealtimeWhisper)
        }
        SpeechProfile::GeminiAudio => None,
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
) -> Result<LiveTranscriber, String> {
    match provider {
        LiveTranscriptionProvider::AppleSpeechAnalyzer => {
            start_apple_live_transcriber(app, sample_rate, channels)
        }
        LiveTranscriptionProvider::GoogleChirp3 => {
            start_google_live_transcriber(app, sample_rate, channels)
        }
        LiveTranscriptionProvider::OpenAiRealtimeWhisper => {
            start_openai_live_transcriber(sample_rate, channels)
        }
    }
}

fn start_apple_live_transcriber(
    app: &tauri::AppHandle,
    sample_rate: u32,
    channels: u16,
) -> Result<LiveTranscriber, String> {
    let helper = resolve_apple_speech_helper(app)?;
    let entries = dictionary::load_dictionary(app).unwrap_or_default();
    let context_path = temp_voice_file_path("apple-speech-live-context", "json");
    let context = serde_json::json!({
        "contextualStrings": apple_speech_contextual_strings(&entries),
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
    let phrases = dictionary::enabled_phrases(&entries);
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

fn start_openai_live_transcriber(
    sample_rate: u32,
    channels: u16,
) -> Result<LiveTranscriber, String> {
    let key = secrets::get_secret("openai")
        .map_err(|_| "OpenAI APIキーを保存してください。".to_string())?;
    if key.trim().is_empty() {
        return Err("OpenAI APIキーを保存してください。".to_string());
    }

    let (sample_tx, sample_rx) = std::sync::mpsc::channel::<Vec<i16>>();
    let join = std::thread::spawn(move || -> Result<String, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(openai_realtime_transcribe(
            key,
            sample_rx,
            sample_rate,
            channels,
        ))
    });

    Ok(LiveTranscriber {
        provider: LiveTranscriptionProvider::OpenAiRealtimeWhisper,
        sample_tx: Some(sample_tx),
        join: Some(join),
    })
}

async fn openai_realtime_transcribe(
    key: String,
    sample_rx: std::sync::mpsc::Receiver<Vec<i16>>,
    sample_rate: u32,
    channels: u16,
) -> Result<String, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::HeaderValue;
    use tokio_tungstenite::tungstenite::Message;

    let mut request = "wss://api.openai.com/v1/realtime?model=gpt-realtime-whisper"
        .into_client_request()
        .map_err(|e| e.to_string())?;
    request.headers_mut().insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {}", key.trim())).map_err(|e| e.to_string())?,
    );

    let (ws_stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("OpenAI Realtimeへ接続できませんでした: {e}"))?;
    let (mut write, mut read) = ws_stream.split();

    let session_update = serde_json::json!({
        "type": "session.update",
        "session": {
            "type": "transcription",
            "audio": {
                "input": {
                    "format": {
                        "type": "audio/pcm",
                        "rate": OPENAI_REALTIME_TRANSCRIPTION_SAMPLE_RATE,
                    },
                    "transcription": {
                        "model": OPENAI_REALTIME_TRANSCRIPTION_MODEL,
                        "language": "ja",
                        "delay": "low",
                    },
                    "turn_detection": null,
                },
            },
        },
    });
    write
        .send(Message::Text(session_update.to_string().into()))
        .await
        .map_err(|e| format!("OpenAI Realtimeへ初期設定を送信できませんでした: {e}"))?;

    let mut receive_task = tokio::spawn(async move {
        while let Some(message) = read.next().await {
            let message = message.map_err(|e| e.to_string())?;
            if !message.is_text() {
                continue;
            }
            let text = message.to_text().map_err(|e| e.to_string())?;
            let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
            match value.get("type").and_then(|kind| kind.as_str()) {
                Some("conversation.item.input_audio_transcription.completed") => {
                    let transcript = value
                        .get("transcript")
                        .and_then(|transcript| transcript.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if transcript.is_empty() {
                        return Err("OpenAI Realtimeの文字起こし結果が空でした。".to_string());
                    }
                    return Ok(transcript);
                }
                Some("error") => {
                    return Err(openai_realtime_error_message(&value));
                }
                _ => {}
            }
        }
        Err("OpenAI Realtimeの完了イベントを受信できませんでした。".to_string())
    });

    let (audio_tx, mut audio_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<i16>>();
    let bridge_join = std::thread::spawn(move || {
        for samples in sample_rx {
            if audio_tx.send(samples).is_err() {
                break;
            }
        }
    });

    let mut converter = StreamingPcmConverter::new(
        sample_rate,
        channels,
        OPENAI_REALTIME_TRANSCRIPTION_SAMPLE_RATE,
    );
    let mut sent_audio = false;

    while let Some(samples) = audio_rx.recv().await {
        let converted = converter.push(&samples);
        if converted.is_empty() {
            continue;
        }
        send_openai_audio_chunk(&mut write, &converted).await?;
        sent_audio = true;
    }

    let converted = converter.finish();
    if !converted.is_empty() {
        send_openai_audio_chunk(&mut write, &converted).await?;
        sent_audio = true;
    }

    let _ = bridge_join.join();

    if !sent_audio {
        receive_task.abort();
        let _ = write.close().await;
        return Err("OpenAI Realtimeへ送信する音声がありませんでした。".to_string());
    }

    write
        .send(Message::Text(
            serde_json::json!({ "type": "input_audio_buffer.commit" })
                .to_string()
                .into(),
        ))
        .await
        .map_err(|e| format!("OpenAI Realtimeへcommitを送信できませんでした: {e}"))?;

    let transcript = match tokio::time::timeout(
        OPENAI_REALTIME_TRANSCRIPTION_TIMEOUT,
        &mut receive_task,
    )
    .await
    {
        Ok(Ok(result)) => result?,
        Ok(Err(err)) => return Err(format!("OpenAI Realtime受信タスクが停止しました: {err}")),
        Err(_) => {
            receive_task.abort();
            return Err("OpenAI Realtimeの完了待ちがタイムアウトしました。".to_string());
        }
    };

    let _ = write.close().await;
    Ok(transcript)
}

async fn send_openai_audio_chunk<S>(write: &mut S, samples: &[i16]) -> Result<(), String>
where
    S: futures_util::Sink<tokio_tungstenite::tungstenite::Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let mut bytes = Vec::with_capacity(samples.len() * std::mem::size_of::<i16>());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let event = serde_json::json!({
        "type": "input_audio_buffer.append",
        "audio": base64::engine::general_purpose::STANDARD.encode(bytes),
    });
    write
        .send(tokio_tungstenite::tungstenite::Message::Text(
            event.to_string().into(),
        ))
        .await
        .map_err(|e| format!("OpenAI Realtimeへ音声を送信できませんでした: {e}"))
}

fn openai_realtime_error_message(value: &serde_json::Value) -> String {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(|message| message.as_str())
        .map(|message| format!("OpenAI Realtime error: {message}"))
        .unwrap_or_else(|| format!("OpenAI Realtime error: {value}"))
}

struct StreamingPcmConverter {
    input_rate: f64,
    channels: usize,
    target_rate: f64,
    buffer: Vec<f32>,
    next_output_pos: f64,
}

impl StreamingPcmConverter {
    fn new(input_rate: u32, channels: u16, target_rate: u32) -> Self {
        Self {
            input_rate: input_rate.max(1) as f64,
            channels: channels.max(1) as usize,
            target_rate: target_rate.max(1) as f64,
            buffer: Vec::new(),
            next_output_pos: 0.0,
        }
    }

    fn push(&mut self, samples: &[i16]) -> Vec<i16> {
        for frame in samples.chunks(self.channels) {
            let mut mono = 0.0_f32;
            for sample in frame {
                mono += *sample as f32 / i16::MAX as f32;
            }
            self.buffer
                .push((mono / frame.len().max(1) as f32).clamp(-1.0, 1.0));
        }
        self.drain(false)
    }

    fn finish(&mut self) -> Vec<i16> {
        self.drain(true)
    }

    fn drain(&mut self, flush: bool) -> Vec<i16> {
        if self.buffer.is_empty() {
            return Vec::new();
        }

        let step = self.input_rate / self.target_rate;
        let mut out = Vec::new();
        let limit = if flush {
            self.buffer.len().saturating_sub(1) as f64
        } else {
            self.buffer.len().saturating_sub(2) as f64
        };

        while self.next_output_pos <= limit {
            let index = self.next_output_pos.floor() as usize;
            let frac = (self.next_output_pos - index as f64) as f32;
            let next_index = (index + 1).min(self.buffer.len() - 1);
            let value = self.buffer[index] + (self.buffer[next_index] - self.buffer[index]) * frac;
            out.push((value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
            self.next_output_pos += step;
        }

        let consumed = self.next_output_pos.floor() as usize;
        if consumed > 0 {
            let drain_to = consumed.min(self.buffer.len().saturating_sub(1));
            self.buffer.drain(0..drain_to);
            self.next_output_pos -= drain_to as f64;
        }

        out
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
    init_tx: std::sync::mpsc::Sender<Result<(), String>>,
    pipeline: PipelineMode,
    live_transcription_provider: Option<LiveTranscriptionProvider>,
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
            match start_live_transcriber(&app, provider, output_sample_rate, output_channels) {
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
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => RecorderCommand::Finish {
            include_stop_buffer: false,
        },
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => RecorderCommand::Cancel,
    };
    if let RecorderCommand::Finish {
        include_stop_buffer: true,
    } = command
    {
        std::thread::sleep(STOP_RECORDING_BUFFER);
    }
    drop(stream);

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

fn samples_to_wav(samples: &[i16], sample_rate: u32, channels: u16) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = Vec::new();
    {
        let cursor = Cursor::new(&mut wav);
        let mut writer = hound::WavWriter::new(cursor, spec).map_err(|e| e.to_string())?;
        for sample in samples {
            writer.write_sample(*sample).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;
    }
    Ok(wav)
}

fn prepare_recorded_audio_for_api(
    samples: &[i16],
    sample_rate: u32,
    channels: u16,
) -> Result<PreparedAudio, String> {
    let prepared = trim_recorded_audio(samples, sample_rate, channels);
    let analysis = prepared.analysis;
    if analysis.active_audio_secs < MIN_ACTIVE_AUDIO_SECS {
        return Err(
            "音声が検出できなかったため、API送信をスキップしました。マイク入力を確認してください。"
                .to_string(),
        );
    }
    if analysis.duration_secs < MIN_API_RECORDING_SECS {
        return Err(
            "録音が短すぎるため、API送信をスキップしました。もう少し長く話してください。"
                .to_string(),
        );
    }
    Ok(prepared)
}

fn trim_recorded_audio(samples: &[i16], sample_rate: u32, channels: u16) -> PreparedAudio {
    let channels = channels.max(1) as usize;
    let frame_len = voice_frame_len(sample_rate, channels);
    let frame_stats = samples
        .chunks(frame_len)
        .map(voice_frame_stats)
        .collect::<Vec<_>>();

    let vad = detect_voice_segments(&frame_stats);
    if vad.segments.is_empty() {
        return PreparedAudio {
            samples: Vec::new(),
            analysis: PreparedAudioAnalysis {
                duration_secs: 0.0,
                active_audio_secs: 0.0,
            },
        };
    }

    let trimmed = render_voice_segments(samples, frame_len, &vad.segments);

    PreparedAudio {
        analysis: PreparedAudioAnalysis {
            duration_secs: audio_duration_secs(trimmed.len(), sample_rate, channels),
            active_audio_secs: vad.speech_frames as f32 * VOICE_FRAME_MS as f32 / 1000.0,
        },
        samples: trimmed,
    }
}

fn voice_frame_len(sample_rate: u32, channels: usize) -> usize {
    ((sample_rate as usize * channels * VOICE_FRAME_MS as usize) / 1000).max(1)
}

fn ms_to_frame_count(ms: u32) -> usize {
    ms.div_ceil(VOICE_FRAME_MS) as usize
}

fn detect_voice_segments(stats: &[VoiceFrameStats]) -> VoiceVadResult {
    if stats.is_empty() {
        return VoiceVadResult {
            segments: Vec::new(),
            speech_frames: 0,
        };
    }

    let config = estimate_voice_vad_config(stats);
    let mut segments = Vec::new();
    let mut speech_frames = 0usize;
    let mut weak_run = 0usize;
    let mut weak_run_start = 0usize;
    let mut segment_start = 0usize;
    let mut last_voice_frame = 0usize;
    let mut silence_run = 0usize;
    let mut in_segment = false;

    for (frame_index, stat) in stats.iter().enumerate() {
        let strong = is_strong_voice_frame(*stat, &config);
        let weak = strong || is_weak_voice_frame(*stat, &config);
        let continuation = weak || is_continuation_voice_frame(*stat, &config);

        if weak {
            speech_frames += 1;
        }

        if !in_segment {
            if weak {
                if weak_run == 0 {
                    weak_run_start = frame_index;
                }
                weak_run += 1;
            } else {
                weak_run = 0;
            }

            if strong || weak_run >= config.min_start_frames {
                let detected_start = if weak_run > 0 {
                    weak_run_start
                } else {
                    frame_index
                };
                segment_start = detected_start.saturating_sub(config.prefix_padding_frames);
                last_voice_frame = frame_index;
                silence_run = 0;
                in_segment = true;
            }

            continue;
        }

        if continuation {
            last_voice_frame = frame_index;
            silence_run = 0;
        } else {
            silence_run += 1;
            if silence_run >= config.end_silence_frames {
                let segment_end =
                    (last_voice_frame + 1 + config.post_padding_frames).min(stats.len());
                push_voice_segment(&mut segments, segment_start, segment_end, &config);
                in_segment = false;
                weak_run = 0;
                silence_run = 0;
            }
        }
    }

    if in_segment {
        let segment_end = (last_voice_frame + 1 + config.post_padding_frames).min(stats.len());
        push_voice_segment(&mut segments, segment_start, segment_end, &config);
    }

    if segments.is_empty() && has_possible_voice_signal(stats) {
        speech_frames = stats.len();
        segments.push(VoiceSegment {
            start_frame: 0,
            end_frame: stats.len(),
        });
    } else {
        protect_terminal_audio(&mut segments, stats.len(), &config);
    }

    VoiceVadResult {
        segments,
        speech_frames,
    }
}

fn estimate_voice_vad_config(stats: &[VoiceFrameStats]) -> VoiceVadConfig {
    let rms_values = stats.iter().map(|stat| stat.rms).collect::<Vec<_>>();
    let peak_values = stats.iter().map(|stat| stat.peak).collect::<Vec<_>>();
    let noise_rms = percentile(&rms_values, 0.20).max(VAD_NOISE_RMS_FLOOR);
    let noise_peak = percentile(&peak_values, 0.20).max(VAD_NOISE_PEAK_FLOOR);
    let p90_rms = percentile(&rms_values, 0.90);
    let p90_peak = percentile(&peak_values, 0.90);
    let rms_dynamic_range = p90_rms / noise_rms;
    let peak_dynamic_range = p90_peak / noise_peak;
    let ambiguous = rms_dynamic_range < VAD_AMBIGUOUS_DYNAMIC_RANGE
        && peak_dynamic_range < VAD_AMBIGUOUS_DYNAMIC_RANGE;

    let (
        continuation_rms_multiplier,
        weak_rms_multiplier,
        strong_rms_multiplier,
        continuation_peak_multiplier,
        weak_peak_multiplier,
        strong_peak_multiplier,
        prefix_padding_ms,
        post_padding_ms,
        end_silence_ms,
        short_gap_merge_ms,
    ) = if ambiguous {
        (
            1.00,
            1.03,
            1.25,
            1.00,
            1.06,
            1.30,
            VAD_AMBIGUOUS_PREFIX_PADDING_MS,
            VAD_AMBIGUOUS_POST_PADDING_MS,
            VAD_AMBIGUOUS_END_SILENCE_MS,
            VAD_AMBIGUOUS_SHORT_GAP_MERGE_MS,
        )
    } else {
        (
            1.08,
            1.30,
            2.30,
            1.12,
            1.45,
            2.40,
            VAD_PREFIX_PADDING_MS,
            VAD_POST_PADDING_MS,
            VAD_END_SILENCE_MS,
            VAD_SHORT_GAP_MERGE_MS,
        )
    };

    let possible_rms_signal = p90_rms >= VAD_MIN_CONTINUATION_RMS_THRESHOLD;
    let possible_peak_signal = p90_peak >= VAD_MIN_CONTINUATION_PEAK_THRESHOLD;

    let mut continuation_rms_threshold =
        (noise_rms * continuation_rms_multiplier).max(VAD_MIN_CONTINUATION_RMS_THRESHOLD);
    let mut weak_rms_threshold = (noise_rms * weak_rms_multiplier).max(VAD_MIN_WEAK_RMS_THRESHOLD);
    let mut strong_rms_threshold =
        (noise_rms * strong_rms_multiplier).max(VAD_MIN_STRONG_RMS_THRESHOLD);

    if possible_rms_signal {
        continuation_rms_threshold = continuation_rms_threshold
            .min((p90_rms * 0.75).max(VAD_MIN_CONTINUATION_RMS_THRESHOLD));
        weak_rms_threshold =
            weak_rms_threshold.min((p90_rms * 0.90).max(VAD_MIN_CONTINUATION_RMS_THRESHOLD));
        strong_rms_threshold =
            strong_rms_threshold.min((p90_rms * 0.98).max(VAD_MIN_CONTINUATION_RMS_THRESHOLD));
    }
    weak_rms_threshold = weak_rms_threshold.max(continuation_rms_threshold);
    strong_rms_threshold = strong_rms_threshold.max(weak_rms_threshold);

    let mut continuation_peak_threshold =
        (noise_peak * continuation_peak_multiplier).max(VAD_MIN_CONTINUATION_PEAK_THRESHOLD);
    let mut weak_peak_threshold =
        (noise_peak * weak_peak_multiplier).max(VAD_MIN_WEAK_PEAK_THRESHOLD);
    let mut strong_peak_threshold =
        (noise_peak * strong_peak_multiplier).max(VAD_MIN_STRONG_PEAK_THRESHOLD);

    if possible_peak_signal {
        continuation_peak_threshold = continuation_peak_threshold
            .min((p90_peak * 0.75).max(VAD_MIN_CONTINUATION_PEAK_THRESHOLD));
        weak_peak_threshold =
            weak_peak_threshold.min((p90_peak * 0.90).max(VAD_MIN_CONTINUATION_PEAK_THRESHOLD));
        strong_peak_threshold =
            strong_peak_threshold.min((p90_peak * 0.98).max(VAD_MIN_CONTINUATION_PEAK_THRESHOLD));
    }
    weak_peak_threshold = weak_peak_threshold.max(continuation_peak_threshold);
    strong_peak_threshold = strong_peak_threshold.max(weak_peak_threshold);

    VoiceVadConfig {
        continuation_rms_threshold,
        weak_rms_threshold,
        strong_rms_threshold,
        continuation_peak_threshold,
        weak_peak_threshold,
        strong_peak_threshold,
        min_start_frames: ms_to_frame_count(VAD_MIN_START_MS),
        min_segment_frames: ms_to_frame_count(VAD_MIN_SEGMENT_MS),
        prefix_padding_frames: ms_to_frame_count(prefix_padding_ms),
        post_padding_frames: ms_to_frame_count(post_padding_ms),
        end_silence_frames: ms_to_frame_count(end_silence_ms),
        short_gap_merge_frames: ms_to_frame_count(short_gap_merge_ms),
        terminal_protection_frames: ms_to_frame_count(VAD_TERMINAL_PROTECTION_MS),
    }
}

fn is_strong_voice_frame(stat: VoiceFrameStats, config: &VoiceVadConfig) -> bool {
    stat.rms >= config.strong_rms_threshold || stat.peak >= config.strong_peak_threshold
}

fn is_weak_voice_frame(stat: VoiceFrameStats, config: &VoiceVadConfig) -> bool {
    stat.rms >= config.weak_rms_threshold || stat.peak >= config.weak_peak_threshold
}

fn is_continuation_voice_frame(stat: VoiceFrameStats, config: &VoiceVadConfig) -> bool {
    stat.rms >= config.continuation_rms_threshold || stat.peak >= config.continuation_peak_threshold
}

fn push_voice_segment(
    segments: &mut Vec<VoiceSegment>,
    start_frame: usize,
    end_frame: usize,
    config: &VoiceVadConfig,
) {
    if end_frame <= start_frame || end_frame.saturating_sub(start_frame) < config.min_segment_frames
    {
        return;
    }

    if let Some(last) = segments.last_mut() {
        if start_frame <= last.end_frame.saturating_add(config.short_gap_merge_frames) {
            last.end_frame = last.end_frame.max(end_frame);
            return;
        }
    }

    segments.push(VoiceSegment {
        start_frame,
        end_frame,
    });
}

fn protect_terminal_audio(
    segments: &mut [VoiceSegment],
    total_frames: usize,
    config: &VoiceVadConfig,
) {
    let Some(last) = segments.last_mut() else {
        return;
    };

    if total_frames.saturating_sub(last.end_frame) <= config.terminal_protection_frames {
        last.end_frame = total_frames;
    }
}

fn has_possible_voice_signal(stats: &[VoiceFrameStats]) -> bool {
    let rms_values = stats.iter().map(|stat| stat.rms).collect::<Vec<_>>();
    let peak_values = stats.iter().map(|stat| stat.peak).collect::<Vec<_>>();
    percentile(&rms_values, 0.90) >= VAD_MIN_CONTINUATION_RMS_THRESHOLD
        || percentile(&peak_values, 0.90) >= VAD_MIN_CONTINUATION_PEAK_THRESHOLD
}

fn render_voice_segments(samples: &[i16], frame_len: usize, segments: &[VoiceSegment]) -> Vec<i16> {
    let retained_samples = segments
        .iter()
        .map(|segment| {
            let start = segment.start_frame.saturating_mul(frame_len);
            let end = segment
                .end_frame
                .saturating_mul(frame_len)
                .min(samples.len());
            end.saturating_sub(start)
        })
        .sum();
    let mut trimmed = Vec::with_capacity(retained_samples);

    for segment in segments {
        let start = segment.start_frame.saturating_mul(frame_len);
        let end = segment
            .end_frame
            .saturating_mul(frame_len)
            .min(samples.len());
        if start < end {
            trimmed.extend_from_slice(&samples[start..end]);
        }
    }

    trimmed
}

fn audio_duration_secs(sample_count: usize, sample_rate: u32, channels: usize) -> f32 {
    let samples_per_second = sample_rate as usize * channels.max(1);
    if samples_per_second == 0 {
        0.0
    } else {
        sample_count as f32 / samples_per_second as f32
    }
}

fn voice_frame_stats(samples: &[i16]) -> VoiceFrameStats {
    if samples.is_empty() {
        return VoiceFrameStats {
            rms: 0.0,
            peak: 0.0,
        };
    }

    let mut peak = 0.0f32;
    let sum = samples.iter().fold(0.0f32, |sum, sample| {
        let value = (*sample as f32 / i16::MAX as f32).clamp(-1.0, 1.0);
        peak = peak.max(value.abs());
        sum + value * value
    });

    VoiceFrameStats {
        rms: (sum / samples.len() as f32).sqrt(),
        peak,
    }
}

fn percentile(values: &[f32], fraction: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let rank = ((sorted.len() - 1) as f32 * fraction.clamp(0.0, 1.0)).round() as usize;
    sorted[rank]
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
                    LiveTranscriptionProvider::OpenAiRealtimeWhisper => {
                        if let Err(err) = usage::record_openai_transcription(
                            app,
                            OPENAI_REALTIME_TRANSCRIPTION_MODEL,
                            clip.duration_secs,
                        ) {
                            eprintln!("[enja] usage tracking failed: {err}");
                        }
                    }
                    LiveTranscriptionProvider::AppleSpeechAnalyzer => {}
                }
                live.result.as_ref().unwrap().clone()
            }
            Some(live) if live.result.is_ok() => {
                transcribe(app, &settings, &entries, &clip).await?
            }
            Some(live) => {
                let err = live
                    .result
                    .as_ref()
                    .err()
                    .cloned()
                    .unwrap_or_else(|| "ライブ文字起こしに失敗しました。".to_string());
                eprintln!("[enja] live transcription failed; falling back to batch: {err}");
                transcribe(app, &settings, &entries, &clip).await?
            }
            None => transcribe(app, &settings, &entries, &clip).await?,
        }
    } else {
        transcribe(app, &settings, &entries, &clip).await?
    };
    finalize_text(
        app,
        &settings,
        &entries,
        mode,
        mode_profile_id,
        selected_text,
        &transcript,
    )
    .await
}

async fn transcribe(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    clip: &AudioClip,
) -> Result<String, String> {
    match settings.voice.speech_profile {
        SpeechProfile::GoogleChirp3 => {
            if clip.duration_secs > 60.0 || clip.wav.len() > 10 * 1024 * 1024 {
                transcribe_long_audio_fallback(app, settings, entries, clip).await
            } else {
                transcribe_google_chirp3(app, settings, entries, clip).await
            }
        }
        SpeechProfile::OpenAiGpt4oTranscribe => {
            transcribe_openai(app, "gpt-4o-transcribe", settings, entries, clip).await
        }
        SpeechProfile::OpenAiGpt4oMiniTranscribe => {
            transcribe_openai(app, "gpt-4o-mini-transcribe", settings, entries, clip).await
        }
        SpeechProfile::GeminiAudio => transcribe_gemini_audio(app, settings, entries, clip).await,
        SpeechProfile::AppleSpeechAnalyzer => transcribe_apple_speech(app, entries, clip).await,
    }
}

async fn transcribe_long_audio_fallback(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    clip: &AudioClip,
) -> Result<String, String> {
    if secrets::get_secret("openai").is_ok_and(|key| !key.trim().is_empty()) {
        return transcribe_openai(app, "gpt-4o-transcribe", settings, entries, clip).await;
    }
    transcribe_gemini_audio(app, settings, entries, clip).await
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
    clip: &AudioClip,
) -> Result<String, String> {
    let wav_path = temp_voice_file_path("apple-speech", "wav");
    let context_path = temp_voice_file_path("apple-speech-context", "json");
    fs::write(&wav_path, &clip.wav).map_err(|e| e.to_string())?;
    let context = serde_json::json!({
        "contextualStrings": apple_speech_contextual_strings(entries),
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

fn apple_speech_contextual_strings(entries: &[DictionaryEntry]) -> Vec<String> {
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
            if values.len() >= 100 {
                return values;
            }
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
    let phrases = dictionary::enabled_phrases(entries);
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
    clip: &AudioClip,
) -> Result<String, String> {
    let key = secrets::get_secret("openai")
        .map_err(|_| "OpenAI APIキーを保存してください。".to_string())?;
    let dictionary_context = dictionary::prompt_lines(entries);
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
    clip: &AudioClip,
) -> Result<String, String> {
    let key = gemini_api_key(app)?;
    let dictionary_context = dictionary::prompt_lines(entries);
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
    let (system, user) = match mode {
        VoiceMode::Dictation => {
            let profile = dictation_profile.expect("dictation profile");
            (
                profile.system_prompt.clone(),
                prompts::voice_mode_user(&profile.user_prompt, &dictionary_section, transcript),
            )
        }
        VoiceMode::Ask if selected_text.trim().is_empty() => (
            prompts::ask_without_selection_system(&settings.prompts.overrides).to_string(),
            prompts::ask_without_selection_user(
                &settings.prompts.overrides,
                &dictionary_section,
                transcript,
            ),
        ),
        VoiceMode::Ask => (
            prompts::ask_with_selection_system(&settings.prompts.overrides).to_string(),
            prompts::ask_with_selection_user(
                &settings.prompts.overrides,
                &dictionary_section,
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
impl AxFocusedText {
    fn capture() -> Option<Self> {
        let focused = AxFocusedElement::capture()?;
        let snapshot = focused.element.read_text_snapshot()?;
        Some(Self {
            element: focused.element,
            snapshot,
        })
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
    if !should_attempt_paste(preferred_target) {
        return false;
    }

    let learning_target = AxFocusedText::capture();
    let ok = perform_clipboard_paste(text);

    if ok {
        if let Some(target) = learning_target {
            start_dictionary_learning_watch(app.clone(), target);
        }
    }
    ok
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
fn start_dictionary_learning_watch(app: tauri::AppHandle, target: AxFocusedText) {
    let Some(after_paste) = target.element.read_text_snapshot() else {
        return;
    };
    if after_paste.pid != target.snapshot.pid {
        return;
    }
    let Some(inserted_range) = inserted_range_from_snapshots(&target.snapshot, &after_paste) else {
        return;
    };

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

#[cfg(target_os = "macos")]
fn paste_text(text: &str, preferred_target: Option<&PasteTargetInfo>) -> bool {
    if !should_attempt_paste(preferred_target) {
        return false;
    }
    perform_clipboard_paste(text)
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
fn should_attempt_paste(preferred_target: Option<&PasteTargetInfo>) -> bool {
    resolve_paste_target_info(preferred_target).is_some()
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
    fn samples_to_wav_writes_valid_header_and_samples() {
        let samples = [0_i16, i16::MAX, i16::MIN + 1, 42];
        let wav = samples_to_wav(&samples, 16_000, 1).expect("wav");
        let cursor = Cursor::new(wav);
        let reader = hound::WavReader::new(cursor).expect("reader");

        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.into_samples::<i16>().count(), samples.len());
    }

    #[test]
    fn prepare_recorded_audio_rejects_short_clip() {
        let samples = vec![2_000_i16; 8_000];

        let err = prepare_recorded_audio_for_api(&samples, 16_000, 1).expect_err("too short");

        assert!(err.contains("短すぎる"));
    }

    #[test]
    fn prepare_recorded_audio_rejects_silent_clip() {
        let samples = vec![0_i16; 16_000];

        let err = prepare_recorded_audio_for_api(&samples, 16_000, 1).expect_err("silent");

        assert!(err.contains("音声が検出"));
    }

    #[test]
    fn prepare_recorded_audio_accepts_audible_clip() {
        let samples = vec![2_000_i16; 16_000];

        let prepared = prepare_recorded_audio_for_api(&samples, 16_000, 1).expect("audible");

        assert!((prepared.analysis.duration_secs - 1.0).abs() < 0.001);
        assert!(prepared.analysis.active_audio_secs >= MIN_ACTIVE_AUDIO_SECS);
    }

    #[test]
    fn prepare_recorded_audio_trims_edge_silence() {
        let mut samples = Vec::new();
        samples.extend(vec![0_i16; 500]);
        samples.extend(vec![2_000_i16; 1_000]);
        samples.extend(vec![0_i16; 500]);

        let prepared = prepare_recorded_audio_for_api(&samples, 1_000, 1).expect("trimmed");

        assert!(prepared.samples.len() < samples.len());
        assert!((prepared.analysis.duration_secs - 1.8).abs() < 0.001);
    }

    #[test]
    fn prepare_recorded_audio_compresses_internal_silence() {
        let mut samples = Vec::new();
        samples.extend(vec![2_000_i16; 1_000]);
        samples.extend(vec![0_i16; 2_000]);
        samples.extend(vec![2_000_i16; 1_000]);

        let prepared = prepare_recorded_audio_for_api(&samples, 1_000, 1).expect("trimmed");

        assert!(prepared.samples.len() < samples.len());
        assert!((prepared.analysis.duration_secs - 2.9).abs() < 0.001);
    }

    #[test]
    fn prepare_recorded_audio_preserves_low_volume_tail_before_stop() {
        let mut samples = Vec::new();
        samples.extend(vec![2_000_i16; 1_000]);
        samples.extend(vec![80_i16; 800]);
        samples.extend(vec![0_i16; 500]);

        let prepared = prepare_recorded_audio_for_api(&samples, 1_000, 1).expect("tail preserved");

        assert_eq!(prepared.samples.len(), samples.len());
    }

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

    fn paste_target(role: &str, subrole: &str, attributes: &[&str]) -> PasteTargetInfo {
        PasteTargetInfo {
            pid: None,
            role: role.to_string(),
            subrole: subrole.to_string(),
            attributes: attributes.iter().map(|value| value.to_string()).collect(),
        }
    }
}
