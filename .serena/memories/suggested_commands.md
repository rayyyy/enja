# Suggested Commands

- Install pinned tools: `mise install`.
- Install JS deps: `bun install`.
- Run full desktop dev app: `bun run tauri dev` from repo root, or `mise run dev`.
- Run frontend only: `bun run dev` from repo root, or `mise run vite-only`.
- Frontend production build/typecheck: `bun run build` from repo root.
- Native checks: `cargo fmt --check` and `cargo check` from `src-tauri/`.
- Full native bundle: `bun run tauri build` from repo root, or `mise run build`.