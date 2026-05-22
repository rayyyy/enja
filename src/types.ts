export type TranslateEvent =
  | { type: "chunk"; text: string }
  | { type: "done" }
  | { type: "error"; message: string };

export type UiLanguage = "en" | "ja";

export type SpeechProfile =
  | "googleChirp3"
  | "openAiGpt4oTranscribe"
  | "openAiGpt4oMiniTranscribe"
  | "geminiAudio";

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
  readings: string[];
  aliases: string[];
  enabled: boolean;
  source: "manual";
  createdAt: number;
  updatedAt: number;
};

export type DictionaryEntryInput = {
  preferred: string;
  readings: string[];
  aliases: string[];
  enabled: boolean;
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

export type VoiceMode = "dictation" | "ask";

export type VoiceStateEvent = {
  state: "idle" | "recording" | "processing" | "inserted" | "fallback" | "error";
  mode: VoiceMode | null;
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

export type ShortcutCapturedEvent = {
  action: ShortcutAction;
  shortcut: ShortcutBinding;
};

export type ShortcutCaptureCancelledEvent = {
  action: ShortcutAction;
  reason: string;
};
