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
    if (windowLabel !== "main") return;

    void Promise.all([getSettings(), getProviderStatus().catch(() => null)]).then(
      ([settings, providerStatus]) => {
        hydrateFromSettings(settings);
        if (!providerStatus?.gemini) {
          useAppStore.getState().setView("settings");
        }
      },
    );
  }, [hydrateFromSettings, windowLabel]);

  useEffect(() => {
    if (windowLabel !== "main") return;

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
  }, [windowLabel]);

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

  useEffect(() => {
    function preventFileNavigation(event: DragEvent) {
      if (!dragHasFiles(event) || dragTargetIsRichEditor(event.target)) return;

      event.preventDefault();
      if (event.dataTransfer) {
        event.dataTransfer.dropEffect = "none";
      }
    }

    window.addEventListener("dragenter", preventFileNavigation, true);
    window.addEventListener("dragover", preventFileNavigation, true);
    window.addEventListener("drop", preventFileNavigation, true);
    return () => {
      window.removeEventListener("dragenter", preventFileNavigation, true);
      window.removeEventListener("dragover", preventFileNavigation, true);
      window.removeEventListener("drop", preventFileNavigation, true);
    };
  }, []);

  if (windowLabel === "voice") {
    return <VoiceOverlay />;
  }

  if (windowLabel.startsWith("sticky-")) {
    return (
      <Suspense fallback={<div className="h-full bg-canvas" />}>
        <StickyNoteWindow noteId={windowLabel.slice("sticky-".length)} />
      </Suspense>
    );
  }

  return (
    <div className="flex h-full min-h-0 w-full cursor-default flex-col overflow-hidden bg-canvas">
      {view === "notes" ? (
        <Suspense fallback={<div className="flex-1 bg-surface" />}>
          <NotesView />
        </Suspense>
      ) : view === "settings" ? (
        <div className="flex min-h-0 flex-1 flex-col bg-surface">
          <SettingsView />
        </div>
      ) : view === "dictionary" ? (
        <div className="flex min-h-0 flex-1 flex-col bg-surface">
          <div className="flex min-h-0 flex-1 p-4 md:p-6">
            <div className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col">
              <DictionaryView />
            </div>
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

function dragHasFiles(event: DragEvent) {
  return Array.from(event.dataTransfer?.types ?? []).includes("Files");
}

function dragTargetIsRichEditor(target: EventTarget | null) {
  const element =
    target instanceof Element
      ? target
      : target instanceof Node
        ? target.parentElement
        : null;
  return Boolean(element?.closest(".ProseMirror"));
}

export default App;
