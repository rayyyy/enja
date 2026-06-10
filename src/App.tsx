import { lazy, Suspense, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { LucideIcon } from "lucide-react";
import { BookOpenText, Languages, Settings, StickyNote } from "lucide-react";
import {
  getProviderStatus,
  getSettings,
  hideStickyNoteWindow,
  hideWindow,
} from "./lib/commands";
import { startTranslation } from "./lib/startTranslation";
import { useAppStore, type View } from "./stores/useAppStore";
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

const APP_NAV_ITEMS: { view: View; label: string; icon: LucideIcon }[] = [
  { view: "translation", label: "翻訳", icon: Languages },
  { view: "notes", label: "メモ", icon: StickyNote },
  { view: "dictionary", label: "辞書", icon: BookOpenText },
  { view: "settings", label: "設定", icon: Settings },
];

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
    <div className="flex h-full min-h-0 w-full cursor-default overflow-hidden bg-canvas">
      <AppNavigation />
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

function AppNavigation() {
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);

  return (
    <aside
      className="flex h-full w-12 shrink-0 flex-col items-center border-r border-edge bg-canvas px-1.5 py-2"
      aria-label="アプリ内ナビゲーション"
    >
      <nav className="flex flex-1 flex-col items-center gap-1" aria-label="主要画面">
        {APP_NAV_ITEMS.map((item) => {
          const Icon = item.icon;
          const active = view === item.view;
          return (
            <button
              key={item.view}
              type="button"
              title={item.label}
              aria-label={item.label}
              aria-current={active ? "page" : undefined}
              onClick={() => setView(item.view)}
              className={`grid size-8 place-items-center rounded-md transition-colors duration-100 focus-ring ${
                active
                  ? "bg-accent-soft text-accent-ink"
                  : "text-ink-faint hover:bg-hover hover:text-ink"
              }`}
            >
              <Icon size={17} strokeWidth={1.8} />
            </button>
          );
        })}
      </nav>
    </aside>
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
