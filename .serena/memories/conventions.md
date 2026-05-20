# Conventions

- User-facing copy is primarily Japanese; keep labels/errors concise and task-oriented.
- Rust settings structs use snake_case fields with serde camelCase bridge to TypeScript settings types.
- Frontend calls native commands through wrappers in `src/lib/commands.ts`; prefer updating wrapper/types together when command payloads change.
- macOS-only native behavior is isolated with `#[cfg(target_os = "macos")]` and non-mac stubs.
- Voice overlay/result state is event-driven via Tauri events (`voice-state`, `voice-level`, `voice-result`).
- Avoid sending stale implicit context to models. Voice Ask mode should use captured selected text only when it was actually obtained; empty selection must stay explicit and must not fall back to prior clipboard/conversation content.