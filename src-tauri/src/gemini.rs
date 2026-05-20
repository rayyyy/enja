use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::ipc::Channel;

use crate::prompts;
use crate::settings::{PromptOverrides, UiLanguage};

const MODEL: &str = "gemini-3.1-flash-lite-preview";
const GEMINI_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
static GEMINI_HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

#[derive(Clone, Serialize)]
#[serde(tag = "type")]
pub enum TranslateEvent {
    #[serde(rename = "chunk")]
    Chunk { text: String },
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContentResponse {
    #[serde(default)]
    candidates: Vec<GenerateCandidate>,
    error: Option<GeminiError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateCandidate {
    content: Option<GenerateContent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContent {
    #[serde(default)]
    parts: Vec<GeneratePart>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeneratePart {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiError {
    message: Option<String>,
}

pub async fn stream_translate(
    api_key: &str,
    user_text: &str,
    channel: Channel<TranslateEvent>,
    source: UiLanguage,
    target: UiLanguage,
    prompt_overrides: &PromptOverrides,
) -> Result<(), String> {
    let prompt = prompts::translation_system_prompt(prompt_overrides, source, target);
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:streamGenerateContent?alt=sse&key={}",
        encode_query_component(api_key)
    );

    let body = serde_json::json!({
        "systemInstruction": {
            "parts": [{ "text": prompt.as_ref() }]
        },
        "contents": [{
            "role": "user",
            "parts": [{ "text": user_text }]
        }],
        "generationConfig": {
            "temperature": 0.3
        }
    });

    let response = http_client()?
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = response.status();
    if !status.is_success() {
        let err_text = response.text().await.unwrap_or_default();
        eprintln!("[enja] Gemini HTTP {status}: {err_text}");
        let _ = channel.send(TranslateEvent::Error {
            message: format!("HTTP {status}: {err_text}"),
        });
        return Ok(());
    }

    let mut stream = response.bytes_stream();
    let mut acc = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&chunk).replace('\r', "");
        acc.push_str(&text);

        while let Some(idx) = acc.find("\n\n") {
            let event_block: String = acc[..idx].to_string();
            acc.drain(..idx + 2);
            if process_sse_block(&event_block, &channel)? {
                return Ok(());
            }
        }
    }

    let _ = channel.send(TranslateEvent::Done);
    Ok(())
}

pub async fn generate_text(
    api_key: &str,
    model: &str,
    thinking_level: &str,
    system_prompt: &str,
    user_text: &str,
    temperature: f32,
) -> Result<String, String> {
    generate_content(
        api_key,
        model,
        thinking_level,
        system_prompt,
        serde_json::json!([{
            "role": "user",
            "parts": [{ "text": user_text }]
        }]),
        temperature,
    )
    .await
}

pub async fn generate_from_audio(
    api_key: &str,
    model: &str,
    thinking_level: &str,
    system_prompt: &str,
    user_text: &str,
    audio_wav: &[u8],
    temperature: f32,
) -> Result<String, String> {
    use base64::Engine;
    let audio = base64::engine::general_purpose::STANDARD.encode(audio_wav);
    generate_content(
        api_key,
        model,
        thinking_level,
        system_prompt,
        serde_json::json!([{
            "role": "user",
            "parts": [
                { "text": user_text },
                {
                    "inlineData": {
                        "mimeType": "audio/wav",
                        "data": audio
                    }
                }
            ]
        }]),
        temperature,
    )
    .await
}

async fn generate_content(
    api_key: &str,
    model: &str,
    thinking_level: &str,
    system_prompt: &str,
    contents: serde_json::Value,
    temperature: f32,
) -> Result<String, String> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        encode_query_component(model),
        encode_query_component(api_key)
    );

    let body = serde_json::json!({
        "systemInstruction": {
            "parts": [{ "text": system_prompt }]
        },
        "contents": contents,
        "generationConfig": {
            "temperature": temperature,
            "thinkingConfig": {
                "thinkingLevel": thinking_level
            }
        }
    });

    let response = http_client()?
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(gemini_request_error)?;

    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Gemini HTTP {status}: {text}"));
    }

    let parsed: GenerateContentResponse = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if let Some(err) = parsed.error {
        return Err(err
            .message
            .unwrap_or_else(|| "Gemini API error".to_string()));
    }
    let out = parsed
        .candidates
        .into_iter()
        .filter_map(|c| c.content)
        .flat_map(|c| c.parts)
        .filter_map(|p| p.text)
        .collect::<Vec<_>>()
        .join("");
    if out.trim().is_empty() {
        Err("Geminiから空の応答が返りました。".to_string())
    } else {
        Ok(out)
    }
}

fn gemini_request_error(err: reqwest::Error) -> String {
    if err.is_timeout() {
        "Geminiの応答がタイムアウトしました。短く録音するか、整形モデルを速いものへ切り替えてください。".to_string()
    } else {
        err.to_string()
    }
}

fn http_client() -> Result<reqwest::Client, String> {
    if let Some(client) = GEMINI_HTTP_CLIENT.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .timeout(GEMINI_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let _ = GEMINI_HTTP_CLIENT.set(client.clone());
    Ok(client)
}

/// Returns `true` if the stream should stop (done or fatal error).
fn process_sse_block(block: &str, channel: &Channel<TranslateEvent>) -> Result<bool, String> {
    for raw in block.lines() {
        let raw = raw.trim();
        if raw.is_empty() || raw.starts_with(':') {
            continue;
        }
        let data = raw.strip_prefix("data:").unwrap_or(raw).trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            let _ = channel.send(TranslateEvent::Done);
            return Ok(true);
        }
        let v: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(err) = v.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Gemini API error");
            let _ = channel.send(TranslateEvent::Error {
                message: msg.to_string(),
            });
            return Ok(true);
        }
        if let Some(text) = extract_text_from_chunk(&v) {
            if !text.is_empty() {
                let _ = channel.send(TranslateEvent::Chunk { text });
            }
        }
    }
    Ok(false)
}

fn extract_text_from_chunk(v: &Value) -> Option<String> {
    let candidates = v.get("candidates")?.as_array()?;
    let first = candidates.first()?;
    let content = first.get("content")?;
    let parts = content.get("parts")?.as_array()?;
    let mut out = String::new();
    for p in parts {
        if let Some(t) = p.get("text").and_then(|x| x.as_str()) {
            out.push_str(t);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn encode_query_component(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}
