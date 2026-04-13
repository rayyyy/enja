use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;

use crate::settings::UiLanguage;

const MODEL: &str = "gemini-3.1-flash-lite-preview";

const SYSTEM_PROMPT_EN_TO_JA: &str = r#"あなたはプロの翻訳家であり、ネイティブスピーカーです。入力された英語のテキストを自然な日本語に翻訳してください。

出力は翻訳文のみとし、見出し・ラベル（「翻訳」など）・前置き・解説・ニュアンス説明・別の表現案・箇条書きは一切出力しないでください。段落が必要な場合は空行で区切ってよい。"#;

const SYSTEM_PROMPT_JA_TO_EN: &str = r#"You are a professional translator and native speaker. Translate the user's Japanese input into natural English.

Output only the translation. Do not output headings, labels (such as "Translation"), preambles, explanations, nuance notes, alternative phrasings, or bullet points. Use a blank line between paragraphs if needed."#;

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

fn system_prompt(source: UiLanguage, target: UiLanguage) -> &'static str {
    match (source, target) {
        (UiLanguage::En, UiLanguage::Ja) => SYSTEM_PROMPT_EN_TO_JA,
        (UiLanguage::Ja, UiLanguage::En) => SYSTEM_PROMPT_JA_TO_EN,
        _ => SYSTEM_PROMPT_EN_TO_JA,
    }
}

pub async fn stream_translate(
    api_key: &str,
    user_text: &str,
    channel: Channel<TranslateEvent>,
    source: UiLanguage,
    target: UiLanguage,
) -> Result<(), String> {
    let prompt = system_prompt(source, target);
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:streamGenerateContent?alt=sse&key={}",
        encode_query_component(api_key)
    );

    let body = serde_json::json!({
        "systemInstruction": {
            "parts": [{ "text": prompt }]
        },
        "contents": [{
            "role": "user",
            "parts": [{ "text": user_text }]
        }],
        "generationConfig": {
            "temperature": 0.3
        }
    });

    let client = reqwest::Client::new();
    let response = client
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
