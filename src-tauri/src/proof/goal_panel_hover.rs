//! Helpers for translating a hover in the Goal State panel into a hover
//! request on the Formal Proof document.
//!
//! Goal-state text is synthetic — the Lean server does not index it — so we
//! cannot query the LSP at panel positions directly. Each panel line maps
//! back to a real Formal Proof source line (see [`super::goal_panel_map`]).
//! To surface type info for an identifier in the panel, we locate that
//! identifier on the mapped source line and issue the LSP hover there.
//!
//! This is intentionally coarse: only the first occurrence of the word on the
//! source line is queried, and if the word isn't found the hover is
//! suppressed. It handles the common case — an identifier that appears once
//! on the source line that produced the panel row.
//!
//! All column offsets returned by these helpers are UTF-16 code-unit offsets,
//! so they can be passed straight to the LSP's `Position.character` field
//! without further conversion.

/// Locate the word containing (or immediately adjacent to) the given column
/// on `line_text`, where `col` is a UTF-16 code-unit offset from the start
/// of the line.
///
/// Returns `(word, start_col, end_col)` as UTF-16 code-unit offsets, or
/// `None` if the position is outside any word.
///
/// "Word" here means a run of Lean-identifier-friendly characters:
/// letters, digits, underscores, apostrophes, and the common Greek letters
/// Lean uses as identifiers. Operators and symbols (`⊢`, `→`, `:`, …) are
/// not word characters.
#[must_use]
pub fn find_word_at(line_text: &str, col: u32) -> Option<(String, u32, u32)> {
    let col = col as usize;
    let mut current_start: Option<(usize, usize)> = None; // (byte_offset, utf16_offset)
    let mut utf16_offset = 0usize;
    let mut byte_offset = 0usize;

    // First pass: walk the line collecting (utf16_start, utf16_end, byte_start, byte_end)
    // for each word, and find the one containing `col`.
    let mut words: Vec<(usize, usize, usize, usize)> = Vec::new();
    for c in line_text.chars() {
        let cu = c.len_utf16();
        let cb = c.len_utf8();
        if is_word_char(c) {
            if current_start.is_none() {
                current_start = Some((byte_offset, utf16_offset));
            }
        } else if let Some((byte_start, utf16_start)) = current_start.take() {
            words.push((utf16_start, utf16_offset, byte_start, byte_offset));
        }
        utf16_offset += cu;
        byte_offset += cb;
    }
    if let Some((byte_start, utf16_start)) = current_start {
        words.push((utf16_start, utf16_offset, byte_start, byte_offset));
    }

    for (utf16_start, utf16_end, byte_start, byte_end) in words {
        // Treat a position that lands exactly at the word's end as belonging
        // to the word (matches CM6's wordAt behavior).
        if col >= utf16_start && col <= utf16_end {
            let word = line_text[byte_start..byte_end].to_string();
            let start = u32::try_from(utf16_start).ok()?;
            let end = u32::try_from(utf16_end).ok()?;
            return Some((word, start, end));
        }
    }
    None
}

/// Find the first occurrence of `word` as a whole word in `source_line`,
/// returning the UTF-16 code-unit offset of its start, or `None` if not
/// found.
///
/// Callers use this to translate a panel-local hover target to a real column
/// on the mapped Formal Proof line. "Whole word" means the match must be
/// bounded by non-word characters (or the start/end of the line) — so `hp`
/// does not match inside `hpp` or `ahp`.
#[must_use]
pub fn locate_in_source(word: &str, source_line: &str) -> Option<u32> {
    if word.is_empty() {
        return None;
    }
    let word_chars: Vec<char> = word.chars().collect();
    let src_chars: Vec<char> = source_line.chars().collect();
    if word_chars.len() > src_chars.len() {
        return None;
    }

    // Walk every candidate start position, tracking the UTF-16 offset of
    // the current char. A naive O(n·m) scan is fine here: the inputs are
    // single source lines.
    let mut utf16_offset: usize = 0;
    for start in 0..=src_chars.len() - word_chars.len() {
        // Check word boundary before.
        let before_ok = start == 0 || !is_word_char(src_chars[start - 1]);
        // Check word boundary after.
        let end = start + word_chars.len();
        let after_ok = end >= src_chars.len() || !is_word_char(src_chars[end]);
        // Check contents match.
        let content_ok = src_chars[start..end]
            .iter()
            .zip(word_chars.iter())
            .all(|(a, b)| a == b);

        if before_ok && after_ok && content_ok {
            return u32::try_from(utf16_offset).ok();
        }
        utf16_offset += src_chars[start].len_utf16();
    }
    None
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_word_at_inside_identifier() {
        let (word, start, end) = find_word_at("hp : p", 1).expect("should find word");
        assert_eq!(word, "hp");
        assert_eq!((start, end), (0, 2));
    }

    #[test]
    fn find_word_at_start_of_identifier() {
        let (word, _, _) = find_word_at("hp : p", 0).expect("should find word");
        assert_eq!(word, "hp");
    }

    #[test]
    fn find_word_at_end_of_identifier() {
        // Column 2 is right after "hp" (end boundary).
        let (word, _, _) = find_word_at("hp : p", 2).expect("should find word");
        assert_eq!(word, "hp");
    }

    #[test]
    fn find_word_at_whitespace_returns_none() {
        // Column 2 in "a    b" lands in the whitespace run between words.
        assert!(find_word_at("a    b", 2).is_none());
    }

    #[test]
    fn find_word_at_on_operator() {
        // "hp ⊢ p" — column 3 is at '⊢' (UTF-16 1 code unit).
        // "hp " = 3 UTF-16 units, so col 3 hits the operator.
        assert!(find_word_at("hp ⊢ p", 3).is_none());
    }

    #[test]
    fn find_word_at_handles_unicode_identifier() {
        // "α : Nat" — α is 1 UTF-16 code unit. Col 0 should find "α".
        let (word, start, end) = find_word_at("α : Nat", 0).expect("should find word");
        assert_eq!(word, "α");
        assert_eq!((start, end), (0, 1));
    }

    #[test]
    fn find_word_at_apostrophe_is_word_char() {
        let (word, _, _) = find_word_at("x' : Nat", 1).expect("should find word");
        assert_eq!(word, "x'");
    }

    #[test]
    fn find_word_at_beyond_line_returns_none() {
        assert!(find_word_at("hp", 5).is_none());
    }

    #[test]
    fn locate_in_source_finds_first_occurrence() {
        assert_eq!(locate_in_source("hp", "intro hp hq"), Some(6));
    }

    #[test]
    fn locate_in_source_missing_returns_none() {
        assert_eq!(locate_in_source("hp", "apply or_left"), None);
    }

    #[test]
    fn locate_in_source_respects_word_boundary_prefix() {
        // `hp` must not match inside `hpp`.
        assert_eq!(locate_in_source("hp", "hpp"), None);
    }

    #[test]
    fn locate_in_source_respects_word_boundary_suffix() {
        // `hp` must not match inside `ahp`.
        assert_eq!(locate_in_source("hp", "ahp"), None);
    }

    #[test]
    fn locate_in_source_first_occurrence_wins() {
        assert_eq!(locate_in_source("x", "let x := y; let x := z"), Some(4));
    }

    #[test]
    fn locate_in_source_unicode_offset() {
        // "α : Nat" — `Nat` starts after "α : " = 4 UTF-16 units.
        assert_eq!(locate_in_source("Nat", "α : Nat"), Some(4));
    }

    #[test]
    fn locate_in_source_empty_word_returns_none() {
        assert_eq!(locate_in_source("", "anything"), None);
    }
}
