# コントリビューションガイド

Enja への関心ありがとうございます。バグ報告・機能提案・ドキュメント改善・小さな修正など、どのような形でも歓迎です。

## 始める前に

- [README.md](README.md) でビルド手順とプライバシー（クリップボード・API キー）を確認してください。
- [SECURITY.md](SECURITY.md): **API キーやクリップボードの内容を Issue などに載せないでください。**
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) に同意できる場合のみ参加をお願いします。

## リポジトリ URL の更新（フォーク時）

`package.json` の `repository` と `src-tauri/Cargo.toml` の `repository` は、現状 **`https://github.com/rayyyy/enja`** を指す想定です。フォークした場合は、自分のリモート URL に合わせて更新してください。

## 変更の範囲について

- **ドキュメント**（README、`docs/`、コメント）の修正は積極的に受け付けます。
- **動作を変えるコード変更** は、Issue で方向性を共有してから PR するとスムーズです。大きな変更はメンテナンスコストが上がるため、分割した PR を推奨します。

## 開発環境

[mise](https://mise.jdx.dev/) を使う場合:

```bash
mise install
bun install
bun run tauri dev
```

詳細は README の「立ち上げ方」を参照してください。

## Pull Request

- 説明文に **何を・なぜ** 変えたかを簡潔に書いてください。
- 関連する Issue 番号があればリンクしてください。
- ローカルで `bun run tauri dev` または `bun run build` が通ることを確認できると助かります（変更内容に応じて）。

## Issue

- **バグ**: 再現手順、macOS のバージョン、期待する動作・実際の動作を書いてください。
- **機能要望**: ユースケースと、あれば代替案を書いてください。

---

## For English readers

Thanks for your interest in Enja. Please read [README.md](README.md) for build instructions and privacy notes. Do not post API keys or clipboard contents in public issues ([SECURITY.md](SECURITY.md)). Open an issue or PR with a short description of the change; large behavior changes are easier to review when discussed first.
