import { useRef, useEffect, useMemo } from "react";
import { useAppStore } from "../stores/useAppStore";
import { startTranslation } from "../lib/startTranslation";
import { TranslationLanguageBar } from "./TranslationLanguageBar";

export function LeftPanel() {
  const inputText = useAppStore((s) => s.inputText);
  const setInputText = useAppStore((s) => s.setInputText);
  const isTranslating = useAppStore((s) => s.isTranslating);
  const sourceLanguage = useAppStore((s) => s.sourceLanguage);
  const setView = useAppStore((s) => s.setView);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const placeholder = useMemo(
    () =>
      sourceLanguage === "en"
        ? "英語のテキストを入力…"
        : "日本語のテキストを入力…",
    [sourceLanguage],
  );

  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (inputText.trim() && !isTranslating) {
        void startTranslation(inputText);
      }
    }
  }

  function openSettings() {
    setView("settings");
  }

  return (
    <div className="flex h-full w-[260px] shrink-0 flex-col border-r border-neutral-200 bg-neutral-50/80">
      <TranslationLanguageBar />
      <textarea
        data-tauri-no-drag-region
        ref={textareaRef}
        className="flex-1 resize-none bg-transparent px-4 pt-3 pb-2 text-[14px] leading-relaxed text-neutral-800 placeholder:text-neutral-400 focus:outline-none"
        placeholder={placeholder}
        value={inputText}
        onChange={(e) => setInputText(e.target.value)}
        onKeyDown={handleKeyDown}
        spellCheck={false}
      />
      <div
        data-tauri-no-drag-region
        className="flex items-start justify-between gap-2 px-4 pb-3"
      >
        <p className="min-w-0 flex-1 text-[11px] leading-relaxed text-neutral-400">
          Enter で翻訳 / Shift+Enter で改行
        </p>
        <button
          type="button"
          onClick={openSettings}
          className="shrink-0 rounded-md p-1.5 text-neutral-400 transition-colors hover:bg-neutral-200/80 hover:text-neutral-600"
          title="設定"
          aria-label="設定を開く"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden
          >
            <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
        </button>
      </div>
    </div>
  );
}
