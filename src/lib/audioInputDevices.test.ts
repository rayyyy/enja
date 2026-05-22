import { expect, test } from "bun:test";
import type { AudioInputDevice } from "../types";
import {
  parseAudioInputDeviceId,
  remapSelectedMicrophoneId,
} from "./audioInputDevices";

const devices: AudioInputDevice[] = [
  { id: "MacBook Pro Microphone#0", name: "MacBook Pro Microphone", isDefault: true },
  { id: "USB Mic#1", name: "USB Mic", isDefault: false },
  { id: "Studio #1#2", name: "Studio #1", isDefault: false },
];

test("parseAudioInputDeviceId reads the trailing index only", () => {
  expect(parseAudioInputDeviceId("MacBook Pro Microphone#0")).toEqual({
    name: "MacBook Pro Microphone",
    index: 0,
  });
  expect(parseAudioInputDeviceId("Studio #1#2")).toEqual({
    name: "Studio #1",
    index: 2,
  });
  expect(parseAudioInputDeviceId("invalid")).toBeNull();
  expect(parseAudioInputDeviceId("Mic#abc")).toBeNull();
});

test("remapSelectedMicrophoneId keeps valid ids unchanged", () => {
  expect(remapSelectedMicrophoneId("USB Mic#1", devices)).toBe("USB Mic#1");
  expect(remapSelectedMicrophoneId(null, devices)).toBeNull();
});

test("remapSelectedMicrophoneId remaps stale ids by unique device name", () => {
  expect(remapSelectedMicrophoneId("USB Mic#9", devices)).toBe("USB Mic#1");
  expect(remapSelectedMicrophoneId("Studio #1#0", devices)).toBe("Studio #1#2");
});

test("remapSelectedMicrophoneId falls back to system default when ambiguous", () => {
  const ambiguous: AudioInputDevice[] = [
    { id: "Duplicate#0", name: "Duplicate", isDefault: false },
    { id: "Duplicate#1", name: "Duplicate", isDefault: false },
  ];

  expect(remapSelectedMicrophoneId("Duplicate#9", ambiguous)).toBeNull();
  expect(remapSelectedMicrophoneId("missing-name#0", devices)).toBeNull();
});
