import { create } from "zustand";

type View = "translation" | "settings";

interface AppState {
  view: View;
  inputText: string;
  outputText: string;
  isTranslating: boolean;
  error: string | null;
  apiKeyDraft: string;
  doubleTapMsDraft: number;
  hasTranslated: boolean;

  setView: (v: View) => void;
  setInputText: (t: string) => void;
  appendOutput: (t: string) => void;
  resetTranslation: () => void;
  setTranslating: (v: boolean) => void;
  setError: (e: string | null) => void;
  setApiKeyDraft: (k: string) => void;
  setDoubleTapMsDraft: (n: number) => void;
  hydrateFromSettings: (apiKey: string, doubleTapMs: number) => void;
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
  hydrateFromSettings: (apiKey, doubleTapMs) =>
    set({ apiKeyDraft: apiKey, doubleTapMsDraft: doubleTapMs }),
  setHasTranslated: (v) => set({ hasTranslated: v }),
}));
