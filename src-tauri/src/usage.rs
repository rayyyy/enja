use crate::gemini::GeminiUsage;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const RETENTION_DAYS: u64 = 90;
const RETENTION_MS: u64 = RETENTION_DAYS * 24 * 60 * 60 * 1000;
const PRICING_VERSION: &str = "2026-05-25";
const OPENAI_GPT4O_TRANSCRIBE_USD_PER_MINUTE: f64 = 0.006;
const OPENAI_GPT4O_MINI_TRANSCRIBE_USD_PER_MINUTE: f64 = 0.003;
const GOOGLE_SPEECH_TO_TEXT_V2_USD_PER_MINUTE: f64 = 0.016;
static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static USAGE_FILE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageService {
    GeminiTranslation,
    GeminiFinalization,
    GeminiAudioInput,
    OpenAiTranscription,
    GoogleSpeechToText,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiUsageEvent {
    pub id: String,
    pub timestamp_ms: u64,
    pub service: UsageService,
    pub provider: String,
    pub model: String,
    pub operation: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub audio_input_tokens: Option<u64>,
    pub duration_secs: Option<f64>,
    pub request_count: u64,
    pub estimated_cost_usd: Option<f64>,
    pub pricing_note: String,
    pub note: Option<String>,
}

struct GeminiRates {
    input_usd_per_million_tokens: f64,
    audio_input_usd_per_million_tokens: Option<f64>,
    output_usd_per_million_tokens: f64,
    long_input_usd_per_million_tokens: Option<f64>,
    long_output_usd_per_million_tokens: Option<f64>,
}

pub fn usage_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("usage.json"))
}

pub fn get_usage_events(app: &AppHandle) -> Result<Vec<ApiUsageEvent>, String> {
    let _guard = usage_file_lock().lock().map_err(|e| e.to_string())?;
    let path = usage_path(app)?;
    let mut events = read_usage_events(&path)?;
    let before = events.len();
    prune_events(&mut events);
    events.sort_by_key(|event| Reverse(event.timestamp_ms));
    if events.len() != before {
        write_usage_events(&path, &events)?;
    }
    Ok(events)
}

pub fn record_gemini_usage(
    app: &AppHandle,
    service: UsageService,
    model: &str,
    usage: Option<GeminiUsage>,
) -> Result<(), String> {
    let (estimated_cost_usd, input_tokens, output_tokens, audio_input_tokens, note) = match usage {
        Some(usage) => {
            let estimated = estimate_gemini_cost_usd(model, usage);
            let note = if estimated.is_some() {
                None
            } else {
                Some("usageMetadataは取得できましたが、このモデルの単価または必要なトークン数が未対応です。".to_string())
            };
            (
                estimated,
                usage.prompt_token_count,
                usage.output_token_count(),
                usage.audio_input_token_count,
                note,
            )
        }
        None => (
            None,
            None,
            None,
            None,
            Some(
                "GeminiレスポンスからusageMetadataを取得できなかったため、金額は未算出です。"
                    .to_string(),
            ),
        ),
    };

    append_usage_event(
        app,
        ApiUsageEvent {
            id: next_event_id(),
            timestamp_ms: now_millis(),
            service,
            provider: "Gemini".to_string(),
            model: model.to_string(),
            operation: operation_label(service).to_string(),
            input_tokens,
            output_tokens,
            audio_input_tokens,
            duration_secs: None,
            request_count: 1,
            estimated_cost_usd,
            pricing_note: format!(
                "Gemini Developer API Standard paid rates checked {PRICING_VERSION}; free tier, tax, discounts, and regional differences are not reflected."
            ),
            note,
        },
    )
}

pub fn record_openai_transcription(
    app: &AppHandle,
    model: &str,
    duration_secs: f32,
) -> Result<(), String> {
    let rate = match model {
        "gpt-4o-mini-transcribe" => OPENAI_GPT4O_MINI_TRANSCRIBE_USD_PER_MINUTE,
        _ => OPENAI_GPT4O_TRANSCRIBE_USD_PER_MINUTE,
    };
    let rounded_secs = duration_secs.max(0.0) as f64;
    append_usage_event(
        app,
        ApiUsageEvent {
            id: next_event_id(),
            timestamp_ms: now_millis(),
            service: UsageService::OpenAiTranscription,
            provider: "OpenAI".to_string(),
            model: model.to_string(),
            operation: operation_label(UsageService::OpenAiTranscription).to_string(),
            input_tokens: None,
            output_tokens: None,
            audio_input_tokens: None,
            duration_secs: Some(rounded_secs),
            request_count: 1,
            estimated_cost_usd: Some(rounded_secs / 60.0 * rate),
            pricing_note: format!(
                "OpenAI transcription rates checked {PRICING_VERSION}; free credits, tax, discounts, and billing adjustments are not reflected."
            ),
            note: None,
        },
    )
}

pub fn record_google_speech_to_text(app: &AppHandle, duration_secs: f32) -> Result<(), String> {
    let rounded_secs = (duration_secs.max(0.0) as f64).ceil();
    append_usage_event(
        app,
        ApiUsageEvent {
            id: next_event_id(),
            timestamp_ms: now_millis(),
            service: UsageService::GoogleSpeechToText,
            provider: "Google Cloud".to_string(),
            model: "chirp_3".to_string(),
            operation: operation_label(UsageService::GoogleSpeechToText).to_string(),
            input_tokens: None,
            output_tokens: None,
            audio_input_tokens: None,
            duration_secs: Some(rounded_secs),
            request_count: 1,
            estimated_cost_usd: Some(rounded_secs / 60.0 * GOOGLE_SPEECH_TO_TEXT_V2_USD_PER_MINUTE),
            pricing_note: format!(
                "Google Cloud Speech-to-Text V2 Standard rates checked {PRICING_VERSION}; free tier, tax, discounts, and account-level volume tiers are not reflected."
            ),
            note: Some("Google Cloud Speech-to-Textは成功レスポンス時に1秒単位で概算しています。".to_string()),
        },
    )
}

fn append_usage_event(app: &AppHandle, event: ApiUsageEvent) -> Result<(), String> {
    let _guard = usage_file_lock().lock().map_err(|e| e.to_string())?;
    let path = usage_path(app)?;
    let mut events = read_usage_events(&path)?;
    events.push(event);
    prune_events(&mut events);
    events.sort_by_key(|event| event.timestamp_ms);
    write_usage_events(&path, &events)
}

fn usage_file_lock() -> &'static Mutex<()> {
    USAGE_FILE_LOCK.get_or_init(|| Mutex::new(()))
}

fn read_usage_events(path: &std::path::Path) -> Result<Vec<ApiUsageEvent>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

fn write_usage_events(path: &std::path::Path, events: &[ApiUsageEvent]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(events).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn prune_events(events: &mut Vec<ApiUsageEvent>) {
    let cutoff = now_millis().saturating_sub(RETENTION_MS);
    events.retain(|event| event.timestamp_ms >= cutoff);
}

fn estimate_gemini_cost_usd(model: &str, usage: GeminiUsage) -> Option<f64> {
    let rates = gemini_rates(model)?;
    let prompt_tokens = usage.prompt_token_count?;
    let output_tokens = usage.output_token_count().unwrap_or(0);
    let audio_tokens = usage
        .audio_input_token_count
        .unwrap_or(0)
        .min(prompt_tokens);
    let text_input_tokens = prompt_tokens.saturating_sub(audio_tokens);
    let long_context = prompt_tokens > 200_000;
    let input_rate = if long_context {
        rates
            .long_input_usd_per_million_tokens
            .unwrap_or(rates.input_usd_per_million_tokens)
    } else {
        rates.input_usd_per_million_tokens
    };
    let output_rate = if long_context {
        rates
            .long_output_usd_per_million_tokens
            .unwrap_or(rates.output_usd_per_million_tokens)
    } else {
        rates.output_usd_per_million_tokens
    };
    let audio_input_rate = rates
        .audio_input_usd_per_million_tokens
        .unwrap_or(input_rate);
    Some(
        (text_input_tokens as f64 / 1_000_000.0 * input_rate)
            + (audio_tokens as f64 / 1_000_000.0 * audio_input_rate)
            + (output_tokens as f64 / 1_000_000.0 * output_rate),
    )
}

fn gemini_rates(model: &str) -> Option<GeminiRates> {
    match model {
        "gemini-3.5-flash" => Some(GeminiRates {
            input_usd_per_million_tokens: 1.50,
            audio_input_usd_per_million_tokens: None,
            output_usd_per_million_tokens: 9.00,
            long_input_usd_per_million_tokens: None,
            long_output_usd_per_million_tokens: None,
        }),
        "gemini-3.1-flash-lite" | "gemini-3.1-flash-lite-preview" => Some(GeminiRates {
            input_usd_per_million_tokens: 0.25,
            audio_input_usd_per_million_tokens: Some(0.50),
            output_usd_per_million_tokens: 1.50,
            long_input_usd_per_million_tokens: None,
            long_output_usd_per_million_tokens: None,
        }),
        "gemini-3.1-pro-preview" => Some(GeminiRates {
            input_usd_per_million_tokens: 2.00,
            audio_input_usd_per_million_tokens: None,
            output_usd_per_million_tokens: 12.00,
            long_input_usd_per_million_tokens: Some(4.00),
            long_output_usd_per_million_tokens: Some(18.00),
        }),
        _ => None,
    }
}

fn operation_label(service: UsageService) -> &'static str {
    match service {
        UsageService::GeminiTranslation => "翻訳",
        UsageService::GeminiFinalization => "音声整形",
        UsageService::GeminiAudioInput => "Gemini音声入力",
        UsageService::OpenAiTranscription => "OpenAI文字起こし",
        UsageService::GoogleSpeechToText => "Google Speech-to-Text",
    }
}

fn next_event_id() -> String {
    let seq = EVENT_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    format!("usage-{}-{seq}", now_millis())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_gemini_audio_with_audio_rate() {
        let cost = estimate_gemini_cost_usd(
            "gemini-3.1-flash-lite",
            GeminiUsage {
                prompt_token_count: Some(2_000),
                candidates_token_count: Some(100),
                thoughts_token_count: Some(0),
                total_token_count: Some(2_100),
                audio_input_token_count: Some(1_000),
            },
        )
        .expect("cost");

        assert!((cost - 0.0009).abs() < 0.000001);
    }

    #[test]
    fn prunes_events_older_than_retention_window() {
        let now = now_millis();
        let mut events = vec![
            ApiUsageEvent {
                id: "old".to_string(),
                timestamp_ms: now - RETENTION_MS - 1,
                service: UsageService::GeminiTranslation,
                provider: "Gemini".to_string(),
                model: "gemini-3.1-flash-lite-preview".to_string(),
                operation: "翻訳".to_string(),
                input_tokens: None,
                output_tokens: None,
                audio_input_tokens: None,
                duration_secs: None,
                request_count: 1,
                estimated_cost_usd: None,
                pricing_note: String::new(),
                note: None,
            },
            ApiUsageEvent {
                id: "new".to_string(),
                timestamp_ms: now,
                service: UsageService::GeminiTranslation,
                provider: "Gemini".to_string(),
                model: "gemini-3.1-flash-lite-preview".to_string(),
                operation: "翻訳".to_string(),
                input_tokens: None,
                output_tokens: None,
                audio_input_tokens: None,
                duration_secs: None,
                request_count: 1,
                estimated_cost_usd: None,
                pricing_note: String::new(),
                note: None,
            },
        ];

        prune_events(&mut events);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "new");
    }
}
