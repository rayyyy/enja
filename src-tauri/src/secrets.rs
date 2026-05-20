use serde::Serialize;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

const SERVICE: &str = "com.aimhack.enja";
static SECRET_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub gemini: bool,
    pub openai: bool,
    pub deepgram: bool,
    pub google_service_account: bool,
}

pub fn provider_status() -> ProviderStatus {
    ProviderStatus {
        gemini: get_secret("gemini").is_ok_and(|s| !s.trim().is_empty()),
        openai: get_secret("openai").is_ok_and(|s| !s.trim().is_empty()),
        deepgram: get_secret("deepgram").is_ok_and(|s| !s.trim().is_empty()),
        google_service_account: get_secret("googleServiceAccount")
            .is_ok_and(|s| !s.trim().is_empty()),
    }
}

pub fn save_secret(provider: &str, secret: &str) -> Result<(), String> {
    let account = account_name(provider)?;
    let _ = Command::new("security")
        .args(["delete-generic-password", "-a", &account, "-s", SERVICE])
        .output();

    if secret.trim().is_empty() {
        update_cache(&account, None);
        return Ok(());
    }

    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-a",
            &account,
            "-s",
            SERVICE,
            "-w",
            secret,
        ])
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        update_cache(&account, Some(secret.to_string()));
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn get_secret(provider: &str) -> Result<String, String> {
    let account = account_name(provider)?;
    if let Some(value) = cached_secret(&account) {
        return Ok(value);
    }
    let output = Command::new("security")
        .args(["find-generic-password", "-w", "-a", &account, "-s", SERVICE])
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string();
        update_cache(&account, Some(value.clone()));
        Ok(value)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn cached_secret(account: &str) -> Option<String> {
    SECRET_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|cache| cache.get(account).cloned())
}

fn update_cache(account: &str, value: Option<String>) {
    if let Ok(mut cache) = SECRET_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        match value {
            Some(value) => {
                cache.insert(account.to_string(), value);
            }
            None => {
                cache.remove(account);
            }
        }
    }
}

fn account_name(provider: &str) -> Result<String, String> {
    match provider {
        "gemini" | "openai" | "deepgram" | "googleServiceAccount" => {
            Ok(format!("{SERVICE}.{provider}"))
        }
        _ => Err("未知のプロバイダーです。".to_string()),
    }
}
