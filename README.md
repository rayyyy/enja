# Enja

**Enja** is a macOS desktop app (Tauri 2) that listens for **two quick Cmd+C presses** (default within 400 ms), reads the clipboard, and streams a **Japanese translation** from the Google **Gemini API** into an overlay. Settings (including your API key) stay on your machine; translation requests go to Google’s servers.

日本語: macOS で **Cmd+C を 2 回連打**（デフォルト 400ms 以内）するとクリップボードのテキストを **Gemini API** で日本語に翻訳し、オーバーレイにストリーミング表示する Tauri 2 アプリです。

| | |
|--|--|
| **Default model** | `gemini-3.1-flash-lite-preview`（[`src-tauri/src/gemini.rs`](src-tauri/src/gemini.rs) 内の定数。Google の提供状況により変更の可能性あり） |
| **License** | [MIT](LICENSE) |
| **Contributing** | [CONTRIBUTING.md](CONTRIBUTING.md) |
| **Security** | [SECURITY.md](SECURITY.md) · [Code of Conduct](CODE_OF_CONDUCT.md) |

## プライバシーとデータの取り扱い

- **クリップボード**: 連打トリガー時に読み取ったテキストが、翻訳のために **Google Gemini API**（`generativelanguage.googleapis.com`）へ送信されます。Google の利用規約・プライバシーポリシーが適用されます。
- **API キー**: アプリの設定で保存したキーは、Tauri のアプリ設定ディレクトリ内の **`settings.json`** に書き込まれます（実装は [`src-tauri/src/settings.rs`](src-tauri/src/settings.rs)）。リポジトリや Issue にキーを載せないでください。
- **常駐と権限**: グローバルな Cmd+C 検出には **アクセシビリティ**の許可が必要です（下記「macOS の権限」）。

## 必要なもの

| もの | 用途 |
|------|------|
| [mise](https://mise.jdx.dev/)（推奨） | Bun / Rust のバージョン管理 |
| [Gemini API キー](https://aistudio.google.com/apikey) | 翻訳 API |
| Xcode Command Line Tools（macOS） | リンカ・SDK（未導入なら `xcode-select --install`） |

このリポジトリでは **[mise.toml](mise.toml)** で **Bun** と **Rust** のバージョンを固定しています。mise を使わない場合は、同等以上のバージョンを手動で入れてください。

---

## 立ち上げ方（初回）

リポジトリのルートで次を実行します。

```bash
cd /path/to/enja

# 1) mise で Bun / Rust を入れる（PATH が通るシェルで）
mise install

# 2) フロントの依存関係
bun install

# 3) 開発モード（Vite + Tauri ウィンドウ）
bun run tauri dev
```

初回は Rust のコンパイルで時間がかかります。API キー未設定のときはウィンドウが開くので、**設定** から Gemini API キーを保存してください。

### mise のタスクを使う場合

```bash
mise install
mise run dev
```

`dev` は `bun install` から実行するので、2 回目以降もそのまま使えます。

---

## 普段の開発（2 回目以降）

```bash
mise install   # mise.toml を変えたときだけでよい
bun run tauri dev
```

フロントだけ試す場合（ネイティブやホットキーなし）:

```bash
bun run dev
# または
mise run vite-only
```

---

## 本番ビルド

```bash
bun run tauri build
# または
mise run build
```

成果物は `src-tauri/target/release/` 付近に生成されます（Tauri の表示に従ってください）。

---

## macOS の権限

- **アクセシビリティ**: グローバルな Cmd+C 検出に必要です。  
  **システム設定 → プライバシーとセキュリティ → アクセシビリティ** で、開発中は **ターミナル** や **Cursor**、配布版では **Enja** を許可してください。
- 初回に説明文を出すには **Info.plist の `NSAccessibilityUsageDescription`** が必要な場合があります（配布ビルド時に検討）。

---

## 使い方（アプリ）

1. **設定** で Gemini API キーを保存する。
2. テキストを選択して **Cmd+C** でコピーし、すぐにもう一度 **Cmd+C** を押すとオーバーレイが開き、翻訳がストリーミング表示される。
3. **Esc** またはオーバーレイ外クリックでウィンドウを閉じる（プロセスは常駐し、ウィンドウは `hide` のみ）。

---

## 構成

- **フロント**: React + TypeScript + Vite + Tailwind CSS v4 + Zustand
- **ネイティブ**: Tauri 2（Rust）。macOS では **CGEventTap** によるパッシブなキー監視（[`src-tauri/src/keyboard.rs`](src-tauri/src/keyboard.rs)）、**arboard** でクリップボード読み取り。
- **Gemini**: フロントから `translate` を `invoke` し、Rust の **reqwest** で Gemini の SSE ストリームを処理してチャネル経由で UI に渡す（[`src-tauri/src/lib.rs`](src-tauri/src/lib.rs)、[`src-tauri/src/gemini.rs`](src-tauri/src/gemini.rs)）

詳細は [docs/architecture.md](docs/architecture.md) を参照してください。

---

## トラブルシューティング

| 症状 | 確認 |
|------|------|
| `cargo` / `rustc` が見つからない | `mise install` 後にシェルを開き直す、`eval "$(mise activate)"` を入れる |
| `tauri` が見つからない | プロジェクト直下で `bun install`（`@tauri-apps/cli` が devDependency） |
| Cmd+C 連打で反応しない | アクセシビリティでホストアプリ（ターミナル等）を許可したか |
| ビルドエラー（リンカ） | Xcode CLT のインストール、`rustup target list` で Apple ターゲット |
| `rustc x.x.x is not supported by the following packages` | [mise.toml](mise.toml) の Rust を上げ、`mise install` をやり直す（シェル再起動） |

バージョンを上げるときは **mise.toml** の `[tools]` を更新してください。

---

## リポジトリ URL について

`package.json` / `Cargo.toml` の `repository` は **`https://github.com/rayyyy/enja`** を想定しています。フォークや組織移動後は、そこを実際の URL に合わせて更新してください（手順は [CONTRIBUTING.md](CONTRIBUTING.md) に記載）。
