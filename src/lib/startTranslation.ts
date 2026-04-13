import { useAppStore } from "../stores/useAppStore";
import { translateStream } from "./commands";

/** Gemini ストリーミング翻訳を開始（ホットキー・再翻訳ボタン共通） */
export async function startTranslation(text: string): Promise<void> {
  const {
    resetTranslation,
    setTranslating,
    setError,
    appendOutput,
  } = useAppStore.getState();

  if (!text.trim()) {
    useAppStore.getState().setError("テキストがありません。");
    return;
  }

  resetTranslation();
  setTranslating(true);
  setError(null);
  useAppStore.getState().setHasTranslated(true);
  try {
    await translateStream(text, (ev) => {
      if (ev.type === "chunk") {
        appendOutput(ev.text);
      } else if (ev.type === "done") {
        setTranslating(false);
      } else if (ev.type === "error") {
        setError(ev.message);
        setTranslating(false);
      }
    });
  } catch (e) {
    setError(String(e));
    setTranslating(false);
  }
}
