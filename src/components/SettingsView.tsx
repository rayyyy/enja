import { openUrl } from "@tauri-apps/plugin-opener";
import { useAppStore } from "../stores/useAppStore";
import { getSettings, saveSettings } from "../lib/commands";
import { useState } from "react";

export function SettingsView() {
  const {
    apiKeyDraft,
    doubleTapMsDraft,
    setApiKeyDraft,
    setDoubleTapMsDraft,
    setView,
    hydrateFromSettings,
  } = useAppStore();
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  async function handleSave() {
    setSaving(true);
    setMsg(null);
    try {
      await saveSettings({
        geminiApiKey: apiKeyDraft.trim(),
        doubleTapThresholdMs: Math.min(2000, Math.max(100, doubleTapMsDraft)),
      });
      const s = await getSettings();
      hydrateFromSettings(s.geminiApiKey, s.doubleTapThresholdMs);
      setMsg("保存しました。連打の閾値は次回起動から反映されます。");
    } catch (e) {
      setMsg(String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-base font-semibold text-neutral-800">設定</h2>
        <button
          type="button"
          className="text-xs text-blue-500 underline-offset-2 hover:underline"
          onClick={() => void openUrl("https://aistudio.google.com/apikey")}
        >
          API キーを取得
        </button>
      </div>

      <label className="flex flex-col gap-1 text-sm">
        <span className="text-neutral-500">Gemini API キー</span>
        <input
          className="rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-800 outline-none ring-blue-500/30 focus:ring-2"
          type="password"
          autoComplete="off"
          value={apiKeyDraft}
          onChange={(e) => setApiKeyDraft(e.target.value)}
          placeholder="AIza..."
        />
      </label>

      <label className="flex flex-col gap-1 text-sm">
        <span className="text-neutral-500">Cmd+C 連打の間隔（ms）</span>
        <input
          className="rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-800 outline-none ring-blue-500/30 focus:ring-2"
          type="number"
          min={100}
          max={2000}
          step={50}
          value={doubleTapMsDraft}
          onChange={(e) => setDoubleTapMsDraft(Number(e.target.value) || 400)}
        />
      </label>

      {msg ? (
        <p className="text-xs text-neutral-500">{msg}</p>
      ) : null}

      <div className="mt-1 flex flex-wrap gap-2">
        <button
          type="button"
          className="rounded-lg bg-blue-500 px-4 py-2 text-sm font-medium text-white shadow-sm transition-colors hover:bg-blue-600 disabled:opacity-50"
          disabled={saving}
          onClick={() => void handleSave()}
        >
          {saving ? "保存中…" : "保存"}
        </button>
        <button
          type="button"
          className="rounded-lg border border-neutral-200 px-4 py-2 text-sm text-neutral-600 transition-colors hover:bg-neutral-50"
          onClick={() => setView("translation")}
        >
          戻る
        </button>
      </div>

      <p className="text-[11px] leading-relaxed text-neutral-400">
        macOS では「システム設定 → プライバシーとセキュリティ →
        アクセシビリティ」でこのアプリを許可してください（Cmd+C
        連打の検出に必要です）。
      </p>
    </div>
  );
}
