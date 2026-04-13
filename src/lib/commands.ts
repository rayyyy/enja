import { invoke } from "@tauri-apps/api/core";
import { Channel } from "@tauri-apps/api/core";
import type { AppSettings, TranslateEvent } from "../types";

export async function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  return invoke("save_settings", { settings });
}

export async function hideWindow(): Promise<void> {
  return invoke("hide_window");
}

export async function translateStream(
  text: string,
  onEvent: (ev: TranslateEvent) => void,
): Promise<void> {
  const channel = new Channel<TranslateEvent>();
  channel.onmessage = (msg) => onEvent(msg);
  await invoke("translate", { text, channel });
}
