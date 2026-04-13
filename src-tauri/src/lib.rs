mod gemini;
mod keyboard;
mod settings;

use gemini::{stream_translate, TranslateEvent};
use settings::{AppSettings, load_settings, save_settings_to_disk};
use tauri::ipc::Channel;
use tauri::{Emitter, Manager};

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<AppSettings, String> {
    load_settings(&app)
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: AppSettings) -> Result<(), String> {
    save_settings_to_disk(&app, &settings)
}

#[tauri::command]
async fn translate(
    app: tauri::AppHandle,
    text: String,
    channel: Channel<TranslateEvent>,
) -> Result<(), String> {
    let settings = load_settings(&app)?;
    if settings.gemini_api_key.trim().is_empty() {
        let _ = channel.send(TranslateEvent::Error {
            message: "先に設定で Gemini API キーを保存してください。".to_string(),
        });
        return Ok(());
    }
    stream_translate(
        &settings.gemini_api_key,
        &text,
        channel,
        settings.source_language,
        settings.target_language,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (trigger_tx, trigger_rx) = std::sync::mpsc::channel::<()>();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let settings = load_settings(&app.handle()).unwrap_or_default();
            let threshold = settings.double_tap_threshold_ms;
            keyboard::spawn_listener(trigger_tx, threshold);

            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                while trigger_rx.recv().is_ok() {
                    let runner = app_handle.clone();
                    let work = app_handle.clone();
                    let _ = runner.run_on_main_thread(move || {
                        let text = read_clipboard_text();
                        if let Some(w) = work.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                        let _ = work.emit("enja-trigger", text);
                    });
                }
            });

            if settings.gemini_api_key.trim().is_empty() {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            translate,
            hide_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
