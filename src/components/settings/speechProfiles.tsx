import { memo } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  AppSettings,
  AppleSpeechStatus,
  FinalizationModel,
  ProviderStatus,
  SpeechProfile,
  SpeechSetupCheck,
} from "../../types";
import { settingsButtonSecondaryClass } from "./SettingsControls";
import type { ProviderSecretsDraft } from "./SettingsSections";

type SpeechProfileDoc = {
  label: string;
  url: string;
};

export type SpeechProfileOption = {
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

export type ProfileRequirement = {
  label: string;
  ok: boolean;
  value: string;
};

export const SPEECH_PROFILES: SpeechProfileOption[] = [
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
      "ライブ文字起こしは現在OpenAI系プロバイダでは無効です。",
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
      "ライブ文字起こしは現在OpenAI系プロバイダでは無効です。",
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

export const FINALIZATION_MODELS: { value: FinalizationModel; label: string }[] = [
  { value: "gemini31FlashLite", label: "Gemini 3.1 Flash-Lite（最速）" },
  { value: "gemini35Flash", label: "Gemini 3.5 Flash（高速・標準）" },
  { value: "gemini31ProPreview", label: "Gemini 3.1 Pro Preview（精度優先）" },
];

export function profileRequirements(
  profile: SpeechProfile,
  settings: AppSettings,
  providerStatus: ProviderStatus | null,
  secrets: ProviderSecretsDraft,
  appleSpeechStatus: AppleSpeechStatus | null,
): ProfileRequirement[] {
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

export function appleSpeechStatusToCheck(status: AppleSpeechStatus): SpeechSetupCheck {
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

export const SpeechProfileRow = memo(function SpeechProfileRow({
  profile,
  selected,
  requirements,
  disabled,
  onSelect,
  onSetup,
}: {
  profile: SpeechProfileOption;
  selected: boolean;
  requirements: ProfileRequirement[];
  disabled?: boolean;
  onSelect: (value: SpeechProfile) => void;
  onSetup: (value: SpeechProfile) => void;
}) {
  const configured = requirements.every((item) => item.ok);

  return (
    <div
      role="button"
      aria-pressed={selected}
      aria-disabled={disabled}
      tabIndex={0}
      onClick={() => onSelect(profile.value)}
      onKeyDown={(event) => {
        if (event.currentTarget !== event.target) return;
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect(profile.value);
        }
      }}
      className={`grid w-full gap-4 rounded-xl border px-4 py-4 text-left transition-colors duration-100 focus-ring lg:grid-cols-[minmax(0,1fr)_auto] ${
        selected
          ? "border-accent/40 bg-accent-soft"
          : "border-edge bg-sunken hover:bg-hover"
      } ${disabled ? "cursor-not-allowed opacity-75" : ""}`}
    >
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <span
            className={`size-2.5 shrink-0 rounded-full transition-colors duration-100 ${
              selected ? "bg-accent" : "bg-edge-strong"
            }`}
          />
          <span className="text-sm font-medium text-ink">{profile.label}</span>
          <span className="rounded-full border border-edge bg-surface px-2 py-0.5 text-[11px] text-ink-mid">
            {profile.badge}
          </span>
          <span
            className={`rounded-full px-2 py-0.5 text-[11px] ${
              configured ? "bg-ok-soft text-ok" : "bg-warn-soft text-warn"
            }`}
          >
            {configured ? "設定済み" : "要セットアップ"}
          </span>
        </div>
        <p className="mt-1 text-xs leading-relaxed text-ink-mid">{profile.note}</p>
        <div className="mt-2 flex flex-wrap gap-1.5">
          {requirements.map((item) => (
            <span
              key={item.label}
              className={`rounded-full px-2 py-0.5 text-[11px] ${
                item.ok ? "border border-edge bg-surface text-ink-mid" : "bg-danger-soft text-danger"
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
            onSetup(profile.value);
          }}
          className="w-fit rounded-lg bg-transparent px-2.5 py-1 text-xs font-medium text-accent-ink transition-colors duration-100 hover:bg-accent-soft"
        >
          セットアップ方法
        </button>
      </div>
    </div>
  );
});

export function ModelMetric({
  label,
  value,
  note,
}: {
  label: string;
  value: string;
  note?: string;
}) {
  return (
    <span className="min-w-[72px] rounded-lg border border-edge bg-surface px-2 py-1.5 text-center">
      <span className="block text-[10px] text-ink-faint">{label}</span>
      <span className="block text-xs font-semibold text-ink">{value}</span>
      {note ? <span className="block truncate text-[10px] text-ink-faint">{note}</span> : null}
    </span>
  );
}

export function SetupDialog({
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
  requirements: ProfileRequirement[];
  onClose: () => void;
  checking: boolean;
  checkResult: SpeechSetupCheck | null;
  onCheck: () => void;
  installingAppleSpeech: boolean;
  onInstallAppleSpeech?: () => void;
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4 backdrop-blur-[2px]">
      <div className="flex max-h-[82vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl bg-raised shadow-modal animate-pop-in">
        <div className="flex items-start justify-between gap-4 border-b border-edge px-5 py-4">
          <div>
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-lg font-semibold text-ink">{profile.label}</h2>
              <span className="rounded-full border border-edge bg-surface px-2 py-0.5 text-xs text-ink-mid">
                {profile.badge}
              </span>
            </div>
            <p className="mt-1 text-sm leading-relaxed text-ink-mid">
              {profile.setupSummary}
            </p>
          </div>
          <div className="flex shrink-0 gap-2">
            <button
              type="button"
              onClick={onCheck}
              disabled={checking}
              className="rounded-lg bg-accent-soft px-2.5 py-1 text-xs font-medium text-accent-ink transition-colors duration-100 hover:bg-accent/20 disabled:opacity-50"
            >
              {checking ? "確認中…" : profile.value === "appleSpeechAnalyzer" ? "状態確認" : "疎通確認"}
            </button>
            {onInstallAppleSpeech ? (
              <button
                type="button"
                onClick={onInstallAppleSpeech}
                disabled={installingAppleSpeech}
                className="rounded-lg bg-ok-soft px-2.5 py-1 text-xs font-medium text-ok transition-colors duration-100 hover:bg-ok/20 disabled:opacity-50"
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
                checkResult.ok ? "bg-ok-soft text-ok" : "bg-danger-soft text-danger"
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
            <h3 className="text-sm font-semibold text-ink">必要な設定</h3>
            <div className="mt-2 flex flex-wrap gap-2">
              {requirements.map((item) => (
                <span
                  key={item.label}
                  className={`rounded-full px-2.5 py-1 text-xs ${
                    item.ok ? "bg-ok-soft text-ok" : "bg-danger-soft text-danger"
                  }`}
                >
                  {item.label}: {item.value}
                </span>
              ))}
            </div>
          </section>

          <section className="mt-5">
            <h3 className="text-sm font-semibold text-ink">セットアップ手順</h3>
            <ol className="mt-2 list-decimal space-y-2 pl-5 text-sm leading-relaxed text-ink-mid">
              {profile.setupSteps.map((step) => (
                <li key={step}>{step}</li>
              ))}
            </ol>
          </section>

          <section className="mt-5">
            <h3 className="text-sm font-semibold text-ink">
              Enjaが設定画面から利用する情報
            </h3>
            <ul className="mt-2 list-disc space-y-2 pl-5 text-sm leading-relaxed text-ink-mid">
              {profile.enjaDataFlow.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
          </section>

          <section className="mt-5">
            <h3 className="text-sm font-semibold text-ink">参照ページ</h3>
            <div className="mt-2 flex flex-wrap gap-2">
              {profile.docs.map((doc) => (
                <button
                  key={doc.url}
                  type="button"
                  onClick={() => void openUrl(doc.url)}
                  className="rounded-lg border border-edge bg-surface px-3 py-1.5 text-xs font-medium text-accent-ink transition-colors duration-100 hover:bg-accent-soft"
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
