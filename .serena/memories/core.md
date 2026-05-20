# Core

- Enja: macOS-only Tauri 2 desktop app for clipboard translation plus voice dictation/Ask workflows.
- Top-level frontend in `src/`; native app and commands in `src-tauri/src/`.
- Read `mem:tech_stack` for pinned tools/frameworks, `mem:suggested_commands` for local commands, `mem:conventions` for implementation patterns, `mem:task_completion` before finishing code tasks.
- Key native modules: `lib.rs` wires Tauri setup, commands, keyboard triggers; `keyboard.rs` listens for global macOS shortcuts via CGEventTap; `voice.rs` handles Fn/Fn+Space recording, selected text capture, transcription, finalization, and paste fallback; `gemini.rs` handles Gemini requests/SSE; `settings.rs`, `dictionary.rs`, `secrets.rs` handle local configuration/state.
- Key frontend modules: `App.tsx` bootstraps settings and Tauri events; `VoiceOverlay.tsx` shows voice recording/result overlay; `SettingsView.tsx` edits settings/providers; `src/lib/commands.ts` wraps Tauri invokes.