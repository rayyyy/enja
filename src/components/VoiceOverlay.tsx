import { useEffect, useLayoutEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { VoiceResultEvent, VoiceStateEvent } from "../types";
import { cancelVoiceSession } from "../lib/commands";
import { useAppStore } from "../stores/useAppStore";

export function VoiceOverlay() {
  const voiceDictationShortcut = useAppStore((s) => s.voiceDictationShortcut);
  const voiceAskShortcut = useAppStore((s) => s.voiceAskShortcut);
  const [state, setState] = useState<VoiceStateEvent>({
    state: "recording",
    mode: "dictation",
    message: null,
  });
  const [result, setResult] = useState<VoiceResultEvent | null>(null);
  const [copied, setCopied] = useState(false);

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
    const resultListener = listen<VoiceResultEvent>("voice-result", (event) => {
      setResult(event.payload);
    });
    return () => {
      void stateListener.then((fn) => fn());
      void resultListener.then((fn) => fn());
    };
  }, []);

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

  function copyResult() {
    if (!result?.text) return;
    void navigator.clipboard.writeText(result.text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    });
  }

  const notify = state.state === "fallback" || state.state === "error";
  const isAskMode = state.mode === "ask";
  const shortcutLabel = isAskMode ? voiceAskShortcut.label : voiceDictationShortcut.label;
  const statusLabel =
    state.state === "processing"
      ? "処理中"
      : state.state === "recording"
        ? "録音中"
        : null;

  if (notify) {
    return (
      <div className="flex h-full w-full items-end justify-center bg-transparent p-2">
        <div className="flex h-full w-full flex-col overflow-hidden rounded-[22px] bg-neutral-950/[0.94] text-white shadow-[0_18px_54px_rgba(0,0,0,0.36)] backdrop-blur-xl">
          <div className="flex min-h-[70px] items-center gap-3 px-4">
            <span
              className={`size-2.5 shrink-0 rounded-full shadow-[0_0_20px_currentColor] ${
                state.state === "error" ? "bg-red-400 text-red-400" : "bg-amber-300 text-amber-300"
              }`}
            />
            <div className="min-w-0 flex-1">
              <p className="truncate text-[15px] font-semibold text-white">
                {state.state === "error" ? "エラー" : "コピーして使用"}
              </p>
              <p className="mt-1 truncate text-[11px] text-white/58">
                {state.message ?? "入力先が見つかりませんでした。"}
              </p>
            </div>
          </div>
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
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full w-full items-end justify-center bg-transparent p-0">
      <div className="flex h-full w-full items-center gap-2 rounded-full bg-neutral-950/[0.94] px-3 text-white backdrop-blur-xl">
        <span
          className={`size-2 shrink-0 rounded-full ${
            state.state === "processing"
              ? "animate-pulse bg-amber-300 shadow-[0_0_14px_rgba(252,211,77,0.55)]"
              : isAskMode
                ? "animate-pulse bg-sky-300 shadow-[0_0_14px_rgba(125,211,252,0.5)]"
                : "animate-pulse bg-emerald-300 shadow-[0_0_14px_rgba(110,231,183,0.5)]"
          }`}
        />
        {statusLabel ? (
          <span className="text-[11px] font-medium text-white/72">{statusLabel}</span>
        ) : null}
        <span className="ml-auto truncate text-[10px] font-semibold text-white/55">
          {shortcutLabel}
        </span>
      </div>
    </div>
  );
}
