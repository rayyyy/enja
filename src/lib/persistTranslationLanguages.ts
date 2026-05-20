import { useAppStore } from "../stores/useAppStore";
import type { UiLanguage } from "../types";
import { otherUiLanguage } from "./uiLanguage";
import { getSettings, saveSettings } from "./commands";
import { withTranslationLanguages } from "./settingsHelpers";

function normalizePair(source: UiLanguage, target: UiLanguage) {
  if (source === target) {
    return { source, target: otherUiLanguage(source) };
  }
  return { source, target };
}

/** Main translation view: persist language pair so the next `translate` invoke reads the same settings. */
export async function persistTranslationLanguages(
  source: UiLanguage,
  target: UiLanguage,
): Promise<void> {
  const { source: src, target: tgt } = normalizePair(source, target);
  const snap = useAppStore.getState();
  const prev = {
    sourceLanguage: snap.sourceLanguage,
    targetLanguage: snap.targetLanguage,
    sourceLanguageDraft: snap.sourceLanguageDraft,
    targetLanguageDraft: snap.targetLanguageDraft,
  };

  useAppStore.setState({
    sourceLanguage: src,
    targetLanguage: tgt,
    sourceLanguageDraft: src,
    targetLanguageDraft: tgt,
  });

  try {
    const current = await getSettings();
    await saveSettings(withTranslationLanguages(current, src, tgt));
  } catch {
    useAppStore.setState(prev);
    throw new Error("言語の保存に失敗しました。もう一度お試しください。");
  }
}
