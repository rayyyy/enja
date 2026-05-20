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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutAction {
    VoiceDictation,
    VoiceAsk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ShortcutModifiers {
    pub command: bool,
    pub option: bool,
    pub control: bool,
    pub shift: bool,
    pub function: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShortcutBinding {
    pub key_code: Option<i64>,
    pub key: String,
    pub label: String,
    pub modifiers: ShortcutModifiers,
}

impl ShortcutBinding {
    pub fn fn_key() -> Self {
        Self {
            key_code: None,
            key: "fn".to_string(),
            label: "Fn".to_string(),
            modifiers: ShortcutModifiers::default(),
        }
    }

    pub fn fn_space() -> Self {
        Self::from_parts(
            Some(49),
            "space".to_string(),
            "Space".to_string(),
            ShortcutModifiers {
                function: true,
                ..ShortcutModifiers::default()
            },
        )
    }

    pub fn from_parts(
        key_code: Option<i64>,
        key: String,
        base_label: String,
        modifiers: ShortcutModifiers,
    ) -> Self {
        let mut binding = Self {
            key_code,
            key,
            label: base_label,
            modifiers,
        };
        binding.normalize();
        binding
    }

    pub fn normalize(&mut self) {
        self.key = self.key.trim().to_ascii_lowercase();
        self.label = if self.is_fn_key() {
            "Fn".to_string()
        } else {
            format_shortcut_label(&self.modifiers, self.label.trim())
        };
    }

    pub fn is_fn_key(&self) -> bool {
        self.key_code.is_none() && self.key == "fn"
    }

    pub fn is_same_shortcut(&self, other: &Self) -> bool {
        self.key_code == other.key_code
            && self.key == other.key
            && self.modifiers == other.modifiers
            && self.is_fn_key() == other.is_fn_key()
    }
}

impl Default for ShortcutBinding {
    fn default() -> Self {
        Self::fn_key()
    }
}

fn format_shortcut_label(modifiers: &ShortcutModifiers, base_label: &str) -> String {
    let mut parts = Vec::new();
    if modifiers.control {
        parts.push("Ctrl".to_string());
    }
    if modifiers.option {
        parts.push("Option".to_string());
    }
    if modifiers.shift {
        parts.push("Shift".to_string());
    }
    if modifiers.command {
        parts.push("Cmd".to_string());
    }
    if modifiers.function {
        parts.push("Fn".to_string());
    }
    if !base_label.trim().is_empty() {
        parts.push(base_label.trim().to_string());
    }
    parts.join(" ")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PromptOverrides {
    pub translate_en_to_ja: Option<String>,
    pub translate_ja_to_en: Option<String>,
    pub openai_transcription: Option<String>,
    pub gemini_audio_system: Option<String>,
    pub gemini_audio_user: Option<String>,
    pub dictation_system: Option<String>,
    pub dictation_user: Option<String>,
    pub ask_without_selection_system: Option<String>,
    pub ask_without_selection_user: Option<String>,
    pub ask_with_selection_system: Option<String>,
    pub ask_with_selection_user: Option<String>,
}

impl PromptOverrides {
    pub fn sanitize(&mut self) {
        fn clean(value: &mut Option<String>) {
            if value.as_ref().is_some_and(|s| s.trim().is_empty()) {
                *value = None;
            }
        }

        clean(&mut self.translate_en_to_ja);
        clean(&mut self.translate_ja_to_en);
        clean(&mut self.openai_transcription);
        clean(&mut self.gemini_audio_system);
        clean(&mut self.gemini_audio_user);
        clean(&mut self.dictation_system);
        clean(&mut self.dictation_user);
        clean(&mut self.ask_without_selection_system);
        clean(&mut self.ask_without_selection_user);
        clean(&mut self.ask_with_selection_system);
        clean(&mut self.ask_with_selection_user);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptTemplates {
    pub translate_en_to_ja: String,
    pub translate_ja_to_en: String,
    pub openai_transcription: String,
    pub gemini_audio_system: String,
    pub gemini_audio_user: String,
    pub dictation_system: String,
    pub dictation_user: String,
    pub ask_without_selection_system: String,
    pub ask_without_selection_user: String,
    pub ask_with_selection_system: String,
    pub ask_with_selection_user: String,
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
    #[serde(default = "default_voice_dictation_shortcut")]
    pub voice_dictation_shortcut: ShortcutBinding,
    #[serde(default = "default_voice_ask_shortcut")]
    pub voice_ask_shortcut: ShortcutBinding,
    #[serde(default)]
    pub prompt_overrides: PromptOverrides,
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

fn default_voice_dictation_shortcut() -> ShortcutBinding {
    ShortcutBinding::fn_key()
}

fn default_voice_ask_shortcut() -> ShortcutBinding {
    ShortcutBinding::fn_space()
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
        self.voice_dictation_shortcut.normalize();
        self.voice_ask_shortcut.normalize();
        self.prompt_overrides.sanitize();
    }

    pub fn validate_shortcuts(&self) -> Result<(), String> {
        validate_shortcut("音声入力開始/停止", &self.voice_dictation_shortcut)?;
        validate_shortcut("選択テキストへの音声指示", &self.voice_ask_shortcut)?;
        if self
            .voice_dictation_shortcut
            .is_same_shortcut(&self.voice_ask_shortcut)
        {
            return Err("音声入力と音声指示に同じショートカットは設定できません。".to_string());
        }
        Ok(())
    }
}

fn validate_shortcut(label: &str, shortcut: &ShortcutBinding) -> Result<(), String> {
    if shortcut.is_fn_key() {
        return Ok(());
    }
    let Some(key_code) = shortcut.key_code else {
        return Err(format!("{label}のショートカットを設定してください。"));
    };
    if key_code == 53 || shortcut.key == "escape" {
        return Err(
            "Escapeは録音キャンセルに使用するため、ショートカットには設定できません。".to_string(),
        );
    }
    if shortcut.key.trim().is_empty() || shortcut.label.trim().is_empty() {
        return Err(format!("{label}のショートカットを設定してください。"));
    }
    Ok(())
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
            voice_dictation_shortcut: default_voice_dictation_shortcut(),
            voice_ask_shortcut: default_voice_ask_shortcut(),
            prompt_overrides: PromptOverrides::default(),
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
