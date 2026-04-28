//! Prose proof generation: translate the formal proof into textbook-style prose.
//!
//! The translator is a **background process**, not an agent.  It runs whenever
//! the goal state changes (debounced), calls the LLM with a dedicated
//! system prompt, and writes the result into [`Proof::prose`](super::Proof).
//!
//! The runtime plumbing — the debounce loop, the staleness checks, the
//! sequence-guarded retry — lives in the crate root (`lib.rs`) alongside the
//! other LSP-adjacent background tasks.  What lives here is the LLM call
//! itself.

use tauri::{AppHandle, Manager};

use crate::format::{scan_spans, Span};
use crate::llm::{models, Llm, LlmError};

/// Render `$...$` (inline) and `$$...$$` (display) math in `text` to `KaTeX` HTML.
///
/// Uses the shared [`scan_spans`] state machine, so delimiter semantics are
/// identical to the chat input parser.  On render failure the original
/// delimiters are preserved verbatim.
///
/// # Panics
///
/// Never panics — `Opts::build()` cannot fail when all fields are optional.
#[must_use]
pub fn render_katex(text: &str) -> String {
    let inline_opts = katex::Opts::builder()
        .display_mode(false)
        .output_type(katex::OutputType::Html)
        .build()
        .unwrap();
    let display_opts = katex::Opts::builder()
        .display_mode(true)
        .output_type(katex::OutputType::Html)
        .build()
        .unwrap();

    let rendered: String = scan_spans(text)
        .into_iter()
        .map(|span| match span {
            Span::Plain(s) => inline_markdown(&s),
            Span::Lean(s) => format!("`{s}`"),
            Span::LaTeX(math) => katex::render_with_opts(&math, inline_opts.clone())
                .unwrap_or_else(|_| format!("${math}$")),
            Span::DisplayLatex(math) => katex::render_with_opts(&math, display_opts.clone())
                .unwrap_or_else(|_| format!("$${math}$$")),
        })
        .collect();

    // Convert paragraph breaks and line breaks to HTML so the {@html} display renders correctly.
    // A line containing only "---" becomes a horizontal rule rather than a paragraph break.
    let normalized = rendered.replace("\r\n", "\n");
    let mut parts: Vec<String> = Vec::new();
    let mut open_p = false;
    for para in normalized.split("\n\n") {
        let trimmed = para.trim();
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            if open_p {
                parts.push("</p>".to_string());
                open_p = false;
            }
            parts.push("<hr>".to_string());
        } else {
            if open_p {
                parts.push("</p>".to_string());
            }
            parts.push(format!("<p>{}", para.replace('\n', "<br>")));
            open_p = true;
        }
    }
    if open_p {
        parts.push("</p>".to_string());
    }
    parts.join("")
}

/// Convert inline Markdown (`**bold**`, `*italic*`) in a plain-text segment to HTML.
/// Applied only to `Plain` spans, never inside math or code.
fn inline_markdown(s: &str) -> String {
    // Bold before italic so **x** isn't partially matched by *x*.
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(after_open) = rest.strip_prefix("**") {
            if let Some(close) = after_open.find("**") {
                out.push_str("<strong>");
                out.push_str(&after_open[..close]);
                out.push_str("</strong>");
                rest = &after_open[close + 2..];
                continue;
            }
        }
        if let Some(after_open) = rest.strip_prefix('*') {
            if let Some(close) = after_open.find('*') {
                out.push_str("<em>");
                out.push_str(&after_open[..close]);
                out.push_str("</em>");
                rest = &after_open[close + 1..];
                continue;
            }
        }
        // Advance by one char.
        let mut chars = rest.chars();
        if let Some(c) = chars.next() {
            out.push(c);
            rest = chars.as_str();
        }
    }
    out
}

/// Default translator prompt, loaded at compile time from `prompts/translator.md`.
///
/// The user can override it via `Settings::translation_prompt`; when that
/// override is `None` the value below is used verbatim.
pub const DEFAULT_TRANSLATION_PROMPT: &str = include_str!("prompts/translator.md");

/// Resolve the translator's system prompt: user override if set, else default.
async fn effective_translation_prompt(app: &AppHandle) -> String {
    let state = app.state::<crate::AppState>();
    let settings = state.settings.lock().await;
    settings
        .translation_prompt
        .clone()
        .unwrap_or_else(|| DEFAULT_TRANSLATION_PROMPT.to_string())
}

/// Resolve the translator's model: user override if set, else backend default.
async fn effective_translation_model(app: &AppHandle) -> String {
    let state = app.state::<crate::AppState>();
    let settings = state.settings.lock().await;
    settings
        .translation_model
        .clone()
        .unwrap_or_else(|| models::default_model_id().to_string())
}

/// Run the LLM translator on `source` and return the generated prose.
///
/// Pure library helper — the Tauri-exposed [`generate_prose`] command wraps
/// this together with state updates and event emission.
///
/// # Errors
///
/// Returns an [`LlmError`] if the LLM call fails.
pub async fn run_translator(
    llm: &dyn Llm,
    source: &str,
    app: &AppHandle,
) -> Result<String, LlmError> {
    let prompt = effective_translation_prompt(app).await;
    let model = effective_translation_model(app).await;
    let turn = llm.complete(&prompt, source, &model, app).await?;
    Ok(turn.content)
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translator_prompt_is_non_empty() {
        assert!(!DEFAULT_TRANSLATION_PROMPT.is_empty());
        assert!(DEFAULT_TRANSLATION_PROMPT.contains("mathematical writing assistant"));
    }

    #[test]
    fn render_katex_leaves_plain_text_unchanged() {
        assert_eq!(render_katex("hello world"), "<p>hello world</p>");
    }

    #[test]
    fn render_katex_renders_inline_math() {
        let out = render_katex("We have $x^2 + 1$ here.");
        assert!(!out.contains('$'));
        assert!(out.contains("katex"));
    }

    #[test]
    fn render_katex_renders_display_math() {
        let out = render_katex("$$\\int_0^1 x\\,dx$$");
        assert!(!out.contains("$$"));
        assert!(out.contains("katex"));
    }

    #[test]
    fn render_katex_mixed_content() {
        let out = render_katex("Inline $a$ and display $$b$$.");
        assert!(!out.contains('$'));
        assert!(out.contains("Inline "));
        assert!(out.contains('.'));
    }

    #[test]
    fn render_katex_unclosed_delimiter_preserved() {
        let out = render_katex("broken $x + 1 here");
        assert_eq!(out, "<p>broken $x + 1 here</p>");
    }

    #[test]
    fn render_katex_paragraph_breaks_become_html() {
        let out = render_katex("first\n\nsecond");
        assert_eq!(out, "<p>first</p><p>second</p>");
    }

    #[test]
    fn render_katex_single_newline_becomes_br() {
        let out = render_katex("line one\nline two");
        assert_eq!(out, "<p>line one<br>line two</p>");
    }

    #[test]
    fn render_katex_bold_markdown() {
        let out = render_katex("**Theorem (Foo)**");
        assert_eq!(out, "<p><strong>Theorem (Foo)</strong></p>");
    }

    #[test]
    fn render_katex_italic_markdown() {
        let out = render_katex("*Proof.*");
        assert_eq!(out, "<p><em>Proof.</em></p>");
    }

    #[test]
    fn render_katex_hr_becomes_hr_tag() {
        let out = render_katex("first\n\n---\n\nsecond");
        assert_eq!(out, "<p>first</p><hr><p>second</p>");
    }

    #[test]
    fn render_katex_bold_not_applied_inside_math() {
        // Math spans bypass inline_markdown — the ** inside $...$ must not become <strong>
        let out = render_katex("$a**b**c$");
        assert!(out.contains("katex"));
        assert!(!out.contains("<strong>"));
    }
}
