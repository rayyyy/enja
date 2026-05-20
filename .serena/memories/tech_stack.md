# Tech Stack

- Tool pins in `mise.toml`: Bun 1.3.3, Rust 1.88.0.
- Frontend: React 19, TypeScript 5.8, Vite 7, Tailwind CSS v4, Zustand 5.
- Desktop/native: Tauri 2, Rust 2021.
- Native crates in `src-tauri/Cargo.toml`: `tauri` with `macos-private-api`, `tauri-plugin-autostart`, `tauri-plugin-opener`, `reqwest` with rustls/json/multipart/stream, `tokio`, `serde`, `serde_json`, `arboard`, `cpal`, `hound`, `jsonwebtoken`, `base64`.
- macOS keyboard path uses CGEventTap directly rather than `rdev`.
- AI/STT providers are Gemini finalization/audio, Google Speech-to-Text Chirp 3, Deepgram Nova 3, and OpenAI transcription endpoints.