import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import { getSettings, saveSettings } from "../lib/commands";

export function SettingsView() {
  const { apiKeyDraft, setApiKeyDraft, setView, hydrateFromSettings } =
    useAppStore();
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  async function handleSave() {
    setSaving(true);
    setMsg(null);
    try {
      const current = await getSettings();
      await saveSettings({
        ...current,
        geminiApiKey: apiKeyDraft.trim(),
      });
      const s = await getSettings();
      hydrateFromSettings(
        s.geminiApiKey,
        s.doubleTapThresholdMs,
        s.sourceLanguage,
        s.targetLanguage,
      );
      setMsg("保存しました。");
    } catch (e) {
      setMsg(String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="flex flex-col gap-5">
      <header className="flex flex-col items-center gap-2 text-center">
        <h1 className="font-play text-[2.25rem] leading-none font-bold tracking-[0.02em] sm:text-[2.5rem]">
          <span className="bg-linear-to-br from-neutral-800 via-neutral-700 to-blue-600 bg-clip-text text-transparent">
            Enja
          </span>
        </h1>
        <p className="max-w-[16rem] text-[11px] leading-snug text-neutral-400">
          Gemini APIキーを入力して翻訳を有効にします
        </p>
      </header>

      <div className="flex items-center justify-between gap-2 border-t border-neutral-100 pt-4">
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
          {"\u623b\u308b"}
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
