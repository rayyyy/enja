import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getProviderStatus, getSettings, hideWindow } from "./lib/commands";
import { startTranslation } from "./lib/startTranslation";
import { useAppStore } from "./stores/useAppStore";
import { LeftPanel } from "./components/LeftPanel";
import { RightPanel } from "./components/RightPanel";
import { SettingsView } from "./components/SettingsView";
import { DictionaryView } from "./components/DictionaryView";
import { VoiceOverlay } from "./components/VoiceOverlay";

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

  if (windowLabel === "voice") {
    return <VoiceOverlay />;
  }

  return (
    <div className="flex h-full min-h-0 w-full cursor-default flex-col overflow-hidden bg-neutral-50">
      {view === "settings" || view === "dictionary" ? (
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
