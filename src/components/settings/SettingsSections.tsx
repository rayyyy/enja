import { useState } from "react";
import { Monitor, Moon, Sun } from "lucide-react";
import {
  setThemePreference,
  useThemePreference,
  type ThemePreference,
} from "../../lib/theme";
import type {
  AppSettings,
  PromptCatalogItem,
  PromptOverrides,
  ProviderStatus,
  ShortcutAction,
  ShortcutBinding,
  VoiceModePresetKey,
  VoiceModeProfile,
} from "../../types";
import {
  PromptEditor,
  SecretField,
  SettingsFieldGroup,
  SettingsSectionPanel,
  ShortcutRow,
  Toggle,
  settingsButtonPrimaryClass,
  settingsButtonSecondaryClass,
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
      description="キー記録中は次に押したキー操作が登録されます。録音停止は常にFn、Escapeは録音キャンセル用に予約されています。"
    >
      <SettingsFieldGroup className="sm:col-span-2">
        <ShortcutRow
          label="音声入力開始"
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
        <ShortcutRow
          label="選択テキストを推敲"
          shortcut={shortcuts.polishSelection}
          capturing={capturingAction === "polishSelection"}
          onCapture={() => onCapture("polishSelection")}
          onCancel={onCancel}
          onReset={() =>
            onReset("polishSelection", DEFAULT_SHORTCUTS.polishSelection)
          }
        />
      </SettingsFieldGroup>

      <label className="flex flex-col gap-1.5 text-sm sm:col-span-2 sm:max-w-xs">
        <span className="font-medium text-ink">Cmd+C連打の判定時間（ミリ秒）</span>
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

type VoiceModeEditorState = {
  kind: "create" | "edit";
  originalId: string | null;
  profile: VoiceModeProfile;
  confirmDelete: boolean;
  error: string | null;
};

export function VoiceModeSettingsSection({
  voice,
  onChange,
}: {
  voice: AppSettings["voice"];
  onChange: (patch: Partial<AppSettings["voice"]>) => void;
}) {
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dragOverId, setDragOverId] = useState<string | null>(null);
  const [editor, setEditor] = useState<VoiceModeEditorState | null>(null);
  const profiles = orderedVoiceModeProfiles(voice.modeProfiles);
  const activeId =
    profiles.some((profile) => profile.id === voice.activeModeProfileId)
      ? voice.activeModeProfileId
      : "default";
  const liveTranscriptionAvailable = supportsLiveTranscription(
    voice.speechProfile,
  );

  function commitProfiles(nextProfiles: VoiceModeProfile[], nextActiveId = activeId) {
    const normalized = withVoiceModeOrder(nextProfiles);
    const resolvedActiveId = normalized.some((profile) => profile.id === nextActiveId)
      ? nextActiveId
      : "default";
    onChange({
      modeProfiles: normalized,
      activeModeProfileId: resolvedActiveId,
    });
  }

  function dropProfile(targetId: string) {
    if (!draggingId || draggingId === targetId) {
      setDraggingId(null);
      setDragOverId(null);
      return;
    }
    const dragged = profiles.find((profile) => profile.id === draggingId);
    if (!dragged) {
      setDraggingId(null);
      setDragOverId(null);
      return;
    }
    const next = profiles.filter((profile) => profile.id !== draggingId);
    const targetIndex = next.findIndex((profile) => profile.id === targetId);
    next.splice(targetIndex < 0 ? next.length : targetIndex, 0, dragged);
    commitProfiles(next);
    setDraggingId(null);
    setDragOverId(null);
  }

  function openEditor(profile: VoiceModeProfile) {
    setEditor({
      kind: "edit",
      originalId: profile.id,
      profile: { ...profile },
      confirmDelete: false,
      error: null,
    });
  }

  function openCreate() {
    const order = profiles.length;
    setEditor({
      kind: "create",
      originalId: null,
      profile: createCustomVoiceModeProfile(order),
      confirmDelete: false,
      error: null,
    });
  }

  function saveEditor() {
    if (!editor) return;
    const nextProfile = {
      ...editor.profile,
      name: editor.profile.name.trim(),
      description: editor.profile.description.trim(),
      systemPrompt: editor.profile.systemPrompt.trim(),
      userPrompt: editor.profile.userPrompt.trim(),
    };
    if (!nextProfile.name) {
      setEditor({ ...editor, error: "モード名を入力してください。" });
      return;
    }
    if (
      nextProfile.formattingEnabled &&
      !nextProfile.userPrompt.includes("{{transcript}}")
    ) {
      setEditor({
        ...editor,
        error: "ユーザープロンプトには {{transcript}} を含めてください。",
      });
      return;
    }

    if (editor.kind === "create") {
      commitProfiles([...profiles, nextProfile], activeId);
    } else {
      commitProfiles(
        profiles.map((profile) =>
          profile.id === editor.originalId ? nextProfile : profile,
        ),
        activeId,
      );
    }
    setEditor(null);
  }

  function deleteEditingProfile() {
    if (!editor || editor.kind !== "edit" || !editor.originalId) return;
    const target = profiles.find((profile) => profile.id === editor.originalId);
    if (!target?.deletable) return;
    if (profiles.length <= 1) {
      setEditor({ ...editor, error: "音声モードを1つ以上残してください。" });
      return;
    }
    if (!editor.confirmDelete) {
      setEditor({ ...editor, confirmDelete: true, error: null });
      return;
    }
    const next = profiles.filter((profile) => profile.id !== editor.originalId);
    commitProfiles(next, activeId === editor.originalId ? "default" : activeId);
    setEditor(null);
  }

  function resetEditorProfile() {
    if (!editor) return;
    const reset = resetVoiceModeProfile(editor.profile);
    setEditor({
      ...editor,
      profile: {
        ...reset,
        id: editor.profile.id,
        deletable: editor.profile.deletable,
        order: editor.profile.order,
      },
      confirmDelete: false,
      error: null,
    });
  }

  function resetAllProfiles() {
    commitProfiles(defaultVoiceModeProfiles(), "default");
    setEditor(null);
    setDraggingId(null);
    setDragOverId(null);
  }

  return (
    <SettingsSectionPanel
      title="音声モード"
      description="Fn録音で使う出力モードです。録音中はControl単体タップでこの順番に切り替わります。左端のグリップをドラッグして順番を変更できます。"
    >
      <div className="sm:col-span-2">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <p className="text-xs leading-relaxed text-ink-mid">
            最後にONだったモードは保存され、次回のFn録音もそのモードで始まります。
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={resetAllProfiles}
              className={settingsButtonSecondaryClass}
            >
              デフォルトに戻す
            </button>
            <button
              type="button"
              onClick={openCreate}
              className={settingsButtonPrimaryClass}
            >
              モードを追加
            </button>
          </div>
        </div>

        <div className="flex flex-col gap-2">
          {profiles.map((profile) => {
            const active = profile.id === activeId;
            const dragging = draggingId === profile.id;
            const dropTarget = dragOverId === profile.id && draggingId !== profile.id;
            return (
              <div
                key={profile.id}
                onDragOver={(event) => {
                  event.preventDefault();
                  event.dataTransfer.dropEffect = "move";
                  setDragOverId(profile.id);
                }}
                onDragLeave={() =>
                  setDragOverId((current) =>
                    current === profile.id ? null : current,
                  )
                }
                onDrop={(event) => {
                  event.preventDefault();
                  dropProfile(profile.id);
                }}
                className={`group flex flex-col gap-3 rounded-xl border bg-sunken px-3 py-3 transition-colors duration-100 sm:flex-row sm:items-center ${
                  active ? "border-accent/40 bg-accent-soft" : "border-edge"
                } ${dropTarget ? "border-accent bg-accent-soft" : ""} ${
                  dragging ? "opacity-45" : ""
                }`}
              >
                <div
                  draggable
                  onDragStart={(event) => {
                    setDraggingId(profile.id);
                    event.dataTransfer.effectAllowed = "move";
                    event.dataTransfer.setData("text/plain", profile.id);
                  }}
                  onDragEnd={() => {
                    setDraggingId(null);
                    setDragOverId(null);
                  }}
                  title="ドラッグして並び替え"
                  className="flex w-full shrink-0 cursor-grab select-none items-center gap-2 rounded-lg border border-dashed border-edge bg-surface px-2 py-2 text-ink-faint transition-colors duration-100 group-hover:border-accent/40 group-hover:text-accent-ink active:cursor-grabbing sm:w-16 sm:flex-col sm:justify-center sm:self-stretch"
                >
                  <span className="grid grid-cols-2 gap-0.5" aria-hidden>
                    {Array.from({ length: 6 }, (_, dot) => (
                      <span key={dot} className="size-1 rounded-full bg-current" />
                    ))}
                  </span>
                  <span className="text-[10px] font-medium leading-none sm:hidden">
                    ドラッグ
                  </span>
                </div>

                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="truncate text-sm font-semibold text-ink">
                      {profile.name || "名称未設定"}
                    </h3>
                    {active ? (
                      <span className="rounded-full bg-accent px-2 py-0.5 text-[11px] font-medium text-white">
                        ON
                      </span>
                    ) : null}
                    {!profile.deletable ? (
                      <span className="rounded-full border border-edge bg-surface px-2 py-0.5 text-[11px] text-ink-mid">
                        固定
                      </span>
                    ) : null}
                    {!profile.formattingEnabled ? (
                      <span className="rounded-full bg-warn-soft px-2 py-0.5 text-[11px] text-warn">
                        整形なし
                      </span>
                    ) : null}
                    {profile.liveTranscriptionEnabled ? (
                      <span
                        className={`rounded-full px-2 py-0.5 text-[11px] ${
                          liveTranscriptionAvailable
                            ? "bg-ok-soft text-ok"
                            : "border border-edge bg-surface text-ink-mid"
                        }`}
                      >
                        {liveTranscriptionAvailable ? "ライブ" : "ライブ未対応"}
                      </span>
                    ) : null}
                  </div>
                  <p className="mt-1 line-clamp-2 text-xs leading-relaxed text-ink-mid">
                    {profile.description || "説明なし"}
                  </p>
                </div>

                <div className="flex w-full shrink-0 items-center justify-end gap-2 sm:w-auto">
                  {!active ? (
                    <button
                      type="button"
                      onClick={() => onChange({ activeModeProfileId: profile.id })}
                      className="rounded-lg bg-accent-soft px-2.5 py-1 text-xs font-medium text-accent-ink transition-colors duration-100 hover:bg-accent/20"
                    >
                      ONにする
                    </button>
                  ) : null}
                  <button
                    type="button"
                    onClick={() => openEditor(profile)}
                    className={settingsButtonSecondaryClass}
                  >
                    編集
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {editor ? (
        <VoiceModeEditorDialog
          editor={editor}
          liveTranscriptionAvailable={liveTranscriptionAvailable}
          onChange={(profile) =>
            setEditor({ ...editor, profile, confirmDelete: false, error: null })
          }
          onClose={() => setEditor(null)}
          onSave={saveEditor}
          onReset={resetEditorProfile}
          onDelete={deleteEditingProfile}
        />
      ) : null}
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
        <span className="font-medium text-ink">Google Cloud Project ID</span>
        <input
          value={voice.googleCloudProjectId}
          onChange={(e) => onVoiceChange({ googleCloudProjectId: e.target.value })}
          placeholder="my-gcp-project"
          className={settingsInputClass}
        />
      </label>
      <label className="flex flex-col gap-1.5 text-sm">
        <span className="font-medium text-ink">Google Cloudリージョン</span>
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
          <span className="font-medium text-ink">
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
    <SettingsSectionPanel
      title="アプリ"
      description="外観、起動、権限まわりの設定です。"
    >
      <AppearanceField />
      <Toggle
        className="sm:col-span-2"
        label="Macログイン時に自動起動"
        checked={app.launchAtLogin}
        onChange={(checked) => onChange({ launchAtLogin: checked })}
      />
      <p className="rounded-xl border border-edge bg-sunken px-4 py-3 text-xs leading-relaxed text-ink-mid sm:col-span-2">
        macOSではアクセシビリティ、入力監視、マイクの許可が必要です。ショートカットが取得できない場合は、プライバシーとセキュリティの許可を確認してください。
      </p>
    </SettingsSectionPanel>
  );
}

const THEME_OPTIONS: Array<{
  value: ThemePreference;
  label: string;
  icon: typeof Monitor;
}> = [
  { value: "system", label: "システム", icon: Monitor },
  { value: "light", label: "ライト", icon: Sun },
  { value: "dark", label: "ダーク", icon: Moon },
];

function AppearanceField() {
  const preference = useThemePreference();

  return (
    <div className="flex flex-col gap-1.5 text-sm sm:col-span-2">
      <span className="font-medium text-ink">テーマ</span>
      <div
        role="radiogroup"
        aria-label="テーマ"
        className="grid w-fit grid-cols-3 gap-0.5 rounded-lg border border-edge bg-sunken p-0.5"
      >
        {THEME_OPTIONS.map((option) => {
          const Icon = option.icon;
          const selected = preference === option.value;
          return (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={selected}
              onClick={() => setThemePreference(option.value)}
              className={`flex items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition-colors duration-150 focus-ring ${
                selected
                  ? "bg-surface text-ink shadow-sm"
                  : "text-ink-mid hover:text-ink"
              }`}
            >
              <Icon size={13} />
              {option.label}
            </button>
          );
        })}
      </div>
      <p className="text-xs leading-relaxed text-ink-mid">
        「システム」はmacOSの外観モードに自動で追従します。
      </p>
    </div>
  );
}

function VoiceModeEditorDialog({
  editor,
  liveTranscriptionAvailable,
  onChange,
  onClose,
  onSave,
  onReset,
  onDelete,
}: {
  editor: VoiceModeEditorState;
  liveTranscriptionAvailable: boolean;
  onChange: (profile: VoiceModeProfile) => void;
  onClose: () => void;
  onSave: () => void;
  onReset: () => void;
  onDelete: () => void;
}) {
  const profile = editor.profile;
  const canDelete = editor.kind === "edit" && profile.deletable;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4 backdrop-blur-[2px]">
      <div
        role="dialog"
        aria-modal="true"
        className="flex max-h-[88vh] w-full max-w-3xl flex-col overflow-hidden rounded-2xl bg-raised shadow-modal animate-pop-in"
      >
        <div className="flex items-start justify-between gap-4 border-b border-edge px-5 py-4">
          <div className="min-w-0">
            <h2 className="text-lg font-semibold text-ink">
              {editor.kind === "create" ? "音声モードを追加" : "音声モードを編集"}
            </h2>
            <p className="mt-1 text-sm leading-relaxed text-ink-mid">
              整形を有効にした場合、ユーザープロンプトには必ず {"{{transcript}}"} を含めます。
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className={settingsButtonSecondaryClass}
          >
            閉じる
          </button>
        </div>

        <div className="min-h-0 space-y-4 overflow-y-auto px-5 py-4">
          {editor.error ? (
            <p className="rounded-xl bg-danger-soft px-3 py-2 text-sm text-danger">
              {editor.error}
            </p>
          ) : null}

          <div className="grid gap-4 sm:grid-cols-2">
            <label className="flex flex-col gap-1.5 text-sm">
              <span className="font-medium text-ink">モード名</span>
              <input
                value={profile.name}
                onChange={(event) =>
                  onChange({ ...profile, name: event.target.value })
                }
                className={settingsInputClass}
                placeholder="例: 議事録"
              />
            </label>
            <label className="flex flex-col gap-1.5 text-sm">
              <span className="font-medium text-ink">説明</span>
              <input
                value={profile.description}
                onChange={(event) =>
                  onChange({ ...profile, description: event.target.value })
                }
                className={settingsInputClass}
                placeholder="一覧に表示する短い説明"
              />
            </label>
          </div>

          <Toggle
            label="整形を行う"
            description="オフにすると、文字起こし結果をGemini整形に通さずすぐに出力します。"
            checked={profile.formattingEnabled}
            onChange={(formattingEnabled) =>
              onChange({ ...profile, formattingEnabled })
            }
          />

          <Toggle
            label="ライブ文字起こし"
            description={
              liveTranscriptionAvailable
                ? "録音中に文字起こしを先行し、停止後は最終本文を一括で出力します。"
                : "現在の音声認識モデルではライブ文字起こしを利用できません。"
            }
            checked={
              liveTranscriptionAvailable && profile.liveTranscriptionEnabled
            }
            disabled={!liveTranscriptionAvailable}
            onChange={(liveTranscriptionEnabled) =>
              onChange({ ...profile, liveTranscriptionEnabled })
            }
          />

          {profile.formattingEnabled ? (
            <>
              <label className="flex flex-col gap-1.5 text-sm">
                <span className="font-medium text-ink">System prompt</span>
                <textarea
                  value={profile.systemPrompt}
                  onChange={(event) =>
                    onChange({ ...profile, systemPrompt: event.target.value })
                  }
                  rows={4}
                  className={`${settingsInputClass} resize-y font-mono text-xs leading-relaxed`}
                />
              </label>

              <label className="flex flex-col gap-1.5 text-sm">
                <span className="flex flex-wrap items-center justify-between gap-2">
                  <span className="font-medium text-ink">User prompt</span>
                  <span className="rounded-full border border-edge bg-surface px-2 py-0.5 text-[11px] text-ink-mid">
                    必須: {"{{transcript}}"} / 任意: {"{{dictionary_section}}"}
                  </span>
                </span>
                <textarea
                  value={profile.userPrompt}
                  onChange={(event) =>
                    onChange({ ...profile, userPrompt: event.target.value })
                  }
                  rows={10}
                  className={`${settingsInputClass} resize-y font-mono text-xs leading-relaxed`}
                />
              </label>
            </>
          ) : (
            <p className="rounded-xl bg-warn-soft px-4 py-3 text-sm leading-relaxed text-warn">
              このモードでは整形プロンプトを使いません。録音停止後、文字起こし結果をそのまま出力します。
            </p>
          )}
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-edge px-5 py-4">
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={onReset}
              className={settingsButtonSecondaryClass}
            >
              初期値に戻す
            </button>
            {canDelete ? (
              <button
                type="button"
                onClick={onDelete}
                className={`rounded-lg px-3 py-1.5 text-xs font-medium transition-colors duration-100 ${
                  editor.confirmDelete
                    ? "bg-danger text-white hover:bg-danger/85"
                    : "bg-danger-soft text-danger hover:bg-danger/20"
                }`}
              >
                {editor.confirmDelete ? "もう一度押して削除" : "削除"}
              </button>
            ) : null}
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onClose}
              className={settingsButtonSecondaryClass}
            >
              キャンセル
            </button>
            <button type="button" onClick={onSave} className={settingsButtonPrimaryClass}>
              保存
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function orderedVoiceModeProfiles(profiles: VoiceModeProfile[]): VoiceModeProfile[] {
  return [...profiles].sort((a, b) => a.order - b.order);
}

function withVoiceModeOrder(profiles: VoiceModeProfile[]): VoiceModeProfile[] {
  return profiles.map((profile, index) => ({ ...profile, order: index }));
}

function createCustomVoiceModeProfile(order: number): VoiceModeProfile {
  return {
    id: `custom-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
    name: "新しいモード",
    description: "用途に合わせて出力文体を調整します。",
    formattingEnabled: true,
    liveTranscriptionEnabled: false,
    systemPrompt: CUSTOM_MODE_SYSTEM_PROMPT,
    userPrompt: CUSTOM_MODE_USER_PROMPT,
    deletable: true,
    order,
    presetKey: null,
  };
}

function resetVoiceModeProfile(profile: VoiceModeProfile): VoiceModeProfile {
  if (profile.presetKey && VOICE_MODE_PRESET_DEFAULTS[profile.presetKey]) {
    return { ...VOICE_MODE_PRESET_DEFAULTS[profile.presetKey] };
  }
  return {
    ...profile,
    formattingEnabled: true,
    liveTranscriptionEnabled: false,
    systemPrompt: CUSTOM_MODE_SYSTEM_PROMPT,
    userPrompt: CUSTOM_MODE_USER_PROMPT,
  };
}

function supportsLiveTranscription(
  speechProfile: AppSettings["voice"]["speechProfile"],
): boolean {
  return (
    speechProfile === "appleSpeechAnalyzer" || speechProfile === "googleChirp3"
  );
}

function defaultVoiceModeProfiles(): VoiceModeProfile[] {
  return (["default", "speed", "aiPrompt", "casual", "formal"] as const).map(
    (key) => ({ ...VOICE_MODE_PRESET_DEFAULTS[key] }),
  );
}

const CUSTOM_MODE_SYSTEM_PROMPT =
  "あなたは日本語の音声入力編集者です。音声認識結果を、ユーザーが指定した用途に合わせて整えます。出力は最終本文のみ。前置き、説明、引用符、ラベルは出しません。";

const CUSTOM_MODE_USER_PROMPT = `{{dictionary_section}}

音声認識結果:
{{transcript}}

要件:
- 用途に合わせて自然な文章へ整える。
- 口癖、言い直し、不要な間を整理する。
- 音声認識結果または文脈から該当語だと判断できる場合だけ、辞書の優先表記に整える。
- 該当すると判断できない語を辞書語へ置き換えない。
- 内容を勝手に増やさない。`;

const VOICE_MODE_PRESET_DEFAULTS: Record<VoiceModePresetKey, VoiceModeProfile> = {
  default: {
    id: "default",
    name: "デフォルト",
    description: "話した内容を自然な日本語文として整えます。",
    formattingEnabled: true,
    liveTranscriptionEnabled: false,
    systemPrompt:
      "あなたは日本語の音声入力編集者です。音声認識結果を、ユーザーがそのまま貼り付けられる自然な日本語文に整形します。出力は最終本文のみ。前置き、説明、引用符、ラベルは出しません。",
    userPrompt: `{{dictionary_section}}

音声認識結果:
{{transcript}}

要件:
- 話し言葉の不要な言い直しを整理する。
- 録音内に「これをこうまとめて」などの指示が含まれる場合、その意図に従って最終文章を作る。
- 音声認識結果または文脈から該当語だと判断できる場合だけ、辞書の優先表記に整える。
- 該当すると判断できない語を辞書語へ置き換えない。
- 内容を勝手に増やさない。`,
    deletable: false,
    order: 0,
    presetKey: "default",
  },
  speed: {
    id: "speed",
    name: "スピード",
    description: "整形せず、文字起こし結果をすぐに出力します。",
    formattingEnabled: false,
    liveTranscriptionEnabled: true,
    systemPrompt:
      "あなたは日本語の音声入力編集者です。音声認識結果を必要最小限だけ整えます。出力は最終本文のみ。前置き、説明、引用符、ラベルは出しません。",
    userPrompt: `{{dictionary_section}}

音声認識結果:
{{transcript}}

要件:
- 文字起こし結果を大きく変えず、明らかな誤字や不要な空白だけ整える。
- 内容を勝手に増やさない。`,
    deletable: true,
    order: 1,
    presetKey: "speed",
  },
  aiPrompt: {
    id: "aiPrompt",
    name: "AIプロンプト",
    description: "話した内容をAIに渡しやすいプロンプトへ整えます。",
    formattingEnabled: true,
    liveTranscriptionEnabled: false,
    systemPrompt:
      "あなたはAIプロンプト設計者です。音声認識結果から、AIに渡しやすい明確で実行可能なプロンプトを作成します。出力はプロンプト本文のみ。前置き、説明、引用符、ラベルは出しません。",
    userPrompt: `{{dictionary_section}}

話した内容:
{{transcript}}

要件:
- 話した意図を、AIへ渡す明確なプロンプトに再構成する。
- 目的、背景、入力、制約、期待する出力形式を必要に応じて整理する。
- 箇条書きや見出しは、プロンプトとして読みやすい場合だけ使う。
- 内容を勝手に増やさず、曖昧な部分は自然な依頼文としてまとめる。`,
    deletable: true,
    order: 2,
    presetKey: "aiPrompt",
  },
  casual: {
    id: "casual",
    name: "カジュアル",
    description: "Slackなどに合う親しみやすい文体へ整えます。",
    formattingEnabled: true,
    liveTranscriptionEnabled: false,
    systemPrompt:
      "あなたは日本語チャット文の編集者です。音声認識結果を、Slackなどのチャットにそのまま送れる親しみやすい文章へ整えます。出力は最終本文のみ。前置き、説明、引用符、ラベルは出しません。",
    userPrompt: `{{dictionary_section}}

音声認識結果:
{{transcript}}

要件:
- くだけすぎない親しみやすい文体にする。
- 必要に応じて感嘆符を使い、硬さを和らげる。
- 口癖、言い直し、不要な間を整理する。
- 音声認識結果または文脈から該当語だと判断できる場合だけ、辞書の優先表記に整える。
- 該当すると判断できない語を辞書語へ置き換えない。
- 内容を勝手に増やさない。`,
    deletable: true,
    order: 3,
    presetKey: "casual",
  },
  formal: {
    id: "formal",
    name: "フォーマル",
    description: "メール返信などに合うやや丁寧な文体へ整えます。",
    formattingEnabled: true,
    liveTranscriptionEnabled: false,
    systemPrompt:
      "あなたは日本語ビジネス文の編集者です。音声認識結果を、メール返信などに適したやや丁寧な文章へ整えます。出力は最終本文のみ。前置き、説明、引用符、ラベルは出しません。",
    userPrompt: `{{dictionary_section}}

音声認識結果:
{{transcript}}

要件:
- メールや業務チャットで使いやすい、やや丁寧な文体にする。
- 過度に堅くしすぎず、自然な敬体で整える。
- 口癖、言い直し、不要な間を整理する。
- 音声認識結果または文脈から該当語だと判断できる場合だけ、辞書の優先表記に整える。
- 該当すると判断できない語を辞書語へ置き換えない。
- 内容を勝手に増やさない。`,
    deletable: true,
    order: 4,
    presetKey: "formal",
  },
};

const DEFAULT_SHORTCUTS: Record<ShortcutAction, ShortcutBinding> = {
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
  polishSelection: {
    keyCode: 35,
    key: "p",
    label: "Ctrl Option P",
    tapCount: 1,
    modifiers: {
      command: false,
      option: true,
      control: true,
      shift: false,
      function: false,
    },
  },
};
