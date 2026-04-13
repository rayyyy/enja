import { useRef, useEffect } from "react";
import { useAppStore } from "../stores/useAppStore";
import { startTranslation } from "../lib/startTranslation";

export function LeftPanel() {
  const inputText = useAppStore((s) => s.inputText);
  const setInputText = useAppStore((s) => s.setInputText);
  const isTranslating = useAppStore((s) => s.isTranslating);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

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

  return (
    <div className="flex h-full w-[260px] shrink-0 flex-col border-r border-neutral-200 bg-neutral-50/80">
      <textarea
        ref={textareaRef}
        className="flex-1 resize-none bg-transparent px-4 pt-4 pb-2 text-[14px] leading-relaxed text-neutral-800 placeholder:text-neutral-400 focus:outline-none"
        placeholder="翻訳するテキストを入力…"
        value={inputText}
        onChange={(e) => setInputText(e.target.value)}
        onKeyDown={handleKeyDown}
        spellCheck={false}
      />
      <div className="px-4 pb-3">
        <p className="text-[11px] leading-relaxed text-neutral-400">
          Enter で翻訳 / Shift+Enter で改行
        </p>
      </div>
    </div>
  );
}
