import { create } from "zustand";
import type { UiLanguage } from "../types";
import { otherUiLanguage } from "../lib/uiLanguage";

type View = "translation" | "settings";

interface AppState {
  view: View;
  inputText: string;
  outputText: string;
  isTranslating: boolean;
  error: string | null;
  apiKeyDraft: string;
  doubleTapMsDraft: number;
  sourceLanguage: UiLanguage;
  targetLanguage: UiLanguage;
  sourceLanguageDraft: UiLanguage;
  targetLanguageDraft: UiLanguage;
  hasTranslated: boolean;

  setView: (v: View) => void;
  setInputText: (t: string) => void;
  appendOutput: (t: string) => void;
  resetTranslation: () => void;
  setTranslating: (v: boolean) => void;
  setError: (e: string | null) => void;
  setApiKeyDraft: (k: string) => void;
  setDoubleTapMsDraft: (n: number) => void;
  setSourceLanguageDraft: (l: UiLanguage) => void;
  setTargetLanguageDraft: (l: UiLanguage) => void;
  syncLanguageDraftsFromSaved: () => void;
  hydrateFromSettings: (
    apiKey: string,
    doubleTapMs: number,
    source: UiLanguage,
    target: UiLanguage,
  ) => void;
  setHasTranslated: (v: boolean) => void;
}

export const useAppStore = create<AppState>((set) => ({
  view: "translation",
  inputText: "",
  outputText: "",
  isTranslating: false,
  error: null,
  apiKeyDraft: "",
  doubleTapMsDraft: 400,
  sourceLanguage: "en",
  targetLanguage: "ja",
  sourceLanguageDraft: "en",
  targetLanguageDraft: "ja",
  hasTranslated: false,

  setView: (v) => set({ view: v }),
  setInputText: (t) => set({ inputText: t }),
  appendOutput: (t) => set((s) => ({ outputText: s.outputText + t })),
  resetTranslation: () =>
    set({ outputText: "", error: null, isTranslating: false }),
  setTranslating: (v) => set({ isTranslating: v }),
  setError: (e) => set({ error: e }),
  setApiKeyDraft: (k) => set({ apiKeyDraft: k }),
  setDoubleTapMsDraft: (n) => set({ doubleTapMsDraft: n }),
  setSourceLanguageDraft: (l) =>
    set({
      sourceLanguageDraft: l,
      targetLanguageDraft: otherUiLanguage(l),
    }),
  setTargetLanguageDraft: (l) =>
    set({
      targetLanguageDraft: l,
      sourceLanguageDraft: otherUiLanguage(l),
    }),
  syncLanguageDraftsFromSaved: () =>
    set((s) => ({
      sourceLanguageDraft: s.sourceLanguage,
      targetLanguageDraft: s.targetLanguage,
    })),
  hydrateFromSettings: (apiKey, doubleTapMs, source, target) =>
    set({
      apiKeyDraft: apiKey,
      doubleTapMsDraft: doubleTapMs,
      sourceLanguage: source,
      targetLanguage: target,
      sourceLanguageDraft: source,
      targetLanguageDraft: target,
    }),
  setHasTranslated: (v) => set({ hasTranslated: v }),
}));
