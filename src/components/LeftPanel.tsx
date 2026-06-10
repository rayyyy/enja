import { useRef, useEffect, useMemo } from "react";
import { Settings, StickyNote } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { startTranslation } from "../lib/startTranslation";
import { TranslationLanguageBar } from "./TranslationLanguageBar";
import { IconButton, Kbd } from "./ui";

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
    <div className="flex h-full w-[264px] shrink-0 flex-col bg-canvas">
      <TranslationLanguageBar />
      <textarea
        ref={textareaRef}
        className="flex-1 resize-none bg-transparent px-4 pt-3 pb-2 text-[14px] leading-relaxed text-ink placeholder:text-ink-faint focus:outline-none"
        placeholder={placeholder}
        value={inputText}
        onChange={(e) => setInputText(e.target.value)}
        onKeyDown={handleKeyDown}
        spellCheck={false}
      />
      <div className="flex items-center justify-between gap-2 px-3 pb-2.5">
        <p className="flex min-w-0 flex-1 items-center gap-1 text-[11px] text-ink-faint">
          <Kbd>⏎</Kbd>
          <span className="mr-1.5">翻訳</span>
          <Kbd>⇧⏎</Kbd>
          <span>改行</span>
        </p>
        <div className="flex shrink-0 items-center gap-0.5">
          <IconButton title="メモ" aria-label="メモを開く" onClick={openNotes}>
            <StickyNote size={15} />
          </IconButton>
          <IconButton title="設定" aria-label="設定を開く" onClick={openSettings}>
            <Settings size={15} />
          </IconButton>
        </div>
      </div>
    </div>
  );
}
