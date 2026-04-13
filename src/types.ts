export type TranslateEvent =
  | { type: "chunk"; text: string }
  | { type: "done" }
  | { type: "error"; message: string };

export type AppSettings = {
  geminiApiKey: string;
  doubleTapThresholdMs: number;
};
