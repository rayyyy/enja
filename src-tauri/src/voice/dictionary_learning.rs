//! 貼り付け後のユーザー修正を監視して辞書に学習させる仕組み。

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) const DICTIONARY_LEARNING_POLL_INTERVAL_MS: u64 = 250;

pub(crate) const DICTIONARY_LEARNING_QUIET_MS: u64 = 2_000;

pub(crate) const DICTIONARY_LEARNING_MAX_WATCH_MS: u64 = 15_000;

pub(crate) const MIN_LEARNED_CORRECTION_CHARS: usize = 2;

pub(crate) const MAX_LEARNED_CORRECTION_CHARS: usize = 40;

pub(crate) const MIN_FULL_INSERT_REWRITE_CHARS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DictionaryLearningQuiescence {
    pub(crate) baseline_value: String,
    pub(crate) last_value: String,
    pub(crate) stable_ms: u64,
    pub(crate) elapsed_ms: u64,
    pub(crate) last_ready_value: Option<String>,
}

impl DictionaryLearningQuiescence {
    fn new(baseline_value: &str) -> Self {
        Self {
            baseline_value: baseline_value.to_string(),
            last_value: baseline_value.to_string(),
            stable_ms: 0,
            elapsed_ms: 0,
            last_ready_value: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DictionaryLearningQuiescenceStep {
    Continue,
    Ready,
    Expired,
}

pub(crate) fn advance_dictionary_learning_quiescence(
    state: &mut DictionaryLearningQuiescence,
    current_value: &str,
    poll_ms: u64,
    quiet_ms: u64,
    max_watch_ms: u64,
) -> DictionaryLearningQuiescenceStep {
    state.elapsed_ms = state.elapsed_ms.saturating_add(poll_ms);

    if current_value == state.last_value {
        state.stable_ms = state.stable_ms.saturating_add(poll_ms);
    } else {
        state.last_value = current_value.to_string();
        state.stable_ms = 0;
        state.last_ready_value = None;
    }

    if state.elapsed_ms > max_watch_ms {
        return DictionaryLearningQuiescenceStep::Expired;
    }

    if current_value != state.baseline_value
        && state.stable_ms >= quiet_ms
        && state.last_ready_value.as_deref() != Some(current_value)
    {
        state.last_ready_value = Some(current_value.to_string());
        return DictionaryLearningQuiescenceStep::Ready;
    }

    DictionaryLearningQuiescenceStep::Continue
}

#[cfg(target_os = "macos")]
pub(crate) fn paste_text_with_dictionary_learning(
    app: &tauri::AppHandle,
    text: &str,
    preferred_target: Option<&PasteTargetInfo>,
) -> bool {
    let Some(paste) = perform_verified_clipboard_paste(text, preferred_target) else {
        return false;
    };

    if let VerifiedPasteInsertion::Changed(inserted_range) = paste.insertion {
        start_dictionary_learning_watch(
            app.clone(),
            paste.target,
            paste.after_paste,
            inserted_range,
        );
    }
    true
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn paste_text_with_dictionary_learning(
    _app: &tauri::AppHandle,
    text: &str,
    _preferred_target: Option<&PasteTargetInfo>,
) -> bool {
    paste_text(text, None)
}

#[cfg(target_os = "macos")]
pub(crate) fn start_dictionary_learning_watch(
    app: tauri::AppHandle,
    target: AxFocusedText,
    after_paste: AxTextSnapshot,
    inserted_range: TextRange,
) {
    std::thread::spawn(move || {
        let baseline = after_paste;
        let mut quiescence = DictionaryLearningQuiescence::new(&baseline.value);
        loop {
            std::thread::sleep(Duration::from_millis(DICTIONARY_LEARNING_POLL_INTERVAL_MS));
            let Some(current) = target.element.read_text_snapshot() else {
                return;
            };
            if current.pid != baseline.pid {
                return;
            }
            match advance_dictionary_learning_quiescence(
                &mut quiescence,
                &current.value,
                DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                DICTIONARY_LEARNING_QUIET_MS,
                DICTIONARY_LEARNING_MAX_WATCH_MS,
            ) {
                DictionaryLearningQuiescenceStep::Continue => {}
                DictionaryLearningQuiescenceStep::Expired => return,
                DictionaryLearningQuiescenceStep::Ready => {
                    let Some((from, to)) = learned_correction_from_values(
                        &baseline.value,
                        &current.value,
                        inserted_range,
                    ) else {
                        continue;
                    };
                    match dictionary::upsert_learned_correction(&app, &from, &to) {
                        Ok(Some(learned)) => {
                            show_dictionary_learning_notice(&app, learned);
                        }
                        Ok(None) => {}
                        Err(err) => eprintln!("[enja] dictionary learning failed: {err}"),
                    }
                    return;
                }
            }
        }
    });
}

#[cfg(target_os = "macos")]
pub(crate) fn inserted_range_from_snapshots(
    before: &AxTextSnapshot,
    after: &AxTextSnapshot,
) -> Option<TextRange> {
    let span = changed_span(&before.value, &after.value)?;
    if span.to.is_empty() {
        return None;
    }
    let selection = before.selected_range;
    let changed_replaced_selection =
        span.old_range.overlaps(selection) || span.old_range.location == selection.location;
    if !changed_replaced_selection {
        return None;
    }
    Some(span.new_range)
}

pub(crate) fn learned_correction_from_values(
    baseline: &str,
    current: &str,
    inserted_range: TextRange,
) -> Option<(String, String)> {
    let span = changed_span(baseline, current)?;
    if !span.old_range.overlaps(inserted_range) {
        return None;
    }
    let from = span.from.trim().to_string();
    let to = span.to.trim().to_string();
    if from.is_empty() || to.is_empty() || from == to {
        return None;
    }
    if !is_learnable_correction(&from, &to, span.old_range, inserted_range) {
        return None;
    }
    Some((from, to))
}

pub(crate) fn value_without_placeholder(value: String, placeholder: Option<&str>) -> String {
    let Some(placeholder) = placeholder else {
        return value;
    };
    if !placeholder.trim().is_empty() && value.trim() == placeholder.trim() {
        String::new()
    } else {
        value
    }
}

pub(crate) fn is_learnable_correction(
    from: &str,
    to: &str,
    changed_range: TextRange,
    inserted_range: TextRange,
) -> bool {
    let from_chars = from.chars().count();
    let to_chars = to.chars().count();
    if from_chars < MIN_LEARNED_CORRECTION_CHARS || to_chars < MIN_LEARNED_CORRECTION_CHARS {
        return false;
    }
    if from_chars > MAX_LEARNED_CORRECTION_CHARS || to_chars > MAX_LEARNED_CORRECTION_CHARS {
        return false;
    }
    if from.is_ascii() && to.is_ascii() && from.eq_ignore_ascii_case(to) {
        return false;
    }
    if is_sentence_like_correction_value(from) || is_sentence_like_correction_value(to) {
        return false;
    }
    if covers_most_inserted_range(changed_range, inserted_range)
        && (from_chars >= MIN_FULL_INSERT_REWRITE_CHARS
            || to_chars >= MIN_FULL_INSERT_REWRITE_CHARS)
    {
        return false;
    }
    true
}

pub(crate) fn is_sentence_like_correction_value(value: &str) -> bool {
    let value = value.trim();
    let char_count = value.chars().count();
    if value.chars().any(is_sentence_punctuation) {
        return true;
    }
    if value.split_whitespace().count() > 3 {
        return true;
    }
    if char_count >= 6
        && [
            "です",
            "ます",
            "でした",
            "ました",
            "ですね",
            "ですよ",
            "でしょう",
            "ください",
            "ません",
            "だよ",
            "だね",
        ]
        .iter()
        .any(|ending| value.ends_with(ending))
    {
        return true;
    }
    if char_count >= 6
        && value.chars().any(|ch| matches!(ch, 'を' | 'が' | 'は'))
        && value
            .chars()
            .last()
            .is_some_and(is_japanese_predicate_ending)
    {
        return true;
    }
    false
}

pub(crate) fn is_sentence_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '。' | '、' | '，' | ',' | '！' | '!' | '？' | '?' | '；' | ';' | '：' | ':' | '\n' | '\r'
    )
}

pub(crate) fn is_japanese_predicate_ending(ch: char) -> bool {
    matches!(
        ch,
        'う' | 'く' | 'ぐ' | 'す' | 'つ' | 'ぬ' | 'ぶ' | 'む' | 'る' | 'た' | 'だ' | 'い'
    )
}

pub(crate) fn covers_most_inserted_range(
    changed_range: TextRange,
    inserted_range: TextRange,
) -> bool {
    if inserted_range.length == 0 {
        return false;
    }
    let overlap_start = changed_range.location.max(inserted_range.location);
    let overlap_end = changed_range.end().min(inserted_range.end());
    if overlap_end <= overlap_start {
        return false;
    }
    let overlap = overlap_end - overlap_start;
    overlap * 100 >= inserted_range.length * 80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_learning_quiescence_waits_while_value_changes() {
        let mut state = DictionaryLearningQuiescence::new("タイプレス");

        for value in ["t", "ty", "typ", "type", "typel"] {
            assert_eq!(
                advance_dictionary_learning_quiescence(
                    &mut state,
                    value,
                    DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                    DICTIONARY_LEARNING_QUIET_MS,
                    DICTIONARY_LEARNING_MAX_WATCH_MS,
                ),
                DictionaryLearningQuiescenceStep::Continue
            );
        }
    }

    #[test]
    fn dictionary_learning_quiescence_requires_quiet_period() {
        let mut state = DictionaryLearningQuiescence::new("タイプレス");
        let stable_polls = DICTIONARY_LEARNING_QUIET_MS / DICTIONARY_LEARNING_POLL_INTERVAL_MS;

        for _ in 0..stable_polls {
            assert_eq!(
                advance_dictionary_learning_quiescence(
                    &mut state,
                    "Typeless",
                    DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                    DICTIONARY_LEARNING_QUIET_MS,
                    DICTIONARY_LEARNING_MAX_WATCH_MS,
                ),
                DictionaryLearningQuiescenceStep::Continue
            );
        }
        assert_eq!(
            advance_dictionary_learning_quiescence(
                &mut state,
                "Typeless",
                DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                DICTIONARY_LEARNING_QUIET_MS,
                DICTIONARY_LEARNING_MAX_WATCH_MS,
            ),
            DictionaryLearningQuiescenceStep::Ready
        );
    }

    #[test]
    fn dictionary_learning_quiescence_ignores_baseline_value() {
        let mut state = DictionaryLearningQuiescence::new("タイプレス");

        for _ in 0..10 {
            assert_eq!(
                advance_dictionary_learning_quiescence(
                    &mut state,
                    "タイプレス",
                    DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                    DICTIONARY_LEARNING_QUIET_MS,
                    DICTIONARY_LEARNING_MAX_WATCH_MS,
                ),
                DictionaryLearningQuiescenceStep::Continue
            );
        }
    }

    #[test]
    fn dictionary_learning_quiescence_can_recover_after_ineligible_candidate() {
        let mut state = DictionaryLearningQuiescence::new("タイプレス");
        let stable_polls = DICTIONARY_LEARNING_QUIET_MS / DICTIONARY_LEARNING_POLL_INTERVAL_MS;

        for _ in 0..stable_polls {
            assert_eq!(
                advance_dictionary_learning_quiescence(
                    &mut state,
                    "T",
                    DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                    DICTIONARY_LEARNING_QUIET_MS,
                    DICTIONARY_LEARNING_MAX_WATCH_MS,
                ),
                DictionaryLearningQuiescenceStep::Continue
            );
        }
        assert_eq!(
            advance_dictionary_learning_quiescence(
                &mut state,
                "T",
                DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                DICTIONARY_LEARNING_QUIET_MS,
                DICTIONARY_LEARNING_MAX_WATCH_MS,
            ),
            DictionaryLearningQuiescenceStep::Ready
        );
        assert_eq!(
            advance_dictionary_learning_quiescence(
                &mut state,
                "T",
                DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                DICTIONARY_LEARNING_QUIET_MS,
                DICTIONARY_LEARNING_MAX_WATCH_MS,
            ),
            DictionaryLearningQuiescenceStep::Continue
        );

        for _ in 0..stable_polls {
            assert_eq!(
                advance_dictionary_learning_quiescence(
                    &mut state,
                    "Typeless",
                    DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                    DICTIONARY_LEARNING_QUIET_MS,
                    DICTIONARY_LEARNING_MAX_WATCH_MS,
                ),
                DictionaryLearningQuiescenceStep::Continue
            );
        }
        assert_eq!(
            advance_dictionary_learning_quiescence(
                &mut state,
                "Typeless",
                DICTIONARY_LEARNING_POLL_INTERVAL_MS,
                DICTIONARY_LEARNING_QUIET_MS,
                DICTIONARY_LEARNING_MAX_WATCH_MS,
            ),
            DictionaryLearningQuiescenceStep::Ready
        );
    }

    #[test]
    fn learned_correction_uses_changes_inside_inserted_range() {
        let inserted_range = TextRange {
            location: 3,
            length: "タイプレスを使う".encode_utf16().count(),
        };

        let correction = learned_correction_from_values(
            "今日はタイプレスを使う",
            "今日はTypelessを使う",
            inserted_range,
        )
        .expect("correction");

        assert_eq!(
            correction,
            ("タイプレス".to_string(), "Typeless".to_string())
        );
    }

    #[test]
    fn learned_correction_ignores_changes_outside_inserted_range() {
        let inserted_range = TextRange {
            location: 3,
            length: "タイプレス".encode_utf16().count(),
        };

        let correction = learned_correction_from_values(
            "今日はタイプレス。明日も。",
            "今日はタイプレス。昨日も。",
            inserted_range,
        );

        assert!(correction.is_none());
    }

    #[test]
    fn learned_correction_ignores_single_character_edits() {
        let inserted_range = TextRange {
            location: 0,
            length: "hello".encode_utf16().count(),
        };

        let correction = learned_correction_from_values("hello", "Hello", inserted_range);

        assert!(correction.is_none());
    }

    #[test]
    fn learned_correction_accepts_multi_character_terms() {
        let inserted_range = TextRange {
            location: 0,
            length: "タイプレス".encode_utf16().count(),
        };

        let correction = learned_correction_from_values("タイプレス", "Typeless", inserted_range)
            .expect("correction");

        assert_eq!(
            correction,
            ("タイプレス".to_string(), "Typeless".to_string())
        );
    }

    #[test]
    fn learned_correction_ignores_sentence_like_values() {
        let inserted = "皆さん、ご飯が美味しいですね！";
        let inserted_range = TextRange {
            location: 0,
            length: inserted.encode_utf16().count(),
        };

        let correction = learned_correction_from_values(
            inserted,
            "フォローアップの変更を求める",
            inserted_range,
        );

        assert!(correction.is_none());
    }

    #[test]
    fn placeholder_value_is_treated_as_empty_text() {
        assert_eq!(
            value_without_placeholder(
                "フォローアップの変更を求める".to_string(),
                Some("フォローアップの変更を求める"),
            ),
            ""
        );
    }

    #[test]
    fn learned_correction_ignores_deleted_text_when_placeholder_is_exposed() {
        let inserted = "皆さん、ご飯が美味しいですね！";
        let inserted_range = TextRange {
            location: 0,
            length: inserted.encode_utf16().count(),
        };
        let current = value_without_placeholder(
            "フォローアップの変更を求める".to_string(),
            Some("フォローアップの変更を求める"),
        );

        let correction = learned_correction_from_values(inserted, &current, inserted_range);

        assert!(correction.is_none());
    }

    #[test]
    fn learned_correction_ignores_long_full_insert_rewrites() {
        let inserted = "ご飯が美味しいですね";
        let inserted_range = TextRange {
            location: 0,
            length: inserted.encode_utf16().count(),
        };

        let correction =
            learned_correction_from_values(inserted, "ランチが最高ですね", inserted_range);

        assert!(correction.is_none());
    }

    #[test]
    fn learned_correction_allows_sentence_local_term_change() {
        let inserted_range = TextRange {
            location: 0,
            length: "今日はタイプレスを使います".encode_utf16().count(),
        };

        let correction = learned_correction_from_values(
            "今日はタイプレスを使います",
            "今日はTypelessを使います",
            inserted_range,
        )
        .expect("correction");

        assert_eq!(
            correction,
            ("タイプレス".to_string(), "Typeless".to_string())
        );
    }
}
