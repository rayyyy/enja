use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub id: String,
    pub preferred: String,
    pub readings: Vec<String>,
    pub aliases: Vec<String>,
    pub enabled: bool,
    pub source: DictionarySource,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DictionarySource {
    Manual,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntryInput {
    pub preferred: String,
    #[serde(default)]
    pub readings: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

pub fn dictionary_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("dictionary.json"))
}

pub fn load_dictionary(app: &AppHandle) -> Result<Vec<DictionaryEntry>, String> {
    let path = dictionary_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let entries: Vec<DictionaryEntry> = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    Ok(entries
        .into_iter()
        .map(normalize_entry)
        .filter(|e| !e.preferred.is_empty())
        .collect())
}

pub fn save_dictionary(app: &AppHandle, entries: &[DictionaryEntry]) -> Result<(), String> {
    let path = dictionary_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub fn create_entry(
    app: &AppHandle,
    input: DictionaryEntryInput,
) -> Result<DictionaryEntry, String> {
    let mut entries = load_dictionary(app)?;
    let now = now_millis();
    let entry = normalize_entry(DictionaryEntry {
        id: format!("dict-{now}"),
        preferred: input.preferred,
        readings: input.readings,
        aliases: input.aliases,
        enabled: input.enabled,
        source: DictionarySource::Manual,
        created_at: now,
        updated_at: now,
    });
    validate_entry(&entry)?;
    entries.push(entry.clone());
    save_dictionary(app, &entries)?;
    Ok(entry)
}

pub fn update_entry(
    app: &AppHandle,
    id: &str,
    input: DictionaryEntryInput,
) -> Result<DictionaryEntry, String> {
    let mut entries = load_dictionary(app)?;
    let Some(idx) = entries.iter().position(|e| e.id == id) else {
        return Err("辞書項目が見つかりません。".to_string());
    };
    let created_at = entries[idx].created_at;
    let entry = normalize_entry(DictionaryEntry {
        id: id.to_string(),
        preferred: input.preferred,
        readings: input.readings,
        aliases: input.aliases,
        enabled: input.enabled,
        source: DictionarySource::Manual,
        created_at,
        updated_at: now_millis(),
    });
    validate_entry(&entry)?;
    entries[idx] = entry.clone();
    save_dictionary(app, &entries)?;
    Ok(entry)
}

pub fn delete_entry(app: &AppHandle, id: &str) -> Result<(), String> {
    let mut entries = load_dictionary(app)?;
    let before = entries.len();
    entries.retain(|e| e.id != id);
    if entries.len() == before {
        return Err("辞書項目が見つかりません。".to_string());
    }
    save_dictionary(app, &entries)
}

pub fn enabled_phrases(entries: &[DictionaryEntry]) -> Vec<String> {
    let mut out = Vec::new();
    for entry in entries.iter().filter(|e| e.enabled) {
        push_unique(&mut out, &entry.preferred);
        for value in entry.readings.iter().chain(entry.aliases.iter()) {
            push_unique(&mut out, value);
        }
    }
    out
}

pub fn prompt_lines(entries: &[DictionaryEntry]) -> String {
    entries
        .iter()
        .filter(|e| e.enabled)
        .map(|e| {
            let mut variants = Vec::new();
            for value in e.readings.iter().chain(e.aliases.iter()) {
                if !value.trim().is_empty() {
                    variants.push(value.trim().to_string());
                }
            }
            if variants.is_empty() {
                format!("- {}", e.preferred)
            } else {
                format!("- {}（候補/読み: {}）", e.preferred, variants.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn validate_entry(entry: &DictionaryEntry) -> Result<(), String> {
    if entry.preferred.trim().is_empty() {
        return Err("単語を入力してください。".to_string());
    }
    if entry.preferred.chars().count() > 100 {
        return Err("単語は100文字以内にしてください。".to_string());
    }
    Ok(())
}

fn normalize_entry(mut entry: DictionaryEntry) -> DictionaryEntry {
    entry.preferred = entry.preferred.trim().to_string();
    entry.readings = normalize_values(entry.readings);
    entry.aliases = normalize_values(entry.aliases);
    entry
}

fn normalize_values(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        push_unique(&mut out, &value);
    }
    out
}

fn push_unique(out: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    if !out.iter().any(|existing| existing == value) {
        out.push(value.to_string());
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_phrases_deduplicates_values() {
        let entries = vec![DictionaryEntry {
            id: "1".to_string(),
            preferred: "岩佐".to_string(),
            readings: vec!["いわさ".to_string(), "岩佐".to_string()],
            aliases: vec![" イワサ ".to_string()],
            enabled: true,
            source: DictionarySource::Manual,
            created_at: 1,
            updated_at: 1,
        }];

        assert_eq!(
            enabled_phrases(&entries),
            vec![
                "岩佐".to_string(),
                "いわさ".to_string(),
                "イワサ".to_string()
            ]
        );
    }
}
