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
      hydrateFromSettings(s.geminiApiKey, s.doubleTapThresholdMs);
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
      // Cmd+W: ブラウザの「タブを閉じる」と同様にオーバーレイを隠す（Cmd+C 起動後も効くようにする）
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
      className="flex h-screen w-screen items-center justify-center bg-transparent p-4"
      onClick={() => void hideWindow()}
    >
      <div
        className="flex h-full w-full cursor-default overflow-hidden rounded-2xl border border-neutral-200/70 bg-white shadow-xl shadow-neutral-900/8"
        onClick={(e) => e.stopPropagation()}
      >
        {view === "settings" ? (
          <div className="flex flex-1 items-center justify-center p-6">
            <div className="w-full max-w-sm">
              <SettingsView />
            </div>
          </div>
        ) : (
          <>
            <LeftPanel />
            <RightPanel />
          </>
        )}
      </div>
    </div>
  );
}

export default App;
