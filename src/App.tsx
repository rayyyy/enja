import { lazy, Suspense, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  getProviderStatus,
  getSettings,
  hideStickyNoteWindow,
  hideWindow,
} from "./lib/commands";
import { startTranslation } from "./lib/startTranslation";
import { useAppStore } from "./stores/useAppStore";
import { LeftPanel } from "./components/LeftPanel";
import { RightPanel } from "./components/RightPanel";
import { SettingsView } from "./components/SettingsView";
import { DictionaryView } from "./components/DictionaryView";
import { VoiceOverlay } from "./components/VoiceOverlay";

const NotesView = lazy(() =>
  import("./components/notes/NotesView").then((module) => ({
    default: module.NotesView,
  })),
);
const StickyNoteWindow = lazy(() =>
  import("./components/notes/NotesView").then((module) => ({
    default: module.StickyNoteWindow,
  })),
);

function App() {
  const windowLabel = getCurrentWindow().label;
  const view = useAppStore((s) => s.view);
  const hydrateFromSettings = useAppStore((s) => s.hydrateFromSettings);

  useEffect(() => {
    void Promise.all([getSettings(), getProviderStatus().catch(() => null)]).then(
      ([settings, providerStatus]) => {
        hydrateFromSettings(settings);
        if (!providerStatus?.gemini) {
          useAppStore.getState().setView("settings");
        }
      },
    );
  }, [hydrateFromSettings]);

  useEffect(() => {
    const unlistenPromise = listen<string>("enja-trigger", (event) => {
      const text = event.payload;
      useAppStore.getState().resetTranslation();
      useAppStore.getState().setInputText(text);
      useAppStore.getState().setView("translation");
      useAppStore.getState().setHasTranslated(false);
      void startTranslation(text);
    });

    return () => {
      void unlistenPromise.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const stickyNoteId = windowLabel.startsWith("sticky-")
      ? windowLabel.slice("sticky-".length)
      : null;

    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        if (stickyNoteId) {
          void hideStickyNoteWindow(stickyNoteId);
          return;
        }
        void hideWindow();
        return;
      }
      // Cmd+W: close sticky windows, otherwise hide the main overlay.
      if (e.metaKey && (e.key === "w" || e.key === "W")) {
        e.preventDefault();
        if (stickyNoteId) {
          void hideStickyNoteWindow(stickyNoteId);
          return;
        }
        void hideWindow();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [windowLabel]);

  if (windowLabel === "voice") {
    return <VoiceOverlay />;
  }

  if (windowLabel.startsWith("sticky-")) {
    return (
      <Suspense fallback={<div className="h-full bg-neutral-50" />}>
        <StickyNoteWindow noteId={windowLabel.slice("sticky-".length)} />
      </Suspense>
    );
  }

  return (
    <div className="flex h-full min-h-0 w-full cursor-default flex-col overflow-hidden bg-neutral-50">
      {view === "notes" ? (
        <Suspense fallback={<div className="flex-1 bg-white" />}>
          <NotesView />
        </Suspense>
      ) : view === "settings" || view === "dictionary" ? (
        <div className="flex min-h-0 flex-1 flex-col bg-white">
          <div className="flex min-h-0 flex-1 p-4 md:p-5">
            {view === "dictionary" ? (
              <div className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col">
                <DictionaryView />
              </div>
            ) : (
              <SettingsView />
            )}
          </div>
        </div>
      ) : (
        <div className="flex min-h-0 min-w-0 flex-1">
          <LeftPanel />
          <RightPanel />
        </div>
      )}
    </div>
  );
}

export default App;
