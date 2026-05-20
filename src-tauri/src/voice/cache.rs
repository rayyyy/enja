use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static SPEECH_HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static TOKEN_HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static GOOGLE_TOKEN_CACHE: OnceLock<Mutex<Option<CachedGoogleToken>>> = OnceLock::new();

#[derive(Clone)]
struct CachedGoogleToken {
    key: String,
    token: String,
    details: Vec<String>,
    expires_at: Instant,
}

pub(super) fn http_client(
    timeout: Duration,
    token_timeout: Duration,
) -> Result<reqwest::Client, String> {
    let cache = if timeout == token_timeout {
        &TOKEN_HTTP_CLIENT
    } else {
        &SPEECH_HTTP_CLIENT
    };
    if let Some(client) = cache.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())?;
    let _ = cache.set(client.clone());
    Ok(client)
}

pub(super) fn cached_google_token(cache_key: &str) -> Option<(String, Vec<String>)> {
    GOOGLE_TOKEN_CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|cache| {
            let cache = cache.as_ref()?;
            if cache.key == cache_key && Instant::now() < cache.expires_at {
                Some((cache.token.clone(), cache.details.clone()))
            } else {
                None
            }
        })
}

pub(super) fn store_google_token(
    cache_key: String,
    token: String,
    details: Vec<String>,
) -> (String, Vec<String>) {
    if let Ok(mut cache) = GOOGLE_TOKEN_CACHE.get_or_init(|| Mutex::new(None)).lock() {
        *cache = Some(CachedGoogleToken {
            key: cache_key,
            token: token.clone(),
            details: details.clone(),
            expires_at: Instant::now() + Duration::from_secs(50 * 60),
        });
    }
    (token, details)
}

pub(super) fn hash_cache_key(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
