import { useRef, useEffect, useMemo } from "react";
import { Settings, StickyNote } from "lucide-react";
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

  function openNotes() {
    setView("notes");
  }

  return (
    <div className="flex h-full w-[260px] shrink-0 flex-col border-r border-neutral-200 bg-neutral-50">
      <TranslationLanguageBar />
      <textarea
        ref={textareaRef}
        className="flex-1 resize-none bg-transparent px-4 pt-3 pb-2 text-[14px] leading-relaxed text-neutral-800 placeholder:text-neutral-400 focus:outline-none"
        placeholder={placeholder}
        value={inputText}
        onChange={(e) => setInputText(e.target.value)}
        onKeyDown={handleKeyDown}
        spellCheck={false}
      />
      <div className="flex items-start justify-between gap-2 px-4 pb-3">
        <p className="min-w-0 flex-1 text-[11px] leading-relaxed text-neutral-400">
          Enter で翻訳 / Shift+Enter で改行
        </p>
        <div className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            onClick={openNotes}
            className="grid size-8 place-items-center rounded-md text-neutral-400 transition-colors hover:bg-neutral-200/80 hover:text-neutral-600"
            title="メモ"
            aria-label="メモを開く"
          >
            <StickyNote size={16} />
          </button>
          <button
            type="button"
            onClick={openSettings}
            className="grid size-8 place-items-center rounded-md text-neutral-400 transition-colors hover:bg-neutral-200/80 hover:text-neutral-600"
            title="設定"
            aria-label="設定を開く"
          >
            <Settings size={16} />
          </button>
        </div>
      </div>
    </div>
  );
}
