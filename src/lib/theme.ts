import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useSyncExternalStore } from "react";

export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "enja-theme";
const THEME_EVENT = "enja-theme-changed";

// tauri.conf.json の main ウィンドウ backgroundColor と合わせる。
// 透明タイトルバー帯にウィンドウ背景色がそのまま見えるため、テーマと同期させる。
const WINDOW_BG: Record<ResolvedTheme, string> = {
  light: "#f7f7f8",
  dark: "#121214",
};

const subscribers = new Set<() => void>();
const systemDark = window.matchMedia("(prefers-color-scheme: dark)");

export function getThemePreference(): ThemePreference {
  const stored = window.localStorage.getItem(STORAGE_KEY);
  return stored === "light" || stored === "dark" ? stored : "system";
}

export function resolveTheme(pref: ThemePreference): ResolvedTheme {
  if (pref === "system") return systemDark.matches ? "dark" : "light";
  return pref;
}

export function setThemePreference(pref: ThemePreference) {
  window.localStorage.setItem(STORAGE_KEY, pref);
  applyTheme();
  notify();
  // 付箋ウィンドウ・音声ウィンドウは別WebViewなのでTauriイベントで同期する。
  void emit(THEME_EVENT, pref).catch(() => undefined);
}

export function useThemePreference(): ThemePreference {
  return useSyncExternalStore(subscribe, getThemePreference);
}

function subscribe(callback: () => void) {
  subscribers.add(callback);
  return () => {
    subscribers.delete(callback);
  };
}

function notify() {
  for (const callback of subscribers) callback();
}

function applyTheme() {
  const resolved = resolveTheme(getThemePreference());
  const root = document.documentElement;
  root.classList.toggle("dark", resolved === "dark");
  // select等のネイティブコントロールとスクロールバーをテーマに追従させる。
  root.style.colorScheme = resolved;
  const window = getCurrentWindow();
  // macOSの透明タイトルバー領域はネイティブ側のテーマに追従する。
  void window.setTheme(resolved).catch(() => undefined);
  void window.setBackgroundColor(WINDOW_BG[resolved]).catch(() => undefined);
}

/** 各ウィンドウのエントリポイントで一度だけ呼ぶ。 */
export function initTheme() {
  applyTheme();
  systemDark.addEventListener("change", () => {
    if (getThemePreference() === "system") {
      applyTheme();
      notify();
    }
  });
  void listen<ThemePreference>(THEME_EVENT, (event) => {
    if (event.payload !== getThemePreference()) {
      window.localStorage.setItem(STORAGE_KEY, event.payload);
    }
    applyTheme();
    notify();
  });
}
