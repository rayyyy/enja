# Core

- Enja: macOS-only Tauri 2 desktop app for clipboard translation plus voice dictation/Ask workflows.
- Top-level frontend in `src/`; native app and commands in `src-tauri/src/`.
- Read `mem:tech_stack` for pinned tools/frameworks, `mem:suggested_commands` for local commands, `mem:conventions` for implementation patterns, `mem:task_completion` before finishing code tasks.
- Key native modules: `lib.rs` wires Tauri setup, commands, keyboard triggers; `keyboard.rs` + `keyboard/macos/` (ffi/state/keys/fn_keys/capture/tap) listen for global macOS shortcuts via CGEventTap; `voice.rs` is the session orchestrator with submodules `voice/{audio,text_diff,screen_context,paste,transcribe,live,recorder,devices,window,events,dictionary_learning}.rs`; `gemini.rs` handles Gemini requests/SSE; `settings.rs`, `dictionary.rs`, `secrets.rs` handle local configuration/state.
- Paste uses poll-based verification (paste.rs): clipboard is restored only after AX confirms insertion (40ms polls, 600ms max); snapshot acquisition retries only BEFORE Cmd+V is sent.
- Voice session start runs screen-context capture and audio-pipeline prep concurrently and prefetches the Google token; finalize joins recorder.finish with OCR resolution (OCR-before-ASR semantics preserved).
- Key frontend modules: `App.tsx` bootstraps settings and Tauri events; `VoiceOverlay.tsx` shows voice recording/result overlay; `SettingsView.tsx` (with `settings/speechProfiles.tsx` data + memoized sections in `settings/SettingsSections.tsx`) edits settings/providers; `src/lib/commands.ts` wraps Tauri invokes.
- Frontend perf invariants: settings sections/controls and note list items are React.memo with stable callbacks; rich-text serialization is WeakMap-cached (`serializeRichTextNode`); notes editor keeps `key={note.id}` remount intentionally for undo isolation.
