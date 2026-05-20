use crate::dictionary::{self, DictionaryEntry};
use crate::gemini;
use crate::prompts;
use crate::secrets;
use crate::settings::{AppSettings, SettingsStore, SpeechProfile};
use base64::Engine;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Sample;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};

const SPEECH_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

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
    message: Option<String>,
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
        cancelled: Arc<Mutex<bool>>,
    },
    Active(ActiveSession),
}

struct ActiveSession {
    mode: VoiceMode,
    selected_text: String,
    recorder: Recorder,
    system_audio_mute: Option<SystemAudioMuteGuard>,
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

impl VoiceManager {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }

    pub fn start_session(&self, app: tauri::AppHandle, mode: VoiceMode) -> Result<(), String> {
        let mut guard = self.active.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Ok(());
        }

        if mode == VoiceMode::Dictation {
            show_voice_window(&app, false);
            emit_state_once(&app, "recording", Some(mode), None);
        }

        let cancelled = Arc::new(Mutex::new(false));
        *guard = Some(SessionState::Starting {
            mode,
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
        let session = {
            let mut guard = self.active.lock().map_err(|e| e.to_string())?;
            guard.take()
        };
        let Some(session) = session else {
            return Ok(());
        };

        match session {
            SessionState::Starting { cancelled, .. } => {
                if let Ok(mut flag) = cancelled.lock() {
                    *flag = true;
                }
                emit_state(&app, "idle", None, None);
                hide_voice_window(&app);
                Ok(())
            }
            SessionState::Active(session) => self.stop_active_session(app, session).await,
        }
    }

    async fn stop_active_session(
        &self,
        app: tauri::AppHandle,
        session: ActiveSession,
    ) -> Result<(), String> {
        let mode = session.mode;
        show_voice_window(&app, false);
        emit_state(
            &app,
            "processing",
            Some(mode),
            Some("音声を整形しています…".to_string()),
        );

        let clip = session.recorder.finish();
        if let Some(guard) = session.system_audio_mute {
            guard.stop();
        }

        if app
            .try_state::<SettingsStore>()
            .map(|store| store.get().interaction_sounds_enabled)
            .unwrap_or(false)
        {
            play_interaction_sound("stop");
        }

        let clip = match clip {
            Ok(clip) => clip,
            Err(message) => {
                show_voice_window(&app, true);
                emit_state(&app, "error", Some(mode), Some(message.clone()));
                return Err(message);
            }
        };
        let result = process_clip(&app, mode, &session.selected_text, clip).await;
        match result {
            Ok(text) => {
                let inserted = paste_text(&text);
                if inserted {
                    emit_state(&app, "inserted", Some(mode), None);
                    emit_result(
                        &app,
                        VoiceResultEvent {
                            text,
                            inserted: true,
                            reason: None,
                        },
                    );
                    hide_voice_window_after(app, Duration::from_millis(1300));
                } else {
                    show_voice_window(&app, true);
                    emit_state(
                        &app,
                        "fallback",
                        Some(mode),
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
                emit_state(&app, "error", Some(mode), Some(message.clone()));
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
                if let Some(guard) = session.system_audio_mute {
                    guard.stop();
                }
            }
            None => {}
        }
        emit_state(&app, "idle", None, None);
        hide_voice_window(&app);
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.active.lock().is_ok_and(|guard| guard.is_some())
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
        emit_state_once(&app, "recording", Some(mode), None);
    }

    let system_audio_mute = prepare_system_audio(&settings);

    if is_start_cancelled(&cancelled) {
        if let Some(guard) = system_audio_mute {
            guard.stop();
        }
        return Ok(());
    }

    let app_for_recorder = app.clone();
    let microphone_id = settings.selected_microphone_id.clone();
    let max_recording_seconds = settings.max_recording_seconds;
    let recorder = tokio::task::spawn_blocking(move || {
        Recorder::start(app_for_recorder, microphone_id, max_recording_seconds)
    })
    .await
    .map_err(|e| e.to_string())?;

    if is_start_cancelled(&cancelled) {
        if let Some(guard) = system_audio_mute {
            guard.stop();
        }
        if let Ok(recorder) = recorder {
            recorder.cancel();
        }
        clear_starting_session(&app);
        emit_state(&app, "idle", None, None);
        hide_voice_window(&app);
        return Ok(());
    }

    let recorder = match recorder {
        Ok(recorder) => recorder,
        Err(err) => {
            if let Some(guard) = system_audio_mute {
                guard.stop();
            }
            fail_start_session(&app, mode, err.clone());
            return Err(err);
        }
    };

    let manager = app
        .try_state::<VoiceManager>()
        .ok_or_else(|| "VoiceManager is unavailable.".to_string())?;
    let mut guard = manager.active.lock().map_err(|e| e.to_string())?;
    let Some(SessionState::Starting {
        mode: starting_mode,
        cancelled: starting_cancelled,
    }) = guard.as_ref()
    else {
        recorder.cancel();
        if let Some(guard) = system_audio_mute {
            guard.stop();
        }
        return Ok(());
    };

    if *starting_mode != mode || is_start_cancelled(starting_cancelled) {
        recorder.cancel();
        if let Some(guard) = system_audio_mute {
            guard.stop();
        }
        return Ok(());
    }

    *guard = Some(SessionState::Active(ActiveSession {
        mode,
        selected_text,
        recorder,
        system_audio_mute,
    }));
    drop(guard);

    emit_state(&app, "recording", Some(mode), None);
    Ok(())
}

fn is_start_cancelled(cancelled: &Arc<Mutex<bool>>) -> bool {
    cancelled.lock().is_ok_and(|flag| *flag)
}

fn clear_starting_session(app: &tauri::AppHandle) {
    let Some(manager) = app.try_state::<VoiceManager>() else {
        return;
    };
    let Ok(mut guard) = manager.active.lock() else {
        return;
    };
    if matches!(guard.as_ref(), Some(SessionState::Starting { .. })) {
        *guard = None;
    }
}

fn fail_start_session(app: &tauri::AppHandle, mode: VoiceMode, err: String) {
    clear_starting_session(app);
    show_voice_window(app, true);
    emit_state(app, "error", Some(mode), Some(err));
}

fn prepare_system_audio(settings: &AppSettings) -> Option<SystemAudioMuteGuard> {
    if settings.mute_system_audio_during_recording {
        if settings.interaction_sounds_enabled {
            play_interaction_sound("start");
        }
        Some(SystemAudioMuteGuard::start())
    } else {
        if settings.interaction_sounds_enabled {
            play_interaction_sound("start");
        }
        None
    }
}

pub fn prewarm_microphone() {
    let host = cpal::default_host();
    if let Some(device) = host.default_input_device() {
        let _ = device.default_input_config();
    }
    let _ = list_audio_input_devices();
}

impl Recorder {
    fn start(
        app: tauri::AppHandle,
        selected_device_id: Option<String>,
        max_recording_seconds: u64,
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

    fn finish(self) -> Result<AudioClip, String> {
        let _ = self.control_tx.send(RecorderCommand::Finish);
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
) -> Result<AudioClip, String> {
    let started_at = Instant::now();
    let setup: Result<(Arc<Mutex<Vec<i16>>>, u32, u16, cpal::Stream), String> = (|| {
        let host = cpal::default_host();
        let device = input_device_by_id(&host, selected_device_id.as_deref())?
            .or_else(|| host.default_input_device())
            .ok_or_else(|| "利用できるマイクが見つかりません。".to_string())?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();
        let config: cpal::StreamConfig = supported.clone().into();
        let samples = Arc::new(Mutex::new(Vec::<i16>::new()));
        let max_samples =
            sample_rate as usize * channels as usize * max_recording_seconds.clamp(5, 600) as usize;
        let last_emit = Arc::new(Mutex::new(Instant::now()));
        let err_fn = |err| eprintln!("[enja] audio input stream error: {err}");

        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build_input_stream::<f32>(
                &device,
                &config,
                samples.clone(),
                last_emit,
                max_samples,
                app,
                err_fn,
            ),
            cpal::SampleFormat::I16 => build_input_stream::<i16>(
                &device,
                &config,
                samples.clone(),
                last_emit,
                max_samples,
                app,
                err_fn,
            ),
            cpal::SampleFormat::U16 => build_input_stream::<u16>(
                &device,
                &config,
                samples.clone(),
                last_emit,
                max_samples,
                app,
                err_fn,
            ),
            _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
        }
        .map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;
        Ok((samples, sample_rate, channels, stream))
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
    let duration_secs = started_at.elapsed().as_secs_f32();
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let path = std::env::temp_dir().join(format!(
        "enja-voice-{}.wav",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    {
        let mut writer = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
        for sample in samples {
            writer.write_sample(sample).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;
    }
    let wav = std::fs::read(&path).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(path);
    Ok(AudioClip { wav, duration_secs })
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Mutex<Vec<i16>>>,
    last_emit: Arc<Mutex<Instant>>,
    max_samples: usize,
    app: tauri::AppHandle,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::Sample + cpal::SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            let mut peak = 0.0f32;
            let mut sum = 0.0f32;
            if let Ok(mut guard) = samples.lock() {
                let remaining = max_samples.saturating_sub(guard.len());
                for sample in data.iter().take(remaining) {
                    let value = f32::from_sample(*sample).clamp(-1.0, 1.0);
                    peak = peak.max(value.abs());
                    sum += value * value;
                    guard.push((value * i16::MAX as f32) as i16);
                }
            }
            if !data.is_empty() {
                let rms = (sum / data.len() as f32).sqrt().clamp(0.0, 1.0);
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
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();
    let mut out = Vec::new();
    for (idx, device) in host.input_devices().map_err(|e| e.to_string())?.enumerate() {
        let name = device
            .name()
            .unwrap_or_else(|_| "名称未取得のマイク".to_string());
        out.push(AudioInputDevice {
            id: format!("{}#{idx}", name),
            is_default: name == default_name,
            name,
        });
    }
    Ok(out)
}

fn input_device_by_id(
    host: &cpal::Host,
    selected_id: Option<&str>,
) -> Result<Option<cpal::Device>, String> {
    let Some(selected_id) = selected_id else {
        return Ok(None);
    };
    for (idx, device) in host.input_devices().map_err(|e| e.to_string())?.enumerate() {
        let name = device.name().unwrap_or_default();
        if selected_id == format!("{}#{idx}", name) {
            return Ok(Some(device));
        }
    }
    Ok(None)
}

pub async fn check_speech_profile_setup(
    _app: &tauri::AppHandle,
    profile: SpeechProfile,
    settings: AppSettings,
) -> Result<SpeechSetupCheck, String> {
    match profile {
        SpeechProfile::GoogleChirp3 => check_google_chirp3_setup(&settings).await,
        SpeechProfile::DeepgramNova3 => Ok(check_secret_setup(
            "Deepgram APIキー",
            "deepgram",
            "Deepgram APIキーが保存されています。",
            "Deepgram APIキーを保存してください。",
        )),
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
    if settings.google_cloud_project_id.trim().is_empty() {
        missing.push("Google Cloud Project ID");
    }
    if settings.google_cloud_region.trim().is_empty() {
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
                    settings.google_cloud_project_id.trim(),
                    settings.google_cloud_region.trim()
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
    selected_text: &str,
    clip: AudioClip,
) -> Result<String, String> {
    let settings = crate::settings::load_settings(app)?;
    let entries = dictionary::load_dictionary(app)?;
    let transcript = transcribe(app, &settings, &entries, &clip).await?;
    finalize_text(app, &settings, &entries, mode, selected_text, &transcript).await
}

async fn transcribe(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    clip: &AudioClip,
) -> Result<String, String> {
    match settings.speech_profile {
        SpeechProfile::GoogleChirp3 => {
            if clip.duration_secs > 60.0 || clip.wav.len() > 10 * 1024 * 1024 {
                transcribe_long_audio_fallback(app, settings, entries, clip).await
            } else {
                transcribe_google_chirp3(settings, entries, clip).await
            }
        }
        SpeechProfile::DeepgramNova3 => transcribe_deepgram(entries, clip).await,
        SpeechProfile::OpenAiGpt4oTranscribe => {
            transcribe_openai("gpt-4o-transcribe", settings, entries, clip).await
        }
        SpeechProfile::OpenAiGpt4oMiniTranscribe => {
            transcribe_openai("gpt-4o-mini-transcribe", settings, entries, clip).await
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
        return transcribe_openai("gpt-4o-transcribe", settings, entries, clip).await;
    }
    if secrets::get_secret("deepgram").is_ok_and(|key| !key.trim().is_empty()) {
        return transcribe_deepgram(entries, clip).await;
    }
    transcribe_gemini_audio(app, settings, entries, clip).await
}

fn http_client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())
}

fn speech_request_error(provider: &str, err: reqwest::Error) -> String {
    if err.is_timeout() {
        format!("{provider}の応答がタイムアウトしました。短く録音するか、別の音声認識モデルを試してください。")
    } else {
        err.to_string()
    }
}

async fn transcribe_google_chirp3(
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    clip: &AudioClip,
) -> Result<String, String> {
    if clip.duration_secs > 60.0 || clip.wav.len() > 10 * 1024 * 1024 {
        return Err(
            "Google Chirp 3の同期認識は1分/10MBまでです。長い録音はDeepgramまたはOpenAIを選択してください。"
                .to_string(),
        );
    }
    let project = settings.google_cloud_project_id.trim();
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
    let region = settings.google_cloud_region.trim();
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
    if settings.google_cloud_use_adc {
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
                return Ok((
                    token,
                    vec![
                        "認証方式: ADC".to_string(),
                        format!("gcloud: {}", gcloud.display()),
                    ],
                ));
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
    Ok((
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

async fn transcribe_deepgram(
    entries: &[DictionaryEntry],
    clip: &AudioClip,
) -> Result<String, String> {
    let key = secrets::get_secret("deepgram")
        .map_err(|_| "Deepgram APIキーを保存してください。".to_string())?;
    let phrases = dictionary::enabled_phrases(entries);
    let mut url =
        reqwest::Url::parse("https://api.deepgram.com/v1/listen").map_err(|e| e.to_string())?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("model", "nova-3");
        q.append_pair("language", "ja");
        q.append_pair("smart_format", "true");
        for phrase in phrases.iter().take(100) {
            q.append_pair("keyterm", phrase);
        }
    }
    let response = http_client(SPEECH_REQUEST_TIMEOUT)?
        .post(url)
        .header("Authorization", format!("Token {key}"))
        .header("Content-Type", "audio/wav")
        .body(clip.wav.clone())
        .send()
        .await
        .map_err(|e| speech_request_error("Deepgram", e))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Deepgram HTTP {status}: {text}"));
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let out = v
        .pointer("/results/channels/0/alternatives/0/transcript")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if out.is_empty() {
        Err("Deepgramの文字起こし結果が空でした。".to_string())
    } else {
        Ok(out)
    }
}

async fn transcribe_openai(
    model: &str,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    clip: &AudioClip,
) -> Result<String, String> {
    let key = secrets::get_secret("openai")
        .map_err(|_| "OpenAI APIキーを保存してください。".to_string())?;
    let dictionary_context = dictionary::prompt_lines(entries);
    let prompt =
        prompts::openai_transcription_prompt(&settings.prompt_overrides, &dictionary_context);
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
    let prompt = prompts::gemini_audio_user(&settings.prompt_overrides, &dictionary_context);
    let system = prompts::gemini_audio_system(&settings.prompt_overrides);
    gemini::generate_from_audio(
        &key,
        settings.finalization_model.model_id(),
        settings.finalization_model.thinking_level(),
        system.as_ref(),
        &prompt,
        &clip.wav,
        0.1,
    )
    .await
}

async fn finalize_text(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    mode: VoiceMode,
    selected_text: &str,
    transcript: &str,
) -> Result<String, String> {
    let key = gemini_api_key(app)?;
    let dictionary_context = dictionary::prompt_lines(entries);
    let dictionary_section = if dictionary_context.trim().is_empty() {
        "優先表記辞書は空です。".to_string()
    } else {
        format!("優先表記辞書:\n{dictionary_context}")
    };
    let (system, user) = match mode {
        VoiceMode::Dictation => (
            prompts::dictation_system(&settings.prompt_overrides),
            prompts::dictation_user(&settings.prompt_overrides, &dictionary_section, transcript),
        ),
        VoiceMode::Ask if selected_text.trim().is_empty() => (
            prompts::ask_without_selection_system(&settings.prompt_overrides),
            prompts::ask_without_selection_user(
                &settings.prompt_overrides,
                &dictionary_section,
                transcript,
            ),
        ),
        VoiceMode::Ask => (
            prompts::ask_with_selection_system(&settings.prompt_overrides),
            prompts::ask_with_selection_user(
                &settings.prompt_overrides,
                &dictionary_section,
                selected_text,
                transcript,
            ),
        ),
    };
    gemini::generate_text(
        &key,
        settings.finalization_model.model_id(),
        settings.finalization_model.thinking_level(),
        system.as_ref(),
        &user,
        0.2,
    )
    .await
    .map(|s| s.trim().to_string())
}

fn gemini_api_key(app: &tauri::AppHandle) -> Result<String, String> {
    if let Ok(key) = secrets::get_secret("gemini") {
        if !key.trim().is_empty() {
            return Ok(key);
        }
    }
    let settings = crate::settings::load_settings(app)?;
    if settings.gemini_api_key.trim().is_empty() {
        Err("Gemini APIキーを保存してください。".to_string())
    } else {
        Ok(settings.gemini_api_key)
    }
}

fn emit_state_once(
    app: &tauri::AppHandle,
    state: &'static str,
    mode: Option<VoiceMode>,
    message: Option<String>,
) {
    let _ = app.emit(
        "voice-state",
        VoiceStateEvent {
            state,
            mode,
            message,
        },
    );
}

fn emit_state(
    app: &tauri::AppHandle,
    state: &'static str,
    mode: Option<VoiceMode>,
    message: Option<String>,
) {
    let event = VoiceStateEvent {
        state,
        mode,
        message,
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

fn show_voice_window(app: &tauri::AppHandle, expanded: bool) {
    let Some(window) = app.get_webview_window("voice") else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    let (mut width, mut height) = if expanded {
        (840.0_f64, 420.0_f64)
    } else {
        (292.0_f64, 42.0_f64)
    };
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let size = monitor.size();
        let logical_width = size.width as f64 / scale;
        let logical_height = size.height as f64 / scale;
        width = width.min((logical_width - 40.0).max(260.0));
        height = height.min((logical_height - 88.0).max(if expanded { 260.0 } else { 40.0 }));
    }
    let _ = window.set_focusable(expanded);
    let _ = window.set_shadow(expanded);
    let _ = window.set_size(tauri::LogicalSize::new(width, height));
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let pos = monitor.position();
        let size = monitor.size();
        let logical_x = pos.x as f64 / scale;
        let logical_y = pos.y as f64 / scale;
        let logical_width = size.width as f64 / scale;
        let logical_height = size.height as f64 / scale;
        let x = logical_x + ((logical_width - width) / 2.0).max(16.0);
        let y = logical_y + logical_height - height - 42.0;
        let _ = window.set_position(tauri::LogicalPosition::new(x, y.max(logical_y + 16.0)));
    }
    let _ = window.set_always_on_top(true);
    let _ = window.show();
}

fn hide_voice_window(app: &tauri::AppHandle) {
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
        .trim_end_matches(|c| c == '\r' || c == '\n')
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
    if !focused_text_target_available() {
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
    let script =
        format!("tell application \"System Events\" to keystroke \"{key}\" using command down");
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(target_os = "macos")]
fn focused_text_target_available() -> bool {
    let script = r#"
tell application "System Events"
  try
    set frontApp to first application process whose frontmost is true
    set focusedElement to value of attribute "AXFocusedUIElement" of frontApp
    set roleValue to ""
    set subroleValue to ""
    try
      set roleValue to value of attribute "AXRole" of focusedElement as text
    end try
    try
      set subroleValue to value of attribute "AXSubrole" of focusedElement as text
    end try
    if roleValue is "AXTextArea" then return "1"
    if roleValue is "AXTextField" then return "1"
    if roleValue is "AXComboBox" then return "1"
    if roleValue is "AXSearchField" then return "1"
    if subroleValue is "AXTextArea" then return "1"
    if subroleValue is "AXTextField" then return "1"
    return "0"
  on error
    return "0"
  end try
end tell
"#;
    std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
        .unwrap_or(false)
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
