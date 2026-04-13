import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import { getSettings, saveSettings } from "../lib/commands";
import type { UiLanguage } from "../types";

function LanguageSegment({
  value,
  onChange,
  ariaLabel,
}: {
  value: UiLanguage;
  onChange: (v: UiLanguage) => void;
  ariaLabel: string;
}) {
  return (
    <div
      className="flex rounded-lg bg-neutral-100 p-0.5"
      role="group"
      aria-label={ariaLabel}
    >
      {(["ja", "en"] as const).map((code) => (
        <button
          key={code}
          type="button"
          onClick={() => onChange(code)}
          className={`flex-1 rounded-md py-2 text-sm transition-colors ${
            value === code
              ? "bg-white font-medium text-neutral-900 shadow-sm"
              : "text-neutral-500 hover:text-neutral-700"
          }`}
        >
          {code === "ja" ? "日本語" : "English"}
        </button>
      ))}
    </div>
  );
}

export function SettingsView() {
  const {
    apiKeyDraft,
    doubleTapMsDraft,
    sourceLanguageDraft,
    targetLanguageDraft,
    setApiKeyDraft,
    setDoubleTapMsDraft,
    setSourceLanguageDraft,
    setTargetLanguageDraft,
    setView,
    hydrateFromSettings,
    syncLanguageDraftsFromSaved,
  } = useAppStore();
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    syncLanguageDraftsFromSaved();
  }, [syncLanguageDraftsFromSaved]);

  async function handleSave() {
    setSaving(true);
    setMsg(null);
    try {
      await saveSettings({
        geminiApiKey: apiKeyDraft.trim(),
        doubleTapThresholdMs: Math.min(2000, Math.max(100, doubleTapMsDraft)),
        sourceLanguage: sourceLanguageDraft,
        targetLanguage: targetLanguageDraft,
      });
      const s = await getSettings();
      hydrateFromSettings(
        s.geminiApiKey,
        s.doubleTapThresholdMs,
        s.sourceLanguage,
        s.targetLanguage,
      );
      setMsg(
        "保存しました。翻訳の言語はすぐに反映されます。連打の間隔値は次回起動から反映されます。",
      );
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

      <div className="flex flex-col gap-1.5 text-sm">
        <span className="text-neutral-500">翻訳前の言語（入力）</span>
        <p className="text-[11px] leading-relaxed text-neutral-400">
          左ペインに貼り付けるテキストの言語です。変更すると翻訳後が反対の言語になります。
        </p>
        <LanguageSegment
          value={sourceLanguageDraft}
          onChange={setSourceLanguageDraft}
          ariaLabel="翻訳前の言語"
        />
      </div>

      <div className="flex flex-col gap-1.5 text-sm">
        <span className="text-neutral-500">翻訳後の言語（出力）</span>
        <p className="text-[11px] leading-relaxed text-neutral-400">
          右ペインに表示する結果の言語です。変更すると翻訳前が反対の言語になります。
        </p>
        <LanguageSegment
          value={targetLanguageDraft}
          onChange={setTargetLanguageDraft}
          ariaLabel="翻訳後の言語"
        />
      </div>

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
