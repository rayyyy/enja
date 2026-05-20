export type TranslationBatcher = {
  append: (text: string) => void;
  flush: () => void;
  dispose: () => void;
};

type TimerId = ReturnType<typeof setTimeout>;

export function createTranslationBatcher(
  onFlush: (text: string) => void,
  delayMs = 16,
): TranslationBatcher {
  let pending = "";
  let timer: TimerId | null = null;

  function flush() {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    if (!pending) return;
    const text = pending;
    pending = "";
    onFlush(text);
  }

  function schedule() {
    if (timer !== null) return;
    timer = setTimeout(flush, delayMs);
  }

  return {
    append(text) {
      if (!text) return;
      pending += text;
      schedule();
    },
    flush,
    dispose() {
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
      pending = "";
    },
  };
}
