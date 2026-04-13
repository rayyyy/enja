import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getSettings, hideWindow } from "./lib/commands";
import { startTranslation } from "./lib/startTranslation";
import { useAppStore } from "./stores/useAppStore";
import { LeftPanel } from "./components/LeftPanel";
import { RightPanel } from "./components/RightPanel";
import { SettingsView } from "./components/SettingsView";

function handleCardPointerDown(e: React.PointerEvent<HTMLDivElement>) {
  if (e.button !== 0) return;
  const el = e.target as HTMLElement | null;
  if (!el) return;
  if (el.closest("[data-tauri-no-drag-region]")) return;
  if (el.closest("button, input, textarea, select, a")) return;
  void getCurrentWindow()
    .startDragging()
    .catch(() => {});
}

function App() {
  const view = useAppStore((s) => s.view);
  const hydrateFromSettings = useAppStore((s) => s.hydrateFromSettings);

  useEffect(() => {
    void getSettings().then((s) => {
      hydrateFromSettings(
        s.geminiApiKey,
        s.doubleTapThresholdMs,
        s.sourceLanguage,
        s.targetLanguage,
      );
      if (!s.geminiApiKey?.trim()) {
        useAppStore.getState().setView("settings");
      }
    });
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
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        void hideWindow();
        return;
      }
      // Cmd+W: hide overlay (also after Cmd+C trigger)
      if (e.metaKey && (e.key === "w" || e.key === "W")) {
        e.preventDefault();
        void hideWindow();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div
      data-tauri-drag-region
      className="flex h-full min-h-0 w-full cursor-default flex-col overflow-hidden rounded-2xl border border-neutral-200/70 bg-white shadow-xl shadow-neutral-900/8"
      onPointerDown={handleCardPointerDown}
    >
      {view === "settings" ? (
        <div className="flex min-h-0 flex-1 items-center justify-center p-6">
          <div className="w-full max-w-sm">
            <SettingsView />
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
