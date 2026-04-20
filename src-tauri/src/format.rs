//! Formatted text parsing for user chat input.
//!
//! Recognizes three kinds of inline markup:
//! - `` `...` `` — Lean code
//! - `$...$` — inline LaTeX (single dollar)
//! - `$$...$$` — display LaTeX (double dollar)
//!
//! Escape sequences: `\$` → literal `$`, `` \` `` → literal backtick.
//! A lone `\` not followed by `$` or `` ` `` is kept verbatim.
//!
//! Unclosed spans are treated as plain text (delete-safe: removing a closing
//! delimiter reverts the region to plain on the next parse).

use serde::{Deserialize, Serialize};

/// A parsed region of formatted chat input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "text")]
pub enum Span {
    Plain(String),
    Lean(String),
    LaTeX(String),
    DisplayLatex(String),
}

/// Parse `input` into a sequence of [`Span`]s.
///
/// Empty `Plain` spans are suppressed; empty formatted spans (e.g. ` `` `) are kept.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn scan_spans(input: &str) -> Vec<Span> {
    enum State {
        Plain,
        Lean,
        LaTeX,
        DisplayLatex,
    }

    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut spans: Vec<Span> = Vec::new();

    // `buf` accumulates the current region's content as a String.  We use byte
    // ranges to append whole slices from `input` rather than pushing one char
    // at a time, which correctly handles multi-byte UTF-8 sequences and is more
    // efficient for long plain-text runs.
    let mut buf = String::new();
    // `region_start` is the byte index in `input` where the current unescaped
    // run began (used to append whole slices in one push).
    let mut region_start = 0;
    let mut state = State::Plain;
    let mut i = 0;

    // Flush `buf` as a Plain span (no-op if empty).
    macro_rules! emit_plain {
        () => {
            if !buf.is_empty() {
                spans.push(Span::Plain(std::mem::take(&mut buf)));
            }
        };
    }

    while i < len {
        let b = bytes[i];

        // Escape: \$ or \` — append the run so far, then append the escaped
        // character literally, then skip both bytes.
        if b == b'\\' && i + 1 < len && (bytes[i + 1] == b'$' || bytes[i + 1] == b'`') {
            buf.push_str(&input[region_start..i]);
            buf.push(bytes[i + 1] as char); // safe: bytes[i+1] is $ or `, both ASCII
            i += 2;
            region_start = i;
            continue;
        }

        match state {
            State::Plain => match b {
                b'$' if i + 1 < len && bytes[i + 1] == b'$' => {
                    buf.push_str(&input[region_start..i]);
                    emit_plain!();
                    state = State::DisplayLatex;
                    i += 2;
                    region_start = i;
                }
                b'$' => {
                    buf.push_str(&input[region_start..i]);
                    emit_plain!();
                    state = State::LaTeX;
                    i += 1;
                    region_start = i;
                }
                b'`' => {
                    buf.push_str(&input[region_start..i]);
                    emit_plain!();
                    state = State::Lean;
                    i += 1;
                    region_start = i;
                }
                _ => i += 1,
            },

            State::Lean => match b {
                b'`' => {
                    buf.push_str(&input[region_start..i]);
                    spans.push(Span::Lean(std::mem::take(&mut buf)));
                    state = State::Plain;
                    i += 1;
                    region_start = i;
                }
                _ => i += 1,
            },

            State::LaTeX => match b {
                b'$' => {
                    buf.push_str(&input[region_start..i]);
                    spans.push(Span::LaTeX(std::mem::take(&mut buf)));
                    state = State::Plain;
                    i += 1;
                    region_start = i;
                }
                _ => i += 1,
            },

            State::DisplayLatex => {
                if b == b'$' && i + 1 < len && bytes[i + 1] == b'$' {
                    buf.push_str(&input[region_start..i]);
                    spans.push(Span::DisplayLatex(std::mem::take(&mut buf)));
                    state = State::Plain;
                    i += 2;
                    region_start = i;
                } else {
                    i += 1;
                }
            }
        }
    }

    // EOF: flush remaining content.
    buf.push_str(&input[region_start..]);
    match state {
        State::Plain => {}
        State::Lean => {
            let content = std::mem::take(&mut buf);
            buf.push('`');
            buf.push_str(&content);
        }
        State::LaTeX => {
            let content = std::mem::take(&mut buf);
            buf.push('$');
            buf.push_str(&content);
        }
        State::DisplayLatex => {
            let content = std::mem::take(&mut buf);
            buf.push_str("$$");
            buf.push_str(&content);
        }
    }
    if !buf.is_empty() {
        spans.push(Span::Plain(buf));
    }

    spans
}

/// Alias for [`scan_spans`] — use whichever name reads more clearly at the call site.
pub use scan_spans as parse;

#[cfg(test)]
mod tests {
    use super::*;

    fn lean(s: &str) -> Span {
        Span::Lean(s.into())
    }
    fn plain(s: &str) -> Span {
        Span::Plain(s.into())
    }
    fn latex(s: &str) -> Span {
        Span::LaTeX(s.into())
    }
    fn display(s: &str) -> Span {
        Span::DisplayLatex(s.into())
    }

    // ── Basic ──────────────────────────────────────────────────────────────

    #[test]
    fn t01_empty_input() {
        assert_eq!(parse(""), vec![]);
    }

    #[test]
    fn t02_plain_text_only() {
        assert_eq!(parse("hello world"), vec![plain("hello world")]);
    }

    #[test]
    fn t03_single_backtick_pair_with_surrounding_text() {
        assert_eq!(
            parse("Green `blue` pink"),
            vec![plain("Green "), lean("blue"), plain(" pink")]
        );
    }

    #[test]
    fn t04_multiple_backtick_pairs_on_same_line() {
        // The user's explicit example: orange must not be formatted.
        assert_eq!(
            parse("Green red `blue` orange `yellow` pink"),
            vec![
                plain("Green red "),
                lean("blue"),
                plain(" orange "),
                lean("yellow"),
                plain(" pink"),
            ]
        );
    }

    #[test]
    fn t05_single_dollar_pair_with_surrounding_text() {
        assert_eq!(
            parse(r"We have $\alpha + \beta$ here."),
            vec![plain("We have "), latex(r"\alpha + \beta"), plain(" here.")]
        );
    }

    #[test]
    fn t06_mixed_lean_and_latex() {
        assert_eq!(
            parse("Use `theorem` and $x^2$ together"),
            vec![
                plain("Use "),
                lean("theorem"),
                plain(" and "),
                latex("x^2"),
                plain(" together"),
            ]
        );
    }

    // ── Unclosed delimiters (delete-safe) ─────────────────────────────────

    #[test]
    fn t07_unclosed_backtick_is_plain() {
        // The plain text before the delimiter is flushed when the delimiter is
        // seen; at EOF the unclosed span becomes a second Plain span.
        assert_eq!(
            parse("here `unclosed"),
            vec![plain("here "), plain("`unclosed")]
        );
    }

    #[test]
    fn t08_unclosed_dollar_is_plain() {
        assert_eq!(
            parse("here $unclosed"),
            vec![plain("here "), plain("$unclosed")]
        );
    }

    // ── Adjacent / empty spans ─────────────────────────────────────────────

    #[test]
    fn t09_adjacent_backtick_pairs() {
        assert_eq!(parse("`a``b`"), vec![lean("a"), lean("b")]);
    }

    #[test]
    fn t10_adjacent_dollar_pairs() {
        // $a$$b$ — greedy: $a$ consumed first, then $b$.
        assert_eq!(parse("$a$$b$"), vec![latex("a"), latex("b")]);
    }

    #[test]
    fn t11_empty_backtick_pair() {
        assert_eq!(parse("``"), vec![lean("")]);
    }

    #[test]
    fn t12_empty_dollar_pair() {
        // $$ is parsed as an opening display-latex delimiter; with no closing $$
        // it is unclosed and emits as Plain.
        assert_eq!(parse("$$"), vec![plain("$$")]);
    }

    #[test]
    fn t13_lone_backtick() {
        assert_eq!(parse("`"), vec![plain("`")]);
    }

    #[test]
    fn t14_lone_dollar() {
        assert_eq!(parse("$"), vec![plain("$")]);
    }

    // ── Delimiter priority (first-opener wins) ────────────────────────────

    #[test]
    fn t15_dollars_inside_backtick_region_not_parsed_as_latex() {
        assert_eq!(
            parse("`code $not latex$ end`"),
            vec![lean("code $not latex$ end")]
        );
    }

    #[test]
    fn t16_backtick_inside_dollar_region_not_parsed_as_lean() {
        assert_eq!(
            parse("$math `not lean` end$"),
            vec![latex("math `not lean` end")]
        );
    }

    // ── Whitespace and multiline ───────────────────────────────────────────

    #[test]
    fn t17_whitespace_between_spans_preserved() {
        assert_eq!(
            parse("  `a`  \t  `b`  "),
            vec![
                plain("  "),
                lean("a"),
                plain("  \t  "),
                lean("b"),
                plain("  ")
            ]
        );
    }

    #[test]
    fn t18_multiline_lean_span() {
        assert_eq!(
            parse("line one\n`lean code`\nline three"),
            vec![
                plain("line one\n"),
                lean("lean code"),
                plain("\nline three")
            ]
        );
    }

    #[test]
    fn t19_latex_spanning_line_boundary() {
        assert_eq!(
            parse("start $multi\nline math$ end"),
            vec![plain("start "), latex("multi\nline math"), plain(" end")]
        );
    }

    // ── No spurious empty plain spans ─────────────────────────────────────

    #[test]
    fn t20_backtick_then_dollar_no_empty_plain() {
        assert_eq!(parse("`code`$math$"), vec![lean("code"), latex("math")]);
    }

    #[test]
    fn t21_no_leading_empty_plain() {
        assert_eq!(parse("$math$"), vec![latex("math")]);
    }

    #[test]
    fn t22_no_trailing_empty_plain() {
        assert_eq!(parse("`code`"), vec![lean("code")]);
    }

    // ── Complex mixed ─────────────────────────────────────────────────────

    #[test]
    fn t23_complex_mixed_spans() {
        assert_eq!(
            parse("Try `f x` or $g(x)$ or `h` and $k$."),
            vec![
                plain("Try "),
                lean("f x"),
                plain(" or "),
                latex("g(x)"),
                plain(" or "),
                lean("h"),
                plain(" and "),
                latex("k"),
                plain("."),
            ]
        );
    }

    #[test]
    fn t24_whitespace_only_input() {
        assert_eq!(parse("   \n\t  "), vec![plain("   \n\t  ")]);
    }

    // ── Escape sequences ──────────────────────────────────────────────────

    #[test]
    fn t27_escaped_dollar_is_plain() {
        assert_eq!(parse(r"costs \$10"), vec![plain("costs $10")]);
    }

    #[test]
    fn t28_escaped_backtick_is_plain() {
        assert_eq!(parse(r"say \`hi\`"), vec![plain("say `hi`")]);
    }

    #[test]
    fn t29_both_dollars_escaped_no_latex_span() {
        assert_eq!(parse(r"\$math\$"), vec![plain("$math$")]);
    }

    #[test]
    fn t30_both_backticks_escaped_no_lean_span() {
        assert_eq!(parse(r"\`code\`"), vec![plain("`code`")]);
    }

    #[test]
    fn t31_escaped_dollar_inside_latex_span_does_not_close_it() {
        assert_eq!(
            parse(r"$math \$ still math$"),
            vec![latex("math $ still math")]
        );
    }

    #[test]
    fn t32_escaped_backtick_inside_lean_span_does_not_close_it() {
        assert_eq!(
            parse(r"`lean \` still lean`"),
            vec![lean("lean ` still lean")]
        );
    }

    #[test]
    fn t33_lone_backslash_not_before_delimiter_kept_verbatim() {
        assert_eq!(parse(r"a\b"), vec![plain(r"a\b")]);
    }

    #[test]
    fn t34_trailing_escaped_dollar() {
        assert_eq!(parse(r"\$"), vec![plain("$")]);
    }

    // ── DisplayLatex ($$...$$) ────────────────────────────────────────────

    #[test]
    fn t35_display_latex_basic() {
        assert_eq!(parse("$$x^2$$"), vec![display("x^2")]);
    }

    #[test]
    fn t36_display_latex_with_surrounding_text() {
        assert_eq!(
            parse(r"before $$\alpha$$ after"),
            vec![plain("before "), display(r"\alpha"), plain(" after")]
        );
    }

    #[test]
    fn t37_unclosed_display_latex_is_plain() {
        assert_eq!(parse("$$unclosed"), vec![plain("$$unclosed")]);
    }

    #[test]
    fn t38_inline_then_display_latex() {
        // $a$$b$$ — after $a$ closes at pos 2, Plain sees $ at pos 3, peeks b
        // (not $), so enters inline LaTeX. Finds $ at pos 5 → LaTeX("b").
        // Remaining $ at pos 6 is unclosed → Plain("$").
        assert_eq!(parse("$a$$b$$"), vec![latex("a"), latex("b"), plain("$")]);
    }

    #[test]
    fn t39_display_then_inline_latex() {
        assert_eq!(
            parse("$$display$$ and $inline$"),
            vec![display("display"), plain(" and "), latex("inline")]
        );
    }

    // ── Unicode passthrough ───────────────────────────────────────────────

    #[test]
    fn t40_unicode_in_plain_text_passes_through() {
        // Multi-byte UTF-8 chars must not be corrupted by byte-level scanning.
        assert_eq!(parse("αβγ"), vec![plain("αβγ")]);
    }

    #[test]
    fn t41_unicode_inside_lean_span() {
        assert_eq!(parse("`α + β`"), vec![lean("α + β")]);
    }

    #[test]
    fn t42_unicode_inside_latex_span() {
        assert_eq!(parse("$\\alpha$"), vec![latex("\\alpha")]);
    }

    // ── Serde round-trip ──────────────────────────────────────────────────

    #[test]
    fn t26_serde_roundtrip() {
        let span = Span::Lean("x".into());
        let json = serde_json::to_string(&span).unwrap();
        assert_eq!(json, r#"{"kind":"Lean","text":"x"}"#);
        let back: Span = serde_json::from_str(&json).unwrap();
        assert_eq!(back, span);
    }
}
