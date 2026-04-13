use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    #[serde(default)]
    pub gemini_api_key: String,
    #[serde(default = "default_threshold")]
    pub double_tap_threshold_ms: u64,
}

fn default_threshold() -> u64 {
    400
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            gemini_api_key: String::new(),
            double_tap_threshold_ms: default_threshold(),
        }
    }
}

pub fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

pub fn load_settings(app: &AppHandle) -> Result<AppSettings, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

pub fn save_settings_to_disk(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
