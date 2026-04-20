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

    scan_spans(text)
        .into_iter()
        .map(|span| match span {
            Span::Plain(s) => s,
            Span::Lean(s) => format!("`{s}`"),
            Span::LaTeX(math) => katex::render_with_opts(&math, inline_opts.clone())
                .unwrap_or_else(|_| format!("${math}$")),
            Span::DisplayLatex(math) => katex::render_with_opts(&math, display_opts.clone())
                .unwrap_or_else(|_| format!("$${math}$$")),
        })
        .collect()
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
        assert_eq!(render_katex("hello world"), "hello world");
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
        assert!(out.starts_with("Inline "));
        assert!(out.ends_with('.'));
    }

    #[test]
    fn render_katex_unclosed_delimiter_preserved() {
        let out = render_katex("broken $x + 1 here");
        assert_eq!(out, "broken $x + 1 here");
    }
}
