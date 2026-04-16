//! Prose proof generation: translate the formal proof into textbook-style prose.
//!
//! The translator is a **background process**, not an agent.  It runs whenever
//! the formal source changes (debounced), calls the LLM with a dedicated
//! system prompt, and writes the result into [`Proof::prose`](super::Proof).
//!
//! The runtime plumbing — the debounce loop, the staleness checks, the
//! sequence-guarded retry — lives in the crate root (`lib.rs`) alongside the
//! other LSP-adjacent background tasks.  What lives here is the LLM call
//! itself and the Tauri command that lets the user force an on-demand
//! regeneration.

use tauri::{AppHandle, Emitter, Manager};

use super::{compute_source_hash, ProsePayload, PROSE_UPDATED_EVENT};
use crate::llm::{models, Llm, LlmError};

/// Default translator prompt, loaded at compile time from prompts/translator.md.
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

/// Force a prose regeneration from the current formal source, bypassing the
/// debounce and dirty-flag checks.  Emits [`PROSE_UPDATED_EVENT`] on success.
///
/// This is a standalone LLM call — separate from the PA conversation — exposed
/// to the UI as an explicit "regenerate prose" button.
#[tauri::command]
pub async fn generate_prose(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    let source = state.proof.lock().await.formal.source.clone();
    if source.trim().is_empty() {
        return Ok(String::new());
    }

    let backend = state.llm.clone();
    let prose_text = run_translator(backend.as_ref(), &source, &app)
        .await
        .map_err(|e| e.0)?;

    let source_hash = compute_source_hash(&source);

    {
        let mut proof = state.proof.lock().await;
        proof.prose.text = prose_text.clone();
        proof.prose.source_hash = source_hash.clone();
    }

    app.emit(
        PROSE_UPDATED_EVENT,
        &ProsePayload {
            text: prose_text.clone(),
            hash: Some(source_hash),
        },
    )
    .ok();

    state
        .session_dirty
        .store(true, std::sync::atomic::Ordering::SeqCst);

    Ok(prose_text)
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
}
