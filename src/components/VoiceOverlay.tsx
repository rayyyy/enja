import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  VoiceDictionaryLearningEvent,
  VoiceLevelEvent,
  VoiceResultEvent,
  VoiceStateEvent,
} from "../types";
import { cancelVoiceSession, undoDictionaryLearning } from "../lib/commands";
import { useAppStore } from "../stores/useAppStore";

const BARS = 12;
const DICTIONARY_NOTICE_MS = 6000;
const DICTIONARY_UNDONE_NOTICE_MS = 1200;

type DictionaryNotice = VoiceDictionaryLearningEvent & {
  status: "added" | "undone";
};

export function VoiceOverlay() {
  const voiceDictationShortcut = useAppStore((s) => s.voiceDictationShortcut);
  const voiceAskShortcut = useAppStore((s) => s.voiceAskShortcut);
  const [state, setState] = useState<VoiceStateEvent>({
    state: "preparing",
    mode: "dictation",
    modeProfileId: "default",
    modeProfileName: "デフォルト",
    message: null,
  });
  const [level, setLevel] = useState<VoiceLevelEvent>({ rms: 0, peak: 0 });
  const [energy, setEnergy] = useState(0);
  const [result, setResult] = useState<VoiceResultEvent | null>(null);
  const [dictionaryNotice, setDictionaryNotice] =
    useState<DictionaryNotice | null>(null);
  const [undoingDictionaryNotice, setUndoingDictionaryNotice] = useState(false);
  const [copied, setCopied] = useState(false);
  const [tick, setTick] = useState(0);
  const latestStateSeq = useRef(0);
  const dictionaryNoticeTimer = useRef<number | null>(null);

  useLayoutEffect(() => {
    document.body.classList.add("voice-window");
    return () => document.body.classList.remove("voice-window");
  }, []);

  function scheduleDictionaryNoticeClear(delayMs: number) {
    if (dictionaryNoticeTimer.current !== null) {
      window.clearTimeout(dictionaryNoticeTimer.current);
    }
    dictionaryNoticeTimer.current = window.setTimeout(() => {
      setDictionaryNotice(null);
      setUndoingDictionaryNotice(false);
      dictionaryNoticeTimer.current = null;
    }, delayMs);
  }

  useEffect(() => {
    const stateListener = listen<VoiceStateEvent>("voice-state", (event) => {
      const next = event.payload;
      if (typeof next.seq === "number") {
        if (next.seq < latestStateSeq.current) {
          return;
        }
        latestStateSeq.current = next.seq;
      }
      setState(next);
      if (
        next.state === "preparing" ||
        next.state === "recording" ||
        next.state === "stopping" ||
        next.state === "processing"
      ) {
        setResult(null);
      }
    });
    const levelListener = listen<VoiceLevelEvent>("voice-level", (event) => {
      setLevel(event.payload);
      setEnergy((prev) =>
        Math.max(
          prev,
          Math.min(1, Math.max(event.payload.rms * 16, event.payload.peak * 4)),
        ),
      );
    });
    const resultListener = listen<VoiceResultEvent>("voice-result", (event) => {
      setResult(event.payload);
    });
    const dictionaryListener = listen<VoiceDictionaryLearningEvent>(
      "voice-dictionary-learning",
      (event) => {
        setDictionaryNotice({ ...event.payload, status: "added" });
        setUndoingDictionaryNotice(false);
        scheduleDictionaryNoticeClear(DICTIONARY_NOTICE_MS);
      },
    );
    return () => {
      if (dictionaryNoticeTimer.current !== null) {
        window.clearTimeout(dictionaryNoticeTimer.current);
      }
      void stateListener.then((fn) => fn());
      void levelListener.then((fn) => fn());
      void resultListener.then((fn) => fn());
      void dictionaryListener.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (
      state.state !== "preparing" &&
      state.state !== "recording" &&
      state.state !== "stopping" &&
      state.state !== "processing"
    ) {
      return;
    }
    const id = window.setInterval(() => {
      setTick((v) => v + 1);
      setEnergy((v) => Math.max(0, v * 0.86 - 0.012));
    }, 70);
    return () => window.clearInterval(id);
  }, [state.state]);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        void cancelVoiceSession();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const isActive =
    state.state === "preparing" ||
    state.state === "recording" ||
    state.state === "stopping" ||
    state.state === "processing";

  const bars = Array.from({ length: BARS }, (_, i) => {
    const wave =
      state.state === "processing" || state.state === "stopping"
        ? Math.sin(i * 0.5 + tick * 1.12) * 0.34 + 0.72
        : Math.sin(i * 0.78 + tick * 0.76) * 0.28 + 0.78;
    const idleMotion =
      state.state === "processing"
        ? 0.58
        : state.state === "stopping"
          ? 0.34
          : state.state === "preparing"
            ? 0.18
            : state.state === "recording"
              ? 0.1
              : 0.04;
    const amount = Math.max(energy, level.rms * 8, level.peak * 2.4, idleMotion);
    return Math.max(5, Math.min(24, amount * 26 * wave));
  });

  function copyResult() {
    if (!result?.text) return;
    void navigator.clipboard.writeText(result.text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    });
  }

  function undoDictionaryNotice() {
    if (!dictionaryNotice || dictionaryNotice.status !== "added") return;
    const notice = dictionaryNotice;
    setUndoingDictionaryNotice(true);
    void undoDictionaryLearning(notice.entryId, notice.from, notice.to)
      .then((undone) => {
        if (undone) {
          setDictionaryNotice({ ...notice, status: "undone" });
          scheduleDictionaryNoticeClear(DICTIONARY_UNDONE_NOTICE_MS);
        } else {
          setDictionaryNotice(null);
        }
      })
      .catch(() => {
        setUndoingDictionaryNotice(false);
      });
  }

  const expanded = state.state === "fallback" || state.state === "error";
  const isAskMode = state.mode === "ask";
  const modeName = isAskMode ? "Ask" : (state.modeProfileName ?? "デフォルト");
  const compactShortcutLabel = isAskMode
    ? voiceAskShortcut.label
    : `${modeName} · ${voiceDictationShortcut.label}`;
  const compactStatusLabel =
    state.state === "preparing"
      ? "準備中"
      : state.state === "stopping"
        ? "終了中"
        : state.state === "processing"
          ? "処理中"
          : compactShortcutLabel;
  const showInlineText = expanded;
  const stateGlyph =
    state.state === "preparing"
      ? "pending"
      : state.state === "recording"
      ? "mic"
      : state.state === "stopping" || state.state === "processing"
        ? "off"
        : state.state === "error"
          ? "error"
          : "none";
  const tone =
    state.state === "error"
      ? "red"
      : state.state === "preparing" ||
          state.state === "stopping" ||
          state.state === "processing"
        ? "amber"
        : state.state === "fallback"
          ? "sky"
          : isAskMode
            ? "sky"
            : "emerald";

  if (dictionaryNotice) {
    const undone = dictionaryNotice.status === "undone";
    return (
      <div className="flex h-full w-full items-end justify-center bg-transparent p-0">
        <div className="flex h-full w-full items-center gap-2 overflow-hidden rounded-full bg-neutral-950/[0.94] px-4 text-white shadow-none backdrop-blur-xl">
          <span className="size-2.5 shrink-0 rounded-full bg-emerald-300 text-emerald-300 shadow-[0_0_18px_currentColor]" />
          <div className="min-w-0 flex-1">
            <p className="truncate text-[12px] font-semibold text-white">
              {undone ? "辞書追加を取り消しました" : "辞書に追加しました"}
            </p>
            <p className="truncate text-[10px] text-white/58">
              {dictionaryNotice.from} → {dictionaryNotice.to}
            </p>
          </div>
          {!undone ? (
            <button
              type="button"
              onClick={undoDictionaryNotice}
              disabled={undoingDictionaryNotice}
              className="h-8 shrink-0 rounded-full bg-white px-3 text-[11px] font-semibold text-neutral-950 hover:bg-white/90 disabled:opacity-45"
            >
              Undo
            </button>
          ) : null}
        </div>
      </div>
    );
  }

  if (!isActive && !expanded) {
    return null;
  }

  return (
    <div className={`flex h-full w-full items-end justify-center bg-transparent ${expanded ? "p-2" : "p-0"}`}>
      <div
        className={`flex h-full w-full overflow-hidden bg-neutral-950/[0.94] text-white backdrop-blur-xl ${
          expanded
            ? "flex-col rounded-[22px] shadow-[0_18px_54px_rgba(0,0,0,0.36)]"
            : "items-center rounded-full px-2.5 shadow-none"
        }`}
      >
        <div
          className={`flex items-center ${
            expanded ? "min-h-[70px] gap-3 px-4" : "h-full w-full gap-1.5"
          }`}
        >
          <div
            className={`flex h-7 shrink-0 items-center ${
              showInlineText ? "w-9 justify-center" : "w-[54px] justify-start pl-2"
            }`}
          >
            {stateGlyph === "mic" ? (
              <span
                className={`size-2.5 rounded-full shadow-[0_0_18px_currentColor] animate-pulse ${
                  isAskMode
                    ? "bg-sky-300 text-sky-300"
                    : "bg-emerald-300 text-emerald-300"
                }`}
              />
            ) : null}
            {stateGlyph === "pending" ? (
              <span className="size-3 rounded-full border-2 border-amber-200/80 border-t-transparent animate-spin" />
            ) : null}
            {stateGlyph === "off" ? (
              <span className="relative h-4 w-4 rounded-full border-2 border-amber-200/80">
                <span className="absolute top-1/2 left-1/2 h-0.5 w-6 -translate-x-1/2 -translate-y-1/2 rotate-45 rounded-full bg-amber-200/90 shadow-[0_0_18px_rgba(253,230,138,0.55)]" />
              </span>
            ) : null}
            {stateGlyph === "error" ? (
              <span className="size-2.5 rounded-full bg-red-400 text-red-400 shadow-[0_0_20px_currentColor]" />
            ) : null}
          </div>
          {showInlineText ? (
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <p className="truncate text-[15px] font-semibold text-white">
                  {state.state === "error" ? "エラー" : "コピーして使用"}
                </p>
                <span
                  className={`rounded-full px-2 py-0.5 text-[10px] font-semibold ${
                    isAskMode
                      ? "bg-sky-300/10 text-sky-100"
                      : "bg-emerald-300/10 text-emerald-100"
                  }`}
                >
                  {modeName}
                </span>
              </div>
              <p className="mt-1 truncate text-[11px] text-white/58">
                {state.message ?? "入力先が見つかりませんでした。"}
              </p>
            </div>
          ) : null}
          {isActive ? (
            <div
              className={`flex items-center justify-center gap-1.5 rounded-full ${
                showInlineText ? "h-10 w-40 bg-white/[0.05] px-3" : "h-8 min-w-0 flex-1 px-1"
              }`}
            >
              {bars.map((height, i) => (
                <span
                  key={i}
                  className={`w-[3px] rounded-full transition-[height,opacity] duration-75 ${
                    tone === "red"
                      ? "bg-red-300/80"
                      : tone === "amber"
                        ? "bg-amber-200/90"
                        : tone === "sky"
                          ? "bg-sky-200/90"
                          : "bg-emerald-200/90"
                  }`}
                  style={{ height }}
                />
              ))}
            </div>
          ) : null}
          {!showInlineText && isActive ? (
            <div className="flex min-w-[72px] max-w-[132px] shrink-0 justify-end">
              <span
                className={`flex h-5 max-w-full items-center truncate rounded-md border px-2 text-[10px] font-semibold leading-none ${
                  isAskMode
                    ? "border-sky-200/25 bg-sky-300/10 text-sky-100"
                    : "border-emerald-200/20 bg-emerald-300/10 text-emerald-100/80"
                }`}
              >
                {compactStatusLabel}
              </span>
            </div>
          ) : null}
        </div>

        {expanded ? (
          <div className="flex min-h-0 flex-1 flex-col px-4 pb-4">
            <div className="min-h-0 flex-1 overflow-y-auto rounded-xl bg-white/[0.06] p-4 text-[14px] leading-relaxed whitespace-pre-wrap text-white/90">
              {result?.text || state.message || "結果がありません。"}
            </div>
            <div className="mt-3 flex items-center justify-end gap-2">
              <button
                type="button"
                onClick={() => void cancelVoiceSession()}
                className="rounded-lg bg-white/[0.07] px-3 py-1.5 text-xs font-medium text-white/70 hover:bg-white/[0.12]"
              >
                閉じる
              </button>
              <button
                type="button"
                onClick={copyResult}
                disabled={!result?.text}
                className="rounded-lg bg-white px-3 py-1.5 text-xs font-semibold text-neutral-950 hover:bg-white/90 disabled:opacity-40"
              >
                {copied ? "コピー済み" : "コピー"}
              </button>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
