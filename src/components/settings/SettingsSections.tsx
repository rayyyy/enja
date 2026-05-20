import type {
  AppSettings,
  PromptCatalogItem,
  PromptOverrides,
  ProviderStatus,
  ShortcutAction,
  ShortcutBinding,
} from "../../types";
import {
  PromptEditor,
  SecretField,
  SettingsFieldGroup,
  SettingsSectionPanel,
  ShortcutRow,
  Toggle,
  settingsInputClass,
} from "./SettingsControls";

export type ProviderSecretsDraft = {
  gemini: string;
  openai: string;
  googleServiceAccount: string;
};

export function ShortcutSettingsSection({
  shortcuts,
  doubleTapThresholdMs,
  capturingAction,
  onCapture,
  onCancel,
  onReset,
  onDoubleTapChange,
}: {
  shortcuts: AppSettings["shortcuts"];
  doubleTapThresholdMs: number;
  capturingAction: ShortcutAction | null;
  onCapture: (action: ShortcutAction) => void;
  onCancel: () => void;
  onReset: (action: ShortcutAction, shortcut: ShortcutBinding) => void;
  onDoubleTapChange: (value: number) => void;
}) {
  return (
    <SettingsSectionPanel
      title="ショートカット"
      description="キー記録中は次に押したキー操作が登録されます。Escapeは録音キャンセル用に予約されています。"
    >
      <SettingsFieldGroup className="sm:col-span-2">
        <ShortcutRow
          label="音声入力開始/停止"
          shortcut={shortcuts.voiceDictation}
          capturing={capturingAction === "voiceDictation"}
          onCapture={() => onCapture("voiceDictation")}
          onCancel={onCancel}
          onReset={() => onReset("voiceDictation", DEFAULT_SHORTCUTS.voiceDictation)}
        />
        <ShortcutRow
          label="選択テキストへの音声指示"
          shortcut={shortcuts.voiceAsk}
          capturing={capturingAction === "voiceAsk"}
          onCapture={() => onCapture("voiceAsk")}
          onCancel={onCancel}
          onReset={() => onReset("voiceAsk", DEFAULT_SHORTCUTS.voiceAsk)}
        />
      </SettingsFieldGroup>

      <label className="flex flex-col gap-1.5 text-sm sm:col-span-2 sm:max-w-xs">
        <span className="font-medium text-neutral-800">Cmd+C連打の判定時間（ミリ秒）</span>
        <input
          type="number"
          min={100}
          max={2000}
          value={doubleTapThresholdMs}
          onChange={(e) => onDoubleTapChange(Number(e.target.value))}
          className={settingsInputClass}
        />
      </label>
    </SettingsSectionPanel>
  );
}

export function PromptSettingsSection({
  promptCatalog,
  overrides,
  onChange,
}: {
  promptCatalog: PromptCatalogItem[];
  overrides: PromptOverrides;
  onChange: (key: keyof PromptOverrides, value: string | null) => void;
}) {
  return (
    <SettingsSectionPanel
      title="プロンプト"
      description="通常は既定テンプレートを使い、必要な項目だけカスタムを有効にしてください。"
    >
      <div className="grid gap-4 sm:col-span-2">
        {promptCatalog.map((field) => (
          <PromptEditor
            key={field.key}
            field={field}
            defaultValue={field.defaultText}
            customValue={overrides[field.key]}
            onChange={(value) => onChange(field.key, value)}
          />
        ))}
      </div>
    </SettingsSectionPanel>
  );
}

export function AuthSettingsSection({
  voice,
  providerStatus,
  secrets,
  onSecretsChange,
  onVoiceChange,
  onOpenGeminiDocs,
}: {
  voice: AppSettings["voice"];
  providerStatus: ProviderStatus | null;
  secrets: ProviderSecretsDraft;
  onSecretsChange: (next: ProviderSecretsDraft) => void;
  onVoiceChange: (patch: Partial<AppSettings["voice"]>) => void;
  onOpenGeminiDocs: () => void;
}) {
  return (
    <SettingsSectionPanel
      title="API / 認証"
      description="音声認識と整形に使う各プロバイダの認証情報です。"
    >
      <SecretField
        label="Gemini APIキー"
        placeholder={providerStatus?.gemini ? "保存済み" : "AIza..."}
        value={secrets.gemini}
        onChange={(value) => onSecretsChange({ ...secrets, gemini: value })}
        helpAction={onOpenGeminiDocs}
        helpLabel="取得"
      />
      <SecretField
        label="OpenAI APIキー"
        placeholder={providerStatus?.openai ? "保存済み" : "sk-..."}
        value={secrets.openai}
        onChange={(value) => onSecretsChange({ ...secrets, openai: value })}
      />
      <label className="flex flex-col gap-1.5 text-sm">
        <span className="font-medium text-neutral-800">Google Cloud Project ID</span>
        <input
          value={voice.googleCloudProjectId}
          onChange={(e) => onVoiceChange({ googleCloudProjectId: e.target.value })}
          placeholder="my-gcp-project"
          className={settingsInputClass}
        />
      </label>
      <label className="flex flex-col gap-1.5 text-sm">
        <span className="font-medium text-neutral-800">Google Cloudリージョン</span>
        <input
          value={voice.googleCloudRegion}
          onChange={(e) => onVoiceChange({ googleCloudRegion: e.target.value })}
          placeholder="asia-northeast1"
          className={settingsInputClass}
        />
      </label>
      <Toggle
        className="sm:col-span-2"
        label="Google Cloud ADCを使用"
        checked={voice.googleCloudUseAdc}
        onChange={(checked) => onVoiceChange({ googleCloudUseAdc: checked })}
      />
      {!voice.googleCloudUseAdc ? (
        <label className="flex flex-col gap-1.5 text-sm sm:col-span-2">
          <span className="font-medium text-neutral-800">
            サービスアカウントJSON
            {providerStatus?.googleServiceAccount ? "（保存済み）" : ""}
          </span>
          <textarea
            value={secrets.googleServiceAccount}
            onChange={(e) =>
              onSecretsChange({
                ...secrets,
                googleServiceAccount: e.target.value,
              })
            }
            rows={4}
            placeholder="{...}"
            className={`${settingsInputClass} resize-none font-mono text-xs`}
          />
        </label>
      ) : null}
    </SettingsSectionPanel>
  );
}

export function AppSettingsSection({
  app,
  onChange,
}: {
  app: AppSettings["app"];
  onChange: (patch: Partial<AppSettings["app"]>) => void;
}) {
  return (
    <SettingsSectionPanel title="アプリ" description="起動と権限まわりの設定です。">
      <Toggle
        className="sm:col-span-2"
        label="Macログイン時に自動起動"
        checked={app.launchAtLogin}
        onChange={(checked) => onChange({ launchAtLogin: checked })}
      />
      <p className="rounded-xl bg-neutral-100/60 px-4 py-3 text-xs leading-relaxed text-neutral-500 sm:col-span-2">
        macOSではアクセシビリティ、入力監視、マイクの許可が必要です。ショートカットが取得できない場合は、プライバシーとセキュリティの許可を確認してください。
      </p>
    </SettingsSectionPanel>
  );
}

const DEFAULT_SHORTCUTS: Record<ShortcutAction, ShortcutBinding> = {
  voiceDictation: {
    keyCode: null,
    key: "fn",
    label: "Fn",
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
    modifiers: {
      command: false,
      option: false,
      control: false,
      shift: false,
      function: true,
    },
  },
};
