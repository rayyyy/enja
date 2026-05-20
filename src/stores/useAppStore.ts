import { create } from "zustand";
import type {
  FinalizationModel,
  ShortcutBinding,
  SpeechProfile,
  UiLanguage,
} from "../types";
import { otherUiLanguage } from "../lib/uiLanguage";

type View = "translation" | "settings" | "dictionary";

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
  selectedMicrophoneId: string | null;
  speechProfile: SpeechProfile;
  finalizationModel: FinalizationModel;
  interactionSoundsEnabled: boolean;
  muteSystemAudioDuringRecording: boolean;
  maxRecordingSeconds: number;
  googleCloudProjectId: string;
  googleCloudRegion: string;
  googleCloudUseAdc: boolean;
  voiceDictationShortcut: ShortcutBinding;
  voiceAskShortcut: ShortcutBinding;
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
    voice?: {
      selectedMicrophoneId: string | null;
      speechProfile: SpeechProfile;
      finalizationModel: FinalizationModel;
      interactionSoundsEnabled: boolean;
      muteSystemAudioDuringRecording: boolean;
      maxRecordingSeconds: number;
      googleCloudProjectId: string;
      googleCloudRegion: string;
      googleCloudUseAdc: boolean;
      voiceDictationShortcut: ShortcutBinding;
      voiceAskShortcut: ShortcutBinding;
    },
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
  selectedMicrophoneId: null,
  speechProfile: "googleChirp3",
  finalizationModel: "gemini35Flash",
  interactionSoundsEnabled: true,
  muteSystemAudioDuringRecording: true,
  maxRecordingSeconds: 300,
  googleCloudProjectId: "",
  googleCloudRegion: "asia-northeast1",
  googleCloudUseAdc: true,
  voiceDictationShortcut: {
    keyCode: null,
    key: "fn",
    label: "Fn",
    modifiers: {
      command: false,
      option: false,
      control: false,
      shift: false,
      function: false,
    },
  },
  voiceAskShortcut: {
    keyCode: 49,
    key: "space",
    label: "Fn Space",
    modifiers: {
      command: false,
      option: false,
      control: false,
      shift: false,
      function: true,
    },
  },
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
  hydrateFromSettings: (apiKey, doubleTapMs, source, target, voice) =>
    set({
      apiKeyDraft: apiKey,
      doubleTapMsDraft: doubleTapMs,
      sourceLanguage: source,
      targetLanguage: target,
      sourceLanguageDraft: source,
      targetLanguageDraft: target,
      ...(voice ?? {}),
    }),
  setHasTranslated: (v) => set({ hasTranslated: v }),
}));
