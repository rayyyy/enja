export type TranslateEvent =
  | { type: "chunk"; text: string }
  | { type: "done" }
  | { type: "error"; message: string };

export type UiLanguage = "en" | "ja";

export type AppSettings = {
  geminiApiKey: string;
  doubleTapThresholdMs: number;
  sourceLanguage: UiLanguage;
  targetLanguage: UiLanguage;
};
