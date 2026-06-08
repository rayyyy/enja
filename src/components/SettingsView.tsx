import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type {
  AppSettings,
  ApiUsageEvent,
  AppleSpeechStatus,
  AudioInputDevice,
  FinalizationModel,
  PromptOverrides,
  PromptCatalogItem,
  ProviderStatus,
  ShortcutAction,
  ShortcutBinding,
  ShortcutCapturedEvent,
  ShortcutCaptureCancelledEvent,
  SpeechProfile,
  SpeechSetupCheck,
} from "../types";
import { useAppStore } from "../stores/useAppStore";
import {
  SettingsSectionPanel,
  Toggle,
  settingsButtonPrimaryClass,
  settingsButtonSecondaryClass,
  settingsInputClass,
  settingsSelectClass,
} from "./settings/SettingsControls";
import {
  AppSettingsSection,
  AuthSettingsSection,
  PromptSettingsSection,
  ShortcutSettingsSection,
  VoiceModeSettingsSection,
  type ProviderSecretsDraft,
} from "./settings/SettingsSections";
import { UsageCostsSection } from "./settings/UsageCostsSection";
import { remapSelectedMicrophoneId } from "../lib/audioInputDevices";
import {
  checkSpeechSetup,
  cancelShortcutCapture,
  getApiUsageEvents,
  getAppleSpeechStatus,
  getPromptCatalog,
  getProviderStatus,
  getSettings,
  installAppleSpeechModel,
  listAudioInputDevices,
  saveProviderSecret,
  saveSettings,
  startShortcutCapture,
} from "../lib/commands";

type SpeechProfileDoc = {
  label: string;
  url: string;
};

type SpeechProfileOption = {
  value: SpeechProfile;
  label: string;
  badge: string;
  note: string;
  price: string;
  priceNote: string;
  speed: string;
  accuracy: string;
  accuracyNote: string;
  setupSummary: string;
  setupSteps: string[];
  enjaDataFlow: string[];
  docs: SpeechProfileDoc[];
};

const SPEECH_PROFILES: SpeechProfileOption[] = [
  {
    value: "googleChirp3",
    label: "Google Chirp 3（精度優先）",
    badge: "推奨",
    note: "日本語精度と辞書ヒントを重視。短い録音はChirp 3、長い録音は保存済みのOpenAI/Geminiへ自動フォールバックします。",
    price: "中",
    priceNote: "目安 $0.016/分",
    speed: "高",
    accuracy: "最高",
    accuracyNote: "日本語/辞書向き",
    setupSummary: "Google CloudのProject ID、リージョン、ADCまたはサービスアカウントJSONを設定します。",
    setupSteps: [
      "Google CloudでSpeech-to-Text APIを有効化します。",
      "利用するProject IDを確認し、EnjaのGoogle Cloud Project IDへ入力します。",
      "Chirp 3が使えるリージョンを選び、EnjaのGoogle Cloudリージョンへ入力します。日本語用途の既定は asia-northeast1 です。",
      "ローカル開発ではADCを使う場合、gcloud auth application-default login を実行します。",
      "Spotlight/Dockから起動したEnjaはターミナルとPATHが違うため、Enjaは /opt/homebrew/bin/gcloud、/usr/local/bin/gcloud、~/google-cloud-sdk/bin/gcloud などを自動探索します。",
      "gcloudが見つからない場合は、Google Cloud SDKを通常の場所へインストールするか、ADCをオフにしてサービスアカウントJSONを保存します。",
      "ADCを使わない場合はサービスアカウントJSONを作成し、EnjaのサービスアカウントJSON欄へ保存します。",
    ],
    enjaDataFlow: [
      "Enjaは録音WAVをSpeech-to-Text V2 recognizeへ送ります。",
      "ADC利用時はgcloudからアクセストークンを取得します。GUIアプリのPATHに依存しないよう代表的なgcloud配置場所も探索します。",
      "modelは chirp_3、languageCodesは ja-JP を使います。",
      "辞書に登録した優先表記はadaptation phraseとして最大1,000件渡します。",
      "音声モードでライブ文字起こしを有効にした場合は、録音中のPCMをSpeech-to-Text V2 StreamingRecognizeへ送り、停止後は最終本文を一括出力します。",
      "取得した文字起こしをGeminiの整形モデルへ渡し、最終文へ整えます。",
    ],
    docs: [
      {
        label: "Chirp 3モデル",
        url: "https://cloud.google.com/speech-to-text/v2/docs/chirp-model",
      },
      {
        label: "Speech-to-Text料金",
        url: "https://cloud.google.com/speech-to-text/pricing",
      },
      {
        label: "ADCの仕組み",
        url: "https://cloud.google.com/docs/authentication/application-default-credentials",
      },
    ],
  },
  {
    value: "openAiGpt4oTranscribe",
    label: "OpenAI gpt-4o-transcribe",
    badge: "高精度",
    note: "セットアップが軽く、長めの録音にも扱いやすい高精度な汎用文字起こしです。",
    price: "低",
    priceNote: "目安 $0.006/分",
    speed: "中",
    accuracy: "高",
    accuracyNote: "汎用高精度",
    setupSummary: "OpenAI APIキーを取得し、EnjaのOpenAI APIキー欄へ保存します。",
    setupSteps: [
      "OpenAI PlatformでAPIキーを作成します。",
      "EnjaのOpenAI APIキー欄へ貼り付けて保存します。",
      "音声認識モデルでOpenAI gpt-4o-transcribeを選択します。",
      "必要に応じてOpenAI Platform側で使用量上限や請求アラートを設定します。",
    ],
    enjaDataFlow: [
      "Enjaは録音WAVをOpenAI /v1/audio/transcriptions へ送ります。",
      "modelは gpt-4o-transcribe、languageは ja を指定します。",
      "辞書に登録した優先表記はpromptとして渡します。",
      "音声モードでライブ文字起こしを有効にした場合は、録音中の先行文字起こしにOpenAI Realtimeのgpt-realtime-whisperを使い、停止後は最終本文を一括出力します。",
      "取得した文字起こしをGeminiの整形モデルへ渡し、最終文へ整えます。",
    ],
    docs: [
      {
        label: "Speech to text",
        url: "https://platform.openai.com/docs/guides/speech-to-text",
      },
      {
        label: "OpenAI料金",
        url: "https://platform.openai.com/docs/pricing/",
      },
      {
        label: "モデル詳細",
        url: "https://platform.openai.com/docs/models/gpt-4o-transcribe",
      },
    ],
  },
  {
    value: "openAiGpt4oMiniTranscribe",
    label: "OpenAI gpt-4o-mini-transcribe",
    badge: "最安寄り",
    note: "OpenAI構成の低コスト版。軽い日常入力ならコストを優先できます。",
    price: "最安",
    priceNote: "目安 $0.003/分",
    speed: "高",
    accuracy: "中",
    accuracyNote: "日常入力向き",
    setupSummary: "OpenAI APIキーを取得し、EnjaのOpenAI APIキー欄へ保存します。",
    setupSteps: [
      "OpenAI PlatformでAPIキーを作成します。",
      "EnjaのOpenAI APIキー欄へ貼り付けて保存します。",
      "音声認識モデルでOpenAI gpt-4o-mini-transcribeを選択します。",
      "精度が足りない場合はgpt-4o-transcribeへ切り替えます。",
    ],
    enjaDataFlow: [
      "Enjaは録音WAVをOpenAI /v1/audio/transcriptions へ送ります。",
      "modelは gpt-4o-mini-transcribe、languageは ja を指定します。",
      "辞書に登録した優先表記はpromptとして渡します。",
      "音声モードでライブ文字起こしを有効にした場合は、録音中の先行文字起こしにOpenAI Realtimeのgpt-realtime-whisperを使い、停止後は最終本文を一括出力します。",
      "取得した文字起こしをGeminiの整形モデルへ渡し、最終文へ整えます。",
    ],
    docs: [
      {
        label: "Speech to text",
        url: "https://platform.openai.com/docs/guides/speech-to-text",
      },
      {
        label: "OpenAI料金",
        url: "https://platform.openai.com/docs/pricing/",
      },
    ],
  },
  {
    value: "geminiAudio",
    label: "Gemini音声入力",
    badge: "簡易",
    note: "Gemini APIキーだけで試せる簡易構成。専用STTではなく音声理解として文字起こしします。",
    price: "変動",
    priceNote: "音声token課金",
    speed: "中",
    accuracy: "中",
    accuracyNote: "簡易構成",
    setupSummary: "Google AI StudioでGemini APIキーを取得し、EnjaのGemini APIキー欄へ保存します。",
    setupSteps: [
      "Google AI StudioでGemini APIキーを作成します。",
      "EnjaのGemini APIキー欄へ貼り付けて保存します。",
      "音声認識モデルでGemini音声入力を選択します。",
      "実際に使うGeminiモデルは、同じ画面の整形モデルで選択したモデルです。",
    ],
    enjaDataFlow: [
      "Enjaは録音WAVをGemini generateContentへ音声入力として送ります。",
      "辞書に登録した優先表記はプロンプト内に含めます。",
      "Geminiの音声理解結果を文字起こしとして受け取り、同じGeminiキーで最終整形します。",
      "Gemini公式ドキュメント上も、専用のリアルタイム文字起こし用途はGoogle Cloud Speech-to-Textが推奨されています。",
    ],
    docs: [
      {
        label: "Gemini音声入力",
        url: "https://ai.google.dev/gemini-api/docs/audio",
      },
      {
        label: "Gemini料金",
        url: "https://ai.google.dev/gemini-api/docs/pricing",
      },
      {
        label: "APIキー作成",
        url: "https://ai.google.dev/tutorials/setup",
      },
    ],
  },
  {
    value: "appleSpeechAnalyzer",
    label: "Apple SpeechAnalyzer",
    badge: "オンデバイス",
    note: "対応Mac上で日本語モデルを端末にインストールし、録音WAVをローカルで文字起こしします。",
    price: "無料",
    priceNote: "STTはオンデバイス",
    speed: "高",
    accuracy: "検証中",
    accuracyNote: "日本語/辞書ヒント",
    setupSummary:
      "macOS 26以降の対応端末で、日本語モデルと音声認識権限を設定します。",
    setupSteps: [
      "このセットアップ画面で状態確認を実行し、音声認識権限を許可します。",
      "日本語モデルが未インストールの場合は、モデルをインストールします。",
      "状態が利用可能になったら、音声認識モデルでApple SpeechAnalyzerを選択します。",
      "録音停止後、Enjaは録音WAVを端末内のApple SpeechAnalyzerへ渡します。",
    ],
    enjaDataFlow: [
      "Enjaは録音WAVを外部APIへ送らず、Swift helper経由でApple SpeechAnalyzerへ渡します。",
      "音声モードでライブ文字起こしを有効にした場合は、録音中のPCMをSwift helperへ流し、停止後は最終本文を一括出力します。",
      "辞書に登録した短い単語は最大100件までcontextualStringsとして渡します。",
      "整形が有効な音声モードでは、文字起こし結果をGeminiの整形モデルへ渡します。",
      "失敗時にGoogle/OpenAI/Geminiへ自動フォールバックしません。",
    ],
    docs: [
      {
        label: "SpeechAnalyzer",
        url: "https://developer.apple.com/documentation/speech/speechanalyzer",
      },
      {
        label: "DictationTranscriber",
        url: "https://developer.apple.com/documentation/speech/dictationtranscriber",
      },
      {
        label: "AssetInventory",
        url: "https://developer.apple.com/documentation/speech/assetinventory",
      },
    ],
  },
];

const FINALIZATION_MODELS: { value: FinalizationModel; label: string }[] = [
  { value: "gemini31FlashLite", label: "Gemini 3.1 Flash-Lite（最速）" },
  { value: "gemini35Flash", label: "Gemini 3.5 Flash（高速・標準）" },
  { value: "gemini31ProPreview", label: "Gemini 3.1 Pro Preview（精度優先）" },
];

type SettingsSection =
  | "voice"
  | "voiceModes"
  | "shortcuts"
  | "prompts"
  | "auth"
  | "usage"
  | "app";

const SETTINGS_SECTIONS: { id: SettingsSection; label: string }[] = [
  { id: "voice", label: "音声入力" },
  { id: "voiceModes", label: "音声モード" },
  { id: "shortcuts", label: "ショートカット" },
  { id: "prompts", label: "プロンプト" },
  { id: "auth", label: "API / 認証" },
  { id: "usage", label: "利用料金" },
  { id: "app", label: "アプリ" },
];

type PromptField = keyof PromptOverrides;

export function SettingsView() {
  const { setView, hydrateFromSettings } = useAppStore();
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [devices, setDevices] = useState<AudioInputDevice[]>([]);
  const [promptCatalog, setPromptCatalog] = useState<PromptCatalogItem[] | null>(
    null,
  );
  const [providerStatus, setProviderStatus] = useState<ProviderStatus | null>(null);
  const [appleSpeechStatus, setAppleSpeechStatus] =
    useState<AppleSpeechStatus | null>(null);
  const [usageEvents, setUsageEvents] = useState<ApiUsageEvent[]>([]);
  const [refreshingUsage, setRefreshingUsage] = useState(false);
  const [installingAppleSpeech, setInstallingAppleSpeech] = useState(false);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [activeSection, setActiveSection] = useState<SettingsSection>("voice");
  const [capturingAction, setCapturingAction] = useState<ShortcutAction | null>(null);
  const [setupProfile, setSetupProfile] = useState<SpeechProfile | null>(null);
  const [checkingSetup, setCheckingSetup] = useState(false);
  const [setupCheck, setSetupCheck] = useState<SpeechSetupCheck | null>(null);
  const [secrets, setSecrets] = useState<ProviderSecretsDraft>({
    gemini: "",
    openai: "",
    googleServiceAccount: "",
  });

  useEffect(() => {
    void (async () => {
      const [s, status, catalog, usage, appleStatus] = await Promise.all([
        getSettings(),
        getProviderStatus().catch(() => null),
        getPromptCatalog(),
        getApiUsageEvents().catch(() => []),
        getAppleSpeechStatus(false).catch(() => null),
      ]);
      setSettings(s);
      setProviderStatus(status);
      setPromptCatalog(catalog);
      setUsageEvents(usage);
      setAppleSpeechStatus(appleStatus);
    })();
  }, []);

  useEffect(() => {
    let cancelled = false;

    function applyDeviceList(nextDevices: AudioInputDevice[]) {
      if (cancelled) {
        return;
      }

      setDevices(nextDevices);
      setSettings((prev) => {
        if (!prev) {
          return prev;
        }

        const nextSelectedId = remapSelectedMicrophoneId(
          prev.voice.selectedMicrophoneId,
          nextDevices,
        );
        if (nextSelectedId === prev.voice.selectedMicrophoneId) {
          return prev;
        }

        return {
          ...prev,
          voice: {
            ...prev.voice,
            selectedMicrophoneId: nextSelectedId,
          },
        };
      });
    }

    async function refreshDevices() {
      const nextDevices = await listAudioInputDevices().catch(() => []);
      applyDeviceList(nextDevices);
    }

    const nativeListener = listen<AudioInputDevice[]>(
      "audio-input-devices-changed",
      (event) => {
        applyDeviceList(event.payload);
      },
    );
    const mediaDevices = navigator.mediaDevices;

    void refreshDevices();
    mediaDevices?.addEventListener?.("devicechange", refreshDevices);

    return () => {
      cancelled = true;
      void nativeListener.then((fn) => fn());
      mediaDevices?.removeEventListener?.("devicechange", refreshDevices);
    };
  }, []);

  useEffect(() => {
    const captured = listen<ShortcutCapturedEvent>("shortcut-captured", (event) => {
      const { action, shortcut } = event.payload;
      setCapturingAction((current) => (current === action ? null : current));
      setSettings((prev) => {
        if (!prev) return prev;
        return action === "voiceDictation"
          ? {
              ...prev,
              shortcuts: { ...prev.shortcuts, voiceDictation: shortcut },
            }
          : {
              ...prev,
              shortcuts: { ...prev.shortcuts, voiceAsk: shortcut },
            };
      });
    });
    const cancelled = listen<ShortcutCaptureCancelledEvent>(
      "shortcut-capture-cancelled",
      (event) => {
        setCapturingAction((current) =>
          current === event.payload.action ? null : current,
        );
        setMsg(event.payload.reason);
      },
    );
    return () => {
      void captured.then((fn) => fn());
      void cancelled.then((fn) => fn());
    };
  }, []);

  function patchVoice(patch: Partial<AppSettings["voice"]>) {
    setSettings((prev) =>
      prev ? { ...prev, voice: { ...prev.voice, ...patch } } : prev,
    );
  }

  function patchApp(patch: Partial<AppSettings["app"]>) {
    setSettings((prev) =>
      prev ? { ...prev, app: { ...prev.app, ...patch } } : prev,
    );
  }

  function patchPrompt(key: PromptField, value: string | null) {
    setSettings((prev) =>
      prev
        ? {
            ...prev,
            prompts: {
              overrides: {
                ...prev.prompts.overrides,
                [key]: value,
              },
            },
          }
        : prev,
    );
  }

  function setShortcut(action: ShortcutAction, shortcut: ShortcutBinding) {
    setSettings((prev) =>
      prev
        ? {
            ...prev,
            shortcuts:
              action === "voiceDictation"
                ? { ...prev.shortcuts, voiceDictation: shortcut }
                : { ...prev.shortcuts, voiceAsk: shortcut },
          }
        : prev,
    );
  }

  async function beginShortcutCapture(action: ShortcutAction) {
    setMsg(null);
    setCapturingAction(action);
    try {
      await startShortcutCapture(action);
    } catch (e) {
      setCapturingAction(null);
      setMsg(String(e));
    }
  }

  async function cancelCapture() {
    await cancelShortcutCapture().catch(() => undefined);
    setCapturingAction(null);
  }

  function openSetup(profile: SpeechProfile) {
    setSetupProfile(profile);
    setSetupCheck(null);
  }

  async function refreshAppleSpeechStatus(requestAuthorization: boolean) {
    const status = await getAppleSpeechStatus(requestAuthorization);
    setAppleSpeechStatus(status);
    return status;
  }

  async function handleSetupCheck(profile: SpeechProfile) {
    if (!settings) return;
    setCheckingSetup(true);
    setSetupCheck(null);
    try {
      if (profile === "appleSpeechAnalyzer") {
        const status = await refreshAppleSpeechStatus(true);
        setSetupCheck(appleSpeechStatusToCheck(status));
        return;
      }
      setSetupCheck(await checkSpeechSetup(profile, settings));
    } catch (e) {
      setSetupCheck({
        ok: false,
        message: String(e),
        details: [],
      });
    } finally {
      setCheckingSetup(false);
    }
  }

  async function handleAppleSpeechInstall() {
    setInstallingAppleSpeech(true);
    setSetupCheck(null);
    try {
      const status = await installAppleSpeechModel();
      setAppleSpeechStatus(status);
      setSetupCheck(appleSpeechStatusToCheck(status));
    } catch (e) {
      setSetupCheck({
        ok: false,
        message: String(e),
        details: [],
      });
    } finally {
      setInstallingAppleSpeech(false);
    }
  }

  async function handleSave() {
    if (!settings) return;
    setSaving(true);
    setMsg(null);
    try {
      await saveSettings(settings);
      await Promise.all(
        ([
          ["gemini", secrets.gemini],
          ["openai", secrets.openai],
          ["googleServiceAccount", secrets.googleServiceAccount],
        ] as const)
          .filter(([, value]) => value.trim())
          .map(([provider, value]) => saveProviderSecret(provider, value.trim())),
      );
      const [fresh, status] = await Promise.all([getSettings(), getProviderStatus()]);
      setSettings(fresh);
      setProviderStatus(status);
      hydrateFromSettings(fresh);
      setSecrets({
        gemini: "",
        openai: "",
        googleServiceAccount: "",
      });
      setMsg("保存しました。");
    } catch (e) {
      setMsg(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function refreshUsageEvents() {
    setRefreshingUsage(true);
    try {
      setUsageEvents(await getApiUsageEvents());
    } catch (e) {
      setMsg(String(e));
    } finally {
      setRefreshingUsage(false);
    }
  }

  if (!settings || !promptCatalog) {
    return <p className="text-sm text-neutral-500">設定を読み込んでいます…</p>;
  }

  const activeSetupProfile = setupProfile
    ? SPEECH_PROFILES.find((profile) => profile.value === setupProfile)
    : null;

  return (
    <div className="flex h-full min-h-0 w-full overflow-hidden bg-white">
      <aside className="flex w-54 shrink-0 flex-col bg-neutral-100/50 md:w-56">
        <div className="px-4 pb-4 pt-5">
          <h1 className="text-lg font-semibold tracking-tight text-neutral-900">設定</h1>
          <p className="mt-1.5 text-xs leading-relaxed text-neutral-500">
            音声入力、ショートカット、プロンプトを管理します。
          </p>
        </div>
        <nav className="flex flex-1 flex-col gap-0.5 px-2.5" aria-label="設定セクション">
          {SETTINGS_SECTIONS.map((section) => (
            <button
              key={section.id}
              type="button"
              onClick={() => setActiveSection(section.id)}
              className={`rounded-lg px-3 py-2 text-left text-sm font-medium transition ${
                activeSection === section.id
                  ? "bg-white text-blue-700"
                  : "text-neutral-600 hover:bg-white/60 hover:text-neutral-900"
              }`}
            >
              {section.label}
            </button>
          ))}
        </nav>
        <div className="mt-auto space-y-0.5 px-2.5 py-3">
          <button
            type="button"
            onClick={() => setView("dictionary")}
            className="w-full rounded-lg px-3 py-2 text-left text-sm text-neutral-600 transition hover:bg-white/80 hover:text-neutral-900"
          >
            辞書
          </button>
          <button
            type="button"
            onClick={() => setView("translation")}
            className="w-full rounded-lg px-3 py-2 text-left text-sm text-neutral-600 transition hover:bg-white/80 hover:text-neutral-900"
          >
            戻る
          </button>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <main className="min-h-0 flex-1 overflow-y-auto">
          <div className="mx-auto w-full max-w-3xl px-6 py-6 md:px-8 md:py-7">
          {activeSection === "voice" ? (
            <SettingsSectionPanel
              title="音声入力"
              description="録音、文字起こし、整形モデルの設定です。"
            >
              <label className="flex flex-col gap-1.5 text-sm sm:col-span-2">
                <span className="font-medium text-neutral-800">マイク</span>
                <select
                  value={settings.voice.selectedMicrophoneId ?? ""}
                  onChange={(e) =>
                    patchVoice({ selectedMicrophoneId: e.target.value || null })
                  }
                  className={settingsSelectClass}
                >
                  <option value="">システム既定</option>
                  {devices.map((device) => (
                    <option key={device.id} value={device.id}>
                      {device.name}
                      {device.isDefault ? "（既定）" : ""}
                    </option>
                  ))}
                </select>
              </label>

              <div className="flex flex-col gap-3 sm:col-span-2">
                <div>
                  <h3 className="text-sm font-medium text-neutral-800">
                    音声認識モデル
                  </h3>
                  <p className="mt-1 text-xs leading-relaxed text-neutral-500">
                    料金は公式ページの目安です。実際の請求は利用量、リージョン、追加機能、契約プランで変わります。
                  </p>
                </div>
                <div className="flex flex-col gap-2">
                  {SPEECH_PROFILES.map((profile) => {
                    const requirements = profileRequirements(
                      profile.value,
                      settings,
                      providerStatus,
                      secrets,
                      appleSpeechStatus,
                    );
                    const configured = requirements.every((item) => item.ok);
                    const disabled =
                      profile.value === "appleSpeechAnalyzer" && !configured;
                    return (
                      <SpeechProfileRow
                        key={profile.value}
                        profile={profile}
                        selected={settings.voice.speechProfile === profile.value}
                        requirements={requirements}
                        disabled={disabled}
                        onSelect={() => {
                          if (disabled) {
                            setMsg(
                              "Apple SpeechAnalyzerは、設定画面で日本語モデルと音声認識権限を準備してから選択できます。",
                            );
                            openSetup(profile.value);
                            return;
                          }
                          patchVoice({ speechProfile: profile.value });
                        }}
                        onSetup={() => openSetup(profile.value)}
                      />
                    );
                  })}
                </div>
              </div>

              <label className="flex flex-col gap-1.5 text-sm">
                <span className="font-medium text-neutral-800">整形モデル</span>
                <select
                  value={settings.voice.finalizationModel}
                  onChange={(e) =>
                    patchVoice({
                      finalizationModel: e.target.value as FinalizationModel,
                    })
                  }
                  className={settingsSelectClass}
                >
                  {FINALIZATION_MODELS.map((model) => (
                    <option key={model.value} value={model.value}>
                      {model.label}
                    </option>
                  ))}
                </select>
              </label>

              <label className="flex flex-col gap-1.5 text-sm">
                <span className="font-medium text-neutral-800">最大録音秒数</span>
                <input
                  type="number"
                  min={5}
                  max={600}
                  value={settings.voice.maxRecordingSeconds}
                  onChange={(e) =>
                    patchVoice({ maxRecordingSeconds: Number(e.target.value) })
                  }
                  className={settingsInputClass}
                />
              </label>

              <Toggle
                label="インタラクション音"
                checked={settings.voice.interactionSoundsEnabled}
                onChange={(checked) =>
                  patchVoice({ interactionSoundsEnabled: checked })
                }
              />
              <fieldset className="sm:col-span-2">
                <legend className="mb-2 text-sm font-medium text-slate-900">
                  PC内部音声の扱い
                </legend>
                <div className="grid gap-2">
                  <label className="flex items-start gap-3 rounded-xl border border-slate-200 bg-white p-3 text-sm">
                    <input
                      type="radio"
                      name="systemAudioHandling"
                      value="mute"
                      className="mt-1"
                      checked={settings.voice.systemAudioHandling === "mute"}
                      onChange={() =>
                        patchVoice({ systemAudioHandling: "mute" })
                      }
                    />
                    <span>
                      <span className="block font-medium">録音中はミュート</span>
                      <span className="block text-xs text-slate-600">
                        録音開始直前にMacの出力をミュートし、終了後に元へ戻します。シンプルですが、ユーザーにも音が聞こえなくなります。
                      </span>
                    </span>
                  </label>
                  <label className="flex items-start gap-3 rounded-xl border border-slate-200 bg-white p-3 text-sm">
                    <input
                      type="radio"
                      name="systemAudioHandling"
                      value="isolate"
                      className="mt-1"
                      checked={settings.voice.systemAudioHandling === "isolate"}
                      onChange={() =>
                        patchVoice({ systemAudioHandling: "isolate" })
                      }
                    />
                    <span>
                      <span className="block font-medium">
                        AECで分離（Typeless相当）
                      </span>
                      <span className="block text-xs text-slate-600">
                        Core Audio Process Tapでシステム音声のみを別経路で取得し、AECでマイクから差し引きます。再生音は止まらず、Netflix等のDRM動画も暗転しません。macOS 14.4以上が必要です。
                      </span>
                    </span>
                  </label>
                  <label className="flex items-start gap-3 rounded-xl border border-slate-200 bg-white p-3 text-sm">
                    <input
                      type="radio"
                      name="systemAudioHandling"
                      value="off"
                      className="mt-1"
                      checked={settings.voice.systemAudioHandling === "off"}
                      onChange={() =>
                        patchVoice({ systemAudioHandling: "off" })
                      }
                    />
                    <span>
                      <span className="block font-medium">何もしない</span>
                      <span className="block text-xs text-slate-600">
                        ヘッドホン利用などで漏れ込みが起きない環境向け。録音への介入はしません。
                      </span>
                    </span>
                  </label>
                </div>
              </fieldset>
            </SettingsSectionPanel>
          ) : null}

          {activeSection === "shortcuts" ? (
            <ShortcutSettingsSection
              shortcuts={settings.shortcuts}
              doubleTapThresholdMs={settings.app.doubleTapThresholdMs}
              capturingAction={capturingAction}
              onCapture={(action) => void beginShortcutCapture(action)}
              onCancel={() => void cancelCapture()}
              onReset={setShortcut}
              onDoubleTapChange={(value) =>
                patchApp({ doubleTapThresholdMs: value })
              }
            />
          ) : null}

          {activeSection === "voiceModes" ? (
            <VoiceModeSettingsSection voice={settings.voice} onChange={patchVoice} />
          ) : null}

          {activeSection === "prompts" ? (
            <PromptSettingsSection
              promptCatalog={promptCatalog}
              overrides={settings.prompts.overrides}
              onChange={patchPrompt}
            />
          ) : null}

          {activeSection === "auth" ? (
            <AuthSettingsSection
              voice={settings.voice}
              providerStatus={providerStatus}
              secrets={secrets}
              onSecretsChange={setSecrets}
              onVoiceChange={patchVoice}
              onOpenGeminiDocs={() => void openUrl("https://aistudio.google.com/apikey")}
            />
          ) : null}

          {activeSection === "usage" ? (
            <UsageCostsSection
              events={usageEvents}
              refreshing={refreshingUsage}
              onRefresh={() => void refreshUsageEvents()}
            />
          ) : null}

          {activeSection === "app" ? (
            <AppSettingsSection app={settings.app} onChange={patchApp} />
          ) : null}
          </div>
        </main>

        <div className="flex shrink-0 items-center gap-3 bg-neutral-100/40 px-6 py-3.5 md:px-8">
          <button
            type="button"
            className={settingsButtonPrimaryClass}
            disabled={saving}
            onClick={() => void handleSave()}
          >
            {saving ? "保存中…" : "保存"}
          </button>
          {msg ? (
            <p
              className={`text-sm ${msg.includes("保存しました") ? "text-emerald-600" : "text-neutral-500"}`}
            >
              {msg}
            </p>
          ) : null}
        </div>
      </div>

      {activeSetupProfile ? (
        <SetupDialog
          profile={activeSetupProfile}
          requirements={profileRequirements(
            activeSetupProfile.value,
            settings,
            providerStatus,
            secrets,
            appleSpeechStatus,
          )}
          onClose={() => setSetupProfile(null)}
          checking={checkingSetup}
          checkResult={setupCheck}
          onCheck={() => void handleSetupCheck(activeSetupProfile.value)}
          installingAppleSpeech={installingAppleSpeech}
          onInstallAppleSpeech={
            activeSetupProfile.value === "appleSpeechAnalyzer"
              ? () => void handleAppleSpeechInstall()
              : undefined
          }
        />
      ) : null}
    </div>
  );
}

function profileRequirements(
  profile: SpeechProfile,
  settings: AppSettings,
  providerStatus: ProviderStatus | null,
  secrets: {
    gemini: string;
    openai: string;
    googleServiceAccount: string;
  },
  appleSpeechStatus: AppleSpeechStatus | null,
) {
  switch (profile) {
    case "googleChirp3":
      return [
        {
          label: "Project ID",
          ok: Boolean(settings.voice.googleCloudProjectId.trim()),
          value: settings.voice.googleCloudProjectId.trim() || "未入力",
        },
        {
          label: "リージョン",
          ok: Boolean(settings.voice.googleCloudRegion.trim()),
          value: settings.voice.googleCloudRegion.trim() || "未入力",
        },
        settings.voice.googleCloudUseAdc
          ? {
              label: "認証",
              ok: true,
              value: "ADCを使用",
            }
          : {
              label: "認証",
              ok:
                Boolean(secrets.googleServiceAccount.trim()) ||
                Boolean(providerStatus?.googleServiceAccount),
              value:
                secrets.googleServiceAccount.trim() || providerStatus?.googleServiceAccount
                  ? "サービスアカウントJSON"
                  : "未保存",
            },
      ];
    case "openAiGpt4oTranscribe":
    case "openAiGpt4oMiniTranscribe":
      return [
        {
          label: "OpenAI APIキー",
          ok: Boolean(secrets.openai.trim()) || Boolean(providerStatus?.openai),
          value:
            secrets.openai.trim() || providerStatus?.openai ? "保存予定/保存済み" : "未保存",
        },
      ];
    case "geminiAudio":
      return [
        {
          label: "Gemini APIキー",
          ok:
            Boolean(secrets.gemini.trim()) ||
            Boolean(providerStatus?.gemini),
          value:
            secrets.gemini.trim() || providerStatus?.gemini
              ? "保存予定/保存済み"
              : "未保存",
        },
      ];
    case "appleSpeechAnalyzer":
      return [
        {
          label: "対応",
          ok: Boolean(appleSpeechStatus?.helperAvailable && appleSpeechStatus.supported),
          value: appleSpeechStatus
            ? appleSpeechStatus.supported
              ? "対応"
              : "未対応"
            : "未確認",
        },
        {
          label: "日本語モデル",
          ok: appleSpeechStatus?.status === "installed",
          value: appleSpeechStatus
            ? appleSpeechModelStatusLabel(appleSpeechStatus.status)
            : "未確認",
        },
        {
          label: "音声認識権限",
          ok: appleSpeechStatus?.authorization === "authorized",
          value: appleSpeechAuthorizationLabel(
            appleSpeechStatus?.authorization ?? "unknown",
          ),
        },
      ];
  }
}

function appleSpeechStatusToCheck(status: AppleSpeechStatus): SpeechSetupCheck {
  const ok =
    status.helperAvailable &&
    status.supported &&
    status.status === "installed" &&
    status.authorization === "authorized";
  return {
    ok,
    message: status.message,
    details: [
      `モデル状態: ${appleSpeechModelStatusLabel(status.status)}`,
      `音声認識権限: ${appleSpeechAuthorizationLabel(status.authorization)}`,
      ...status.details,
    ],
  };
}

function appleSpeechModelStatusLabel(status: AppleSpeechStatus["status"]) {
  switch (status) {
    case "installed":
      return "利用可能";
    case "supported":
      return "未インストール";
    case "downloading":
      return "インストール中";
    case "unsupported":
      return "未対応";
    case "unknown":
      return "不明";
  }
}

function appleSpeechAuthorizationLabel(
  authorization: AppleSpeechStatus["authorization"],
) {
  switch (authorization) {
    case "authorized":
      return "許可済み";
    case "notDetermined":
      return "未確認";
    case "denied":
      return "拒否";
    case "restricted":
      return "制限";
    case "unknown":
      return "不明";
  }
}

function SpeechProfileRow({
  profile,
  selected,
  requirements,
  disabled,
  onSelect,
  onSetup,
}: {
  profile: SpeechProfileOption;
  selected: boolean;
  requirements: ReturnType<typeof profileRequirements>;
  disabled?: boolean;
  onSelect: () => void;
  onSetup: () => void;
}) {
  const configured = requirements.every((item) => item.ok);

  return (
    <div
      role="button"
      aria-pressed={selected}
      aria-disabled={disabled}
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.currentTarget !== event.target) return;
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
      className={`grid w-full gap-4 rounded-xl px-4 py-4 text-left transition lg:grid-cols-[minmax(0,1fr)_auto] ${
        selected ? "bg-blue-50" : "bg-neutral-100/70 hover:bg-neutral-100"
      } ${disabled ? "cursor-not-allowed opacity-75" : ""}`}
    >
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <span
            className={`size-3 shrink-0 rounded-full ${
              selected ? "bg-blue-600" : "bg-neutral-300"
            }`}
          />
          <span className="font-medium text-neutral-900">{profile.label}</span>
          <span className="rounded-full bg-neutral-100 px-2 py-0.5 text-[11px] text-neutral-600">
            {profile.badge}
          </span>
          <span
            className={`rounded-full px-2 py-0.5 text-[11px] ${
              configured
                ? "bg-emerald-50 text-emerald-700"
                : "bg-amber-50 text-amber-700"
            }`}
          >
            {configured ? "設定済み" : "要セットアップ"}
          </span>
        </div>
        <p className="mt-1 text-xs leading-relaxed text-neutral-500">{profile.note}</p>
        <div className="mt-2 flex flex-wrap gap-1.5">
          {requirements.map((item) => (
            <span
              key={item.label}
              className={`rounded-full px-2 py-0.5 text-[11px] ${
                item.ok ? "bg-neutral-100 text-neutral-600" : "bg-red-50 text-red-700"
              }`}
            >
              {item.label}: {item.value}
            </span>
          ))}
        </div>
      </div>

      <div className="flex flex-col gap-2 md:items-end">
        <div className="grid grid-cols-3 gap-1.5">
          <ModelMetric label="価格" value={profile.price} note={profile.priceNote} />
          <ModelMetric label="スピード" value={profile.speed} />
          <ModelMetric label="精度" value={profile.accuracy} note={profile.accuracyNote} />
        </div>
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            onSetup();
          }}
          className="w-fit rounded-lg bg-white/80 px-2.5 py-1 text-xs font-medium text-blue-600 transition hover:bg-blue-100/80"
        >
          セットアップ方法
        </button>
      </div>
    </div>
  );
}

function ModelMetric({
  label,
  value,
  note,
}: {
  label: string;
  value: string;
  note?: string;
}) {
  return (
    <span className="min-w-[72px] rounded-lg bg-neutral-100/80 px-2 py-1.5 text-center">
      <span className="block text-[10px] text-neutral-400">{label}</span>
      <span className="block text-xs font-semibold text-neutral-800">{value}</span>
      {note ? <span className="block truncate text-[10px] text-neutral-400">{note}</span> : null}
    </span>
  );
}

function SetupDialog({
  profile,
  requirements,
  onClose,
  checking,
  checkResult,
  onCheck,
  installingAppleSpeech,
  onInstallAppleSpeech,
}: {
  profile: SpeechProfileOption;
  requirements: ReturnType<typeof profileRequirements>;
  onClose: () => void;
  checking: boolean;
  checkResult: SpeechSetupCheck | null;
  onCheck: () => void;
  installingAppleSpeech: boolean;
  onInstallAppleSpeech?: () => void;
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-4 backdrop-blur-[2px]">
      <div className="flex max-h-[82vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl bg-white shadow-2xl">
        <div className="flex items-start justify-between gap-4 bg-neutral-50 px-5 py-4">
          <div>
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-lg font-semibold text-neutral-900">{profile.label}</h2>
              <span className="rounded-full bg-neutral-100 px-2 py-0.5 text-xs text-neutral-600">
                {profile.badge}
              </span>
            </div>
            <p className="mt-1 text-sm leading-relaxed text-neutral-500">
              {profile.setupSummary}
            </p>
          </div>
          <div className="flex shrink-0 gap-2">
            <button
              type="button"
              onClick={onCheck}
              disabled={checking}
              className="rounded-lg bg-blue-100 px-2.5 py-1 text-xs font-medium text-blue-700 transition hover:bg-blue-200/70 disabled:opacity-50"
            >
              {checking ? "確認中…" : profile.value === "appleSpeechAnalyzer" ? "状態確認" : "疎通確認"}
            </button>
            {onInstallAppleSpeech ? (
              <button
                type="button"
                onClick={onInstallAppleSpeech}
                disabled={installingAppleSpeech}
                className="rounded-lg bg-emerald-100 px-2.5 py-1 text-xs font-medium text-emerald-700 transition hover:bg-emerald-200/70 disabled:opacity-50"
              >
                {installingAppleSpeech ? "インストール中…" : "モデルをインストール"}
              </button>
            ) : null}
            <button
              type="button"
              onClick={onClose}
              className={settingsButtonSecondaryClass}
            >
              閉じる
            </button>
          </div>
        </div>

        <div className="min-h-0 overflow-y-auto px-5 py-4">
          {checkResult ? (
            <div
              className={`mb-4 rounded-xl p-3 text-sm leading-relaxed ${
                checkResult.ok
                  ? "bg-emerald-50 text-emerald-900"
                  : "bg-red-50 text-red-900"
              }`}
            >
              <p className="font-medium">{checkResult.message}</p>
              {checkResult.details.length ? (
                <ul className="mt-2 list-disc space-y-1 pl-5 text-xs">
                  {checkResult.details.map((detail) => (
                    <li key={detail}>{detail}</li>
                  ))}
                </ul>
              ) : null}
            </div>
          ) : null}

          <div className="grid gap-3 md:grid-cols-3">
            <ModelMetric label="価格" value={profile.price} note={profile.priceNote} />
            <ModelMetric label="スピード" value={profile.speed} />
            <ModelMetric label="精度" value={profile.accuracy} note={profile.accuracyNote} />
          </div>

          <section className="mt-5">
            <h3 className="text-sm font-semibold text-neutral-900">必要な設定</h3>
            <div className="mt-2 flex flex-wrap gap-2">
              {requirements.map((item) => (
                <span
                  key={item.label}
                  className={`rounded-full px-2.5 py-1 text-xs ${
                    item.ok ? "bg-emerald-50 text-emerald-700" : "bg-red-50 text-red-700"
                  }`}
                >
                  {item.label}: {item.value}
                </span>
              ))}
            </div>
          </section>

          <section className="mt-5">
            <h3 className="text-sm font-semibold text-neutral-900">セットアップ手順</h3>
            <ol className="mt-2 list-decimal space-y-2 pl-5 text-sm leading-relaxed text-neutral-600">
              {profile.setupSteps.map((step) => (
                <li key={step}>{step}</li>
              ))}
            </ol>
          </section>

          <section className="mt-5">
            <h3 className="text-sm font-semibold text-neutral-900">
              Enjaが設定画面から利用する情報
            </h3>
            <ul className="mt-2 list-disc space-y-2 pl-5 text-sm leading-relaxed text-neutral-600">
              {profile.enjaDataFlow.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
          </section>

          <section className="mt-5">
            <h3 className="text-sm font-semibold text-neutral-900">参照ページ</h3>
            <div className="mt-2 flex flex-wrap gap-2">
              {profile.docs.map((doc) => (
                <button
                  key={doc.url}
                  type="button"
                  onClick={() => void openUrl(doc.url)}
                  className="rounded-lg bg-neutral-100 px-3 py-1.5 text-xs font-medium text-blue-600 transition hover:bg-blue-100/80"
                >
                  {doc.label}
                </button>
              ))}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
