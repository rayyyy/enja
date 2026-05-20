use serde::{Deserialize, Serialize};
use std::sync::RwLock;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SpeechProfile {
    #[default]
    GoogleChirp3,
    DeepgramNova3,
    OpenAiGpt4oTranscribe,
    OpenAiGpt4oMiniTranscribe,
    GeminiAudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FinalizationModel {
    Gemini31ProPreview,
    #[default]
    Gemini35Flash,
    Gemini31FlashLite,
}

impl FinalizationModel {
    pub fn model_id(self) -> &'static str {
        match self {
            FinalizationModel::Gemini31ProPreview => "gemini-3.1-pro-preview",
            FinalizationModel::Gemini35Flash => "gemini-3.5-flash",
            FinalizationModel::Gemini31FlashLite => "gemini-3.1-flash-lite",
        }
    }

    pub fn thinking_level(self) -> &'static str {
        match self {
            FinalizationModel::Gemini31ProPreview => "low",
            FinalizationModel::Gemini35Flash | FinalizationModel::Gemini31FlashLite => "minimal",
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
    #[serde(default)]
    pub selected_microphone_id: Option<String>,
    #[serde(default)]
    pub speech_profile: SpeechProfile,
    #[serde(default)]
    pub finalization_model: FinalizationModel,
    #[serde(default = "default_interaction_sounds_enabled")]
    pub interaction_sounds_enabled: bool,
    #[serde(default = "default_mute_system_audio_during_recording")]
    pub mute_system_audio_during_recording: bool,
    #[serde(default = "default_max_recording_seconds")]
    pub max_recording_seconds: u64,
    #[serde(default)]
    pub google_cloud_project_id: String,
    #[serde(default = "default_google_cloud_region")]
    pub google_cloud_region: String,
    #[serde(default = "default_google_cloud_use_adc")]
    pub google_cloud_use_adc: bool,
}

fn default_threshold() -> u64 {
    400
}

fn default_target_language() -> UiLanguage {
    UiLanguage::Ja
}

fn default_interaction_sounds_enabled() -> bool {
    true
}

fn default_mute_system_audio_during_recording() -> bool {
    true
}

fn default_max_recording_seconds() -> u64 {
    300
}

fn default_google_cloud_region() -> String {
    "asia-northeast1".to_string()
}

fn default_google_cloud_use_adc() -> bool {
    true
}

impl AppSettings {
    /// Ensures `source_language` and `target_language` differ (en/ja pair only).
    pub fn sanitize(&mut self) {
        if self.source_language == self.target_language {
            self.target_language = self.source_language.other();
        }
        self.max_recording_seconds = self.max_recording_seconds.clamp(5, 600);
        if self.google_cloud_region.trim().is_empty() {
            self.google_cloud_region = default_google_cloud_region();
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
            selected_microphone_id: None,
            speech_profile: SpeechProfile::default(),
            finalization_model: FinalizationModel::default(),
            interaction_sounds_enabled: default_interaction_sounds_enabled(),
            mute_system_audio_during_recording: default_mute_system_audio_during_recording(),
            max_recording_seconds: default_max_recording_seconds(),
            google_cloud_project_id: String::new(),
            google_cloud_region: default_google_cloud_region(),
            google_cloud_use_adc: default_google_cloud_use_adc(),
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

pub struct SettingsStore {
    settings: RwLock<AppSettings>,
}

impl SettingsStore {
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        Ok(Self {
            settings: RwLock::new(load_settings(app)?),
        })
    }

    pub fn with_defaults() -> Self {
        Self {
            settings: RwLock::new(AppSettings::default()),
        }
    }

    pub fn get(&self) -> AppSettings {
        self.settings
            .read()
            .map(|settings| settings.clone())
            .unwrap_or_default()
    }

    pub fn replace(&self, settings: AppSettings) {
        if let Ok(mut guard) = self.settings.write() {
            *guard = settings;
        }
    }
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
