import { invoke } from "@tauri-apps/api/core";
import { Channel } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AudioInputDevice,
  DictionaryEntry,
  DictionaryEntryInput,
  PromptTemplates,
  ProviderStatus,
  ShortcutAction,
  SpeechProfile,
  SpeechSetupCheck,
  TranslateEvent,
  VoiceMode,
} from "../types";

export async function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  return invoke("save_settings", { settings });
}

export async function getPromptDefaults(): Promise<PromptTemplates> {
  return invoke<PromptTemplates>("get_prompt_defaults");
}

export async function startShortcutCapture(
  action: ShortcutAction,
): Promise<void> {
  return invoke("start_shortcut_capture", { action });
}

export async function cancelShortcutCapture(): Promise<void> {
  return invoke("cancel_shortcut_capture");
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

export async function listAudioInputDevices(): Promise<AudioInputDevice[]> {
  return invoke<AudioInputDevice[]>("list_audio_input_devices");
}

export async function startVoiceSession(mode: VoiceMode): Promise<void> {
  return invoke("start_voice_session", { mode });
}

export async function stopVoiceSession(): Promise<void> {
  return invoke("stop_voice_session");
}

export async function cancelVoiceSession(): Promise<void> {
  return invoke("cancel_voice_session");
}

export async function getDictionary(): Promise<DictionaryEntry[]> {
  return invoke<DictionaryEntry[]>("get_dictionary");
}

export async function createDictionaryEntry(
  entry: DictionaryEntryInput,
): Promise<DictionaryEntry> {
  return invoke<DictionaryEntry>("create_dictionary_entry", { entry });
}

export async function updateDictionaryEntry(
  id: string,
  entry: DictionaryEntryInput,
): Promise<DictionaryEntry> {
  return invoke<DictionaryEntry>("update_dictionary_entry", { id, entry });
}

export async function deleteDictionaryEntry(id: string): Promise<void> {
  return invoke("delete_dictionary_entry", { id });
}

export async function saveProviderSecret(
  provider: string,
  secret: string,
): Promise<void> {
  return invoke("save_provider_secret", { provider, secret });
}

export async function getProviderStatus(): Promise<ProviderStatus> {
  return invoke<ProviderStatus>("get_provider_status");
}

export async function checkSpeechSetup(
  profile: SpeechProfile,
  settings: AppSettings,
): Promise<SpeechSetupCheck> {
  return invoke<SpeechSetupCheck>("check_speech_setup", { profile, settings });
}
