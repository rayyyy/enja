import { create } from "zustand";
import type { AppSettings, ShortcutBinding, UiLanguage } from "../types";
import { otherUiLanguage } from "../lib/uiLanguage";

type View = "translation" | "settings" | "dictionary";

const defaultVoiceDictationShortcut: ShortcutBinding = {
  keyCode: null,
  key: "fn",
  label: "Fn",
  tapCount: 1,
  modifiers: {
    command: false,
    option: false,
    control: false,
    shift: false,
    function: false,
  },
};

const defaultVoiceAskShortcut: ShortcutBinding = {
  keyCode: 49,
  key: "space",
  label: "Fn Space",
  tapCount: 1,
  modifiers: {
    command: false,
    option: false,
    control: false,
    shift: false,
    function: true,
  },
};

const defaultPolishSelectionShortcut: ShortcutBinding = {
  keyCode: 35,
  key: "p",
  label: "Ctrl Option P",
  tapCount: 1,
  modifiers: {
    command: false,
    option: true,
    control: true,
    shift: false,
    function: false,
  },
};

interface AppState {
  view: View;
  inputText: string;
  outputText: string;
  isTranslating: boolean;
  error: string | null;
  sourceLanguage: UiLanguage;
  targetLanguage: UiLanguage;
  sourceLanguageDraft: UiLanguage;
  targetLanguageDraft: UiLanguage;
  voiceDictationShortcut: ShortcutBinding;
  voiceAskShortcut: ShortcutBinding;
  polishSelectionShortcut: ShortcutBinding;
  hasTranslated: boolean;

  setView: (v: View) => void;
  setInputText: (t: string) => void;
  appendOutput: (t: string) => void;
  resetTranslation: () => void;
  setTranslating: (v: boolean) => void;
  setError: (e: string | null) => void;
  setSourceLanguageDraft: (l: UiLanguage) => void;
  setTargetLanguageDraft: (l: UiLanguage) => void;
  syncLanguageDraftsFromSaved: () => void;
  hydrateFromSettings: (settings: AppSettings) => void;
  setHasTranslated: (v: boolean) => void;
}

export const useAppStore = create<AppState>((set) => ({
  view: "translation",
  inputText: "",
  outputText: "",
  isTranslating: false,
  error: null,
  sourceLanguage: "en",
  targetLanguage: "ja",
  sourceLanguageDraft: "en",
  targetLanguageDraft: "ja",
  voiceDictationShortcut: defaultVoiceDictationShortcut,
  voiceAskShortcut: defaultVoiceAskShortcut,
  polishSelectionShortcut: defaultPolishSelectionShortcut,
  hasTranslated: false,

  setView: (v) => set({ view: v }),
  setInputText: (t) => set({ inputText: t }),
  appendOutput: (t) => set((s) => ({ outputText: s.outputText + t })),
  resetTranslation: () =>
    set({ outputText: "", error: null, isTranslating: false }),
  setTranslating: (v) => set({ isTranslating: v }),
  setError: (e) => set({ error: e }),
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
  hydrateFromSettings: (settings) =>
    set({
      sourceLanguage: settings.translation.sourceLanguage,
      targetLanguage: settings.translation.targetLanguage,
      sourceLanguageDraft: settings.translation.sourceLanguage,
      targetLanguageDraft: settings.translation.targetLanguage,
      voiceDictationShortcut: settings.shortcuts.voiceDictation,
      voiceAskShortcut: settings.shortcuts.voiceAsk,
      polishSelectionShortcut: settings.shortcuts.polishSelection,
    }),
  setHasTranslated: (v) => set({ hasTranslated: v }),
}));
