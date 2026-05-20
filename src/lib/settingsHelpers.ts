import type { AppSettings, UiLanguage } from "../types";

export function withTranslationLanguages(
  settings: AppSettings,
  sourceLanguage: UiLanguage,
  targetLanguage: UiLanguage,
): AppSettings {
  return {
    ...settings,
    translation: {
      sourceLanguage,
      targetLanguage,
    },
  };
}
