# Enja アーキテクチャ（現行実装）

この文書は **リポジトリ内の実装**（2026 年時点）に合わせた説明です。

## 技術スタック

| 層 | 技術 |
|----|------|
| デスクトップシェル | Tauri 2（Rust + WebView） |
| フロント | React 19、TypeScript、Vite、Tailwind CSS v4、Zustand |
| パッケージ／実行 | Bun（`bun install` / `bun run`） |
| キー監視（macOS） | **Core Graphics の CGEventTap**（listen-only）。`rdev` は使用しない |
| クリップボード | `arboard` |
| HTTP / ストリーム | `reqwest`（rustls）、Gemini の **SSE**（`streamGenerateContent` + `alt=sse`） |
| 生成モデル | `gemini-3.1-flash-lite-preview`（`src-tauri/src/gemini.rs` の `MODEL` 定数） |

### なぜ CGEventTap か（`rdev` でないか）

`keyboard.rs` モジュール冒頭のコメントのとおり、`rdev` は内部で Text Services Manager をイベントタップスレッドから呼び出すことがあり、macOS Sequoia 以降のキュー前提の検証で **SIGTRAP** になり得る。Enja は **Cmd+C のキーコードとフラグ** だけが必要なため、TSM を経由しない **CGEventTap 直呼び** にしている。

## 「Cmd+C 2 回連打」の流れ

1. **常駐**: メインウィンドウは起動時 `visible: false`。API キー未設定なら設定のため一度表示する（`lib.rs` の `setup`）。
2. **監視**: `keyboard::spawn_listener` がバックグラウンドで CGEventTap を回し、`Cmd` + `C` の組み合わせを検知。
3. **連打判定**: 前回押下からの経過が `AppSettings::double_tap_threshold_ms`（既定 400 ms）以内ならチャネルに `()` を送る。
4. **メインスレッド**: 受信側で `run_on_main_thread` により次を実行する。
   - `arboard` でクリップボードテキスト取得
   - WebView ウィンドウを `show` / `set_focus`
   - Tauri イベント `enja-trigger` でテキストをフロントへ通知
5. **翻訳**: フロントが `translate` を `invoke`。Rust の `stream_translate` が Gemini に POST し、SSE チャンクをパースして `Channel<TranslateEvent>` 経由で `chunk` / `done` / `error` を UI に流す。
6. **閉じる**: ユーザー操作で `hide_window` が呼ばれ、ウィンドウは `hide` のみ（プロセスは終了しない）。

## Gemini 呼び出し（Rust）

- **エンドポイント**: `v1beta/models/{MODEL}:streamGenerateContent?alt=sse`
- **システムプロンプト**: 日本語への翻訳のみ（見出し・解説・別表現などは出さない旨を `SYSTEM_PROMPT` で指定）。実際の文言は `gemini.rs` を参照。
- **ストリーミング**: レスポンス本文を SSE として行単位で読み、JSON から `candidates[].content.parts[].text` を抽出してチャネルに送る。

フロントから Gemini URL へ **直接 fetch しない**。API キーは Rust 側の設定読み込み結果のみで使う。

## 音声入力の画面文脈

Fn 音声入力では、録音開始時に現在の貼り付け先を基準に画面文脈を取得する。

- **AX文脈**: macOS Accessibility API で前面アプリ、ウィンドウ名、入力欄のカーソル前後、選択範囲、前面ウィンドウの表示テキストを取得する。
- **OCR文脈**: `src-tauri/screen-context-helper/main.swift` の helper が前面ウィンドウのあるディスプレイ上で前面から最大3ウィンドウを個別にキャプチャし、Apple Vision の OCR でアクセシビリティから読めない表示文字を補う。低信頼のOCR行は除外する。推敲だけのフローでは実行せず、音声認識または整形にOCRが届く経路だけで走らせる。失敗しても音声入力自体は継続する。
- **ASRヒント**: 画面文脈から短い固有名詞、ファイル名、コード識別子を抽出し、Google Speech-to-Text / Apple SpeechAnalyzer / OpenAI / Gemini 音声入力のヒントへ辞書語と一緒に渡す。
- **整形ヒント**: Gemini の最終整形プロンプトへ、貼り付け先と周辺文脈を `{{screen_context}}` として渡す。音声内容と矛盾する情報は使わないよう、デフォルトプロンプト側で制約する。

OCRは画面収録権限に依存する。通常のスクリーンショットで読める表示はOCRできるが、権限やアプリ側の制約で空になることがある。その場合でもAX文脈と辞書ヒントで動作する。

## 設定とシークレットの保存

- `settings.json` を Tauri の **アプリ設定ディレクトリ**（`app.path().app_config_dir()`）に保存。
- 内容は `AppSettings` のネスト構造（`translation`、`voice`、`shortcuts`、`prompts`、`app`）。実装は `src-tauri/src/settings.rs`。
- Gemini / OpenAI / Google Service Account などのシークレットは `src-tauri/src/secrets.rs` 経由で macOS Keychain に保存し、`settings.json` には入れない。

## UI・ウィンドウ

- `tauri.conf.json` で透明・デコレーションなしのオーバーレイ風ウィンドウ。
- ウィンドウを破棄せず `hide` することで、再表示のコストを抑える。

## モジュール構成（2026-06 リファクタ後）

音声まわりは `src-tauri/src/voice.rs`（セッション状態機械とオーケストレーション）を親に、責務別のサブモジュールへ分割されている。

| モジュール | 責務 |
|------|------|
| `voice/audio.rs` | VAD トリム・無音圧縮・WAV エンコード（純ロジック） |
| `voice/text_diff.rs` | UTF-16 範囲・差分検出（純ロジック） |
| `voice/screen_context.rs` | AX 読み取り・OCR ヘルパー・ヒント語抽出 |
| `voice/paste.rs` | ペースト先解決・AX FFI・ポーリング検証付き貼り付け |
| `voice/transcribe.rs` | バッチ ASR（Google/Apple/OpenAI/Gemini）・整形・トークン先読み |
| `voice/live.rs` | ライブ文字起こし（Apple / Google ストリーミング） |
| `voice/recorder.rs` | 録音スレッド・AEC・システム音声ミュート/分離 |
| `voice/devices.rs` | 入力デバイス列挙・変更監視 |
| `voice/window.rs` | オーバーレイウィンドウ配置・カーソル追従 |
| `voice/events.rs` | `voice-state` などのイベント定義と発行 |
| `voice/dictionary_learning.rs` | 貼り付け後のユーザー修正の辞書学習 |

キー監視は `src-tauri/src/keyboard.rs`（公開 API と非 mac スタブ）+ `keyboard/macos/`（`ffi` / `state` / `keys` / `fn_keys` / `capture` / `tap` / `tests`）に分割されている。

### 貼り付けの検証（ポーリング方式）

Cmd+V 送信後は固定ディレイではなく、挿入の証拠を 40ms 間隔（最大 600ms）でポーリングして確認し、**確認できてからクリップボードを復元**する。これにより、貼り付けの遅いアプリで復元済みの元クリップボード内容が貼られてしまう競合を防ぎ、成功時の体感も速くなる。スナップショット取得は Cmd+V 送信前のみ再試行し、送信後は二重貼り付け防止のため再試行しない。

挿入の確認チャネルは 3 つ（`voice/paste.rs`）:

1. **AX テキスト差分**: 貼り付け前後の AX スナップショット（AXValue + AXSelectedTextRange）の差分。唯一、挿入位置まで特定できるため辞書学習はこのチャネルでのみ動く。
2. **キャレット移動**: AX テキスト差分が取れない非 Web ターゲット向け。AXSelectedTextRange / AXSelectedTextMarkerRange の変化を挿入の証拠とする。マーカーは CFEqual が値比較を実装しているかを 2 回読みで較正し、ポインタ比較に落ちる実装ではチャネル自体を捨てる（偽成功防止）。AXWebArea は通常ページ本体でも marker range が動くことがあり、Chrome 等で偽成功になりやすいため、このチャネルから除外する。
3. **自プロセスの paste イベント**: 貼り付け先が Enja 自身（メモ等）の場合、フロントエンドが編集可能要素への paste イベントを `record_editable_paste` コマンドで通知する。Enja のウィンドウにフォーカスがあるだけでキャレットがどこにも無いケースを失敗として検出できる。

どのチャネルでも確認できなかった場合、AXTextArea / AXSelectedTextRange のようなロールや属性名だけでは成功扱いしない。Cursor/Monaco/Electron の隠し textarea や Chrome のページ本体は、見た目には入力欄でない場所でも入力系の AX ロールや属性を出し続けることがあるため、未確認成功はコピー用フォールバックダイアログが出ない原因になる。確認手段がひとつも無く、確認できる見込みもないターゲットには Cmd+V 自体を送らない（クリップボードにも触れない）。「カーソルがテキスト入力に無いときは必ずダイアログを出す」が最優先で、偽ダイアログの削減はその次という優先順位。

貼り付け結果は `PasteReport` として構造化する。成功種別（Verified / Unverified / Failed）、確認チャネル、失敗理由、対象 pid/role/subrole/attributes を保持し、UI には短いフォールバック理由だけを出す。Unverified / Failed はデバッグログへ詳細を出し、アプリ個別分岐を増やす前に汎用ルールの弱点を切り分けられるようにする。辞書学習は引き続き AX テキスト差分で Verified になった挿入だけを対象にする。

### 今後の入力安定化方針

音声入力の出力処理を改善するときは、**入力できていないのに成功扱いしないこと**を最優先にしつつ、アプリ固有の分岐を増やしすぎない範囲で直挿入率を上げる。出力先は録音開始時ではなく、引き続き**確定時点のフォーカス**を正とする。録音開始時の貼り付け先情報は画面文脈や認識ヒントには使ってよいが、出力先の固定には使わない。

初回の根本見直しでは、挿入手段は Cmd+V に寄せる。短文の実キーボード入力シミュレーションや AXValue / AXSelectedTextRange への直接書き込みは、壊れ方がターゲットごとに異なりやすいため入れない。改善の主戦場は、貼り付け可能判定と挿入確認チャネルの強化に置く。特に AXWebArea、Electron/Monaco 系、表計算グリッド、リッチテキストのような汎用ターゲットで、カーソル・選択・値変化・paste イベント相当の証拠をより広く拾う。ただし証拠が取れない場合は成功扱いせず、コピー用フォールバックダイアログへ倒す。

独立した native input helper には、AX / CGEvent / paste 検証を小さい実行ファイルへ隔離できる、CLI fixture で検証しやすい、将来 Swift/AppKit 側の AX observer や NSPasteboard 操作へ広げやすい、という利点がある。一方で TCC 権限、署名、プロセス間通信、バイナリ同梱、障害ログの追跡が増える。現行 Rust 実装はすでに AX と CGEvent を直接呼べるため、まずは `voice/paste.rs` 内で判定・検証ロジックを整理し、必要になった時点で同じ API 境界を helper に移す。

### セッション開始/確定の並行化

- 開始時: 画面文脈の取得と音声パイプライン準備（ミュート/分離）を並行実行し、Google ASR 利用時はアクセストークンを録音中に先読みする。
- 確定時: 録音停止（VAD+WAV 化）と OCR 結果の解決を並行実行する。OCR を ASR 送信前に待つ仕様は維持。

## 参考ファイル

| 目的 | パス |
|------|------|
| エントリ・コマンド・トリガー処理 | `src-tauri/src/lib.rs` |
| 音声セッションのオーケストレーション | `src-tauri/src/voice.rs` |
| Gemini SSE・プロンプト | `src-tauri/src/gemini.rs` |
| キー監視 | `src-tauri/src/keyboard.rs`, `src-tauri/src/keyboard/macos/` |
| 設定 | `src-tauri/src/settings.rs` |
| フロントエントリ | `src/main.tsx`, `src/App.tsx` |
