# Task Completion

- For Rust/native changes: run `cargo fmt --check` and `cargo check` in `src-tauri/`.
- For frontend/TypeScript changes: run `bun run build` from repo root.
- For cross-boundary Tauri command/type changes: run both native checks and `bun run build`.
- For packaging or entitlement/config changes: prefer `bun run tauri build` when time permits.
- Before final response, inspect `git diff`/`git status --short` and call out any checks not run.