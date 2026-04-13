use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::Manager;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UiLanguage {
    #[default]
    En,
    Ja,
}

impl UiLanguage {
    pub fn other(self) -> Self {
        match self {
            UiLanguage::En => UiLanguage::Ja,
            UiLanguage::Ja => UiLanguage::En,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    #[serde(default)]
    pub gemini_api_key: String,
    #[serde(default = "default_threshold")]
    pub double_tap_threshold_ms: u64,
    #[serde(default)]
    pub source_language: UiLanguage,
    #[serde(default = "default_target_language")]
    pub target_language: UiLanguage,
    #[serde(default)]
    pub launch_at_login: bool,
}

fn default_threshold() -> u64 {
    400
}

fn default_target_language() -> UiLanguage {
    UiLanguage::Ja
}

impl AppSettings {
    /// Ensures `source_language` and `target_language` differ (en/ja pair only).
    pub fn sanitize(&mut self) {
        if self.source_language == self.target_language {
            self.target_language = self.source_language.other();
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            gemini_api_key: String::new(),
            double_tap_threshold_ms: default_threshold(),
            source_language: UiLanguage::En,
            target_language: default_target_language(),
            launch_at_login: false,
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
    let mut s: AppSettings = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    s.sanitize();
    Ok(s)
}

pub fn save_settings_to_disk(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    let mut settings = settings.clone();
    settings.sanitize();
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
