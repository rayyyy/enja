import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useState } from "react";
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
import {
  FINALIZATION_MODELS,
  SPEECH_PROFILES,
  SetupDialog,
  SpeechProfileRow,
  appleSpeechStatusToCheck,
  profileRequirements,
  type ProfileRequirement,
} from "./settings/speechProfiles";
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

function withShortcut(
  shortcuts: AppSettings["shortcuts"],
  action: ShortcutAction,
  shortcut: ShortcutBinding,
): AppSettings["shortcuts"] {
  switch (action) {
    case "voiceDictation":
      return { ...shortcuts, voiceDictation: shortcut };
    case "voiceAsk":
      return { ...shortcuts, voiceAsk: shortcut };
    case "polishSelection":
      return { ...shortcuts, polishSelection: shortcut };
  }
}

export function SettingsView() {
  const hydrateFromSettings = useAppStore((s) => s.hydrateFromSettings);
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
        return {
          ...prev,
          shortcuts: withShortcut(prev.shortcuts, action, shortcut),
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

  const patchVoice = useCallback((patch: Partial<AppSettings["voice"]>) => {
    setSettings((prev) =>
      prev ? { ...prev, voice: { ...prev.voice, ...patch } } : prev,
    );
  }, []);

  const patchApp = useCallback((patch: Partial<AppSettings["app"]>) => {
    setSettings((prev) =>
      prev ? { ...prev, app: { ...prev.app, ...patch } } : prev,
    );
  }, []);

  const patchPrompt = useCallback(
    (key: keyof PromptOverrides, value: string | null) => {
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
    },
    [],
  );

  const setShortcut = useCallback(
    (action: ShortcutAction, shortcut: ShortcutBinding) => {
      setSettings((prev) =>
        prev
          ? {
              ...prev,
              shortcuts: withShortcut(prev.shortcuts, action, shortcut),
            }
          : prev,
      );
    },
    [],
  );

  const beginShortcutCapture = useCallback((action: ShortcutAction) => {
    setMsg(null);
    setCapturingAction(action);
    void startShortcutCapture(action).catch((e) => {
      setCapturingAction(null);
      setMsg(String(e));
    });
  }, []);

  const cancelCapture = useCallback(() => {
    void cancelShortcutCapture()
      .catch(() => undefined)
      .finally(() => setCapturingAction(null));
  }, []);

  const handleDoubleTapChange = useCallback(
    (value: number) => patchApp({ doubleTapThresholdMs: value }),
    [patchApp],
  );

  const openGeminiDocs = useCallback(
    () => void openUrl("https://aistudio.google.com/apikey"),
    [],
  );

  const openSetup = useCallback((profile: SpeechProfile) => {
    setSetupProfile(profile);
    setSetupCheck(null);
  }, []);

  // SpeechProfileRow のメモ化を効かせるため、依存する設定値が変わったときだけ
  // 要件一覧を再計算する。
  const requirementsByProfile = useMemo(() => {
    if (!settings) return null;
    const map = {} as Record<SpeechProfile, ProfileRequirement[]>;
    for (const profile of SPEECH_PROFILES) {
      map[profile.value] = profileRequirements(
        profile.value,
        settings,
        providerStatus,
        secrets,
        appleSpeechStatus,
      );
    }
    return map;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    settings?.voice.googleCloudProjectId,
    settings?.voice.googleCloudRegion,
    settings?.voice.googleCloudUseAdc,
    providerStatus,
    secrets,
    appleSpeechStatus,
  ]);

  const handleSelectSpeechProfile = useCallback(
    (value: SpeechProfile) => {
      const requirements = requirementsByProfile?.[value] ?? [];
      const configured = requirements.every((item) => item.ok);
      if (value === "appleSpeechAnalyzer" && !configured) {
        setMsg(
          "Apple SpeechAnalyzerは、設定画面で日本語モデルと音声認識権限を準備してから選択できます。",
        );
        openSetup(value);
        return;
      }
      patchVoice({ speechProfile: value });
    },
    [openSetup, patchVoice, requirementsByProfile],
  );

  const refreshAppleSpeechStatus = useCallback(
    async (requestAuthorization: boolean) => {
      const status = await getAppleSpeechStatus(requestAuthorization);
      setAppleSpeechStatus(status);
      return status;
    },
    [],
  );

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

  const refreshUsageEvents = useCallback(() => {
    setRefreshingUsage(true);
    getApiUsageEvents()
      .then(setUsageEvents)
      .catch((e) => setMsg(String(e)))
      .finally(() => setRefreshingUsage(false));
  }, []);

  if (!settings || !promptCatalog || !requirementsByProfile) {
    return <p className="p-6 text-sm text-ink-mid">設定を読み込んでいます…</p>;
  }

  const activeSetupProfile = setupProfile
    ? SPEECH_PROFILES.find((profile) => profile.value === setupProfile)
    : null;

  return (
    <div className="flex h-full min-h-0 w-full overflow-hidden bg-surface">
      <aside className="flex w-54 shrink-0 flex-col border-r border-edge bg-canvas md:w-56">
        <div className="px-4 pb-4 pt-5">
          <h1 className="text-lg font-semibold tracking-tight text-ink">設定</h1>
          <p className="mt-1.5 text-xs leading-relaxed text-ink-mid">
            音声入力、ショートカット、プロンプトを管理します。
          </p>
        </div>
        <nav className="flex flex-1 flex-col gap-0.5 px-2.5" aria-label="設定セクション">
          {SETTINGS_SECTIONS.map((section) => (
            <button
              key={section.id}
              type="button"
              onClick={() => setActiveSection(section.id)}
              className={`rounded-md px-3 py-1.5 text-left text-[13px] font-medium transition-colors duration-100 focus-ring ${
                activeSection === section.id
                  ? "bg-accent-soft text-accent-ink"
                  : "text-ink-mid hover:bg-hover hover:text-ink"
              }`}
            >
              {section.label}
            </button>
          ))}
        </nav>
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
                <span className="font-medium text-ink">マイク</span>
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
                  <h3 className="text-sm font-medium text-ink">
                    音声認識モデル
                  </h3>
                  <p className="mt-1 text-xs leading-relaxed text-ink-mid">
                    料金は公式ページの目安です。実際の請求は利用量、リージョン、追加機能、契約プランで変わります。
                  </p>
                </div>
                <div className="flex flex-col gap-2">
                  {SPEECH_PROFILES.map((profile) => {
                    const requirements = requirementsByProfile[profile.value];
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
                        onSelect={handleSelectSpeechProfile}
                        onSetup={openSetup}
                      />
                    );
                  })}
                </div>
              </div>

              <label className="flex flex-col gap-1.5 text-sm">
                <span className="font-medium text-ink">整形モデル</span>
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
                <span className="font-medium text-ink">最大録音秒数</span>
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
              <Toggle
                label="画面文脈を使う"
                description="入力先のアプリ、ウィンドウ、カーソル前後、表示テキストを音声認識と整形のヒントに使います。"
                checked={settings.voice.screenContextEnabled}
                onChange={(checked) =>
                  patchVoice({ screenContextEnabled: checked })
                }
              />
              <Toggle
                label="画面OCRを使う"
                description="アクセシビリティで読めない表示文字を、音声認識や整形に効く経路だけ対象ディスプレイ上の前面から最大3ウィンドウのOCRで補います。"
                checked={
                  settings.voice.screenContextEnabled &&
                  settings.voice.screenContextOcrEnabled
                }
                disabled={!settings.voice.screenContextEnabled}
                onChange={(checked) =>
                  patchVoice({ screenContextOcrEnabled: checked })
                }
              />
              <fieldset className="sm:col-span-2">
                <legend className="mb-2 text-sm font-medium text-ink">
                  PC内部音声の扱い
                </legend>
                <div className="grid gap-2">
                  <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-edge bg-sunken p-3 text-sm transition-colors duration-100 hover:bg-hover has-checked:border-accent/40 has-checked:bg-accent-soft">
                    <input
                      type="radio"
                      name="systemAudioHandling"
                      value="mute"
                      className="mt-1 accent-[var(--accent)]"
                      checked={settings.voice.systemAudioHandling === "mute"}
                      onChange={() =>
                        patchVoice({ systemAudioHandling: "mute" })
                      }
                    />
                    <span>
                      <span className="block font-medium text-ink">録音中はミュート</span>
                      <span className="block text-xs leading-relaxed text-ink-mid">
                        録音開始直前にMacの出力をミュートし、終了後に元へ戻します。シンプルですが、ユーザーにも音が聞こえなくなります。
                      </span>
                    </span>
                  </label>
                  <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-edge bg-sunken p-3 text-sm transition-colors duration-100 hover:bg-hover has-checked:border-accent/40 has-checked:bg-accent-soft">
                    <input
                      type="radio"
                      name="systemAudioHandling"
                      value="isolate"
                      className="mt-1 accent-[var(--accent)]"
                      checked={settings.voice.systemAudioHandling === "isolate"}
                      onChange={() =>
                        patchVoice({ systemAudioHandling: "isolate" })
                      }
                    />
                    <span>
                      <span className="block font-medium text-ink">
                        AECで分離（Typeless相当）
                      </span>
                      <span className="block text-xs leading-relaxed text-ink-mid">
                        Core Audio Process Tapでシステム音声のみを別経路で取得し、AECでマイクから差し引きます。再生音は止めずに録音への混入を抑えます。macOS 14.4以上が必要です。
                      </span>
                    </span>
                  </label>
                  <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-edge bg-sunken p-3 text-sm transition-colors duration-100 hover:bg-hover has-checked:border-accent/40 has-checked:bg-accent-soft">
                    <input
                      type="radio"
                      name="systemAudioHandling"
                      value="off"
                      className="mt-1 accent-[var(--accent)]"
                      checked={settings.voice.systemAudioHandling === "off"}
                      onChange={() =>
                        patchVoice({ systemAudioHandling: "off" })
                      }
                    />
                    <span>
                      <span className="block font-medium text-ink">何もしない</span>
                      <span className="block text-xs leading-relaxed text-ink-mid">
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
              onCapture={beginShortcutCapture}
              onCancel={cancelCapture}
              onReset={setShortcut}
              onDoubleTapChange={handleDoubleTapChange}
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
              onOpenGeminiDocs={openGeminiDocs}
            />
          ) : null}

          {activeSection === "usage" ? (
            <UsageCostsSection
              events={usageEvents}
              refreshing={refreshingUsage}
              onRefresh={refreshUsageEvents}
            />
          ) : null}

          {activeSection === "app" ? (
            <AppSettingsSection app={settings.app} onChange={patchApp} />
          ) : null}
          </div>
        </main>

        <div className="flex shrink-0 items-center gap-3 border-t border-edge bg-canvas px-6 py-3 md:px-8">
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
              className={`text-sm ${msg.includes("保存しました") ? "text-ok" : "text-ink-mid"}`}
            >
              {msg}
            </p>
          ) : null}
        </div>
      </div>

      {activeSetupProfile ? (
        <SetupDialog
          profile={activeSetupProfile}
          requirements={requirementsByProfile[activeSetupProfile.value]}
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
