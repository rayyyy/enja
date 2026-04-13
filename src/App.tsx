import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getSettings, hideWindow } from "./lib/commands";
import { startTranslation } from "./lib/startTranslation";
import { useAppStore } from "./stores/useAppStore";
import { LeftPanel } from "./components/LeftPanel";
import { RightPanel } from "./components/RightPanel";
import { SettingsView } from "./components/SettingsView";

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
    <div className="flex h-full min-h-0 w-full cursor-default flex-col overflow-hidden bg-neutral-50">
      {view === "settings" ? (
        <div className="flex min-h-0 flex-1 flex-col border-t border-neutral-200/90 shadow-[0_1px_2px_-1px_rgba(0,0,0,0.06)]">
          <div className="flex min-h-0 flex-1 items-center justify-center p-6">
            <div className="w-full max-w-sm">
              <SettingsView />
            </div>
          </div>
        </div>
      ) : (
        <div className="flex min-h-0 min-w-0 flex-1 border-t border-neutral-200/90 shadow-[0_1px_2px_-1px_rgba(0,0,0,0.06)]">
          <LeftPanel />
          <RightPanel />
        </div>
      )}
    </div>
  );
}

export default App;
