use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;
use tauri::{AppHandle, Manager};

static DICTIONARY_CACHE: OnceLock<Mutex<Option<DictionaryCache>>> = OnceLock::new();

#[derive(Clone)]
struct DictionaryCache {
    path: PathBuf,
    modified: Option<SystemTime>,
    entries: Vec<DictionaryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub id: String,
    pub preferred: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readings: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub corrections: Vec<DictionaryCorrection>,
    pub enabled: bool,
    pub source: DictionarySource,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryCorrection {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DictionarySource {
    Manual,
    Learned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedCorrection {
    pub entry_id: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntryInput {
    pub preferred: String,
    #[serde(default)]
    pub aliases: Option<Vec<String>>,
    #[serde(default)]
    pub corrections: Option<Vec<DictionaryCorrection>>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

/// 改行区切りなどでまとめて登録した結果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkCreateResult {
    pub added: Vec<DictionaryEntry>,
    pub skipped: usize,
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
    let modified = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok();
    if let Some(entries) = cached_dictionary(&path, modified) {
        return Ok(entries);
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let entries: Vec<DictionaryEntry> = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    let entries = entries
        .into_iter()
        .map(normalize_entry)
        .filter(|e| !e.preferred.is_empty())
        .collect::<Vec<_>>();
    update_dictionary_cache(path, modified, entries.clone());
    Ok(entries)
}

pub fn save_dictionary(app: &AppHandle, entries: &[DictionaryEntry]) -> Result<(), String> {
    let path = dictionary_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let modified = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok();
    update_dictionary_cache(path, modified, entries.to_vec());
    Ok(())
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
        readings: Vec::new(),
        aliases: input.aliases.unwrap_or_default(),
        corrections: input.corrections.unwrap_or_default(),
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

/// 複数の単語をまとめて追加する。空文字・既存と重複する優先表記・バッチ内の重複・
/// バリデーション違反（100文字超など）はスキップし、追加できたものだけを返す。
/// `skipped` は理由を区別しないため、文字数超過などの妥当性チェックは UI 側で
/// 事前に弾く想定（[`DictionaryView`] 参照）。
pub fn create_entries(
    app: &AppHandle,
    inputs: Vec<DictionaryEntryInput>,
) -> Result<BulkCreateResult, String> {
    let mut entries = load_dictionary(app)?;
    let mut seen = entries
        .iter()
        .map(|e| e.preferred.clone())
        .collect::<HashSet<_>>();
    let now = now_millis();
    let mut added = Vec::new();
    let mut skipped = 0usize;
    for (idx, input) in inputs.into_iter().enumerate() {
        let preferred = input.preferred.trim().to_string();
        if preferred.is_empty() || seen.contains(&preferred) {
            skipped += 1;
            continue;
        }
        let entry = normalize_entry(DictionaryEntry {
            id: format!("dict-{now}-{idx}"),
            preferred,
            readings: Vec::new(),
            aliases: input.aliases.unwrap_or_default(),
            corrections: input.corrections.unwrap_or_default(),
            enabled: input.enabled,
            source: DictionarySource::Manual,
            created_at: now,
            updated_at: now,
        });
        if validate_entry(&entry).is_err() {
            skipped += 1;
            continue;
        }
        seen.insert(entry.preferred.clone());
        entries.push(entry.clone());
        added.push(entry);
    }
    if !added.is_empty() {
        save_dictionary(app, &entries)?;
    }
    Ok(BulkCreateResult { added, skipped })
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
        readings: entries[idx].readings.clone(),
        aliases: input
            .aliases
            .unwrap_or_else(|| entries[idx].aliases.clone()),
        corrections: input
            .corrections
            .unwrap_or_else(|| entries[idx].corrections.clone()),
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

pub fn upsert_learned_correction(
    app: &AppHandle,
    from: &str,
    to: &str,
) -> Result<Option<LearnedCorrection>, String> {
    let mut entries = load_dictionary(app)?;
    let learned = upsert_learned_correction_in_entries(&mut entries, from, to, now_millis())?;
    if learned.is_some() {
        save_dictionary(app, &entries)?;
    }
    Ok(learned)
}

pub fn undo_learned_correction(
    app: &AppHandle,
    entry_id: &str,
    from: &str,
    to: &str,
) -> Result<bool, String> {
    let mut entries = load_dictionary(app)?;
    let undone = undo_learned_correction_in_entries(&mut entries, entry_id, from, to);
    if undone {
        save_dictionary(app, &entries)?;
    }
    Ok(undone)
}

pub fn enabled_phrases(entries: &[DictionaryEntry]) -> Vec<String> {
    let mut out = Vec::new();
    for entry in entries.iter().filter(|e| e.enabled) {
        push_unique(&mut out, &entry.preferred);
    }
    out
}

fn upsert_learned_correction_in_entries(
    entries: &mut Vec<DictionaryEntry>,
    from: &str,
    to: &str,
    now: u64,
) -> Result<Option<LearnedCorrection>, String> {
    let from = from.trim().to_string();
    let to = to.trim().to_string();
    if from.is_empty() || to.is_empty() || from == to {
        return Ok(None);
    }
    if from.chars().count() > 100 || to.chars().count() > 100 {
        return Ok(None);
    }

    for entry in entries.iter() {
        if entry
            .corrections
            .iter()
            .any(|correction| correction.from == from)
            || entry.aliases.iter().any(|alias| alias == &from)
        {
            return Ok(None);
        }
    }

    let correction = DictionaryCorrection {
        from: from.clone(),
        to: to.clone(),
    };
    if let Some(entry) = entries.iter_mut().find(|entry| entry.preferred == to) {
        entry.corrections.push(correction);
        entry.updated_at = now;
        *entry = normalize_entry(entry.clone());
        validate_entry(entry)?;
        return Ok(Some(LearnedCorrection {
            entry_id: entry.id.clone(),
            from,
            to,
        }));
    }

    let entry_id = format!("learned-{now}");
    let entry = normalize_entry(DictionaryEntry {
        id: entry_id.clone(),
        preferred: to.clone(),
        readings: Vec::new(),
        aliases: Vec::new(),
        corrections: vec![correction],
        enabled: true,
        source: DictionarySource::Learned,
        created_at: now,
        updated_at: now,
    });
    validate_entry(&entry)?;
    entries.push(entry);
    Ok(Some(LearnedCorrection { entry_id, from, to }))
}

fn undo_learned_correction_in_entries(
    entries: &mut Vec<DictionaryEntry>,
    entry_id: &str,
    from: &str,
    to: &str,
) -> bool {
    let Some(idx) = entries.iter().position(|entry| entry.id == entry_id) else {
        return false;
    };

    let before = entries[idx].corrections.len();
    entries[idx]
        .corrections
        .retain(|correction| !(correction.from == from && correction.to == to));
    if entries[idx].corrections.len() == before {
        return false;
    }

    if entries[idx].source == DictionarySource::Learned
        && entries[idx].corrections.is_empty()
        && entries[idx].aliases.is_empty()
    {
        entries.remove(idx);
    } else {
        entries[idx].updated_at = now_millis();
    }
    true
}

pub fn prompt_lines(entries: &[DictionaryEntry]) -> String {
    entries
        .iter()
        .filter(|e| e.enabled)
        .map(|e| {
            let variants = variant_values(e);
            let corrections = e
                .corrections
                .iter()
                .map(|correction| format!("{} -> {}", correction.from, correction.to))
                .collect::<Vec<_>>();
            let mut detail = Vec::new();
            if !variants.is_empty() {
                detail.push(format!("誤認識候補: {}", variants.join(", ")));
            }
            if !corrections.is_empty() {
                detail.push(format!("補正: {}", corrections.join(" / ")));
            }
            if detail.is_empty() {
                format!("- {}", e.preferred)
            } else {
                format!("- {}（{}）", e.preferred, detail.join(" / "))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn apply_transcript_corrections(transcript: &str, entries: &[DictionaryEntry]) -> String {
    let mut out = transcript.to_string();
    for rule in replacement_rules(entries) {
        out = out.replace(&rule.from, &rule.to);
    }
    out
}

fn validate_entry(entry: &DictionaryEntry) -> Result<(), String> {
    if entry.preferred.trim().is_empty() {
        return Err("単語を入力してください。".to_string());
    }
    if entry.preferred.chars().count() > 100 {
        return Err("単語は100文字以内にしてください。".to_string());
    }
    for value in entry.readings.iter().chain(entry.aliases.iter()) {
        if value.chars().count() > 100 {
            return Err("誤認識した表記は100文字以内にしてください。".to_string());
        }
    }
    for correction in &entry.corrections {
        if correction.from.chars().count() > 100 || correction.to.chars().count() > 100 {
            return Err("補正ルールは100文字以内にしてください。".to_string());
        }
    }
    Ok(())
}

fn normalize_entry(mut entry: DictionaryEntry) -> DictionaryEntry {
    entry.preferred = entry.preferred.trim().to_string();
    let readings = std::mem::take(&mut entry.readings);
    let aliases = std::mem::take(&mut entry.aliases);
    entry.aliases = normalize_values(readings.into_iter().chain(aliases).collect());
    entry.corrections = normalize_corrections(entry.corrections);
    entry
}

fn normalize_values(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        push_unique(&mut out, &value);
    }
    out
}

fn normalize_corrections(corrections: Vec<DictionaryCorrection>) -> Vec<DictionaryCorrection> {
    let mut seen = HashSet::<String>::new();
    let mut out = Vec::new();
    for correction in corrections {
        let from = correction.from.trim().to_string();
        let to = correction.to.trim().to_string();
        if from.is_empty() || to.is_empty() || from == to || !seen.insert(from.clone()) {
            continue;
        }
        out.push(DictionaryCorrection { from, to });
    }
    out
}

fn variant_values(entry: &DictionaryEntry) -> Vec<String> {
    let mut variants = Vec::new();
    for value in entry.readings.iter().chain(entry.aliases.iter()) {
        let value = value.trim();
        if !value.is_empty() && value != entry.preferred {
            push_unique(&mut variants, value);
        }
    }
    variants
}

#[derive(Debug, Clone)]
struct ReplacementRule {
    from: String,
    to: String,
    order: usize,
}

fn replacement_rules(entries: &[DictionaryEntry]) -> Vec<ReplacementRule> {
    let mut rules = Vec::<ReplacementRule>::new();
    let mut seen = HashSet::<String>::new();
    for entry in entries.iter().filter(|entry| entry.enabled) {
        for correction in &entry.corrections {
            push_rule(&mut rules, &mut seen, &correction.from, &correction.to);
        }
    }
    rules.sort_by(|a, b| {
        b.from
            .chars()
            .count()
            .cmp(&a.from.chars().count())
            .then_with(|| a.order.cmp(&b.order))
    });
    rules
}

fn push_rule(rules: &mut Vec<ReplacementRule>, seen: &mut HashSet<String>, from: &str, to: &str) {
    let from = from.trim();
    let to = to.trim();
    if from.is_empty() || to.is_empty() || from == to || !seen.insert(from.to_string()) {
        return;
    }
    rules.push(ReplacementRule {
        from: from.to_string(),
        to: to.to_string(),
        order: rules.len(),
    });
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

fn cached_dictionary(path: &Path, modified: Option<SystemTime>) -> Option<Vec<DictionaryEntry>> {
    DICTIONARY_CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|cache| {
            let cache = cache.as_ref()?;
            if cache.path == path && cache.modified == modified {
                Some(cache.entries.clone())
            } else {
                None
            }
        })
}

fn update_dictionary_cache(
    path: PathBuf,
    modified: Option<SystemTime>,
    entries: Vec<DictionaryEntry>,
) {
    if let Ok(mut cache) = DICTIONARY_CACHE.get_or_init(|| Mutex::new(None)).lock() {
        *cache = Some(DictionaryCache {
            path,
            modified,
            entries,
        });
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

    fn entry(id: &str, preferred: &str, enabled: bool) -> DictionaryEntry {
        DictionaryEntry {
            id: id.to_string(),
            preferred: preferred.to_string(),
            readings: Vec::new(),
            aliases: Vec::new(),
            corrections: Vec::new(),
            enabled,
            source: DictionarySource::Manual,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn enabled_phrases_deduplicates_and_skips_disabled() {
        let entries = vec![
            entry("1", "岩佐", true),
            entry("2", "岩佐", true),
            entry("3", "長野", false),
        ];

        assert_eq!(enabled_phrases(&entries), vec!["岩佐".to_string()]);
    }

    #[test]
    fn prompt_lines_lists_enabled_preferred() {
        let entries = vec![entry("1", "岩佐", true), entry("2", "長野", false)];
        assert_eq!(prompt_lines(&entries), "- 岩佐");
    }

    #[test]
    fn prompt_lines_lists_aliases_and_corrections() {
        let mut entry = entry("1", "Typeless", true);
        entry.aliases = vec!["タイプレス".to_string()];
        entry.corrections = vec![DictionaryCorrection {
            from: "タイプですか？".to_string(),
            to: "Typelessか".to_string(),
        }];

        let lines = prompt_lines(&[normalize_entry(entry)]);

        assert!(lines.contains("Typeless"));
        assert!(lines.contains("タイプレス"));
        assert!(lines.contains("タイプですか？ -> Typelessか"));
    }

    #[test]
    fn apply_transcript_corrections_keeps_aliases_as_hints_without_llm() {
        let mut typeless = entry("1", "Typeless", true);
        typeless.aliases = vec!["タイプレス".to_string()];
        let mut aqua = entry("2", "AquaVoice", true);
        aqua.aliases = vec!["アクアボイス".to_string()];

        let out = apply_transcript_corrections(
            "タイプレスかアクアボイスどっちがいいの？",
            &[normalize_entry(typeless), normalize_entry(aqua)],
        );

        assert_eq!(out, "タイプレスかアクアボイスどっちがいいの？");
    }

    #[test]
    fn apply_transcript_corrections_uses_explicit_rules() {
        let mut typeless = entry("1", "Typeless", true);
        typeless.aliases = vec!["タイプです".to_string()];
        typeless.corrections = vec![DictionaryCorrection {
            from: "タイプですか？アクアボイス".to_string(),
            to: "TypelessかAquaVoice".to_string(),
        }];
        let mut aqua = entry("2", "AquaVoice", true);
        aqua.aliases = vec!["アクアボイス".to_string()];

        let out = apply_transcript_corrections(
            "タイプですか？アクアボイスどっちがいいの？",
            &[normalize_entry(typeless), normalize_entry(aqua)],
        );

        assert_eq!(out, "TypelessかAquaVoiceどっちがいいの？");
    }

    #[test]
    fn apply_transcript_corrections_skips_disabled_entries() {
        let mut typeless = entry("1", "Typeless", false);
        typeless.aliases = vec!["タイプレス".to_string()];

        let out = apply_transcript_corrections("タイプレス", &[normalize_entry(typeless)]);

        assert_eq!(out, "タイプレス");
    }

    #[test]
    fn learned_correction_adds_to_existing_preferred_entry() {
        let mut entries = vec![entry("1", "Typeless", true)];

        let learned =
            upsert_learned_correction_in_entries(&mut entries, "タイプレス", "Typeless", 2)
                .expect("upsert");

        assert_eq!(
            learned,
            Some(LearnedCorrection {
                entry_id: "1".to_string(),
                from: "タイプレス".to_string(),
                to: "Typeless".to_string(),
            })
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].corrections,
            vec![DictionaryCorrection {
                from: "タイプレス".to_string(),
                to: "Typeless".to_string(),
            }]
        );
        assert_eq!(entries[0].updated_at, 2);
    }

    #[test]
    fn learned_correction_creates_learned_entry() {
        let mut entries = Vec::new();

        let learned =
            upsert_learned_correction_in_entries(&mut entries, "タイプレス", "Typeless", 2)
                .expect("upsert");

        assert_eq!(
            learned,
            Some(LearnedCorrection {
                entry_id: "learned-2".to_string(),
                from: "タイプレス".to_string(),
                to: "Typeless".to_string(),
            })
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].preferred, "Typeless");
        assert!(matches!(entries[0].source, DictionarySource::Learned));
        assert_eq!(entries[0].corrections[0].from, "タイプレス");
    }

    #[test]
    fn learned_correction_skips_duplicate_from() {
        let mut typeless = entry("1", "Typeless", true);
        typeless.corrections = vec![DictionaryCorrection {
            from: "タイプレス".to_string(),
            to: "Typeless".to_string(),
        }];
        let mut entries = vec![normalize_entry(typeless)];

        let learned =
            upsert_learned_correction_in_entries(&mut entries, "タイプレス", "Typeless", 2)
                .expect("upsert");

        assert_eq!(learned, None);
        assert_eq!(entries[0].corrections.len(), 1);
    }

    #[test]
    fn undo_learned_correction_removes_matching_rule() {
        let mut entries = vec![entry("1", "Typeless", true)];
        entries[0].corrections = vec![DictionaryCorrection {
            from: "タイプレス".to_string(),
            to: "Typeless".to_string(),
        }];

        let undone =
            undo_learned_correction_in_entries(&mut entries, "1", "タイプレス", "Typeless");

        assert!(undone);
        assert!(entries[0].corrections.is_empty());
    }

    #[test]
    fn undo_learned_correction_removes_empty_learned_entry() {
        let mut entries = vec![normalize_entry(DictionaryEntry {
            id: "learned-2".to_string(),
            preferred: "Typeless".to_string(),
            readings: Vec::new(),
            aliases: Vec::new(),
            corrections: vec![DictionaryCorrection {
                from: "タイプレス".to_string(),
                to: "Typeless".to_string(),
            }],
            enabled: true,
            source: DictionarySource::Learned,
            created_at: 2,
            updated_at: 2,
        })];

        let undone =
            undo_learned_correction_in_entries(&mut entries, "learned-2", "タイプレス", "Typeless");

        assert!(undone);
        assert!(entries.is_empty());
    }
}
