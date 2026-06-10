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

## 参考ファイル

| 目的 | パス |
|------|------|
| エントリ・コマンド・トリガー処理 | `src-tauri/src/lib.rs` |
| Gemini SSE・プロンプト | `src-tauri/src/gemini.rs` |
| キー監視 | `src-tauri/src/keyboard.rs` |
| 設定 | `src-tauri/src/settings.rs` |
| フロントエントリ | `src/main.tsx`, `src/App.tsx` |
