# Apple SpeechAnalyzer support

## Goal

Add an Apple on-device speech recognition provider for Enja's existing voice
input flow. The first implementation targets Japanese dictation on supported
macOS releases and keeps the current "record, stop, transcribe, finalize,
insert" interaction model.

## Decisions

- The provider is available only on supported macOS builds and devices.
- The first implementation is batch transcription: Enja records a WAV, then a
  Swift helper runs Apple Speech APIs on that file after recording stops.
- Live transcription and partial-result UI are out of scope for this pass.
- The locale is fixed to `ja-JP`.
- The speech module is `DictationTranscriber` with a long dictation preset,
  because Enja's voice input is dictation-oriented rather than meeting-style
  transcription.
- Japanese model status and installation live in Settings. Normal voice input
  never starts model download automatically.
- If the Apple provider fails or the model is missing, Enja shows a clear error
  and does not automatically fall back to Google, OpenAI, or Gemini.
- Existing Gemini finalization remains unchanged. Dictation modes that disable
  formatting return the Apple transcript directly.
- Dictionary entries are passed to Apple best-effort via
  `AnalysisContext.contextualStrings`: enabled short `preferred` and `alias`
  values, capped at 100 phrases.
- Apple recognition is treated as on-device and not logged as API usage cost.
  Gemini finalization usage is still logged when formatting is enabled.
- The Swift helper prints machine-readable JSON on stdout; stderr is reserved
  for diagnostics.
- The helper is optional at build time. Builds without a compatible Apple SDK
  should still compile, but the Apple provider reports that the helper is
  unavailable.
- The helper itself is built with a macOS 26 deployment target. The main Tauri
  app keeps its existing minimum OS and treats helper launch failure as
  unsupported Apple SpeechAnalyzer status.

## Runtime Flow

```text
Settings
  -> check status for ja-JP
  -> request Speech permission if needed
  -> install ja-JP model when the user explicitly clicks install

Voice input
  -> record WAV with existing Rust/cpal recorder
  -> write a temp WAV file
  -> invoke Swift helper: transcribe <wav-path> ja-JP <context-json-path>
  -> parse JSON transcript
  -> run existing Gemini finalization unless the active mode disables it
```

## Helper Commands

```text
enja-speech-helper status ja-JP
enja-speech-helper install ja-JP
enja-speech-helper transcribe /path/to/audio.wav ja-JP /path/to/context.json
```

Expected JSON shapes:

```json
{ "ok": true, "status": "installed", "supported": true }
{ "ok": true, "status": "unsupported", "supported": false, "reason": "macOS 26 or later is required" }
{ "ok": true, "transcript": "..." }
{ "ok": false, "error": "Japanese dictation model is not installed" }
```

## Remaining Risks

- Apple Speech APIs and model asset behavior can vary by OS release and device.
- The exact bundle location for the helper must be validated in packaged builds.
- `contextualStrings` is a bias, not a guaranteed dictionary replacement.
- Live recognition could improve perceived latency later, but it requires a
  larger UI and streaming pipeline change.
