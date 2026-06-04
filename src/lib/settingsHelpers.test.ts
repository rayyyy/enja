import { expect, test } from "bun:test";
import type { AppSettings } from "../types";
import { withTranslationLanguages } from "./settingsHelpers";

const baseSettings: AppSettings = {
  translation: {
    sourceLanguage: "en",
    targetLanguage: "ja",
  },
  voice: {
    selectedMicrophoneId: null,
    speechProfile: "googleChirp3",
    finalizationModel: "gemini35Flash",
    interactionSoundsEnabled: true,
    systemAudioHandling: "mute",
    maxRecordingSeconds: 300,
    googleCloudProjectId: "",
    googleCloudRegion: "asia-northeast1",
    googleCloudUseAdc: true,
    modeProfiles: [
      {
        id: "default",
        name: "デフォルト",
        description: "話した内容を自然な日本語文として整えます。",
        formattingEnabled: true,
        systemPrompt: "system",
        userPrompt: "{{transcript}}",
        deletable: false,
        order: 0,
        presetKey: "default",
      },
    ],
    activeModeProfileId: "default",
  },
  shortcuts: {
    voiceDictation: {
      keyCode: null,
      key: "fn",
      label: "Fn",
      tapCount: 1,
      modifiers: {
        command: false,
        option: false,
        control: false,
        shift: false,
        function: false,
      },
    },
    voiceAsk: {
      keyCode: 49,
      key: "space",
      label: "Fn Space",
      tapCount: 1,
      modifiers: {
        command: false,
        option: false,
        control: false,
        shift: false,
        function: true,
      },
    },
  },
  prompts: {
    overrides: {
      translateEnToJa: null,
      translateJaToEn: null,
      openaiTranscription: null,
      geminiAudioSystem: null,
      geminiAudioUser: null,
      dictationSystem: null,
      dictationUser: null,
      askWithoutSelectionSystem: null,
      askWithoutSelectionUser: null,
      askWithSelectionSystem: null,
      askWithSelectionUser: null,
    },
  },
  app: {
    doubleTapThresholdMs: 400,
    launchAtLogin: false,
  },
};

test("withTranslationLanguages updates only the nested translation settings", () => {
  const next = withTranslationLanguages(baseSettings, "ja", "en");

  expect(next.translation).toEqual({
    sourceLanguage: "ja",
    targetLanguage: "en",
  });
  expect(next.voice).toBe(baseSettings.voice);
  expect(next.shortcuts).toBe(baseSettings.shortcuts);
  expect(baseSettings.translation.sourceLanguage).toBe("en");
});
