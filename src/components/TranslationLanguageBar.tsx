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
      width="11"
      height="11"
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
      width="13"
      height="13"
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

const selectClass =
  "w-full min-w-0 cursor-pointer appearance-none rounded-md border border-edge bg-surface py-1 pr-6 pl-2 text-[11px] font-medium text-ink outline-none transition-[border-color,box-shadow] duration-100 hover:border-edge-strong focus:border-accent focus:ring-2 focus:ring-accent/25 disabled:cursor-not-allowed disabled:opacity-50";

/** 左ペイン上部の言語スイッチ */
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
      className="flex shrink-0 flex-col gap-1 px-2.5 pt-2.5 pb-1.5"
      role="group"
      aria-label={"翻訳の言語"}
      aria-busy={saving}
    >
      <div className="flex items-center gap-1.5">
        <div className="group relative min-w-0 flex-1">
          <label htmlFor="enja-source-lang" className="sr-only">
            {"翻訳前の言語"}
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
            title={"翻訳前: " + languageLabelForUi(sourceLanguage)}
            className={selectClass}
          >
            {LANG_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          <ChevronDownIcon className="pointer-events-none absolute top-1/2 right-1.5 -translate-y-1/2 text-ink-faint" />
        </div>

        <button
          type="button"
          disabled={isTranslating || saving}
          onClick={() => void runPersist(targetLanguage, sourceLanguage)}
          className={`grid size-7 shrink-0 place-items-center rounded-md border border-edge bg-surface text-ink-mid transition-[transform,background-color,color] duration-100 focus-ring hover:bg-hover hover:text-ink active:scale-95 disabled:pointer-events-none disabled:opacity-45 ${
            saving ? "animate-pulse" : ""
          }`}
          title="言語を入れ替え"
          aria-label={
            "翻訳前と翻訳後の言語を入れ替える"
          }
        >
          <SwapIcon />
        </button>

        <div className="group relative min-w-0 flex-1">
          <label htmlFor="enja-target-lang" className="sr-only">
            {"翻訳後の言語"}
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
            title={"翻訳後: " + languageLabelForUi(targetLanguage)}
            className={selectClass}
          >
            {LANG_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          <ChevronDownIcon className="pointer-events-none absolute top-1/2 right-1.5 -translate-y-1/2 text-ink-faint" />
        </div>
      </div>

      {saveError ? (
        <p className="text-center text-[10px] leading-tight text-danger">
          {saveError}
        </p>
      ) : null}
    </div>
  );
}
