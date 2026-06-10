//! UTF-16 ベースのテキスト範囲と差分検出。
//! macOS Accessibility API(AXSelectedTextRange など)が UTF-16 オフセットを
//! 使うため、範囲計算はすべて UTF-16 単位で行う。純ロジックのみ。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextRange {
    pub(crate) location: usize,
    pub(crate) length: usize,
}

impl TextRange {
    pub(crate) fn end(self) -> usize {
        self.location.saturating_add(self.length)
    }

    pub(crate) fn overlaps(self, other: TextRange) -> bool {
        self.location < other.end() && other.location < self.end()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangedSpan {
    pub(crate) old_range: TextRange,
    pub(crate) new_range: TextRange,
    pub(crate) from: String,
    pub(crate) to: String,
}

pub(crate) fn changed_span(before: &str, after: &str) -> Option<ChangedSpan> {
    if before == after {
        return None;
    }

    let before_chars = before.chars().collect::<Vec<_>>();
    let after_chars = after.chars().collect::<Vec<_>>();
    let mut prefix = 0usize;
    while prefix < before_chars.len()
        && prefix < after_chars.len()
        && before_chars[prefix] == after_chars[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while suffix + prefix < before_chars.len()
        && suffix + prefix < after_chars.len()
        && before_chars[before_chars.len() - 1 - suffix]
            == after_chars[after_chars.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let before_end = before_chars.len().saturating_sub(suffix);
    let after_end = after_chars.len().saturating_sub(suffix);
    let from = before_chars[prefix..before_end].iter().collect::<String>();
    let to = after_chars[prefix..after_end].iter().collect::<String>();
    let prefix_utf16 = utf16_len_chars(&before_chars[..prefix]);
    Some(ChangedSpan {
        old_range: TextRange {
            location: prefix_utf16,
            length: utf16_len(&from),
        },
        new_range: TextRange {
            location: prefix_utf16,
            length: utf16_len(&to),
        },
        from,
        to,
    })
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn utf16_len_chars(chars: &[char]) -> usize {
    chars.iter().map(|ch| ch.len_utf16()).sum()
}

pub(crate) fn utf16_range_text(value: &str, range: TextRange) -> Option<String> {
    let start = utf16_offset_to_byte_index(value, range.location)?;
    let end = utf16_offset_to_byte_index(value, range.end())?;
    if start > end {
        return None;
    }
    Some(value[start..end].to_string())
}

pub(crate) fn utf16_offset_to_byte_index(value: &str, offset: usize) -> Option<usize> {
    let mut utf16_offset = 0usize;
    for (byte_index, ch) in value.char_indices() {
        if utf16_offset == offset {
            return Some(byte_index);
        }
        utf16_offset = utf16_offset.saturating_add(ch.len_utf16());
        if utf16_offset > offset {
            return None;
        }
    }
    if utf16_offset == offset {
        Some(value.len())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_span_tracks_utf16_ranges() {
        let span = changed_span("絵文字🙂タイプレスです", "絵文字🙂Typelessです").expect("span");

        assert_eq!(span.from, "タイプレス");
        assert_eq!(span.to, "Typeless");
        assert_eq!(span.old_range.location, "絵文字🙂".encode_utf16().count());
        assert_eq!(span.old_range.length, "タイプレス".encode_utf16().count());
    }

    #[test]
    fn utf16_range_text_reads_ranges_with_surrogates() {
        let value = "絵文字🙂タイプレス";
        let range = TextRange {
            location: "絵文字🙂".encode_utf16().count(),
            length: "タイプレス".encode_utf16().count(),
        };

        assert_eq!(
            utf16_range_text(value, range).as_deref(),
            Some("タイプレス")
        );
    }
}
