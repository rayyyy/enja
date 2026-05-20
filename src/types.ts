export type TranslateEvent =
  | { type: "chunk"; text: string }
  | { type: "done" }
  | { type: "error"; message: string };

export type UiLanguage = "en" | "ja";

export type SpeechProfile =
  | "googleChirp3"
  | "deepgramNova3"
  | "openAiGpt4oTranscribe"
  | "openAiGpt4oMiniTranscribe"
  | "geminiAudio";

export type FinalizationModel =
  | "gemini31ProPreview"
  | "gemini35Flash"
  | "gemini31FlashLite";

export type AppSettings = {
  geminiApiKey: string;
  doubleTapThresholdMs: number;
  sourceLanguage: UiLanguage;
  targetLanguage: UiLanguage;
  launchAtLogin: boolean;
  selectedMicrophoneId: string | null;
  speechProfile: SpeechProfile;
  finalizationModel: FinalizationModel;
  interactionSoundsEnabled: boolean;
  muteSystemAudioDuringRecording: boolean;
  maxRecordingSeconds: number;
  googleCloudProjectId: string;
  googleCloudRegion: string;
  googleCloudUseAdc: boolean;
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
  deepgram: boolean;
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
