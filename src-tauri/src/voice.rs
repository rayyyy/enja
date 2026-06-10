mod aec;
mod audio;
mod cache;
mod devices;
mod dictionary_learning;
mod events;
mod live;
mod paste;
mod recorder;
mod screen_context;
mod system_tap;
mod text_diff;
mod transcribe;
mod window;

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
pub(crate) use devices::*;
pub(crate) use dictionary_learning::*;
pub(crate) use events::*;
pub(crate) use live::*;
pub(crate) use paste::*;
pub(crate) use recorder::*;
use screen_context::{
    finalization_screen_context_section, resolve_voice_screen_context, screen_context_terms,
    should_capture_voice_screen_context_ocr, start_voice_screen_context_capture,
    transcription_contextual_phrases, transcription_prompt_context, VoiceScreenContext,
    VoiceScreenContextOcr,
};
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
use text_diff::{changed_span, utf16_offset_to_byte_index, utf16_range_text, TextRange};
use tokio::sync::oneshot;
pub(crate) use transcribe::*;
pub(crate) use window::*;

const POLISH_SELECTION_INSTRUCTION: &str = "推敲して";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoiceMode {
    Dictation,
    Ask,
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
    Active(Box<ActiveSession>),
    Processing {
        cancelled: ProcessingCancel,
    },
}

type ProcessingCancel = Arc<AtomicBool>;

struct ActiveSession {
    mode: VoiceMode,
    mode_profile_id: String,
    /// Ask モードの選択テキスト取得(AX 直読みか Cmd+C 合成)。録音開始を
    /// ブロックしないようバックグラウンドで走らせ、確定時に合流する。
    selected_text_task: Option<tokio::task::JoinHandle<String>>,
    screen_context: VoiceScreenContext,
    screen_context_ocr_rx: Option<oneshot::Receiver<Option<VoiceScreenContextOcr>>>,
    recorder: Recorder,
    audio_aux: Option<AudioAux>,
}

#[derive(Debug, Clone)]
pub(crate) struct VoiceModeProfileSnapshot {
    id: String,
    name: String,
    formatting_enabled: bool,
}

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

        self.stop_active_session(app, *session, cancelled).await
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
            selected_text_task,
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
        // 録音停止(VAD トリム+WAV 化まで含むブロッキング処理)と OCR 結果の
        // 解決を並行に走らせる。OCR を ASR 送信前に待つ仕様は維持しつつ、
        // 待ち時間を録音停止処理の裏に隠す。
        let finish_task = tokio::task::spawn_blocking(move || {
            recorder.finish(move || {
                if let Some(aux) = audio_aux {
                    aux.stop();
                }
                if should_play_stop_sound {
                    play_interaction_sound("stop");
                }
            })
        });
        let selected_text_future = async {
            match selected_text_task {
                Some(handle) => handle.await.unwrap_or_else(|e| {
                    eprintln!("[enja] 選択テキスト取得タスクが失敗: {e}");
                    String::new()
                }),
                None => String::new(),
            }
        };
        let (clip, screen_context, selected_text) = tokio::join!(
            finish_task,
            resolve_voice_screen_context(screen_context, screen_context_ocr_rx),
            selected_text_future
        );
        let clip = clip.unwrap_or_else(|e| Err(e.to_string()));

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
                    paste_text_with_dictionary_learning(&app, &text)
                } else {
                    paste_text(&text)
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
                            "カーソル位置への貼り付けを確認できなかったため、コピー用に表示しています。".to_string(),
                        ),
                    );
                    emit_result(
                        &app,
                        VoiceResultEvent {
                            text,
                            inserted: false,
                            reason: Some(
                                "カーソル位置への貼り付けを確認できませんでした。".to_string(),
                            ),
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

    let screen_context_ocr_enabled = should_capture_voice_screen_context_ocr(
        &settings,
        mode,
        &settings.voice.active_mode_profile_id,
    );

    // Google ASR を使う見込みなら、確定時のトークン取得 RTT(ADC では gcloud
    // 呼び出し)を録音中に済ませておく。結果は cache 側が保持する。
    prefetch_google_speech_token(&settings, mode);

    // Ask の選択テキスト取得(AX 直読み。失敗時は Cmd+C 合成で最大 700ms)は
    // 録音開始をブロックしないようバックグラウンドへ逃し、確定時に合流する。
    let selected_text_task =
        (mode == VoiceMode::Ask).then(|| tokio::task::spawn_blocking(capture_selected_text));

    // 画面文脈の取得(AX 走査+osascript)と音声パイプライン準備(システム音声の
    // ミュート/分離)は互いに独立なので並行に走らせる。どちらも同期 I/O のため
    // spawn_blocking に出し、async ワーカーを塞がない。
    let app_for_context = app.clone();
    let settings_for_context = settings.clone();
    let target_for_context = paste_target.clone();
    let context_task = tokio::task::spawn_blocking(move || {
        start_voice_screen_context_capture(
            &app_for_context,
            &settings_for_context,
            target_for_context.as_ref(),
            screen_context_ocr_enabled,
        )
    });
    let settings_for_pipeline = settings.clone();
    let pipeline_task =
        tokio::task::spawn_blocking(move || prepare_audio_pipeline(&settings_for_pipeline));

    let (screen_context_capture, pipeline) = tokio::join!(context_task, pipeline_task);
    // join 失敗で Starting 状態を残すと以後のセッションが開始できなくなる。
    let screen_context_capture = match screen_context_capture {
        Ok(capture) => capture,
        Err(e) => {
            let err = e.to_string();
            if let Ok((Some(aux), _)) = pipeline {
                aux.stop();
            }
            fail_start_session(&app, mode, &cancelled, err.clone());
            return Err(err);
        }
    };
    let (audio_aux, pipeline_mode) = match pipeline {
        Ok(pipeline) => pipeline,
        Err(e) => {
            let err = e.to_string();
            fail_start_session(&app, mode, &cancelled, err.clone());
            return Err(err);
        }
    };

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
    let recorder = match tokio::task::spawn_blocking(move || {
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
    {
        Ok(result) => result,
        Err(e) => {
            // join 失敗で Starting 状態を残すと以後のセッションが開始できなくなる。
            if let Some(aux) = audio_aux {
                aux.stop();
            }
            let err = e.to_string();
            fail_start_session(&app, mode, &cancelled, err.clone());
            return Err(err);
        }
    };

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
    *guard = Some(SessionState::Active(Box::new(ActiveSession {
        mode,
        mode_profile_id: starting_mode_profile_id.clone(),
        selected_text_task,
        screen_context: screen_context_capture.context,
        screen_context_ocr_rx: screen_context_capture.ocr_rx,
        recorder,
        audio_aux,
    })));
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
    // 選択テキスト取得(AX 失敗時は Cmd+C 合成で最大 700ms)と画面文脈取得を
    // 並行に走らせ、推敲開始の体感を軽くする。
    let app_for_context = app.clone();
    let settings_for_context = settings.clone();
    let target_for_context = paste_target.clone();
    let context_task = tokio::task::spawn_blocking(move || {
        start_voice_screen_context_capture(
            &app_for_context,
            &settings_for_context,
            target_for_context.as_ref(),
            false,
        )
    });
    let selected_text_task = tokio::task::spawn_blocking(capture_selected_text);
    let (screen_context_capture, selected_text) = tokio::join!(context_task, selected_text_task);
    // join 失敗で Processing 状態を残すと以後のセッションが開始できなくなる。
    let fail_polish = |err: String| {
        if clear_processing_session_for_app(&app, &cancelled) {
            show_voice_window(&app, true);
            emit_state(&app, "error", Some(VoiceMode::Ask), None, Some(err.clone()));
        }
        err
    };
    let screen_context_capture = match screen_context_capture {
        Ok(capture) => capture,
        Err(e) => return Err(fail_polish(e.to_string())),
    };
    let selected_text = match selected_text {
        Ok(text) => text,
        Err(e) => return Err(fail_polish(e.to_string())),
    };
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
                    Some(VoiceMode::Ask),
                    None,
                    Some("カーソル位置への貼り付けを確認できなかったため、コピー用に表示しています。".to_string()),
                );
                emit_result(
                    &app,
                    VoiceResultEvent {
                        text,
                        inserted: false,
                        reason: Some(
                            "カーソル位置への貼り付けを確認できませんでした。".to_string(),
                        ),
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

async fn process_clip(
    app: &tauri::AppHandle,
    mode: VoiceMode,
    mode_profile_id: &str,
    selected_text: &str,
    screen_context: &VoiceScreenContext,
    clip: AudioClip,
) -> Result<String, String> {
    // 設定はメモリ上の SettingsStore から取る(確定パスでのディスク再読込を避ける)。
    let settings = app
        .try_state::<SettingsStore>()
        .map(|store| store.get())
        .ok_or_else(|| "SettingsStore is unavailable.".to_string())?;
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
    let settings = app
        .try_state::<SettingsStore>()
        .map(|store| store.get())
        .ok_or_else(|| "SettingsStore is unavailable.".to_string())?;
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
