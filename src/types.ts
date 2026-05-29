export type TranslateEvent =
  | { type: "chunk"; text: string }
  | { type: "done" }
  | { type: "error"; message: string };

export type UiLanguage = "en" | "ja";

export type SpeechProfile =
  | "googleChirp3"
  | "openAiGpt4oTranscribe"
  | "openAiGpt4oMiniTranscribe"
  | "geminiAudio"
  | "appleSpeechAnalyzer";

export type FinalizationModel =
  | "gemini31ProPreview"
  | "gemini35Flash"
  | "gemini31FlashLite";

export type ShortcutAction = "voiceDictation" | "voiceAsk";

export type ShortcutModifiers = {
  command: boolean;
  option: boolean;
  control: boolean;
  shift: boolean;
  function: boolean;
};

export type ShortcutBinding = {
  keyCode: number | null;
  key: string;
  label: string;
  modifiers: ShortcutModifiers;
};

export type PromptOverrides = {
  translateEnToJa: string | null;
  translateJaToEn: string | null;
  openaiTranscription: string | null;
  geminiAudioSystem: string | null;
  geminiAudioUser: string | null;
  dictationSystem: string | null;
  dictationUser: string | null;
  askWithoutSelectionSystem: string | null;
  askWithoutSelectionUser: string | null;
  askWithSelectionSystem: string | null;
  askWithSelectionUser: string | null;
};

export type PromptCatalogItem = {
  key: keyof PromptOverrides;
  label: string;
  rows: number;
  required: string[];
  defaultText: string;
};

export type TranslationSettings = {
  sourceLanguage: UiLanguage;
  targetLanguage: UiLanguage;
};

export type SystemAudioHandling = "mute" | "isolate" | "off";

export type VoiceModePresetKey =
  | "default"
  | "speed"
  | "aiPrompt"
  | "casual"
  | "formal";

export type VoiceModeProfile = {
  id: string;
  name: string;
  description: string;
  formattingEnabled: boolean;
  systemPrompt: string;
  userPrompt: string;
  deletable: boolean;
  order: number;
  presetKey: VoiceModePresetKey | null;
};

export type VoiceSettings = {
  selectedMicrophoneId: string | null;
  speechProfile: SpeechProfile;
  finalizationModel: FinalizationModel;
  interactionSoundsEnabled: boolean;
  systemAudioHandling: SystemAudioHandling;
  maxRecordingSeconds: number;
  googleCloudProjectId: string;
  googleCloudRegion: string;
  googleCloudUseAdc: boolean;
  modeProfiles: VoiceModeProfile[];
  activeModeProfileId: string;
};

export type ShortcutSettings = {
  voiceDictation: ShortcutBinding;
  voiceAsk: ShortcutBinding;
};

export type PromptSettings = {
  overrides: PromptOverrides;
};

export type AppBehaviorSettings = {
  doubleTapThresholdMs: number;
  launchAtLogin: boolean;
};

export type AppSettings = {
  translation: TranslationSettings;
  voice: VoiceSettings;
  shortcuts: ShortcutSettings;
  prompts: PromptSettings;
  app: AppBehaviorSettings;
};

export type AudioInputDevice = {
  id: string;
  name: string;
  isDefault: boolean;
};

export type DictionaryEntry = {
  id: string;
  preferred: string;
  aliases: string[];
  corrections: DictionaryCorrection[];
  enabled: boolean;
  source: "manual" | "learned";
  createdAt: number;
  updatedAt: number;
};

export type DictionaryCorrection = {
  from: string;
  to: string;
};

export type DictionaryEntryInput = {
  preferred: string;
  aliases?: string[];
  corrections?: DictionaryCorrection[];
  enabled: boolean;
};

export type DictionaryBulkCreateResult = {
  added: DictionaryEntry[];
  skipped: number;
};

export type ProviderStatus = {
  gemini: boolean;
  openai: boolean;
  googleServiceAccount: boolean;
};

export type SpeechSetupCheck = {
  ok: boolean;
  message: string;
  details: string[];
};

export type AppleSpeechStatusValue =
  | "unknown"
  | "unsupported"
  | "supported"
  | "downloading"
  | "installed";

export type AppleSpeechAuthorization =
  | "unknown"
  | "notDetermined"
  | "denied"
  | "restricted"
  | "authorized";

export type AppleSpeechStatus = {
  helperAvailable: boolean;
  supported: boolean;
  status: AppleSpeechStatusValue;
  authorization: AppleSpeechAuthorization;
  message: string;
  details: string[];
};

export type VoiceMode = "dictation" | "ask";

export type ApiUsageService =
  | "geminiTranslation"
  | "geminiFinalization"
  | "geminiAudioInput"
  | "openAiTranscription"
  | "googleSpeechToText";

export type ApiUsageEvent = {
  id: string;
  timestampMs: number;
  service: ApiUsageService;
  provider: string;
  model: string;
  operation: string;
  inputTokens: number | null;
  outputTokens: number | null;
  audioInputTokens: number | null;
  durationSecs: number | null;
  requestCount: number;
  estimatedCostUsd: number | null;
  pricingNote: string;
  note: string | null;
};

export type VoiceStateEvent = {
  state: "idle" | "recording" | "processing" | "inserted" | "fallback" | "error";
  mode: VoiceMode | null;
  modeProfileId: string | null;
  modeProfileName: string | null;
  message: string | null;
  seq?: number;
};

export type VoiceLevelEvent = {
  rms: number;
  peak: number;
};

export type VoiceResultEvent = {
  text: string;
  inserted: boolean;
  reason: string | null;
};

export type VoiceDictionaryLearningEvent = {
  entryId: string;
  from: string;
  to: string;
};

export type ShortcutCapturedEvent = {
  action: ShortcutAction;
  shortcut: ShortcutBinding;
};

export type ShortcutCaptureCancelledEvent = {
  action: ShortcutAction;
  reason: string;
};
