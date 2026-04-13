import { useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import { startTranslation } from "../lib/startTranslation";
import { StreamingMarkdown } from "./StreamingMarkdown";
import { languageLabelForUi } from "../lib/uiLanguage";

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
      className="rounded-md p-1 text-neutral-400 transition-colors hover:bg-neutral-100 hover:text-neutral-600"
      title="コピー"
    >
      {copied ? (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="20 6 9 17 4 12" />
        </svg>
      ) : (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
        </svg>
      )}
    </button>
  );
}

function EmptyState() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
      <div className="rounded-full bg-blue-50 p-3">
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" className="text-blue-400">
          <path d="M5 8l6 6" />
          <path d="M4 14l6-6 2-3" />
          <path d="M2 5h12" />
          <path d="M7 2h1" />
          <path d="M22 22l-5-10-5 10" />
          <path d="M14 18h6" />
        </svg>
      </div>
      <div>
        <p className="text-sm font-medium text-neutral-500">翻訳を開始</p>
        <p className="mt-1 text-xs leading-relaxed text-neutral-400">
          Cmd+C を2回押すか、左のエリアに
          <br />
          テキストを入力してください
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
      <div className="flex min-w-0 flex-1 flex-col">
        <EmptyState />
      </div>
    );
  }

  const sourceLabel = languageLabelForUi(sourceLanguage);
  const targetLabel = languageLabelForUi(targetLanguage);

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div className="shrink-0 border-b border-neutral-200 px-4 pt-3 pb-3">
        <div className="mb-1.5 flex items-center justify-between gap-2">
          <span className="text-[11px] font-medium tracking-wide text-neutral-400">
            翻訳前（{sourceLabel}）
          </span>
          <div className="flex items-center gap-1">
            <CopyButton text={inputText} />
          </div>
        </div>
        <p className="max-h-[100px] overflow-y-auto whitespace-pre-wrap wrap-break-word text-[13px] leading-relaxed text-neutral-700">
          {inputText || "（テキストなし）"}
        </p>
      </div>

      <div className="flex min-h-0 flex-1 flex-col px-4 pt-3 pb-3">
        <div className="mb-1.5 flex items-center justify-end gap-2">
          <span className="mr-auto text-[11px] font-medium tracking-wide text-neutral-400">
            翻訳後（{targetLabel}）
          </span>
          {outputText && !isTranslating && (
            <CopyButton text={outputText} />
          )}
          <button
            type="button"
            className="rounded-full bg-blue-500 px-3 py-0.5 text-[11px] font-medium text-white shadow-sm transition-colors hover:bg-blue-600 disabled:opacity-50"
            disabled={isTranslating || !inputText.trim()}
            onClick={() => void startTranslation(inputText)}
          >
            {isTranslating ? "翻訳中…" : "再翻訳"}
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {error ? (
            <p className="text-sm text-red-500">{error}</p>
          ) : (
            <StreamingMarkdown text={outputText} streaming={isTranslating} />
          )}
        </div>
      </div>
    </div>
  );
}
