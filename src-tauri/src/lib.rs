mod gemini;
mod keyboard;
mod settings;

use gemini::{stream_translate, TranslateEvent};
use settings::{AppSettings, load_settings, save_settings_to_disk};
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, Runtime};
use tauri_plugin_autostart::ManagerExt;

fn show_main_window<R: Runtime>(app: &impl Manager<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
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
    load_settings(&app)
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: AppSettings) -> Result<(), String> {
    save_settings_to_disk(&app, &settings)?;
    apply_launch_at_login(&app, settings.launch_at_login)?;
    Ok(())
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
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let settings = load_settings(&app.handle()).unwrap_or_default();
            let threshold = settings.double_tap_threshold_ms;
            if let Err(e) = apply_launch_at_login(&app.handle(), settings.launch_at_login) {
                eprintln!("[enja] launch at login: {e}");
            }
            keyboard::spawn_listener(trigger_tx, threshold);

            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                while trigger_rx.recv().is_ok() {
                    let runner = app_handle.clone();
                    let work = app_handle.clone();
                    let _ = runner.run_on_main_thread(move || {
                        let text = read_clipboard_text();
                        show_main_window(&work);
                        let _ = work.emit("enja-trigger", text);
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
            hide_window
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

fn read_clipboard_text() -> String {
    match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[enja] clipboard read failed: {e}");
            String::new()
        }
    }
}
