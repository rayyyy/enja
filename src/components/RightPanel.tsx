import { useState } from "react";
import { Check, Copy, Languages } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { startTranslation } from "../lib/startTranslation";
import { StreamingMarkdown } from "./StreamingMarkdown";
import { languageLabelForUi } from "../lib/uiLanguage";
import { Kbd } from "./ui";

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);

  function handleCopy() {
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }

  return (
    <button
      type="button"
      onClick={handleCopy}
      className={`grid size-6 place-items-center rounded-md transition-colors duration-100 focus-ring ${
        copied
          ? "text-ok"
          : "text-ink-faint hover:bg-hover hover:text-ink"
      }`}
      title="コピー"
      aria-label="コピー"
    >
      {copied ? <Check size={13} /> : <Copy size={13} />}
    </button>
  );
}

function EmptyState() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 px-6 text-center">
      <div className="grid size-11 place-items-center rounded-xl bg-sunken text-ink-faint">
        <Languages size={20} strokeWidth={1.5} />
      </div>
      <div>
        <p className="text-sm font-medium text-ink-mid">翻訳を開始</p>
        <p className="mt-1.5 flex items-center justify-center gap-1 text-xs leading-relaxed text-ink-faint">
          <Kbd>⌘C</Kbd>
          <Kbd>⌘C</Kbd>
          <span className="ml-1">または左にテキストを入力</span>
        </p>
      </div>
    </div>
  );
}

export function RightPanel() {
  const inputText = useAppStore((s) => s.inputText);
  const outputText = useAppStore((s) => s.outputText);
  const isTranslating = useAppStore((s) => s.isTranslating);
  const error = useAppStore((s) => s.error);
  const hasTranslated = useAppStore((s) => s.hasTranslated);
  const sourceLanguage = useAppStore((s) => s.sourceLanguage);
  const targetLanguage = useAppStore((s) => s.targetLanguage);

  const showContent = hasTranslated || isTranslating || outputText || error;

  if (!showContent) {
    return (
      <div className="flex min-w-0 flex-1 flex-col border-l border-edge bg-surface">
        <EmptyState />
      </div>
    );
  }

  const sourceLabel = languageLabelForUi(sourceLanguage);
  const targetLabel = languageLabelForUi(targetLanguage);

  return (
    <div className="flex min-w-0 flex-1 flex-col border-l border-edge bg-surface">
      <div className="shrink-0 border-b border-edge px-4 pt-3 pb-3">
        <div className="mb-1.5 flex items-center justify-between gap-2">
          <span className="text-[11px] font-medium tracking-wide text-ink-faint">
            原文 · {sourceLabel}
          </span>
          <div className="flex items-center gap-1">
            <CopyButton text={inputText} />
          </div>
        </div>
        <div className="max-h-[100px] overflow-y-auto">
          <p className="whitespace-pre-wrap wrap-break-word text-[13px] leading-relaxed text-ink-mid">
            {inputText || "（テキストなし）"}
          </p>
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col px-4 pt-3 pb-3">
        <div className="mb-1.5 flex items-center justify-end gap-1.5">
          <span className="mr-auto text-[11px] font-medium tracking-wide text-ink-faint">
            訳文 · {targetLabel}
          </span>
          {outputText && !isTranslating && <CopyButton text={outputText} />}
          <button
            type="button"
            className="rounded-full bg-accent px-3 py-1 text-[11px] font-medium text-white transition-colors duration-100 focus-ring hover:bg-accent-deep active:scale-[0.98] disabled:opacity-50"
            disabled={isTranslating || !inputText.trim()}
            onClick={() => void startTranslation(inputText)}
          >
            {isTranslating ? "翻訳中…" : "再翻訳"}
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {error ? (
            <div className="rounded-lg bg-danger-soft px-3 py-2.5 text-[13px] leading-relaxed text-danger">
              {error}
            </div>
          ) : (
            <StreamingMarkdown text={outputText} streaming={isTranslating} />
          )}
        </div>
      </div>
    </div>
  );
}
