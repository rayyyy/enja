import type { AudioInputDevice } from "../types";

export function parseAudioInputDeviceId(
  id: string,
): { name: string; index: number } | null {
  const hashIndex = id.lastIndexOf("#");
  if (hashIndex <= 0) {
    return null;
  }

  const suffix = id.slice(hashIndex + 1);
  if (!/^\d+$/.test(suffix)) {
    return null;
  }

  const index = Number(suffix);
  if (!Number.isInteger(index)) {
    return null;
  }

  return {
    name: id.slice(0, hashIndex),
    index,
  };
}

export function remapSelectedMicrophoneId(
  selectedId: string | null,
  devices: AudioInputDevice[],
): string | null {
  if (!selectedId) {
    return null;
  }
  if (devices.some((device) => device.id === selectedId)) {
    return selectedId;
  }

  const parsed = parseAudioInputDeviceId(selectedId);
  if (!parsed) {
    return null;
  }

  const matches = devices.filter((device) => device.name === parsed.name);
  if (matches.length === 1) {
    return matches[0].id;
  }

  return null;
}
