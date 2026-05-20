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
            let base_label = shortcut_base_label(&self.label, &self.key);
            format_shortcut_label(&self.modifiers, &base_label)
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

fn shortcut_base_label(label: &str, key: &str) -> String {
    let mut parts = label.split_whitespace().collect::<Vec<_>>();
    while parts
        .first()
        .is_some_and(|part| is_shortcut_modifier_label(part))
    {
        parts.remove(0);
    }

    let stripped = parts.join(" ");
    if stripped.trim().is_empty() {
        fallback_key_label(key)
    } else {
        stripped
    }
}

fn is_shortcut_modifier_label(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "ctrl" | "control" | "option" | "alt" | "shift" | "cmd" | "command" | "fn"
    )
}

fn fallback_key_label(key: &str) -> String {
    match key {
        "space" => "Space".to_string(),
        "return" => "Return".to_string(),
        "tab" => "Tab".to_string(),
        "escape" => "Escape".to_string(),
        "delete" => "Delete".to_string(),
        value if value.len() == 1 => value.to_ascii_uppercase(),
        value => value.to_string(),
    }
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
pub struct PromptCatalogItem {
    pub key: String,
    pub label: String,
    pub rows: u8,
    pub required: Vec<String>,
    pub default_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TranslationSettings {
    pub source_language: UiLanguage,
    pub target_language: UiLanguage,
}

impl Default for TranslationSettings {
    fn default() -> Self {
        Self {
            source_language: UiLanguage::En,
            target_language: UiLanguage::Ja,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SystemAudioHandling {
    #[default]
    Mute,
    Isolate,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoiceSettings {
    pub selected_microphone_id: Option<String>,
    pub speech_profile: SpeechProfile,
    pub finalization_model: FinalizationModel,
    pub interaction_sounds_enabled: bool,
    pub system_audio_handling: SystemAudioHandling,
    pub max_recording_seconds: u64,
    pub google_cloud_project_id: String,
    pub google_cloud_region: String,
    pub google_cloud_use_adc: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            selected_microphone_id: None,
            speech_profile: SpeechProfile::default(),
            finalization_model: FinalizationModel::default(),
            interaction_sounds_enabled: true,
            system_audio_handling: SystemAudioHandling::Mute,
            max_recording_seconds: 300,
            google_cloud_project_id: String::new(),
            google_cloud_region: default_google_cloud_region(),
            google_cloud_use_adc: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShortcutSettings {
    pub voice_dictation: ShortcutBinding,
    pub voice_ask: ShortcutBinding,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            voice_dictation: ShortcutBinding::fn_key(),
            voice_ask: ShortcutBinding::fn_space(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PromptSettings {
    pub overrides: PromptOverrides,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppBehaviorSettings {
    pub double_tap_threshold_ms: u64,
    pub launch_at_login: bool,
}

impl Default for AppBehaviorSettings {
    fn default() -> Self {
        Self {
            double_tap_threshold_ms: 400,
            launch_at_login: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub translation: TranslationSettings,
    pub voice: VoiceSettings,
    pub shortcuts: ShortcutSettings,
    pub prompts: PromptSettings,
    pub app: AppBehaviorSettings,
}

fn default_google_cloud_region() -> String {
    "asia-northeast1".to_string()
}

impl AppSettings {
    pub fn sanitize(&mut self) {
        if self.translation.source_language == self.translation.target_language {
            self.translation.target_language = self.translation.source_language.other();
        }
        self.voice.max_recording_seconds = self.voice.max_recording_seconds.clamp(5, 600);
        if self.voice.google_cloud_region.trim().is_empty() {
            self.voice.google_cloud_region = default_google_cloud_region();
        }
        self.app.double_tap_threshold_ms = self.app.double_tap_threshold_ms.clamp(100, 2000);
        self.shortcuts.voice_dictation.normalize();
        self.shortcuts.voice_ask.normalize();
        self.prompts.overrides.sanitize();
    }

    pub fn validate_shortcuts(&self) -> Result<(), String> {
        validate_shortcut("音声入力開始/停止", &self.shortcuts.voice_dictation)?;
        validate_shortcut("選択テキストへの音声指示", &self.shortcuts.voice_ask)?;
        if self
            .shortcuts
            .voice_dictation
            .is_same_shortcut(&self.shortcuts.voice_ask)
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
    let mut settings: AppSettings = serde_json::from_str(&data).unwrap_or_default();
    settings.sanitize();
    Ok(settings)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_nested_settings_contract() {
        let settings = AppSettings::default();
        assert_eq!(settings.translation.source_language, UiLanguage::En);
        assert_eq!(settings.translation.target_language, UiLanguage::Ja);
        assert_eq!(settings.app.double_tap_threshold_ms, 400);
        assert_eq!(settings.shortcuts.voice_dictation.label, "Fn");
    }

    #[test]
    fn sanitize_clamps_and_normalizes_nested_values() {
        let mut settings = AppSettings::default();
        settings.translation.target_language = UiLanguage::En;
        settings.voice.max_recording_seconds = 999;
        settings.voice.google_cloud_region = String::new();
        settings.app.double_tap_threshold_ms = 5;
        settings.sanitize();

        assert_eq!(settings.translation.target_language, UiLanguage::Ja);
        assert_eq!(settings.voice.max_recording_seconds, 600);
        assert_eq!(settings.voice.google_cloud_region, "asia-northeast1");
        assert_eq!(settings.app.double_tap_threshold_ms, 100);
    }

    #[test]
    fn shortcut_normalize_does_not_duplicate_modifier_labels() {
        let mut shortcut = ShortcutBinding::from_parts(
            Some(49),
            "space".to_string(),
            "Fn Space".to_string(),
            ShortcutModifiers {
                function: true,
                ..ShortcutModifiers::default()
            },
        );

        shortcut.normalize();
        shortcut.normalize();

        assert_eq!(shortcut.label, "Fn Space");
    }

    #[test]
    fn shortcut_normalize_repairs_existing_duplicated_fn_labels() {
        let mut shortcut = ShortcutBinding {
            key_code: Some(49),
            key: "space".to_string(),
            label: "Fn Fn Fn Space".to_string(),
            modifiers: ShortcutModifiers {
                function: true,
                ..ShortcutModifiers::default()
            },
        };

        shortcut.normalize();

        assert_eq!(shortcut.label, "Fn Space");
    }
}
