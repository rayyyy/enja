//! ライブ文字起こし(Apple SpeechAnalyzer / Google ストリーミング)。

#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveTranscriptionProvider {
    AppleSpeechAnalyzer,
    GoogleChirp3,
}

pub(crate) struct LiveTranscriber {
    pub(crate) provider: LiveTranscriptionProvider,
    pub(crate) sample_tx: Option<std::sync::mpsc::Sender<Vec<i16>>>,
    pub(crate) join: Option<std::thread::JoinHandle<Result<String, String>>>,
}

pub(crate) struct LiveTranscript {
    pub(crate) provider: LiveTranscriptionProvider,
    pub(crate) result: Result<String, String>,
}

impl LiveTranscriber {
    pub(crate) fn sample_sender(&self) -> Option<std::sync::mpsc::Sender<Vec<i16>>> {
        self.sample_tx.as_ref().cloned()
    }

    pub(crate) fn finish(mut self) -> Result<String, String> {
        self.sample_tx.take();
        match self.join.take() {
            Some(join) => join
                .join()
                .unwrap_or_else(|_| Err("ライブ文字起こしスレッドが停止しました。".to_string())),
            None => Err("ライブ文字起こしが開始されていません。".to_string()),
        }
    }

    pub(crate) fn cancel(mut self) {
        self.sample_tx.take();
        self.join.take();
    }
}

pub(crate) fn live_transcription_provider_for_settings(
    settings: &AppSettings,
    mode: VoiceMode,
) -> Option<LiveTranscriptionProvider> {
    if mode != VoiceMode::Dictation {
        return None;
    }
    let profile = settings.voice.active_mode_profile()?;
    if !profile.live_transcription_enabled {
        return None;
    }
    live_transcription_provider_for_speech_profile(settings.voice.speech_profile)
}

pub(crate) fn live_transcription_provider_for_speech_profile(
    profile: SpeechProfile,
) -> Option<LiveTranscriptionProvider> {
    match profile {
        SpeechProfile::AppleSpeechAnalyzer => Some(LiveTranscriptionProvider::AppleSpeechAnalyzer),
        SpeechProfile::GoogleChirp3 => Some(LiveTranscriptionProvider::GoogleChirp3),
        SpeechProfile::OpenAiGpt4oTranscribe
        | SpeechProfile::OpenAiGpt4oMiniTranscribe
        | SpeechProfile::GeminiAudio => None,
    }
}

pub(crate) fn should_use_live_transcript(
    settings: &AppSettings,
    mode: VoiceMode,
    mode_profile_id: &str,
) -> bool {
    if mode != VoiceMode::Dictation {
        return false;
    }
    let Some(profile) = settings.voice.mode_profile_or_default(mode_profile_id) else {
        return false;
    };
    profile.live_transcription_enabled
        && live_transcription_provider_for_speech_profile(settings.voice.speech_profile).is_some()
}

pub(crate) fn start_live_transcriber(
    app: &tauri::AppHandle,
    provider: LiveTranscriptionProvider,
    sample_rate: u32,
    channels: u16,
    screen_context: &VoiceScreenContext,
) -> Result<LiveTranscriber, String> {
    match provider {
        LiveTranscriptionProvider::AppleSpeechAnalyzer => {
            start_apple_live_transcriber(app, sample_rate, channels, screen_context)
        }
        LiveTranscriptionProvider::GoogleChirp3 => {
            start_google_live_transcriber(app, sample_rate, channels, screen_context)
        }
    }
}

pub(crate) fn start_apple_live_transcriber(
    app: &tauri::AppHandle,
    sample_rate: u32,
    channels: u16,
    screen_context: &VoiceScreenContext,
) -> Result<LiveTranscriber, String> {
    let helper = resolve_apple_speech_helper(app)?;
    let entries = dictionary::load_dictionary(app).unwrap_or_default();
    let context_path = temp_voice_file_path("apple-speech-live-context", "json");
    let contextual_strings = apple_speech_contextual_strings(&entries, screen_context);
    let context = serde_json::json!({
        "contextualStrings": contextual_strings,
    });
    fs::write(&context_path, context.to_string()).map_err(|e| e.to_string())?;

    let mut command = std::process::Command::new(&helper);
    command
        .arg("stream-transcribe")
        .arg(sample_rate.to_string())
        .arg(channels.to_string())
        .arg("ja-JP")
        .arg(context_path.display().to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            let _ = fs::remove_file(&context_path);
            return Err(format!(
                "Apple SpeechAnalyzer helper（path: {}）を開始できませんでした: {err}",
                helper.display()
            ));
        }
    };

    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(&context_path);
        return Err("Apple SpeechAnalyzer helperのstdinを取得できませんでした。".to_string());
    };

    let (sample_tx, sample_rx) = std::sync::mpsc::channel::<Vec<i16>>();
    let writer_join = std::thread::spawn(move || -> Result<(), String> {
        for samples in sample_rx {
            write_i16_samples(&mut stdin, &samples)?;
        }
        stdin.flush().map_err(|e| e.to_string())
    });

    let join = std::thread::spawn(move || -> Result<String, String> {
        let writer_result = writer_join
            .join()
            .unwrap_or_else(|_| Err("ライブ音声送信スレッドが停止しました。".to_string()));
        let output = child
            .wait_with_output()
            .map_err(|e| format!("Apple SpeechAnalyzer helperの出力を取得できませんでした: {e}"));
        let _ = fs::remove_file(&context_path);
        writer_result?;
        let output = output?;
        parse_apple_speech_transcript_output(output)
    });

    Ok(LiveTranscriber {
        provider: LiveTranscriptionProvider::AppleSpeechAnalyzer,
        sample_tx: Some(sample_tx),
        join: Some(join),
    })
}

pub(crate) fn write_i16_samples(writer: &mut impl Write, samples: &[i16]) -> Result<(), String> {
    let bytes = i16_samples_to_bytes(samples);
    writer.write_all(&bytes).map_err(|e| e.to_string())
}

pub(crate) fn i16_samples_to_bytes(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(samples));
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

pub(crate) fn parse_apple_speech_transcript_output(
    output: std::process::Output,
) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(if detail.trim().is_empty() {
            format!(
                "Apple SpeechAnalyzer helperが失敗しました: {}",
                output.status
            )
        } else {
            detail
        });
    }
    let response: AppleSpeechHelperResponse = serde_json::from_str(&stdout).map_err(|err| {
        if stderr.is_empty() {
            format!("Apple SpeechAnalyzer helperからJSON応答が返りませんでした: {err}")
        } else {
            format!("Apple SpeechAnalyzer helperからJSON応答が返りませんでした: {err}: {stderr}")
        }
    })?;
    if !response.ok {
        return Err(response
            .error
            .or(response.reason)
            .unwrap_or_else(|| "Apple SpeechAnalyzer helperが失敗しました。".to_string()));
    }
    response
        .transcript
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Apple SpeechAnalyzerのライブ文字起こし結果が空でした。".to_string())
}

pub(crate) fn start_google_live_transcriber(
    app: &tauri::AppHandle,
    sample_rate: u32,
    channels: u16,
    screen_context: &VoiceScreenContext,
) -> Result<LiveTranscriber, String> {
    let settings = app
        .try_state::<SettingsStore>()
        .map(|store| store.get())
        .unwrap_or_default();
    let project = settings.voice.google_cloud_project_id.trim().to_string();
    if project.is_empty() {
        return Err("Google Cloud Project IDを設定してください。".to_string());
    }
    let region = settings.voice.google_cloud_region.trim().to_string();
    if region.is_empty() {
        return Err("Google Cloudリージョンを設定してください。".to_string());
    }
    let entries = dictionary::load_dictionary(app).unwrap_or_default();
    let screen_context = screen_context.clone();
    let (sample_tx, sample_rx) = std::sync::mpsc::channel::<Vec<i16>>();
    let join = std::thread::spawn(move || -> Result<String, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(google_streaming_transcribe(
            settings,
            entries,
            sample_rx,
            sample_rate,
            channels,
            project,
            region,
            screen_context,
        ))
    });

    Ok(LiveTranscriber {
        provider: LiveTranscriptionProvider::GoogleChirp3,
        sample_tx: Some(sample_tx),
        join: Some(join),
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn google_streaming_transcribe(
    settings: AppSettings,
    entries: Vec<DictionaryEntry>,
    sample_rx: std::sync::mpsc::Receiver<Vec<i16>>,
    sample_rate: u32,
    channels: u16,
    project: String,
    region: String,
    screen_context: VoiceScreenContext,
) -> Result<String, String> {
    use googleapis_tonic_google_cloud_speech_v2::google::cloud::speech::v2::{
        explicit_decoding_config, phrase_set, recognition_config, speech_adaptation,
        speech_client::SpeechClient, streaming_recognize_request, ExplicitDecodingConfig,
        PhraseSet, RecognitionConfig, RecognitionFeatures, SpeechAdaptation,
        StreamingRecognitionConfig, StreamingRecognitionFeatures, StreamingRecognizeRequest,
    };
    use tonic::metadata::MetadataValue;
    use tonic::service::Interceptor;
    use tonic::transport::Channel;

    #[derive(Clone)]
    struct GoogleAuthInterceptor {
        authorization: MetadataValue<tonic::metadata::Ascii>,
    }

    impl Interceptor for GoogleAuthInterceptor {
        fn call(
            &mut self,
            mut request: tonic::Request<()>,
        ) -> Result<tonic::Request<()>, tonic::Status> {
            request
                .metadata_mut()
                .insert("authorization", self.authorization.clone());
            Ok(request)
        }
    }

    let token = google_access_token(&settings).await?;
    let endpoint = format!("https://{region}-speech.googleapis.com");
    let channel = Channel::from_shared(endpoint.clone())
        .map_err(|e| e.to_string())?
        .connect()
        .await
        .map_err(|e| format!("Google Speech-to-Text gRPCへ接続できませんでした: {e}"))?;
    let authorization = MetadataValue::try_from(format!("Bearer {token}"))
        .map_err(|e| format!("Google認証メタデータを作成できませんでした: {e}"))?;
    let mut client =
        SpeechClient::with_interceptor(channel, GoogleAuthInterceptor { authorization });

    let recognizer = format!("projects/{project}/locations/{region}/recognizers/_");
    let phrases = transcription_contextual_phrases(&entries, &screen_context, 1000);
    let phrase_values = phrases
        .iter()
        .take(1000)
        .map(|value| phrase_set::Phrase {
            value: value.clone(),
            boost: GOOGLE_SPEECH_DICTIONARY_BOOST,
        })
        .collect::<Vec<_>>();
    let adaptation = if phrase_values.is_empty() {
        None
    } else {
        Some(SpeechAdaptation {
            phrase_sets: vec![speech_adaptation::AdaptationPhraseSet {
                value: Some(
                    speech_adaptation::adaptation_phrase_set::Value::InlinePhraseSet(PhraseSet {
                        phrases: phrase_values,
                        boost: GOOGLE_SPEECH_DICTIONARY_BOOST,
                        ..Default::default()
                    }),
                ),
            }],
            custom_classes: Vec::new(),
        })
    };
    let config = RecognitionConfig {
        model: "chirp_3".to_string(),
        language_codes: vec!["ja-JP".to_string()],
        features: Some(RecognitionFeatures {
            enable_automatic_punctuation: true,
            ..Default::default()
        }),
        adaptation,
        decoding_config: Some(recognition_config::DecodingConfig::ExplicitDecodingConfig(
            ExplicitDecodingConfig {
                encoding: explicit_decoding_config::AudioEncoding::Linear16 as i32,
                sample_rate_hertz: sample_rate as i32,
                audio_channel_count: channels as i32,
            },
        )),
        ..Default::default()
    };
    let streaming_config = StreamingRecognitionConfig {
        config: Some(config),
        streaming_features: Some(StreamingRecognitionFeatures {
            interim_results: true,
            ..Default::default()
        }),
        ..Default::default()
    };

    let (request_tx, request_rx) = tokio::sync::mpsc::channel::<StreamingRecognizeRequest>(16);
    request_tx
        .send(StreamingRecognizeRequest {
            recognizer: recognizer.clone(),
            streaming_request: Some(
                streaming_recognize_request::StreamingRequest::StreamingConfig(streaming_config),
            ),
        })
        .await
        .map_err(|_| "Google Speech-to-Text gRPCの送信開始に失敗しました。".to_string())?;

    let bridge_join = std::thread::spawn(move || -> Result<(), String> {
        for samples in sample_rx {
            let bytes = i16_samples_to_bytes(&samples);
            for chunk in bytes.chunks(14 * 1024) {
                request_tx
                    .blocking_send(StreamingRecognizeRequest {
                        recognizer: recognizer.clone(),
                        streaming_request: Some(
                            streaming_recognize_request::StreamingRequest::Audio(chunk.to_vec()),
                        ),
                    })
                    .map_err(|_| {
                        "Google Speech-to-Text gRPCへの音声送信が停止しました。".to_string()
                    })?;
            }
        }
        Ok(())
    });

    let mut response_stream = client
        .streaming_recognize(tokio_stream::wrappers::ReceiverStream::new(request_rx))
        .await
        .map_err(|e| format!("Google Speech-to-Text streamingRecognizeが失敗しました: {e}"))?
        .into_inner();
    let mut final_parts = Vec::new();
    let mut latest_interim = String::new();

    while let Some(response) = response_stream
        .message()
        .await
        .map_err(|e| format!("Google Speech-to-Text streaming応答の取得に失敗しました: {e}"))?
    {
        for result in response.results {
            let transcript = result
                .alternatives
                .first()
                .map(|alternative| alternative.transcript.trim().to_string())
                .unwrap_or_default();
            if transcript.is_empty() {
                continue;
            }
            if result.is_final {
                if final_parts.last() != Some(&transcript) {
                    final_parts.push(transcript);
                }
                latest_interim.clear();
            } else {
                latest_interim = transcript;
            }
        }
    }

    let bridge_result = bridge_join.join().unwrap_or_else(|_| {
        Err("Google Speech-to-Text音声送信スレッドが停止しました。".to_string())
    });
    bridge_result?;

    if final_parts.is_empty() && !latest_interim.trim().is_empty() {
        final_parts.push(latest_interim);
    }
    let transcript = final_parts.join("\n").trim().to_string();
    if transcript.is_empty() {
        Err("Google Speech-to-Textのライブ文字起こし結果が空でした。".to_string())
    } else {
        Ok(transcript)
    }
}
