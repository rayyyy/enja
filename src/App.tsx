import { lazy, Suspense, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { LucideIcon } from "lucide-react";
import { BookOpenText, Languages, Settings, StickyNote } from "lucide-react";
import {
  getProviderStatus,
  getSettings,
  hideStickyNoteWindow,
  hideWindow,
  recordEditablePaste,
} from "./lib/commands";
import { startTranslation } from "./lib/startTranslation";
import { useAppStore, type View } from "./stores/useAppStore";
import { LeftPanel } from "./components/LeftPanel";
import { RightPanel } from "./components/RightPanel";
import { SettingsView } from "./components/SettingsView";
import { DictionaryView } from "./components/DictionaryView";
import { VoiceOverlay } from "./components/VoiceOverlay";
import { WindowDragRegion } from "./components/ui";

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

const APP_NAV_ITEMS: {
  view: View;
  label: string;
  icon: LucideIcon;
  shortcut?: string;
  commandKey: string;
}[] = [
  {
    view: "translation",
    label: "翻訳",
    icon: Languages,
    shortcut: "⌘1",
    commandKey: "1",
  },
  {
    view: "notes",
    label: "メモ",
    icon: StickyNote,
    shortcut: "⌘2",
    commandKey: "2",
  },
  {
    view: "dictionary",
    label: "辞書",
    icon: BookOpenText,
    shortcut: "⌘3",
    commandKey: "3",
  },
  {
    view: "settings",
    label: "設定",
    icon: Settings,
    shortcut: "⌘4 / ⌘,",
    commandKey: "4",
  },
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
        return;
      }

      if (!stickyNoteId) {
        const shortcutView = viewShortcutForEvent(e);
        if (shortcutView) {
          e.preventDefault();
          useAppStore.getState().setView(shortcutView);
        }
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [windowLabel]);

  // 編集可能要素への paste を Rust 側へ通知する。音声入力の貼り付けが Enja
  // 自身のウィンドウ(メモ等)に向いたとき、本当に挿入されたかの検証に使われる。
  // ProseMirror が伝播を止めても拾えるよう capture フェーズで監視する。
  useEffect(() => {
    function onPaste(event: ClipboardEvent) {
      if (!pasteTargetIsEditable(event.target)) return;
      void recordEditablePaste().catch(() => {});
    }
    document.addEventListener("paste", onPaste, true);
    return () => document.removeEventListener("paste", onPaste, true);
  }, []);

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
    <div className="relative flex h-full min-h-0 w-full cursor-default overflow-hidden bg-canvas pt-[40px]">
      <WindowDragRegion className="absolute inset-x-0 top-0 z-20 h-[40px] bg-canvas" />
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
  const [isCommandPressed, setIsCommandPressed] = useState(false);

  useEffect(() => {
    function updateCommandState(event: KeyboardEvent) {
      setIsCommandPressed(event.metaKey);
    }

    function clearCommandState() {
      setIsCommandPressed(false);
    }

    window.addEventListener("keydown", updateCommandState);
    window.addEventListener("keyup", updateCommandState);
    window.addEventListener("blur", clearCommandState);
    return () => {
      window.removeEventListener("keydown", updateCommandState);
      window.removeEventListener("keyup", updateCommandState);
      window.removeEventListener("blur", clearCommandState);
    };
  }, []);

  return (
    <WindowDragRegion
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
              title={item.shortcut ? `${item.label}（${item.shortcut}）` : item.label}
              aria-label={item.label}
              aria-current={active ? "page" : undefined}
              onClick={() => setView(item.view)}
              className={`relative grid size-8 place-items-center rounded-md transition-colors duration-100 focus-ring ${
                active
                  ? "bg-accent-soft text-accent-ink"
                  : "text-ink-faint hover:bg-hover hover:text-ink"
              }`}
            >
              <Icon size={17} strokeWidth={1.8} />
              <span
                aria-hidden="true"
                className={`absolute -top-0.5 -right-0.5 grid size-4 place-items-center rounded-[5px] border font-sans text-[9px] font-semibold leading-none tracking-normal shadow-sm transition-all duration-100 ${
                  isCommandPressed
                    ? "scale-100 opacity-100"
                    : "pointer-events-none scale-90 opacity-0"
                } ${
                    active
                      ? "border-accent/20 bg-surface text-accent-ink"
                      : "border-edge bg-raised text-ink-mid"
                }`}
              >
                {item.commandKey}
              </span>
            </button>
          );
        })}
      </nav>
      <div
        title="⌘を押すと画面切替ショートカットを表示"
        className={`grid size-8 place-items-center rounded-md border font-sans text-[13px] font-medium leading-none tracking-normal transition-colors duration-100 ${
          isCommandPressed
            ? "border-accent/25 bg-accent-soft text-accent-ink"
            : "border-edge bg-sunken text-ink-mid"
        }`}
      >
        ⌘
      </div>
    </WindowDragRegion>
  );
}

function pasteTargetIsEditable(target: EventTarget | null): boolean {
  const element =
    target instanceof Element
      ? target
      : target instanceof Node
        ? target.parentElement
        : null;
  if (!element) return false;
  if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
    return !element.readOnly && !element.disabled;
  }
  return element instanceof HTMLElement && element.isContentEditable;
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

function viewShortcutForEvent(event: KeyboardEvent): View | null {
  if (!event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
    return null;
  }

  switch (event.key) {
    case "1":
      return "translation";
    case "2":
      return "notes";
    case "3":
      return "dictionary";
    case "4":
    case ",":
      return "settings";
    default:
      return null;
  }
}

export default App;
