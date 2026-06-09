import { invoke } from "@tauri-apps/api/core";
import { Channel } from "@tauri-apps/api/core";
import type {
  AppSettings,
  ApiUsageEvent,
  AppleSpeechStatus,
  AudioInputDevice,
  DictionaryBulkCreateResult,
  DictionaryEntry,
  DictionaryEntryInput,
  PromptCatalogItem,
  ProviderStatus,
  ShortcutAction,
  SpeechProfile,
  SpeechSetupCheck,
  StickyNote,
  StickyNoteInput,
  StoredNoteImage,
  TranslateEvent,
  VoiceMode,
} from "../types";

export async function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  return invoke("save_settings", { settings });
}

export async function getPromptCatalog(): Promise<PromptCatalogItem[]> {
  return invoke<PromptCatalogItem[]>("get_prompt_catalog");
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

export async function createDictionaryEntries(
  entries: DictionaryEntryInput[],
): Promise<DictionaryBulkCreateResult> {
  return invoke<DictionaryBulkCreateResult>("create_dictionary_entries", {
    entries,
  });
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

export async function getStickyNotes(): Promise<StickyNote[]> {
  return invoke<StickyNote[]>("get_sticky_notes");
}

export async function createStickyNote(): Promise<StickyNote> {
  return invoke<StickyNote>("create_sticky_note");
}

export async function updateStickyNote(
  note: StickyNoteInput,
): Promise<StickyNote> {
  return invoke<StickyNote>("update_sticky_note", { note });
}

export async function deleteStickyNote(id: string): Promise<void> {
  return invoke("delete_sticky_note", { id });
}

export async function showStickyNoteWindow(id: string): Promise<void> {
  return invoke("show_sticky_note_window", { id });
}

export async function hideStickyNoteWindow(id: string): Promise<void> {
  return invoke("hide_sticky_note_window", { id });
}

export async function saveStickyNoteImage(params: {
  noteId: string;
  mimeType: string;
  dataBase64: string;
  fileName?: string | null;
}): Promise<StoredNoteImage> {
  return invoke<StoredNoteImage>("save_sticky_note_image", params);
}

export async function undoDictionaryLearning(
  entryId: string,
  from: string,
  to: string,
): Promise<boolean> {
  return invoke<boolean>("undo_dictionary_learning", { entryId, from, to });
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

export async function getApiUsageEvents(): Promise<ApiUsageEvent[]> {
  return invoke<ApiUsageEvent[]>("get_api_usage_events");
}

export async function checkSpeechSetup(
  profile: SpeechProfile,
  settings: AppSettings,
): Promise<SpeechSetupCheck> {
  return invoke<SpeechSetupCheck>("check_speech_setup", { profile, settings });
}

export async function getAppleSpeechStatus(
  requestAuthorization: boolean,
): Promise<AppleSpeechStatus> {
  return invoke<AppleSpeechStatus>("get_apple_speech_status", {
    requestAuthorization,
  });
}

export async function installAppleSpeechModel(): Promise<AppleSpeechStatus> {
  return invoke<AppleSpeechStatus>("install_apple_speech_model");
}
