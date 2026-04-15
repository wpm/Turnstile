//! Goal-panel-line → Formal-Proof-line mapping.
//!
//! The Goal State panel renders the rendered goal text (Markdown with fenced
//! code blocks) line by line; each code-block line is a clickable row. When
//! the user clicks a row, the UI highlights the **earliest** line of the
//! Formal Proof whose rendered goal state contains that exact text. The
//! mapping from panel-row → source-line is computed here so the frontend
//! only has to look it up by index.
//!
//! Parallels `parseBlocks` in `app/src/lib/markdown.ts`: only fenced code
//! blocks contribute rows; text blocks contribute nothing. The result vector
//! is indexed by the flattened iteration of code-block lines in the panel.

use std::collections::HashMap;

/// Build the goal-panel-line → formal-proof-line mapping.
///
/// For each text line in each fenced code block of `full_goal`, find the
/// **earliest** (lowest-indexed) entry of `per_line_goals` whose rendered
/// text contains that exact line, and record its 1-indexed position. Blank
/// lines slot in as `None` (they carry no proof-line meaning and would
/// otherwise match the empty goal state at end-of-proof).
///
/// The result is parallel to the flattened iteration of code-block lines
/// in `full_goal`, matching how `GoalPanel.svelte` renders rows.
#[must_use]
pub fn build_panel_line_to_source_line(
    full_goal: &str,
    per_line_goals: &[String],
) -> Vec<Option<u32>> {
    // Invert `per_line_goals`: for each distinct text-line, remember the
    // earliest 1-indexed source line that produced it.
    let mut earliest: HashMap<&str, u32> = HashMap::new();
    for (idx, goal) in per_line_goals.iter().enumerate() {
        let source_line = u32::try_from(idx + 1).expect("source line count fits in u32");
        for line in goal.split('\n') {
            earliest.entry(line).or_insert(source_line);
        }
    }

    let mut result = Vec::new();
    for block in parse_fenced_blocks(full_goal) {
        if !block.is_code {
            continue;
        }
        for line in block.content.split('\n') {
            if line.trim().is_empty() {
                result.push(None);
            } else {
                result.push(earliest.get(line).copied());
            }
        }
    }
    result
}

struct FencedBlock<'a> {
    is_code: bool,
    content: &'a str,
}

/// Parse `text` into alternating text and fenced-code blocks.
///
/// Mirrors the semantics of the frontend's `parseBlocks` regex
/// (`/^```(\w*)\n([\s\S]*?)^```/gm`): a fence is three backticks at the
/// start of a line, content runs up to the next start-of-line fence,
/// unmatched leading fence runs are treated as text. Nested or escaped
/// fences are not recognized, matching the frontend.
///
/// Byte indexing is safe because all sentinels (`\n`, `` ` ``) are ASCII,
/// and slice boundaries are always found via `str::find('\n')` or
/// arithmetic from ASCII offsets.
fn parse_fenced_blocks(text: &str) -> Vec<FencedBlock<'_>> {
    let mut blocks = Vec::new();
    let bytes = text.as_bytes();
    let mut cursor = 0;

    while cursor < bytes.len() {
        let at_line_start = cursor == 0 || bytes[cursor - 1] == b'\n';
        if at_line_start && bytes[cursor..].starts_with(b"```") {
            let after_ticks = cursor + 3;
            let Some(nl_rel) = text[after_ticks..].find('\n') else {
                break;
            };
            let content_start = after_ticks + nl_rel + 1;

            let Some(close_idx) = find_closing_fence(text, content_start) else {
                break;
            };

            // Content preserves the terminal '\n' before the closing fence
            // (matching `parseBlocks`), so flattened splits of code blocks
            // yield a trailing empty line that the caller records as `None`.
            blocks.push(FencedBlock {
                is_code: true,
                content: &text[content_start..close_idx],
            });
            cursor = close_idx + 3;
            if cursor < bytes.len() && bytes[cursor] == b'\n' {
                cursor += 1;
            }
        } else {
            let start = cursor;
            let mut probe = cursor;
            let end = loop {
                match text[probe..].find('\n') {
                    Some(rel) => {
                        let next_line = probe + rel + 1;
                        if next_line < bytes.len() && bytes[next_line..].starts_with(b"```") {
                            break next_line;
                        }
                        probe = next_line;
                    }
                    None => break bytes.len(),
                }
            };
            blocks.push(FencedBlock {
                is_code: false,
                content: &text[start..end],
            });
            cursor = end;
        }
    }

    blocks
}

fn find_closing_fence(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut probe = from;
    loop {
        if probe >= bytes.len() {
            return None;
        }
        let at_line_start = probe == 0 || bytes[probe - 1] == b'\n';
        if at_line_start && bytes[probe..].starts_with(b"```") {
            return Some(probe);
        }
        let nl = text[probe..].find('\n')?;
        probe += nl + 1;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn to_vec(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn empty_inputs_produce_empty_vec() {
        assert!(build_panel_line_to_source_line("", &[]).is_empty());
        assert!(build_panel_line_to_source_line("", &to_vec(&["⊢ p"])).is_empty());
    }

    #[test]
    fn full_text_without_code_block_produces_empty_vec() {
        assert!(build_panel_line_to_source_line("plain text only", &to_vec(&["⊢ p"])).is_empty());
    }

    #[test]
    fn maps_each_code_line_to_first_per_line_goal_containing_it() {
        // Worked example: four proof lines produce rendered goals that
        // introduce `hp : p` / `hq : q` on line 1, then split into two
        // cases on line 2, leaving only `case right` on line 3, and
        // finally no goals on line 4.
        let full = [
            "```lean",
            "case left",
            "hp : p",
            "hq : q",
            "⊢ p",
            "",
            "case right",
            "hp : p",
            "hq : q",
            "⊢ q",
            "```",
        ]
        .join("\n");

        let per_line = vec![
            "hp : p\nhq : q\n⊢ p ∧ q".to_string(),
            [
                "case left",
                "hp : p",
                "hq : q",
                "⊢ p",
                "",
                "case right",
                "hp : p",
                "hq : q",
                "⊢ q",
            ]
            .join("\n"),
            ["case right", "hp : p", "hq : q", "⊢ q"].join("\n"),
            String::new(),
        ];

        assert_eq!(
            build_panel_line_to_source_line(&full, &per_line),
            vec![
                Some(2),
                Some(1),
                Some(1),
                Some(2),
                None,
                Some(2),
                Some(1),
                Some(1),
                Some(2),
                None,
            ]
        );
    }

    #[test]
    fn line_matching_uses_exact_string_equality() {
        let full = "```\n⊢ p\n```";
        assert_eq!(
            build_panel_line_to_source_line(full, &to_vec(&["⊢ p ", "  ⊢ p"])),
            vec![None, None]
        );
    }

    #[test]
    fn iterates_text_blocks_skipped_and_code_blocks_in_order() {
        let full = [
            "Some intro text",
            "",
            "```lean",
            "goal A",
            "```",
            "",
            "More prose",
            "",
            "```lean",
            "goal B",
            "```",
        ]
        .join("\n");
        let per_line = to_vec(&["goal A", "goal A\ngoal B"]);

        assert_eq!(
            build_panel_line_to_source_line(&full, &per_line),
            vec![Some(1), None, Some(2), None]
        );
    }

    #[test]
    fn unmatched_lines_are_none() {
        let full = "```\nno match\n```";
        assert_eq!(
            build_panel_line_to_source_line(full, &to_vec(&["something else", "also different"])),
            vec![None, None]
        );
    }

    #[test]
    fn empty_per_line_goals_all_none() {
        let full = "```\n⊢ p\n```";
        assert_eq!(build_panel_line_to_source_line(full, &[]), vec![None, None]);
    }
}
