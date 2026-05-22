use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    #[serde(alias = "deepgramNova3")]
    GoogleChirp3,
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

pub const DEFAULT_VOICE_MODE_ID: &str = "default";
const TRANSCRIPT_TOKEN: &str = "{{transcript}}";

const DEFAULT_MODE_SYSTEM: &str = "あなたは日本語の音声入力編集者です。音声認識結果を、ユーザーがそのまま貼り付けられる自然な日本語文に整形します。出力は最終本文のみ。前置き、説明、引用符、ラベルは出しません。";

const DEFAULT_MODE_USER: &str = r#"{{dictionary_section}}

音声認識結果:
{{transcript}}

要件:
- 話し言葉の不要な言い直しを整理する。
- 録音内に「これをこうまとめて」などの指示が含まれる場合、その意図に従って最終文章を作る。
- 辞書の優先表記を必ず尊重する。
- 内容を勝手に増やさない。"#;

const SPEED_MODE_SYSTEM: &str = "あなたは日本語の音声入力編集者です。音声認識結果を必要最小限だけ整えます。出力は最終本文のみ。前置き、説明、引用符、ラベルは出しません。";

const SPEED_MODE_USER: &str = r#"{{dictionary_section}}

音声認識結果:
{{transcript}}

要件:
- 文字起こし結果を大きく変えず、明らかな誤字や不要な空白だけ整える。
- 内容を勝手に増やさない。"#;

const AI_PROMPT_MODE_SYSTEM: &str = "あなたはAIプロンプト設計者です。音声認識結果から、AIに渡しやすい明確で実行可能なプロンプトを作成します。出力はプロンプト本文のみ。前置き、説明、引用符、ラベルは出しません。";

const AI_PROMPT_MODE_USER: &str = r#"{{dictionary_section}}

話した内容:
{{transcript}}

要件:
- 話した意図を、AIへ渡す明確なプロンプトに再構成する。
- 目的、背景、入力、制約、期待する出力形式を必要に応じて整理する。
- 箇条書きや見出しは、プロンプトとして読みやすい場合だけ使う。
- 内容を勝手に増やさず、曖昧な部分は自然な依頼文としてまとめる。"#;

const CASUAL_MODE_SYSTEM: &str = "あなたは日本語チャット文の編集者です。音声認識結果を、Slackなどのチャットにそのまま送れる親しみやすい文章へ整えます。出力は最終本文のみ。前置き、説明、引用符、ラベルは出しません。";

const CASUAL_MODE_USER: &str = r#"{{dictionary_section}}

音声認識結果:
{{transcript}}

要件:
- くだけすぎない親しみやすい文体にする。
- 必要に応じて感嘆符を使い、硬さを和らげる。
- 口癖、言い直し、不要な間を整理する。
- 辞書の優先表記を必ず尊重する。
- 内容を勝手に増やさない。"#;

const FORMAL_MODE_SYSTEM: &str = "あなたは日本語ビジネス文の編集者です。音声認識結果を、メール返信などに適したやや丁寧な文章へ整えます。出力は最終本文のみ。前置き、説明、引用符、ラベルは出しません。";

const FORMAL_MODE_USER: &str = r#"{{dictionary_section}}

音声認識結果:
{{transcript}}

要件:
- メールや業務チャットで使いやすい、やや丁寧な文体にする。
- 過度に堅くしすぎず、自然な敬体で整える。
- 口癖、言い直し、不要な間を整理する。
- 辞書の優先表記を必ず尊重する。
- 内容を勝手に増やさない。"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VoiceModePresetKey {
    Default,
    Speed,
    AiPrompt,
    Casual,
    Formal,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoiceModeProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default = "default_true")]
    pub formatting_enabled: bool,
    pub system_prompt: String,
    pub user_prompt: String,
    pub deletable: bool,
    pub order: i64,
    pub preset_key: Option<VoiceModePresetKey>,
}

impl Default for VoiceModeProfile {
    fn default() -> Self {
        default_voice_mode_profile()
    }
}

fn voice_mode_profile(
    id: &str,
    name: &str,
    description: &str,
    formatting_enabled: bool,
    system_prompt: &str,
    user_prompt: &str,
    deletable: bool,
    order: i64,
    preset_key: Option<VoiceModePresetKey>,
) -> VoiceModeProfile {
    VoiceModeProfile {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        formatting_enabled,
        system_prompt: system_prompt.to_string(),
        user_prompt: user_prompt.to_string(),
        deletable,
        order,
        preset_key,
    }
}

fn default_voice_mode_profile() -> VoiceModeProfile {
    voice_mode_profile(
        DEFAULT_VOICE_MODE_ID,
        "デフォルト",
        "話した内容を自然な日本語文として整えます。",
        true,
        DEFAULT_MODE_SYSTEM,
        DEFAULT_MODE_USER,
        false,
        0,
        Some(VoiceModePresetKey::Default),
    )
}

pub fn default_voice_mode_profiles() -> Vec<VoiceModeProfile> {
    vec![
        default_voice_mode_profile(),
        voice_mode_profile(
            "speed",
            "スピード",
            "整形せず、文字起こし結果をすぐに出力します。",
            false,
            SPEED_MODE_SYSTEM,
            SPEED_MODE_USER,
            true,
            1,
            Some(VoiceModePresetKey::Speed),
        ),
        voice_mode_profile(
            "aiPrompt",
            "AIプロンプト",
            "話した内容をAIに渡しやすいプロンプトへ整えます。",
            true,
            AI_PROMPT_MODE_SYSTEM,
            AI_PROMPT_MODE_USER,
            true,
            2,
            Some(VoiceModePresetKey::AiPrompt),
        ),
        voice_mode_profile(
            "casual",
            "カジュアル",
            "Slackなどに合う親しみやすい文体へ整えます。",
            true,
            CASUAL_MODE_SYSTEM,
            CASUAL_MODE_USER,
            true,
            3,
            Some(VoiceModePresetKey::Casual),
        ),
        voice_mode_profile(
            "formal",
            "フォーマル",
            "メール返信などに合うやや丁寧な文体へ整えます。",
            true,
            FORMAL_MODE_SYSTEM,
            FORMAL_MODE_USER,
            true,
            4,
            Some(VoiceModePresetKey::Formal),
        ),
    ]
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
    pub mode_profiles: Vec<VoiceModeProfile>,
    pub active_mode_profile_id: String,
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
            mode_profiles: default_voice_mode_profiles(),
            active_mode_profile_id: DEFAULT_VOICE_MODE_ID.to_string(),
        }
    }
}

impl VoiceSettings {
    fn ensure_mode_profiles(&mut self, overrides: &PromptOverrides) {
        if self.mode_profiles.is_empty() {
            self.mode_profiles = default_voice_mode_profiles();
            if let Some(default_profile) = self
                .mode_profiles
                .iter_mut()
                .find(|profile| profile.id == DEFAULT_VOICE_MODE_ID)
            {
                if let Some(system_prompt) = overrides.dictation_system.as_deref() {
                    default_profile.system_prompt = system_prompt.trim().to_string();
                }
                if let Some(user_prompt) = overrides.dictation_user.as_deref() {
                    default_profile.user_prompt = user_prompt.trim().to_string();
                }
            }
        }

        if !self
            .mode_profiles
            .iter()
            .any(|profile| profile.id == DEFAULT_VOICE_MODE_ID)
        {
            self.mode_profiles.insert(0, default_voice_mode_profile());
        }

        let mut seen = HashSet::new();
        for (index, profile) in self.mode_profiles.iter_mut().enumerate() {
            profile.id = profile.id.trim().to_string();
            if profile.id.is_empty() {
                profile.id = format!("custom-{index}");
            }
            if !seen.insert(profile.id.clone()) {
                profile.id = format!("custom-{index}");
                seen.insert(profile.id.clone());
            }
            profile.name = profile.name.trim().to_string();
            profile.description = profile.description.trim().to_string();
            profile.system_prompt = profile.system_prompt.trim().to_string();
            profile.user_prompt = profile.user_prompt.trim().to_string();
            if profile.id == DEFAULT_VOICE_MODE_ID {
                profile.deletable = false;
                profile.preset_key = Some(VoiceModePresetKey::Default);
            }
        }

        self.mode_profiles.sort_by(|a, b| a.order.cmp(&b.order));
        for (index, profile) in self.mode_profiles.iter_mut().enumerate() {
            profile.order = index as i64;
        }

        if self.active_mode_profile_id.trim().is_empty()
            || !self
                .mode_profiles
                .iter()
                .any(|profile| profile.id == self.active_mode_profile_id)
        {
            self.active_mode_profile_id = DEFAULT_VOICE_MODE_ID.to_string();
        }
    }

    pub fn mode_profile_by_id(&self, id: &str) -> Option<&VoiceModeProfile> {
        self.mode_profiles.iter().find(|profile| profile.id == id)
    }

    pub fn mode_profile_or_default(&self, id: &str) -> Option<&VoiceModeProfile> {
        self.mode_profile_by_id(id)
            .or_else(|| self.mode_profile_by_id(DEFAULT_VOICE_MODE_ID))
            .or_else(|| self.mode_profiles.first())
    }

    pub fn active_mode_profile(&self) -> Option<&VoiceModeProfile> {
        self.mode_profile_or_default(&self.active_mode_profile_id)
    }

    pub fn next_mode_profile_id(&self, current_id: &str) -> Option<String> {
        if self.mode_profiles.is_empty() {
            return None;
        }
        let current_index = self
            .mode_profiles
            .iter()
            .position(|profile| profile.id == current_id)
            .unwrap_or_else(|| {
                self.mode_profiles
                    .iter()
                    .position(|profile| profile.id == DEFAULT_VOICE_MODE_ID)
                    .unwrap_or(0)
            });
        let next_index = (current_index + 1) % self.mode_profiles.len();
        Some(self.mode_profiles[next_index].id.clone())
    }

    pub fn validate_mode_profiles(&self) -> Result<(), String> {
        if self.mode_profiles.is_empty() {
            return Err("音声モードを1つ以上設定してください。".to_string());
        }

        let mut seen = HashSet::new();
        let mut has_default = false;
        for profile in &self.mode_profiles {
            if profile.id.trim().is_empty() {
                return Err("音声モードのIDが空です。".to_string());
            }
            if !seen.insert(profile.id.as_str()) {
                return Err("音声モードのIDが重複しています。".to_string());
            }
            if profile.name.trim().is_empty() {
                return Err("音声モード名を入力してください。".to_string());
            }
            if profile.formatting_enabled && !profile.user_prompt.contains(TRANSCRIPT_TOKEN) {
                return Err(format!(
                    "{}のユーザープロンプトには {TRANSCRIPT_TOKEN} を含めてください。",
                    profile.name
                ));
            }
            if profile.id == DEFAULT_VOICE_MODE_ID {
                has_default = true;
                if profile.deletable {
                    return Err("デフォルトモードは削除不可にしてください。".to_string());
                }
            }
        }

        if !has_default {
            return Err("デフォルトモードが必要です。".to_string());
        }
        if self
            .mode_profile_by_id(&self.active_mode_profile_id)
            .is_none()
        {
            return Err("現在ONの音声モードが見つかりません。".to_string());
        }
        Ok(())
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
        self.voice.ensure_mode_profiles(&self.prompts.overrides);
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
        assert_eq!(settings.voice.active_mode_profile_id, DEFAULT_VOICE_MODE_ID);
        assert_eq!(settings.voice.mode_profiles.len(), 5);
        assert_eq!(settings.voice.mode_profiles[1].id, "speed");
        assert!(!settings.voice.mode_profiles[1].formatting_enabled);
        assert!(
            !settings
                .voice
                .mode_profile_by_id(DEFAULT_VOICE_MODE_ID)
                .expect("default profile")
                .deletable
        );
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
    fn sanitize_migrates_legacy_dictation_prompts_into_default_mode() {
        let mut settings = AppSettings::default();
        settings.voice.mode_profiles.clear();
        settings.voice.active_mode_profile_id = String::new();
        settings.prompts.overrides.dictation_system = Some("legacy system".to_string());
        settings.prompts.overrides.dictation_user = Some("legacy {{transcript}}".to_string());

        settings.sanitize();

        let default_profile = settings
            .voice
            .mode_profile_by_id(DEFAULT_VOICE_MODE_ID)
            .expect("default profile");
        assert_eq!(default_profile.system_prompt, "legacy system");
        assert_eq!(default_profile.user_prompt, "legacy {{transcript}}");
        assert_eq!(settings.voice.active_mode_profile_id, DEFAULT_VOICE_MODE_ID);
    }

    #[test]
    fn sanitize_restores_default_mode_and_falls_back_active_mode() {
        let mut settings = AppSettings::default();
        settings.voice.mode_profiles = vec![VoiceModeProfile {
            id: "custom".to_string(),
            name: "Custom".to_string(),
            description: String::new(),
            formatting_enabled: true,
            system_prompt: "system".to_string(),
            user_prompt: "{{transcript}}".to_string(),
            deletable: true,
            order: 0,
            preset_key: None,
        }];
        settings.voice.active_mode_profile_id = "missing".to_string();

        settings.sanitize();

        assert!(settings
            .voice
            .mode_profile_by_id(DEFAULT_VOICE_MODE_ID)
            .is_some());
        assert_eq!(settings.voice.active_mode_profile_id, DEFAULT_VOICE_MODE_ID);
    }

    #[test]
    fn validate_mode_profiles_requires_transcript_placeholder() {
        let mut settings = AppSettings::default();
        settings.voice.mode_profiles[0].user_prompt = "missing".to_string();

        assert!(settings.voice.validate_mode_profiles().is_err());
    }

    #[test]
    fn validate_mode_profiles_allows_missing_prompt_when_formatting_disabled() {
        let mut settings = AppSettings::default();
        settings.voice.mode_profiles[1].formatting_enabled = false;
        settings.voice.mode_profiles[1].user_prompt = String::new();

        assert!(settings.voice.validate_mode_profiles().is_ok());
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
