mod dictionary;
mod gemini;
mod keyboard;
#[cfg(target_os = "macos")]
mod macos_show_on_activate;
mod prompts;
mod secrets;
mod settings;
mod voice;

use dictionary::{DictionaryEntry, DictionaryEntryInput};
use gemini::{stream_translate, TranslateEvent};
use keyboard::KeyboardTrigger;
use serde::Serialize;
use settings::{
    load_settings, save_settings_to_disk, AppSettings, PromptCatalogItem, SettingsStore,
    ShortcutAction, ShortcutBinding, SpeechProfile,
};
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, Runtime, State};
use tauri_plugin_autostart::ManagerExt;
use voice::{AudioInputDevice, SpeechSetupCheck, VoiceManager, VoiceMode};

fn show_main_window<R: Runtime>(app: &impl Manager<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn apply_launch_at_login(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        app.autolaunch().enable().map_err(|e| e.to_string())
    } else {
        app.autolaunch().disable().map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<AppSettings, String> {
    let settings = app
        .try_state::<SettingsStore>()
        .map(|store| store.get())
        .unwrap_or_else(|| load_settings(&app).unwrap_or_default());
    Ok(settings)
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: AppSettings) -> Result<(), String> {
    let mut sanitized = settings.clone();
    sanitized.sanitize();
    sanitized.validate_shortcuts()?;
    sanitized.voice.validate_mode_profiles()?;
    prompts::validate_overrides(&sanitized.prompts.overrides)?;

    save_settings_to_disk(&app, &sanitized)?;
    if let Some(store) = app.try_state::<SettingsStore>() {
        store.replace(sanitized.clone());
    }
    keyboard::update_runtime_settings(keyboard::KeyboardRuntimeSettings::from(&sanitized));
    apply_launch_at_login(&app, sanitized.app.launch_at_login)?;
    Ok(())
}

#[tauri::command]
fn get_prompt_catalog() -> Vec<PromptCatalogItem> {
    prompts::catalog()
}

#[tauri::command]
fn start_shortcut_capture(action: ShortcutAction) -> Result<(), String> {
    keyboard::begin_shortcut_capture(action)
}

#[tauri::command]
fn cancel_shortcut_capture() -> Result<(), String> {
    keyboard::cancel_shortcut_capture()
}

#[tauri::command]
async fn translate(
    app: tauri::AppHandle,
    text: String,
    channel: Channel<TranslateEvent>,
) -> Result<(), String> {
    let settings = load_settings(&app)?;
    let api_key = secrets::get_secret("gemini").unwrap_or_default();
    if api_key.trim().is_empty() {
        let _ = channel.send(TranslateEvent::Error {
            message: "先に設定で Gemini API キーを保存してください。".to_string(),
        });
        return Ok(());
    }
    stream_translate(
        &api_key,
        &text,
        channel,
        settings.translation.source_language,
        settings.translation.target_language,
        &settings.prompts.overrides,
    )
    .await
}

#[tauri::command]
fn hide_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        w.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn list_audio_input_devices() -> Result<Vec<AudioInputDevice>, String> {
    voice::list_audio_input_devices()
}

#[tauri::command]
fn start_voice_session(
    app: tauri::AppHandle,
    manager: State<'_, VoiceManager>,
    mode: VoiceMode,
) -> Result<(), String> {
    manager.start_session(app, mode)
}

#[tauri::command]
async fn stop_voice_session(
    app: tauri::AppHandle,
    manager: State<'_, VoiceManager>,
) -> Result<(), String> {
    manager.stop_session(app).await
}

#[tauri::command]
fn cancel_voice_session(
    app: tauri::AppHandle,
    manager: State<'_, VoiceManager>,
) -> Result<(), String> {
    manager.cancel_session(app)
}

#[tauri::command]
fn get_dictionary(app: tauri::AppHandle) -> Result<Vec<DictionaryEntry>, String> {
    dictionary::load_dictionary(&app)
}

#[tauri::command]
fn create_dictionary_entry(
    app: tauri::AppHandle,
    entry: DictionaryEntryInput,
) -> Result<DictionaryEntry, String> {
    dictionary::create_entry(&app, entry)
}

#[tauri::command]
fn update_dictionary_entry(
    app: tauri::AppHandle,
    id: String,
    entry: DictionaryEntryInput,
) -> Result<DictionaryEntry, String> {
    dictionary::update_entry(&app, &id, entry)
}

#[tauri::command]
fn delete_dictionary_entry(app: tauri::AppHandle, id: String) -> Result<(), String> {
    dictionary::delete_entry(&app, &id)
}

#[tauri::command]
fn save_provider_secret(provider: String, secret: String) -> Result<(), String> {
    secrets::save_secret(&provider, &secret)
}

#[tauri::command]
fn get_provider_status() -> Result<secrets::ProviderStatus, String> {
    Ok(secrets::provider_status())
}

#[tauri::command]
async fn check_speech_setup(
    app: tauri::AppHandle,
    profile: SpeechProfile,
    settings: AppSettings,
) -> Result<SpeechSetupCheck, String> {
    voice::check_speech_profile_setup(&app, profile, settings).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (trigger_tx, trigger_rx) = std::sync::mpsc::channel::<KeyboardTrigger>();

    tauri::Builder::default()
        .manage(VoiceManager::new())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(move |app| {
            let settings_store = SettingsStore::new(app.handle()).unwrap_or_else(|err| {
                eprintln!("[enja] settings cache init failed: {err}");
                SettingsStore::with_defaults()
            });
            app.manage(settings_store);

            let settings = app.state::<SettingsStore>().get();
            if let Err(e) = apply_launch_at_login(app.handle(), settings.app.launch_at_login) {
                eprintln!("[enja] launch at login: {e}");
            }
            keyboard::spawn_listener(
                trigger_tx,
                keyboard::KeyboardRuntimeSettings::from(&settings),
            );

            #[cfg(target_os = "macos")]
            macos_show_on_activate::init(app.handle().clone());

            std::thread::spawn(voice::prewarm_microphone);
            voice::spawn_audio_input_device_watcher(app.handle().clone());

            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                while let Ok(trigger) = trigger_rx.recv() {
                    let runner = app_handle.clone();
                    let work = app_handle.clone();
                    let _ = runner.run_on_main_thread(move || {
                        handle_keyboard_trigger(work, trigger);
                    });
                }
            });

            // アプリ起動時は常にメインウィンドウを表示（以前は API キーがあると非表示のままだった）
            show_main_window(app.handle());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            translate,
            hide_window,
            list_audio_input_devices,
            start_voice_session,
            stop_voice_session,
            cancel_voice_session,
            get_dictionary,
            create_dictionary_entry,
            update_dictionary_entry,
            delete_dictionary_entry,
            save_provider_secret,
            get_provider_status,
            check_speech_setup,
            get_prompt_catalog,
            start_shortcut_capture,
            cancel_shortcut_capture
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                // Dock アイコンや「ウィンドウを開く」相当の操作で前面に出す
                show_main_window(app_handle);
            }
        });
}

fn handle_keyboard_trigger(app: tauri::AppHandle, trigger: KeyboardTrigger) {
    match trigger {
        KeyboardTrigger::CmdCopyDouble => {
            let text = read_clipboard_text();
            show_main_window(&app);
            let _ = app.emit("enja-trigger", text);
        }
        KeyboardTrigger::FunctionTap => {
            // The keyboard listener emits this on Fn *release* only when no
            // Space was pressed during the hold — so the user's intent is
            // unambiguously "Dictation toggle".
            let Some(manager) = app.try_state::<VoiceManager>() else {
                return;
            };
            if manager.is_active() {
                let app_for_task = app.clone();
                tauri::async_runtime::spawn(async move {
                    let Some(manager) = app_for_task.try_state::<VoiceManager>() else {
                        return;
                    };
                    let _ = manager.stop_session(app_for_task.clone()).await;
                });
            } else {
                let _ = manager.start_session(app.clone(), VoiceMode::Dictation);
            }
        }
        KeyboardTrigger::FunctionSpace => {
            let Some(manager) = app.try_state::<VoiceManager>() else {
                return;
            };
            if !manager.is_active() {
                let _ = manager.start_session(app.clone(), VoiceMode::Ask);
            }
        }
        KeyboardTrigger::VoiceModeCycle => {
            if let Some(manager) = app.try_state::<VoiceManager>() {
                if let Err(err) = manager.cycle_mode_profile(app.clone()) {
                    eprintln!("[enja] voice mode cycle failed: {err}");
                }
            }
        }
        KeyboardTrigger::Escape => {
            if let Some(manager) = app.try_state::<VoiceManager>() {
                let _ = manager.cancel_session(app.clone());
            }
        }
        KeyboardTrigger::ShortcutCaptured { action, shortcut } => {
            let _ = app.emit(
                "shortcut-captured",
                ShortcutCapturedEvent { action, shortcut },
            );
        }
        KeyboardTrigger::ShortcutCaptureCancelled { action, reason } => {
            let _ = app.emit(
                "shortcut-capture-cancelled",
                ShortcutCaptureCancelledEvent { action, reason },
            );
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutCapturedEvent {
    action: ShortcutAction,
    shortcut: ShortcutBinding,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutCaptureCancelledEvent {
    action: ShortcutAction,
    reason: String,
}

fn read_clipboard_text() -> String {
    match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[enja] clipboard read failed: {e}");
            String::new()
        }
    }
}
