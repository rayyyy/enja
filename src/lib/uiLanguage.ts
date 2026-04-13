import type { UiLanguage } from "../types";

export function otherUiLanguage(l: UiLanguage): UiLanguage {
  return l === "en" ? "ja" : "en";
}

/** Short label shown in the UI (Japanese app chrome). */
export function languageLabelForUi(lang: UiLanguage): string {
  return lang === "ja" ? "日本語" : "English";
}
