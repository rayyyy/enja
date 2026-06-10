//! バッチ音声認識(Google/Apple/OpenAI/Gemini)と整形 API 呼び出し。

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) const SPEECH_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);

pub(crate) const APPLE_SPEECH_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) const APPLE_SPEECH_INSTALL_TIMEOUT: Duration = Duration::from_secs(900);

pub(crate) const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) const GOOGLE_SPEECH_DICTIONARY_BOOST: f32 = 8.0;

pub(crate) const APPLE_SPEECH_CONTEXTUAL_STRINGS_MAX: usize = 180;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechSetupCheck {
    pub ok: bool,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleSpeechStatus {
    pub helper_available: bool,
    pub supported: bool,
    pub status: String,
    pub authorization: String,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppleSpeechHelperResponse {
    pub(crate) ok: bool,
    pub(crate) status: Option<String>,
    pub(crate) supported: Option<bool>,
    pub(crate) authorization: Option<String>,
    pub(crate) reason: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) details: Option<Vec<String>>,
    pub(crate) transcript: Option<String>,
}

pub(crate) async fn check_google_chirp3_setup(
    settings: &AppSettings,
) -> Result<SpeechSetupCheck, String> {
    let mut missing = Vec::new();
    if settings.voice.google_cloud_project_id.trim().is_empty() {
        missing.push("Google Cloud Project ID");
    }
    if settings.voice.google_cloud_region.trim().is_empty() {
        missing.push("Google Cloudリージョン");
    }
    if !missing.is_empty() {
        return Ok(SpeechSetupCheck {
            ok: false,
            message: format!("未入力の設定があります: {}", missing.join(", ")),
            details: Vec::new(),
        });
    }

    match google_access_token_with_details(settings).await {
        Ok((_token, mut details)) => {
            details.insert(
                0,
                format!(
                    "Project ID: {} / リージョン: {}",
                    settings.voice.google_cloud_project_id.trim(),
                    settings.voice.google_cloud_region.trim()
                ),
            );
            details.push(
                "認証トークン取得まで確認しました。Speech-to-Text API有効化と権限は実際の認識リクエスト時に検証されます。"
                    .to_string(),
            );
            Ok(SpeechSetupCheck {
                ok: true,
                message: "Google Chirp 3の認証設定は利用可能です。".to_string(),
                details,
            })
        }
        Err(message) => Ok(SpeechSetupCheck {
            ok: false,
            message,
            details: Vec::new(),
        }),
    }
}

pub(crate) fn check_secret_setup(
    label: &str,
    provider: &str,
    ok_message: &str,
    missing_message: &str,
) -> SpeechSetupCheck {
    match secrets::get_secret(provider) {
        Ok(value) if !value.trim().is_empty() => SpeechSetupCheck {
            ok: true,
            message: ok_message.to_string(),
            details: vec![format!("{label}: 保存済み")],
        },
        _ => SpeechSetupCheck {
            ok: false,
            message: missing_message.to_string(),
            details: vec![format!("{label}: 未保存")],
        },
    }
}

pub(crate) async fn transcribe(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    match settings.voice.speech_profile {
        SpeechProfile::GoogleChirp3 => {
            if clip.duration_secs > 60.0 || clip.wav.len() > 10 * 1024 * 1024 {
                transcribe_long_audio_fallback(app, settings, entries, screen_context, clip).await
            } else {
                transcribe_google_chirp3(app, settings, entries, screen_context, clip).await
            }
        }
        SpeechProfile::OpenAiGpt4oTranscribe => {
            transcribe_openai(
                app,
                "gpt-4o-transcribe",
                settings,
                entries,
                screen_context,
                clip,
            )
            .await
        }
        SpeechProfile::OpenAiGpt4oMiniTranscribe => {
            transcribe_openai(
                app,
                "gpt-4o-mini-transcribe",
                settings,
                entries,
                screen_context,
                clip,
            )
            .await
        }
        SpeechProfile::GeminiAudio => {
            transcribe_gemini_audio(app, settings, entries, screen_context, clip).await
        }
        SpeechProfile::AppleSpeechAnalyzer => {
            transcribe_apple_speech(app, entries, screen_context, clip).await
        }
    }
}

pub(crate) async fn transcribe_long_audio_fallback(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    if secrets::get_secret("openai").is_ok_and(|key| !key.trim().is_empty()) {
        return transcribe_openai(
            app,
            "gpt-4o-transcribe",
            settings,
            entries,
            screen_context,
            clip,
        )
        .await;
    }
    transcribe_gemini_audio(app, settings, entries, screen_context, clip).await
}

pub(crate) fn http_client(timeout: Duration) -> Result<reqwest::Client, String> {
    cache::http_client(timeout, TOKEN_REQUEST_TIMEOUT)
}

pub(crate) fn speech_request_error(provider: &str, err: reqwest::Error) -> String {
    if err.is_timeout() {
        format!("{provider}の応答がタイムアウトしました。短く録音するか、別の音声認識モデルを試してください。")
    } else {
        err.to_string()
    }
}

pub fn apple_speech_status(
    app: &tauri::AppHandle,
    request_authorization: bool,
) -> Result<AppleSpeechStatus, String> {
    match run_apple_speech_helper(
        app,
        &[
            "status".to_string(),
            "ja-JP".to_string(),
            if request_authorization {
                "--request-authorization".to_string()
            } else {
                "--no-request-authorization".to_string()
            },
        ],
        APPLE_SPEECH_REQUEST_TIMEOUT,
    ) {
        Ok(response) => Ok(apple_status_from_helper(response)),
        Err(err) => Ok(AppleSpeechStatus {
            helper_available: false,
            supported: false,
            status: "unknown".to_string(),
            authorization: "unknown".to_string(),
            message: "Apple SpeechAnalyzer helperを利用できません。".to_string(),
            details: vec![err],
        }),
    }
}

pub fn install_apple_speech_model(app: &tauri::AppHandle) -> Result<AppleSpeechStatus, String> {
    let response = run_apple_speech_helper(
        app,
        &["install".to_string(), "ja-JP".to_string()],
        APPLE_SPEECH_INSTALL_TIMEOUT,
    )?;
    Ok(apple_status_from_helper(response))
}

pub(crate) fn apple_status_from_helper(response: AppleSpeechHelperResponse) -> AppleSpeechStatus {
    let status = response.status.unwrap_or_else(|| "unknown".to_string());
    let authorization = response
        .authorization
        .unwrap_or_else(|| "unknown".to_string());
    let supported = response.supported.unwrap_or(status != "unsupported");
    let reason = response.reason.or(response.error);
    let mut details = response.details.unwrap_or_default();
    if let Some(reason) = reason.as_ref() {
        if !reason.trim().is_empty() {
            details.insert(0, reason.clone());
        }
    }
    let message = if response.ok {
        match (supported, status.as_str(), authorization.as_str()) {
            (false, _, _) => "このMacではApple SpeechAnalyzerを利用できません。".to_string(),
            (_, "installed", "authorized") => {
                "Apple SpeechAnalyzer日本語モデルは利用可能です。".to_string()
            }
            (_, "installed", "notDetermined") => {
                "Apple SpeechAnalyzer日本語モデルはインストール済みです。音声認識権限の確認が必要です。"
                    .to_string()
            }
            (_, "installed", "denied" | "restricted") => {
                "音声認識権限が許可されていません。macOSの設定で許可してください。".to_string()
            }
            (_, "downloading", _) => {
                "Apple SpeechAnalyzer日本語モデルをインストール中です。".to_string()
            }
            (_, "supported", _) => {
                "Apple SpeechAnalyzer日本語モデルは未インストールです。".to_string()
            }
            _ => "Apple SpeechAnalyzerの状態を確認しました。".to_string(),
        }
    } else {
        "Apple SpeechAnalyzerの状態確認に失敗しました。".to_string()
    };

    AppleSpeechStatus {
        helper_available: true,
        supported,
        status,
        authorization,
        message,
        details,
    }
}

pub(crate) fn apple_speech_setup_check(status: &AppleSpeechStatus) -> SpeechSetupCheck {
    let ok = status.helper_available
        && status.supported
        && status.status == "installed"
        && status.authorization == "authorized";
    let mut details = vec![
        format!("モデル状態: {}", status.status),
        format!("音声認識権限: {}", status.authorization),
    ];
    details.extend(status.details.clone());
    SpeechSetupCheck {
        ok,
        message: status.message.clone(),
        details,
    }
}

pub(crate) fn run_apple_speech_helper(
    app: &tauri::AppHandle,
    args: &[String],
    timeout: Duration,
) -> Result<AppleSpeechHelperResponse, String> {
    let helper = resolve_apple_speech_helper(app)?;
    let mut command = std::process::Command::new(&helper);
    command.args(args);
    let output = command_output_with_timeout(
        command,
        timeout,
        &format!("Apple SpeechAnalyzer helper（path: {}）", helper.display()),
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let response = if stdout.is_empty() {
        None
    } else {
        serde_json::from_str::<AppleSpeechHelperResponse>(&stdout).ok()
    };
    if let Some(response) = response {
        if response.ok || response.status.is_some() {
            return Ok(response);
        }
        return Err(response
            .error
            .or(response.reason)
            .unwrap_or_else(|| "Apple SpeechAnalyzer helperが失敗しました。".to_string()));
    }
    let detail = if stderr.is_empty() {
        stdout
    } else if stdout.is_empty() {
        stderr
    } else {
        format!("{stdout}\n{stderr}")
    };
    Err(if detail.trim().is_empty() {
        "Apple SpeechAnalyzer helperからJSON応答が返りませんでした。".to_string()
    } else {
        detail
    })
}

pub(crate) fn resolve_apple_speech_helper(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let executable_name = "enja-speech-helper";
    let target_name = format!("enja-speech-helper-{}", env!("ENJA_TARGET_TRIPLE"));
    let mut candidates = Vec::<PathBuf>::new();
    if let Ok(path) = std::env::var("ENJA_SPEECH_HELPER_PATH") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(executable_name));
        }
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join(executable_name));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bin")
            .join(target_name),
    );

    for path in &candidates {
        if path.is_file() {
            return Ok(path.clone());
        }
    }
    Err(format!(
        "Apple SpeechAnalyzer helperが見つかりません。探した場所: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

pub(crate) async fn transcribe_apple_speech(
    app: &tauri::AppHandle,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    let wav_path = temp_voice_file_path("apple-speech", "wav");
    let context_path = temp_voice_file_path("apple-speech-context", "json");
    fs::write(&wav_path, &clip.wav).map_err(|e| e.to_string())?;
    let contextual_strings = apple_speech_contextual_strings(entries, screen_context);
    let context = serde_json::json!({
        "contextualStrings": contextual_strings,
    });
    fs::write(&context_path, context.to_string()).map_err(|e| e.to_string())?;

    let args = vec![
        "transcribe".to_string(),
        wav_path.display().to_string(),
        "ja-JP".to_string(),
        context_path.display().to_string(),
    ];
    let result =
        run_apple_speech_helper(app, &args, APPLE_SPEECH_REQUEST_TIMEOUT).and_then(|response| {
            response
                .transcript
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    response.error.or(response.reason).unwrap_or_else(|| {
                        "Apple SpeechAnalyzerの文字起こし結果が空でした。".to_string()
                    })
                })
        });

    let _ = fs::remove_file(&wav_path);
    let _ = fs::remove_file(&context_path);
    result
}

pub(crate) fn apple_speech_contextual_strings(
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut values = Vec::<String>::new();
    for entry in entries.iter().filter(|entry| entry.enabled) {
        let value = entry.preferred.trim();
        if value.is_empty() || value.chars().count() > 40 {
            continue;
        }
        let key = value.to_lowercase();
        if seen.insert(key) {
            values.push(value.to_string());
            if values.len() >= APPLE_SPEECH_CONTEXTUAL_STRINGS_MAX {
                return values;
            }
        }
    }
    for value in screen_context_terms(screen_context) {
        if values.len() >= APPLE_SPEECH_CONTEXTUAL_STRINGS_MAX {
            break;
        }
        let key = value.to_lowercase();
        if seen.insert(key) {
            values.push(value);
        }
    }
    values
}

pub(crate) fn temp_voice_file_path(label: &str, extension: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "enja-{label}-{}-{nonce}.{extension}",
        std::process::id()
    ))
}

pub(crate) async fn transcribe_google_chirp3(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    if clip.duration_secs > 60.0 || clip.wav.len() > 10 * 1024 * 1024 {
        return Err(
            "Google Chirp 3の同期認識は1分/10MBまでです。長い録音はOpenAIまたはGeminiへ自動フォールバックします。"
                .to_string(),
        );
    }
    let project = settings.voice.google_cloud_project_id.trim();
    if project.is_empty() {
        return Err("Google Cloud Project IDを設定してください。".to_string());
    }
    let token = google_access_token(settings).await?;
    let phrases = transcription_contextual_phrases(entries, screen_context, 1000);
    // chirp_3 は最大1,000フレーズの適応辞書に対応。高い boost は false positive も
    // 増やすため、既定は中程度に留める。
    let phrase_values = phrases
        .iter()
        .take(1000)
        .map(|value| {
            serde_json::json!({
                "value": value,
                "boost": GOOGLE_SPEECH_DICTIONARY_BOOST,
            })
        })
        .collect::<Vec<_>>();
    let mut config = serde_json::json!({
        "autoDecodingConfig": {},
        "languageCodes": ["ja-JP"],
        "model": "chirp_3",
        "features": {
            "enableAutomaticPunctuation": true
        }
    });
    if !phrase_values.is_empty() {
        config["adaptation"] = serde_json::json!({
            "phraseSets": [{
                "inlinePhraseSet": {
                    "phrases": phrase_values
                }
            }]
        });
    }
    let body = serde_json::json!({
        "config": config,
        "content": base64::engine::general_purpose::STANDARD.encode(&clip.wav)
    });
    let region = settings.voice.google_cloud_region.trim();
    let url = format!(
        "https://{region}-speech.googleapis.com/v2/projects/{project}/locations/{region}/recognizers/_:recognize"
    );
    let response = http_client(SPEECH_REQUEST_TIMEOUT)?
        .post(url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| speech_request_error("Google Speech-to-Text", e))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Google Speech-to-Text HTTP {status}: {text}"));
    }
    if let Err(err) = usage::record_google_speech_to_text(app, clip.duration_secs) {
        eprintln!("[enja] usage tracking failed: {err}");
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let out = v
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|result| {
            result
                .get("alternatives")
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
                .and_then(|a| a.get("transcript"))
                .and_then(|t| t.as_str())
        })
        .collect::<Vec<_>>()
        .join("\n");
    if out.trim().is_empty() {
        Err("文字起こし結果が空でした。".to_string())
    } else {
        Ok(out)
    }
}

/// Google ASR(バッチ Chirp3 またはライブ)を使う見込みのとき、確定時に
/// 直列で発生するアクセストークン取得を録音中に先回りして温める。
/// 取得結果は cache 側が保持するため、ここでは結果を捨ててよい。
pub(crate) fn prefetch_google_speech_token(settings: &AppSettings, mode: VoiceMode) {
    let uses_google_batch = settings.voice.speech_profile == SpeechProfile::GoogleChirp3;
    let uses_google_live = matches!(
        live_transcription_provider_for_settings(settings, mode),
        Some(LiveTranscriptionProvider::GoogleChirp3)
    );
    if !uses_google_batch && !uses_google_live {
        return;
    }
    let settings = settings.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = google_access_token(&settings).await {
            eprintln!("[enja] Googleトークンの先読みに失敗: {err}");
        }
    });
}

pub(crate) async fn google_access_token(settings: &AppSettings) -> Result<String, String> {
    google_access_token_with_details(settings)
        .await
        .map(|(token, _details)| token)
}

pub(crate) async fn google_access_token_with_details(
    settings: &AppSettings,
) -> Result<(String, Vec<String>), String> {
    if settings.voice.google_cloud_use_adc {
        let cache_key = "adc".to_string();
        if let Some(cached) = cache::cached_google_token(&cache_key) {
            return Ok(cached);
        }
        let gcloud = resolve_gcloud_path()?;
        let mut command = std::process::Command::new(&gcloud);
        command.args(["auth", "application-default", "print-access-token"]);
        let output = command_output_with_timeout(
            command,
            TOKEN_REQUEST_TIMEOUT,
            &format!("gcloud（path: {}）", gcloud.display()),
        )?;
        if output.status.success() {
            let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !token.is_empty() {
                let details = vec![
                    "認証方式: ADC".to_string(),
                    format!("gcloud: {}", gcloud.display()),
                ];
                return Ok(cache::store_google_token(cache_key, token, details));
            }
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!(
                "gcloudからアクセストークンが返りませんでした。ターミナルで `{} auth application-default login` を実行してください。",
                gcloud.display()
            )
        } else {
            stderr
        });
    }

    #[derive(Deserialize)]
    struct ServiceAccount {
        client_email: String,
        private_key: String,
        token_uri: String,
    }
    #[derive(Serialize)]
    struct Claims<'a> {
        iss: &'a str,
        scope: &'a str,
        aud: &'a str,
        exp: usize,
        iat: usize,
    }
    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let secret = secrets::get_secret("googleServiceAccount")
        .map_err(|_| "Google CloudサービスアカウントJSONを保存してください。".to_string())?;
    let cache_key = format!("service:{}", cache::hash_cache_key(&secret));
    if let Some(cached) = cache::cached_google_token(&cache_key) {
        return Ok(cached);
    }
    let account: ServiceAccount = serde_json::from_str(&secret).map_err(|e| e.to_string())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as usize;
    let claims = Claims {
        iss: &account.client_email,
        scope: "https://www.googleapis.com/auth/cloud-platform",
        aud: &account.token_uri,
        exp: now + 3600,
        iat: now,
    };
    let assertion = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_rsa_pem(account.private_key.as_bytes())
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let response = http_client(TOKEN_REQUEST_TIMEOUT)?
        .post(account.token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", assertion.as_str()),
        ])
        .send()
        .await
        .map_err(|e| speech_request_error("Google OAuth", e))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Google OAuth HTTP {status}: {text}"));
    }
    let token: TokenResponse = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(cache::store_google_token(
        cache_key,
        token.access_token,
        vec!["認証方式: サービスアカウントJSON".to_string()],
    ))
}

pub(crate) fn command_output_with_timeout(
    mut command: std::process::Command,
    timeout: Duration,
    label: &str,
) -> Result<std::process::Output, String> {
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("{label}を実行できませんでした: {e}"))?;
    let start = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|e| format!("{label}の出力を取得できませんでした: {e}"));
            }
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{label}がタイムアウトしました。"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("{label}の終了状態を確認できませんでした: {e}")),
        }
    }
}

pub(crate) fn resolve_gcloud_path() -> Result<PathBuf, String> {
    let mut searched = Vec::<String>::new();
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let path = dir.join("gcloud");
            searched.push(path.display().to_string());
            if path.exists() {
                return Ok(path);
            }
        }
    }

    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/gcloud"),
        PathBuf::from("/usr/local/bin/gcloud"),
        PathBuf::from("/opt/google-cloud-sdk/bin/gcloud"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join("google-cloud-sdk/bin/gcloud"));
        candidates.push(home.join("Downloads/google-cloud-sdk/bin/gcloud"));
    }
    if let Some(root) = std::env::var_os("CLOUDSDK_ROOT_DIR") {
        candidates.push(PathBuf::from(root).join("bin/gcloud"));
    }

    for path in candidates {
        searched.push(path.display().to_string());
        if path.exists() {
            return Ok(path);
        }
    }

    let mut command = std::process::Command::new("/bin/zsh");
    command.args(["-lc", "command -v gcloud"]);
    if let Ok(output) = command_output_with_timeout(command, Duration::from_secs(3), "gcloud検索")
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                let path = PathBuf::from(path);
                searched.push(path.display().to_string());
                if path.exists() {
                    return Ok(path);
                }
            }
        }
    }

    Err(format!(
        "gcloudが見つかりません。ターミナルではログイン済みでも、Spotlight/Dockから起動したEnjaではPATHが異なることがあります。Google Cloud SDKをHomebrewなど通常の場所に入れるか、ADCをオフにしてサービスアカウントJSONを保存してください。探した場所: {}",
        searched.join(", ")
    ))
}

pub(crate) async fn transcribe_openai(
    app: &tauri::AppHandle,
    model: &str,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    let key = secrets::get_secret("openai")
        .map_err(|_| "OpenAI APIキーを保存してください。".to_string())?;
    let dictionary_context = transcription_prompt_context(entries, screen_context);
    let prompt =
        prompts::openai_transcription_prompt(&settings.prompts.overrides, &dictionary_context);
    let file = reqwest::multipart::Part::bytes(clip.wav.clone())
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new()
        .part("file", file)
        .text("model", model.to_string())
        .text("language", "ja")
        .text("response_format", "json")
        .text("prompt", prompt);
    let response = http_client(SPEECH_REQUEST_TIMEOUT)?
        .post("https://api.openai.com/v1/audio/transcriptions")
        .bearer_auth(key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| speech_request_error("OpenAI", e))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("OpenAI HTTP {status}: {text}"));
    }
    if let Err(err) = usage::record_openai_transcription(app, model, clip.duration_secs) {
        eprintln!("[enja] usage tracking failed: {err}");
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let out = v
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if out.is_empty() {
        Err("OpenAIの文字起こし結果が空でした。".to_string())
    } else {
        Ok(out)
    }
}

pub(crate) async fn transcribe_gemini_audio(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    clip: &AudioClip,
) -> Result<String, String> {
    let key = gemini_api_key(app)?;
    let dictionary_context = transcription_prompt_context(entries, screen_context);
    let prompt = prompts::gemini_audio_user(&settings.prompts.overrides, &dictionary_context);
    let system = prompts::gemini_audio_system(&settings.prompts.overrides);
    let model = settings.voice.finalization_model.model_id();
    let output = gemini::generate_from_audio_with_usage(
        &key,
        model,
        settings.voice.finalization_model.thinking_level(),
        system.as_ref(),
        &prompt,
        &clip.wav,
        0.1,
    )
    .await?;
    if let Err(err) =
        usage::record_gemini_usage(app, UsageService::GeminiAudioInput, model, output.usage)
    {
        eprintln!("[enja] usage tracking failed: {err}");
    }
    Ok(output.text)
}

pub(crate) async fn finalize_text(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    entries: &[DictionaryEntry],
    mode: VoiceMode,
    mode_profile_id: &str,
    selected_text: &str,
    screen_context: &VoiceScreenContext,
    transcript: &str,
) -> Result<String, String> {
    let dictation_profile = if mode == VoiceMode::Dictation {
        Some(
            settings
                .voice
                .mode_profile_or_default(mode_profile_id)
                .ok_or_else(|| "音声モードが見つかりません。".to_string())?,
        )
    } else {
        None
    };
    if dictation_profile.is_some_and(|profile| !profile.formatting_enabled) {
        return Ok(dictionary::apply_transcript_corrections(
            transcript.trim(),
            entries,
        ));
    }

    let key = gemini_api_key(app)?;
    let dictionary_context = dictionary::prompt_lines(entries);
    let dictionary_section = if dictionary_context.trim().is_empty() {
        "優先表記辞書は空です。".to_string()
    } else {
        format!("優先表記辞書（該当語だと判断できる場合のみ使用）:\n{dictionary_context}")
    };
    let screen_context_section = finalization_screen_context_section(screen_context);
    let (system, user) = match mode {
        VoiceMode::Dictation => {
            let profile = dictation_profile.expect("dictation profile");
            (
                profile.system_prompt.clone(),
                prompts::voice_mode_user_with_context(
                    &profile.user_prompt,
                    &dictionary_section,
                    &screen_context_section,
                    transcript,
                ),
            )
        }
        VoiceMode::Ask if selected_text.trim().is_empty() => (
            prompts::ask_without_selection_system(&settings.prompts.overrides).to_string(),
            prompts::ask_without_selection_user(
                &settings.prompts.overrides,
                &dictionary_section,
                &screen_context_section,
                transcript,
            ),
        ),
        VoiceMode::Ask => (
            prompts::ask_with_selection_system(&settings.prompts.overrides).to_string(),
            prompts::ask_with_selection_user(
                &settings.prompts.overrides,
                &dictionary_section,
                &screen_context_section,
                selected_text,
                transcript,
            ),
        ),
    };
    let model = settings.voice.finalization_model.model_id();
    let output = gemini::generate_text_with_usage(
        &key,
        model,
        settings.voice.finalization_model.thinking_level(),
        &system,
        &user,
        0.2,
    )
    .await?;
    if let Err(err) =
        usage::record_gemini_usage(app, UsageService::GeminiFinalization, model, output.usage)
    {
        eprintln!("[enja] usage tracking failed: {err}");
    }
    Ok(output.text.trim().to_string())
}

pub(crate) fn gemini_api_key(_app: &tauri::AppHandle) -> Result<String, String> {
    if let Ok(key) = secrets::get_secret("gemini") {
        if !key.trim().is_empty() {
            return Ok(key);
        }
    }
    Err("Gemini APIキーを保存してください。".to_string())
}
