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
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Sample;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Write};
#[cfg(target_os = "macos")]
use std::os::raw::c_void;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use system_tap::SystemTap;
use tauri::{Emitter, Manager};

const SPEECH_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const AUDIO_INPUT_DEVICES_CHANGED_EVENT: &str = "audio-input-devices-changed";
const VOICE_WINDOW_EDGE_MARGIN: f64 = 16.0;
const VOICE_WINDOW_BOTTOM_MARGIN: f64 = 42.0;
const VOICE_WINDOW_FOLLOW_INTERVAL_MS: u64 = 180;
const MIN_API_RECORDING_SECS: f32 = 0.7;
const VOICE_FRAME_MS: u32 = 20;
const ACTIVE_AUDIO_RMS_THRESHOLD: f32 = 0.006;
const MIN_ACTIVE_AUDIO_SECS: f32 = 0.08;
const EDGE_SILENCE_PADDING_MS: u32 = 160;
const MAX_INTERNAL_SILENCE_MS: u32 = 320;
static VOICE_STATE_SEQ: AtomicU64 = AtomicU64::new(1);
static VOICE_WINDOW_FOLLOW_SEQ: AtomicU64 = AtomicU64::new(0);

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
    recorder: Recorder,
    audio_aux: Option<AudioAux>,
}

#[derive(Debug, Clone)]
struct VoiceModeProfileSnapshot {
    id: String,
    name: String,
    formatting_enabled: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecorderCommand {
    Finish,
    Cancel,
}

struct AudioClip {
    wav: Vec<u8>,
    duration_secs: f32,
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

type RecorderSetup = (Arc<Mutex<Vec<i16>>>, u32, u16, cpal::Stream);

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

        if mode == VoiceMode::Dictation {
            show_voice_window(&app, false);
            emit_state_once(&app, "recording", Some(mode), mode_profile.clone(), None);
        }

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
            recorder,
            audio_aux,
        } = session;
        let mode_profile = profile_snapshot_for_id(&app, &mode_profile_id);
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
            "processing",
            Some(mode),
            mode_profile.clone(),
            Some(processing_message.to_string()),
        );

        let clip = recorder.finish_after_signal(move || {
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
                let inserted = paste_text(&text);
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
        match guard.as_mut() {
            Some(SessionState::Starting {
                mode,
                mode_profile_id,
                ..
            }) if *mode == VoiceMode::Dictation => {
                *mode_profile_id = next_id;
            }
            Some(SessionState::Active(session)) if session.mode == VoiceMode::Dictation => {
                session.mode_profile_id = next_id;
            }
            _ => return Ok(()),
        }
        drop(guard);

        emit_state(
            &app,
            "recording",
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

    if mode == VoiceMode::Ask {
        show_voice_window(&app, false);
        emit_state_once(&app, "recording", Some(mode), None, None);
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
    let recorder = tokio::task::spawn_blocking(move || {
        Recorder::start(
            app_for_recorder,
            microphone_id,
            max_recording_seconds,
            pipeline_for_recorder,
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
        recorder,
        audio_aux,
    }));
    drop(guard);

    emit_state(
        &app,
        "recording",
        Some(mode),
        profile_snapshot_for_id(&app, &starting_mode_profile_id),
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
    if settings.voice.interaction_sounds_enabled {
        play_interaction_sound("start");
    }
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
    ) -> Result<Self, String> {
        let (control_tx, control_rx) = std::sync::mpsc::channel::<RecorderCommand>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<AudioClip, String>>();
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        std::thread::spawn(move || {
            let result = run_recording_thread(
                app,
                selected_device_id,
                max_recording_seconds,
                control_rx,
                init_tx,
                pipeline,
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

    fn finish_after_signal(self, after_signal: impl FnOnce()) -> Result<AudioClip, String> {
        let _ = self.control_tx.send(RecorderCommand::Finish);
        after_signal();
        match self.done_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(result) => result,
            Err(_) => Err("録音停止処理がタイムアウトしました。".to_string()),
        }
    }

    fn cancel(self) {
        let _ = self.control_tx.send(RecorderCommand::Cancel);
        let _ = self.done_rx.recv_timeout(Duration::from_secs(2));
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
) -> Result<AudioClip, String> {
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
                app,
                device_channels,
                aec_pipeline,
                err_fn,
            ),
            cpal::SampleFormat::I16 => build_input_stream::<i16>(
                &device,
                &config,
                samples.clone(),
                last_emit,
                max_samples,
                app,
                device_channels,
                aec_pipeline,
                err_fn,
            ),
            cpal::SampleFormat::U16 => build_input_stream::<u16>(
                &device,
                &config,
                samples.clone(),
                last_emit,
                max_samples,
                app,
                device_channels,
                aec_pipeline,
                err_fn,
            ),
            _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
        }
        .map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;
        Ok((samples, output_sample_rate, output_channels, stream))
    })();

    let (samples, sample_rate, channels, stream) = match setup {
        Ok(values) => {
            let _ = init_tx.send(Ok(()));
            values
        }
        Err(err) => {
            let _ = init_tx.send(Err(err.clone()));
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

    if command == RecorderCommand::Cancel {
        return Err("録音をキャンセルしました。".to_string());
    }

    let samples = samples.lock().map_err(|e| e.to_string())?.clone();
    if samples.is_empty() {
        return Err("音声が録音されていません。".to_string());
    }
    let prepared = prepare_recorded_audio_for_api(&samples, sample_rate, channels)?;
    let wav = samples_to_wav(&prepared.samples, sample_rate, channels)?;
    Ok(AudioClip {
        wav,
        duration_secs: prepared.analysis.duration_secs,
    })
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
    let frames = samples
        .chunks(frame_len)
        .map(|frame| frame_rms(frame) >= ACTIVE_AUDIO_RMS_THRESHOLD)
        .collect::<Vec<_>>();
    let active_frames = frames.iter().filter(|active| **active).count();
    let Some(first_active) = frames.iter().position(|active| *active) else {
        return PreparedAudio {
            samples: Vec::new(),
            analysis: PreparedAudioAnalysis {
                duration_secs: 0.0,
                active_audio_secs: 0.0,
            },
        };
    };
    let last_active = frames
        .iter()
        .rposition(|active| *active)
        .unwrap_or(first_active);
    let edge_padding_frames = ms_to_frame_count(EDGE_SILENCE_PADDING_MS);
    let max_internal_silence_frames = ms_to_frame_count(MAX_INTERNAL_SILENCE_MS);
    let start_frame = first_active.saturating_sub(edge_padding_frames);
    let end_frame = (last_active + 1 + edge_padding_frames).min(frames.len());
    let mut trimmed = Vec::with_capacity(samples.len());
    let mut internal_silence_frames = 0usize;

    for frame_index in start_frame..end_frame {
        let active = frames[frame_index];
        let is_edge_padding = frame_index < first_active || frame_index > last_active;
        let should_keep = if active || is_edge_padding {
            internal_silence_frames = 0;
            true
        } else if internal_silence_frames < max_internal_silence_frames {
            internal_silence_frames += 1;
            true
        } else {
            false
        };

        if should_keep {
            let start = frame_index * frame_len;
            let end = (start + frame_len).min(samples.len());
            trimmed.extend_from_slice(&samples[start..end]);
        }
    }

    PreparedAudio {
        analysis: PreparedAudioAnalysis {
            duration_secs: audio_duration_secs(trimmed.len(), sample_rate, channels),
            active_audio_secs: active_frames as f32 * VOICE_FRAME_MS as f32 / 1000.0,
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

fn audio_duration_secs(sample_count: usize, sample_rate: u32, channels: usize) -> f32 {
    let samples_per_second = sample_rate as usize * channels.max(1);
    if samples_per_second == 0 {
        0.0
    } else {
        sample_count as f32 / samples_per_second as f32
    }
}

fn frame_rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples
        .iter()
        .map(|sample| {
            let value = (*sample as f32 / i16::MAX as f32).clamp(-1.0, 1.0);
            value * value
        })
        .sum::<f32>();
    (sum / samples.len() as f32).sqrt()
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
                pipeline.drain_frames(|frame| {
                    if let Ok(mut guard) = samples_buf.lock() {
                        let remaining = max_samples.saturating_sub(guard.len());
                        for value in frame.iter().take(remaining) {
                            guard.push((value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                        }
                    }
                });
            } else if let Ok(mut guard) = samples.lock() {
                let remaining = max_samples.saturating_sub(guard.len());
                for sample in data.iter().take(remaining) {
                    let value = f32::from_sample(*sample).clamp(-1.0, 1.0);
                    peak = peak.max(value.abs());
                    sum += value * value;
                    count += 1;
                    guard.push((value * i16::MAX as f32) as i16);
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
    _app: &tauri::AppHandle,
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
    let transcript = transcribe(app, &settings, &entries, &clip).await?;
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
    let phrase_values = phrases
        .iter()
        .take(500)
        .map(|value| serde_json::json!({ "value": value, "boost": 12.0 }))
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
        return Ok(transcript.trim().to_string());
    }

    let key = gemini_api_key(app)?;
    let dictionary_context = dictionary::prompt_lines(entries);
    let dictionary_section = if dictionary_context.trim().is_empty() {
        "優先表記辞書は空です。".to_string()
    } else {
        format!("優先表記辞書:\n{dictionary_context}")
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

fn emit_state_once(
    app: &tauri::AppHandle,
    state: &'static str,
    mode: Option<VoiceMode>,
    mode_profile: Option<VoiceModeProfileSnapshot>,
    message: Option<String>,
) {
    let seq = next_voice_state_seq();
    let (mode_profile_id, mode_profile_name) = state_profile_fields(mode_profile);
    let _ = app.emit(
        "voice-state",
        VoiceStateEvent {
            state,
            mode,
            mode_profile_id,
            mode_profile_name,
            message,
            seq,
        },
    );
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

fn next_voice_state_seq() -> u64 {
    VOICE_STATE_SEQ.fetch_add(1, Ordering::SeqCst)
}

fn show_voice_window(app: &tauri::AppHandle, expanded: bool) {
    let Some(window) = app.get_webview_window("voice") else {
        crate::keyboard::set_voice_overlay_visible(false);
        return;
    };
    let monitor_key = configure_voice_window(app, &window, expanded);
    let _ = window.set_always_on_top(true);
    if window.show().is_ok() {
        crate::keyboard::set_voice_overlay_visible(true);
    }
    start_voice_window_follow(app, expanded, monitor_key);
}

fn configure_voice_window(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    expanded: bool,
) -> Option<VoiceWindowMonitorKey> {
    let target_monitor = voice_window_target_monitor(app);
    let monitor_key = target_monitor.as_ref().map(voice_window_monitor_key);
    let scale = target_monitor
        .as_ref()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or_else(|| window.scale_factor().unwrap_or(1.0))
        .max(1.0);
    let (mut width, mut height) = if expanded {
        (840.0_f64, 420.0_f64)
    } else {
        (292.0_f64, 42.0_f64)
    };
    if let Some(monitor) = target_monitor.as_ref() {
        let size = monitor.size();
        let logical_width = size.width as f64 / scale;
        let logical_height = size.height as f64 / scale;
        width = width.min((logical_width - 40.0).max(260.0));
        height = height.min((logical_height - 88.0).max(if expanded { 260.0 } else { 40.0 }));
    }
    let _ = window.set_focusable(expanded);
    let _ = window.set_shadow(expanded);
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
    expanded: bool,
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
                current_monitor = configure_voice_window(&app, &window, expanded);
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
fn paste_text(text: &str) -> bool {
    if !should_attempt_paste() {
        return false;
    }
    let original = read_clipboard_text();
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if clipboard.set_text(text.to_string()).is_err() {
            return false;
        }
    } else {
        return false;
    }
    let ok = run_keystroke("v");
    std::thread::sleep(Duration::from_millis(180));
    restore_clipboard(original);
    ok
}

#[cfg(not(target_os = "macos"))]
fn paste_text(_text: &str) -> bool {
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
fn should_attempt_paste() -> bool {
    let script = r#"
tell application "System Events"
  try
    set frontApp to first application process whose frontmost is true
    set focusedElement to value of attribute "AXFocusedUIElement" of frontApp
    set roleValue to ""
    set subroleValue to ""
    set attributeNames to {}
    try
      set roleValue to value of attribute "AXRole" of focusedElement as text
    end try
    try
      set subroleValue to value of attribute "AXSubrole" of focusedElement as text
    end try
    try
      set attributeNames to name of every attribute of focusedElement
    end try
    if roleValue is "AXTextArea" then return "1"
    if roleValue is "AXTextField" then return "1"
    if roleValue is "AXComboBox" then return "1"
    if roleValue is "AXSearchField" then return "1"
    if subroleValue is "AXTextArea" then return "1"
    if subroleValue is "AXTextField" then return "1"
    if attributeNames contains "AXSelectedTextRange" then return "1"
    if attributeNames contains "AXInsertionPointLineNumber" then return "1"
    if roleValue is "AXButton" then return "0"
    if roleValue is "AXCheckBox" then return "0"
    if roleValue is "AXRadioButton" then return "0"
    if roleValue is "AXMenuItem" then return "0"
    if roleValue is "AXMenuButton" then return "0"
    if roleValue is "AXPopUpButton" then return "0"
    if roleValue is "AXSlider" then return "0"
    if roleValue is "AXScrollBar" then return "0"
    if roleValue is "AXToolbar" then return "0"
    if roleValue is "AXTabGroup" then return "0"
    if roleValue is "AXWindow" then return "0"
    if roleValue is "AXSheet" then return "0"
    if roleValue is "AXMenuBar" then return "0"
    if roleValue is "AXMenu" then return "0"
    if roleValue is "AXList" then return "0"
    if roleValue is "AXOutline" then return "0"
    if roleValue is "AXTable" then return "0"
    if roleValue is "AXRow" then return "0"
    if roleValue is "AXCell" then return "0"
    -- Electron/WebKit editors often expose the focused editor as AXGroup,
    -- AXWebArea, AXScrollArea, or AXUnknown. Treat unclear roles as pasteable
    -- and let the actual Cmd+V decide whether insertion is possible.
    if roleValue is "AXStaticText" then return "0"
    return "1"
  on error
    return "1"
  end try
end tell
"#;
    std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
        .unwrap_or(true)
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

#[allow(dead_code)]
fn write_debug_audio(path: &std::path::Path, wav: &[u8]) {
    if let Ok(mut file) = std::fs::File::create(path) {
        let _ = file.write_all(wav);
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
        assert!((prepared.analysis.duration_secs - 1.32).abs() < 0.001);
    }

    #[test]
    fn prepare_recorded_audio_compresses_internal_silence() {
        let mut samples = Vec::new();
        samples.extend(vec![2_000_i16; 1_000]);
        samples.extend(vec![0_i16; 2_000]);
        samples.extend(vec![2_000_i16; 1_000]);

        let prepared = prepare_recorded_audio_for_api(&samples, 1_000, 1).expect("trimmed");

        assert!(prepared.samples.len() < samples.len());
        assert!((prepared.analysis.duration_secs - 2.32).abs() < 0.001);
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
}
