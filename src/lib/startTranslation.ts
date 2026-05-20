import { useAppStore } from "../stores/useAppStore";
import { translateStream } from "./commands";
import { createTranslationBatcher } from "./translationBatcher";

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
  const batcher = createTranslationBatcher(appendOutput);
  try {
    await translateStream(text, (ev) => {
      if (ev.type === "chunk") {
        batcher.append(ev.text);
      } else if (ev.type === "done") {
        batcher.flush();
        setTranslating(false);
      } else if (ev.type === "error") {
        batcher.flush();
        setError(ev.message);
        setTranslating(false);
      }
    });
    batcher.flush();
  } catch (e) {
    batcher.flush();
    setError(String(e));
    setTranslating(false);
  } finally {
    batcher.dispose();
  }
}
