import { useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import type { UiLanguage } from "../types";
import { languageLabelForUi, otherUiLanguage } from "../lib/uiLanguage";
import { persistTranslationLanguages } from "../lib/persistTranslationLanguages";

const LANG_OPTIONS: { value: UiLanguage; label: string }[] = [
  { value: "ja", label: "日本語" },
  { value: "en", label: "English" },
];

function ChevronDownIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="m6 9 6 6 6-6" />
    </svg>
  );
}

function SwapIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M7.5 21L3 16.5m0 0l4.5-4.5M3 16.5h13.5m0-13.5L21 7.5m0 0l-4.5 4.5M21 7.5H7.5" />
    </svg>
  );
}

/** 左ペイン左上用・幅・高さを抑えた言語スイッチ（青系アクセント） */
export function TranslationLanguageBar() {
  const sourceLanguage = useAppStore((s) => s.sourceLanguage);
  const targetLanguage = useAppStore((s) => s.targetLanguage);
  const isTranslating = useAppStore((s) => s.isTranslating);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  async function runPersist(source: UiLanguage, target: UiLanguage) {
    setSaveError(null);
    setSaving(true);
    try {
      await persistTranslationLanguages(source, target);
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div
      className="flex shrink-0 flex-col gap-1 border-b border-neutral-200 px-2 py-2"
      role="group"
      aria-label={"\u7ffb\u8a33\u306e\u8a00\u8a9e"}
      aria-busy={saving}
    >
      <div className="flex items-center gap-1.5">
        <span
          className={`mt-0.5 h-1.5 w-1.5 shrink-0 self-center rounded-full bg-blue-500 ${saving ? "animate-pulse" : ""}`}
          title="言語"
          aria-hidden
        />
        <div className="group relative min-w-0 flex-1">
          <label htmlFor="enja-source-lang" className="sr-only">
            {"\u7ffb\u8a33\u524d\u306e\u8a00\u8a9e"}
          </label>
          <select
            id="enja-source-lang"
            value={sourceLanguage}
            disabled={isTranslating || saving}
            onChange={(e) =>
              void runPersist(
                e.target.value as UiLanguage,
                otherUiLanguage(e.target.value as UiLanguage),
              )
            }
            title={
              "\u7ffb\u8a33\u524d: " + languageLabelForUi(sourceLanguage)
            }
            className="w-full min-w-0 cursor-pointer appearance-none rounded-md border border-neutral-200 bg-white py-1 pr-7 pl-2 text-[11px] font-medium text-neutral-800 outline-none transition-[border-color,box-shadow] hover:border-neutral-300 focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {LANG_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          <ChevronDownIcon className="pointer-events-none absolute top-1/2 right-1.5 -translate-y-1/2 text-neutral-500" />
        </div>

        <button
          type="button"
          disabled={isTranslating || saving}
          onClick={() => void runPersist(targetLanguage, sourceLanguage)}
          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-blue-500 text-white transition-[transform,filter] hover:bg-blue-600 active:scale-95 disabled:pointer-events-none disabled:opacity-45"
          title="言語を入れ替え"
          aria-label={
            "\u7ffb\u8a33\u524d\u3068\u7ffb\u8a33\u5f8c\u306e\u8a00\u8a9e\u3092\u5165\u308c\u66ff\u3048\u308b"
          }
        >
          <SwapIcon className="text-white" />
        </button>

        <div className="group relative min-w-0 flex-1">
          <label htmlFor="enja-target-lang" className="sr-only">
            {"\u7ffb\u8a33\u5f8c\u306e\u8a00\u8a9e"}
          </label>
          <select
            id="enja-target-lang"
            value={targetLanguage}
            disabled={isTranslating || saving}
            onChange={(e) =>
              void runPersist(
                otherUiLanguage(e.target.value as UiLanguage),
                e.target.value as UiLanguage,
              )
            }
            title={
              "\u7ffb\u8a33\u5f8c: " + languageLabelForUi(targetLanguage)
            }
            className="w-full min-w-0 cursor-pointer appearance-none rounded-md border border-neutral-200 bg-white py-1 pr-7 pl-2 text-[11px] font-medium text-neutral-800 outline-none transition-[border-color,box-shadow] hover:border-neutral-300 focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {LANG_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          <ChevronDownIcon className="pointer-events-none absolute top-1/2 right-1.5 -translate-y-1/2 text-neutral-500" />
        </div>
      </div>

      {saveError ? (
        <p className="text-center text-[9px] leading-tight text-red-600">
          {saveError}
        </p>
      ) : null}
    </div>
  );
}
