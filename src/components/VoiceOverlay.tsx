import { useEffect, useLayoutEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { VoiceLevelEvent, VoiceResultEvent, VoiceStateEvent } from "../types";
import { cancelVoiceSession } from "../lib/commands";
import { useAppStore } from "../stores/useAppStore";

const BARS = 12;

export function VoiceOverlay() {
  const voiceDictationShortcut = useAppStore((s) => s.voiceDictationShortcut);
  const voiceAskShortcut = useAppStore((s) => s.voiceAskShortcut);
  const [state, setState] = useState<VoiceStateEvent>({
    state: "recording",
    mode: "dictation",
    message: null,
  });
  const [level, setLevel] = useState<VoiceLevelEvent>({ rms: 0, peak: 0 });
  const [energy, setEnergy] = useState(0);
  const [result, setResult] = useState<VoiceResultEvent | null>(null);
  const [copied, setCopied] = useState(false);
  const [tick, setTick] = useState(0);

  useLayoutEffect(() => {
    document.body.classList.add("voice-window");
    return () => document.body.classList.remove("voice-window");
  }, []);

  useEffect(() => {
    const stateListener = listen<VoiceStateEvent>("voice-state", (event) => {
      setState(event.payload);
      if (event.payload.state === "recording" || event.payload.state === "processing") {
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
    return () => {
      void stateListener.then((fn) => fn());
      void levelListener.then((fn) => fn());
      void resultListener.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (state.state !== "recording" && state.state !== "processing") {
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

  const bars = Array.from({ length: BARS }, (_, i) => {
    const wave =
      state.state === "processing"
        ? Math.sin(i * 0.5 + tick * 1.12) * 0.34 + 0.72
        : Math.sin(i * 0.78 + tick * 0.76) * 0.28 + 0.78;
    const idleMotion =
      state.state === "processing" ? 0.58 : state.state === "recording" ? 0.1 : 0.04;
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

  const expanded = state.state === "fallback" || state.state === "error";
  const isAskMode = state.mode === "ask";
  const showInlineText = expanded;
  const stateGlyph =
    state.state === "recording"
      ? "mic"
      : state.state === "processing"
        ? "off"
        : state.state === "inserted"
          ? "done"
          : state.state === "error"
            ? "error"
            : "none";
  const tone =
    state.state === "error"
      ? "red"
      : state.state === "processing"
        ? "amber"
        : state.state === "fallback"
          ? "sky"
          : isAskMode
            ? "sky"
            : "emerald";

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
            {stateGlyph === "off" ? (
              <span className="relative h-4 w-4 rounded-full border-2 border-amber-200/80">
                <span className="absolute top-1/2 left-1/2 h-0.5 w-6 -translate-x-1/2 -translate-y-1/2 rotate-45 rounded-full bg-amber-200/90 shadow-[0_0_18px_rgba(253,230,138,0.55)]" />
              </span>
            ) : null}
            {stateGlyph === "done" ? (
              <span className="h-3.5 w-2.5 rotate-45 border-r-2 border-b-2 border-emerald-200" />
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
              </div>
              <p className="mt-1 truncate text-[11px] text-white/58">
                {state.message ?? "入力先が見つかりませんでした。"}
              </p>
            </div>
          ) : null}
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
          {!showInlineText ? (
            <div className="flex min-w-[54px] max-w-[96px] shrink-0 justify-end">
              <span
                className={`flex h-5 max-w-full items-center truncate rounded-md border px-2 text-[10px] font-semibold leading-none ${
                  isAskMode
                    ? "border-sky-200/25 bg-sky-300/10 text-sky-100"
                    : "border-emerald-200/20 bg-emerald-300/10 text-emerald-100/80"
                }`}
              >
                {isAskMode ? voiceAskShortcut.label : voiceDictationShortcut.label}
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
