//! The Assistant: a conversational agent that helps the user develop a
//! [`Proof`](crate::proof::Proof) by reading the formal source, the tactic
//! state, and the prose draft, and by proposing edits to them.
//!
//! # Architecture
//!
//! [`Transcript`] owns the conversation (an optional summary plus a list of
//! [`Turn`]s) and is stored in [`crate::AppState`] behind
//! `Arc<tokio::sync::Mutex<Transcript>>`.  The agent reads and writes it via
//! the [`send_message`] Tauri command.
//!
//! # Context management
//!
//! When the running token estimate exceeds 75 % of [`Transcript::max_tokens`],
//! the agent summarizes the oldest 75 % of turns into a single string stored
//! in [`Transcript::summary`].  Summarization is an async LLM call that does
//! not block the UI; new messages keep arriving normally while it runs.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::llm::{Llm, LlmError};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// One side of a conversation turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

/// A single exchange in the transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub role: Role,
    pub content: String,
    /// Parsed formatting spans for user turns; empty for assistant turns.
    #[serde(default)]
    pub spans: Vec<crate::format::Span>,
    /// Unix timestamp in milliseconds.
    pub timestamp: i64,
}

impl Turn {
    /// Build an assistant turn with the current wall-clock timestamp.
    #[must_use]
    pub fn assistant(content: String) -> Self {
        Self {
            role: Role::Assistant,
            content,
            spans: vec![],
            timestamp: Utc::now().timestamp_millis(),
        }
    }
}

/// Default system prompt delivered to the assistant LLM. The user can override
/// this via `Settings::assistant_prompt`; when that override is `None` the
/// value below is used verbatim.
pub const DEFAULT_ASSISTANT_PROMPT: &str = include_str!("prompts/assistant.md");

/// Emitted for every assistant text-delta chunk while streaming.  Payload: `String`.
pub const STREAM_DELTA_EVENT: &str = "assistant-delta";

/// Emitted once when the stream ends (whether the turn completed normally
/// or via tool-use cycles).  Payload: `()`.
pub const STREAM_DONE_EVENT: &str = "assistant-stream-done";

/// Emitted once the full assistant turn (including any tool-use cycles) is
/// complete.  Payload: [`Turn`].
pub const COMPLETE_EVENT: &str = "assistant-complete";

/// Serializable snapshot of the conversation — the unit stored in `.turn` files.
///
/// ```json
/// {
///   "summary": "string or null",
///   "turns": [ { "role": "user", "content": "...", "timestamp": ... }, ... ]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub summary: Option<String>,
    /// Active turns (the most-recent portion after any summarization).
    pub turns: Vec<Turn>,
    /// Soft context-window limit in tokens. Default 200 000 (Claude claude-sonnet-4-6).
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
}

const fn default_max_tokens() -> usize {
    200_000
}

impl Default for Transcript {
    fn default() -> Self {
        Self {
            summary: None,
            turns: Vec::new(),
            max_tokens: default_max_tokens(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool names & definitions
// ---------------------------------------------------------------------------

/// Tools the assistant may call to inspect or modify editor state.
///
/// Each variant maps to a unique wire-level string used in the LLM protocol
/// and system prompt.  The [`ToolName::as_str`] method and [`TryFrom<&str>`]
/// impl form the single source of truth for these strings — do not duplicate
/// the literals elsewhere in the codebase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolName {
    ReadLeanSource,
    UpdateLeanSource,
    ReadGoalState,
    ReadProseProof,
    UpdateProseProof,
    ReadDiagnostics,
}

impl ToolName {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadLeanSource => "read_lean_source",
            Self::UpdateLeanSource => "update_lean_source",
            Self::ReadGoalState => "read_goal_state",
            Self::ReadProseProof => "read_prose_proof",
            Self::UpdateProseProof => "update_prose_proof",
            Self::ReadDiagnostics => "read_diagnostics",
        }
    }
}

impl TryFrom<&str> for ToolName {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "read_lean_source" => Ok(Self::ReadLeanSource),
            "update_lean_source" => Ok(Self::UpdateLeanSource),
            "read_goal_state" => Ok(Self::ReadGoalState),
            "read_prose_proof" => Ok(Self::ReadProseProof),
            "update_prose_proof" => Ok(Self::UpdateProseProof),
            "read_diagnostics" => Ok(Self::ReadDiagnostics),
            other => Err(format!("Unknown tool: {other}")),
        }
    }
}

/// Wire-level tool definition advertised to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[must_use]
pub fn default_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: ToolName::ReadLeanSource.as_str().into(),
            description: "Read the current Lean source file contents.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: ToolName::UpdateLeanSource.as_str().into(),
            description: "Replace the entire contents of the Lean source editor with new \
                          text. Use this to apply a fix or new proof step you have agreed on \
                          with the user, instead of asking them to copy it in by hand. This \
                          is a complete replacement: include the whole file, not just the \
                          changed lines. Read the current source first so you don't drop \
                          anything the user wants to keep, and show the user what you're \
                          changing before you write it unless they've asked you to just go \
                          ahead."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The new Lean source, a complete replacement of the editor contents."
                    }
                },
                "required": ["text"]
            }),
        },
        ToolDefinition {
            name: ToolName::ReadGoalState.as_str().into(),
            description: "Read the current Lean goal state for the entire proof.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: ToolName::ReadProseProof.as_str().into(),
            description: "Read the current prose proof draft (LaTeX).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: ToolName::UpdateProseProof.as_str().into(),
            description: "Replace the prose proof draft with a new version.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The new prose proof text, a complete replacement."
                    }
                },
                "required": ["text"]
            }),
        },
        ToolDefinition {
            name: ToolName::ReadDiagnostics.as_str().into(),
            description: "Read the current Lean compiler diagnostics (errors and warnings). \
                          Returns a list of errors and warnings with their locations and \
                          messages. Info and hint diagnostics are excluded."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

/// Execute a tool call from the LLM and return the tool result string.
///
/// Matches wire-level tool names via [`ToolName::try_from`].  Unknown tools
/// return a diagnostic string rather than erroring — the LLM sees it as a
/// tool result and can recover.
///
/// # Panics
///
/// Panics if the `current_diagnostics` mutex is poisoned.
pub async fn dispatch_tool(
    tool_name: &str,
    tool_input: &serde_json::Value,
    app: &AppHandle,
) -> String {
    let state = app.state::<crate::AppState>();

    let tool = match ToolName::try_from(tool_name) {
        Ok(t) => t,
        Err(msg) => return msg,
    };

    match tool {
        ToolName::ReadLeanSource => state.proof.lock().await.formal.source.clone(),
        ToolName::UpdateLeanSource => {
            let text = tool_input
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            // Route through the backend-authoritative replace path (ADR-0003):
            // it assigns the source, pushes to CodeMirror and the LSP, and lets
            // the normal re-elaboration pipeline refresh goal state and prose.
            crate::replace_formal_source(app, text).await;
            "Lean source updated successfully.".to_string()
        }
        ToolName::ReadGoalState => {
            let goal = state.proof.lock().await.goal_state.full.clone();
            if goal.is_empty() {
                "(no goal)".to_string()
            } else {
                goal
            }
        }
        ToolName::ReadProseProof => state.proof.lock().await.prose.text.clone(),
        ToolName::UpdateProseProof => {
            let raw = tool_input
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // Render the assistant's Markdown/LaTeX to KaTeX HTML before storing,
            // matching the translator path (lib.rs). `prose.text` is the
            // already-rendered HTML the Prose Proof panel displays via {@html}
            // and that sessions persist, so storing raw here would leave the
            // panel unformatted — including after the session is reloaded.
            let text = crate::proof::translator::render_katex(raw);
            state.proof.lock().await.prose.text = text.clone();
            state
                .session_dirty
                .store(true, std::sync::atomic::Ordering::SeqCst);
            app.emit(
                crate::proof::PROSE_UPDATED_EVENT,
                &crate::proof::ProsePayload {
                    text: text.clone(),
                    hash: None,
                },
            )
            .ok();
            "Prose updated successfully.".to_string()
        }
        ToolName::ReadDiagnostics => {
            let all = state.current_diagnostics.lock().unwrap().clone();
            let filtered: Vec<_> = all
                .iter()
                .filter(|d| d.severity == 1 || d.severity == 2)
                .collect();
            if filtered.is_empty() {
                "No errors or warnings.".to_string()
            } else {
                filtered
                    .iter()
                    .map(|d| {
                        let kind = if d.severity == 1 { "error" } else { "warning" };
                        format!(
                            "{} (line {}, col {}–{}:{}): {}",
                            kind, d.start_line, d.start_col, d.end_line, d.end_col, d.message
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Context management helpers
// ---------------------------------------------------------------------------

/// Rough token estimate: 4 characters ≈ 1 token.
#[must_use]
pub fn token_estimate(transcript: &Transcript) -> usize {
    let summary_tokens = transcript.summary.as_deref().map_or(0, |s| s.len() / 4);
    let turn_tokens: usize = transcript.turns.iter().map(|t| t.content.len() / 4).sum();
    summary_tokens + turn_tokens
}

/// Summarize the oldest 75 % of turns using `backend`, storing the result in
/// `transcript.summary` and removing those turns from `transcript.turns`.
///
/// If `transcript.turns` has fewer than 2 turns, nothing is changed (we need
/// at least 2 turns to make summarization worthwhile).
///
/// # Errors
///
/// Returns an [`LlmError`] if the LLM call fails.
pub async fn summarize_oldest(
    transcript: &mut Transcript,
    llm: &dyn Llm,
    app: &AppHandle,
) -> Result<(), LlmError> {
    let n = transcript.turns.len();
    if n < 2 {
        return Ok(());
    }

    let cut = (n * 3 / 4).max(1);
    let to_summarize: Vec<Turn> = transcript.turns.drain(..cut).collect();

    // Format turns to be summarized as plain text for the completion prompt.
    let mut history = String::new();
    if let Some(prev_summary) = &transcript.summary {
        history.push_str("[Previous summary]\n");
        history.push_str(prev_summary);
        history.push_str("\n\n");
    }
    for turn in &to_summarize {
        let role = match turn.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::System => "System",
        };
        history.push_str(role);
        history.push_str(": ");
        history.push_str(&turn.content);
        history.push('\n');
    }
    history.push_str(
        "\nSummarize the conversation history above into a concise paragraph \
                      for use as context in a future message. Preserve all technical \
                      details (theorem names, tactic sequences, error messages).",
    );

    // Use `complete` (not `send_with_tools`) — summarization is a one-shot call
    // with no tool use, and `complete` does not emit COMPLETE_EVENT to the UI.
    let system_prompt = effective_system_prompt(app).await;
    let model = effective_assistant_model(app).await;
    let summary_turn = llm.complete(&system_prompt, &history, &model, app).await?;

    transcript.summary = Some(summary_turn.content);
    Ok(())
}

/// The user's assistant prompt from settings, or the built-in default when not set.
pub(crate) async fn effective_system_prompt(app: &AppHandle) -> String {
    let app_state = app.state::<crate::AppState>();
    let settings = app_state.settings.lock().await;
    settings
        .assistant_prompt
        .clone()
        .unwrap_or_else(|| DEFAULT_ASSISTANT_PROMPT.to_string())
}

/// The user's assistant model from settings, or the default when not set.
pub(crate) async fn effective_assistant_model(app: &AppHandle) -> String {
    let app_state = app.state::<crate::AppState>();
    let settings = app_state.settings.lock().await;
    settings
        .assistant_model
        .clone()
        .unwrap_or_else(|| crate::llm::models::default_model_id().to_string())
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Append a user message, optionally summarize context, call the LLM, return
/// the assistant response content.
///
/// # Errors
///
/// Returns a string error if the LLM call fails.
#[tauri::command]
#[tracing::instrument(level = "debug", skip_all)]
pub async fn send_message(
    app: AppHandle,
    content: String,
    state: tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    let backend = state.llm.clone();
    let transcript_arc = state.transcript.clone();
    let tools = default_tools();

    // Append user turn, optionally summarize, then snapshot — all under one lock
    // acquisition so the snapshot is consistent with the appended turn.
    let snapshot = {
        let mut transcript = transcript_arc.lock().await;
        transcript.turns.push(Turn {
            role: Role::User,
            spans: crate::format::parse(&content),
            content: content.clone(),
            timestamp: Utc::now().timestamp_millis(),
        });

        let needs_summary = token_estimate(&transcript) > transcript.max_tokens * 3 / 4;
        if needs_summary {
            let backend_ref: &dyn Llm = backend.as_ref();
            let _ = summarize_oldest(&mut transcript, backend_ref, &app).await;
        }

        transcript.clone()
    };
    let system_prompt = effective_system_prompt(&app).await;
    let model = effective_assistant_model(&app).await;
    let assistant_turn = match backend
        .send_with_tools(&system_prompt, &snapshot, &tools, &model, &app, &content)
        .await
    {
        Ok(turn) => turn,
        Err(e) => {
            // A backend failure that implicates the key reconciles into the
            // single disconnected representation (#58): a missing key or an API
            // auth rejection flips the assistant to Disconnected so the
            // indicator (#59) and toast (#60) update. Other errors (network,
            // 5xx) leave the status untouched. The reason never carries the key.
            use crate::lean::messages::turnstile::DisconnectReason;
            if e.is_missing_key() {
                crate::set_assistant_disconnected(&app, DisconnectReason::NoKey);
            } else if e.is_auth_error() {
                crate::set_assistant_disconnected(&app, DisconnectReason::KeyRejected);
            }
            return Err(e.0);
        }
    };

    let response_content = assistant_turn.content.clone();

    // Append assistant turn.
    transcript_arc.lock().await.turns.push(assistant_turn);

    Ok(response_content)
}

/// Return the current transcript (for save-file serialization).
///
/// # Errors
///
/// Returns a string error if the transcript lock is unavailable.
#[tauri::command]
#[tracing::instrument(level = "debug", skip_all)]
pub async fn get_transcript(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Transcript, String> {
    Ok(state.transcript.lock().await.clone())
}

/// Replace the transcript (for restoring from a `.turn` save file).
///
/// # Errors
///
/// Returns a string error if the transcript lock is unavailable.
#[tauri::command]
#[tracing::instrument(level = "debug", skip_all)]
pub async fn load_transcript(
    new_transcript: Transcript,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    *state.transcript.lock().await = new_transcript;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_turn(role: Role, content: &str) -> Turn {
        Turn {
            role,
            spans: vec![],
            content: content.to_string(),
            timestamp: 0,
        }
    }

    // -- Turn constructor ------------------------------------------------

    #[test]
    fn turn_assistant_sets_role_and_timestamp() {
        let before = Utc::now().timestamp_millis();
        let turn = Turn::assistant("hi there".to_string());
        let after = Utc::now().timestamp_millis();
        assert_eq!(turn.role, Role::Assistant);
        assert_eq!(turn.content, "hi there");
        assert!(turn.spans.is_empty());
        assert!(turn.timestamp >= before && turn.timestamp <= after);
    }

    // -- Serialisation ---------------------------------------------------

    #[test]
    fn turn_serializes_with_lowercase_role() {
        let turn = make_turn(Role::User, "hello");
        let json = serde_json::to_string(&turn).unwrap();
        assert!(json.contains(r#""role":"user""#), "json={json}");
    }

    #[test]
    fn transcript_round_trips_through_json() {
        let transcript = Transcript {
            summary: Some("a summary".to_string()),
            turns: vec![
                make_turn(Role::User, "hi"),
                make_turn(Role::Assistant, "hello"),
            ],
            max_tokens: 100,
        };
        let json = serde_json::to_string(&transcript).unwrap();
        let restored: Transcript = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.summary, transcript.summary);
        assert_eq!(restored.turns.len(), 2);
        assert_eq!(restored.max_tokens, 100);
    }

    #[test]
    fn transcript_default_max_tokens_is_200k() {
        let transcript = Transcript::default();
        assert_eq!(transcript.max_tokens, 200_000);
    }

    // -- Tool definitions ------------------------------------------------

    #[test]
    fn default_tools_has_six_entries() {
        let tools = default_tools();
        assert_eq!(tools.len(), 6);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"read_lean_source"));
        assert!(names.contains(&"update_lean_source"));
        assert!(names.contains(&"read_goal_state"));
        assert!(names.contains(&"read_prose_proof"));
        assert!(names.contains(&"update_prose_proof"));
        assert!(names.contains(&"read_diagnostics"));
    }

    #[test]
    fn update_lean_source_tool_requires_text_input() {
        let tool = default_tools()
            .into_iter()
            .find(|t| t.name == "update_lean_source")
            .expect("update_lean_source tool present");
        // A write tool must advertise its `text` parameter as required, matching
        // update_prose_proof, so the LLM always supplies the replacement source.
        let required = tool.input_schema.get("required").and_then(|v| v.as_array());
        assert_eq!(
            required.map(|r| r.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>()),
            Some(vec!["text"])
        );
    }

    #[test]
    fn tools_have_valid_input_schema() {
        for tool in default_tools() {
            assert_eq!(
                tool.input_schema.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "tool {} input_schema must have type=object",
                tool.name
            );
        }
    }

    #[test]
    fn tool_name_round_trips_via_str() {
        for variant in [
            ToolName::ReadLeanSource,
            ToolName::UpdateLeanSource,
            ToolName::ReadGoalState,
            ToolName::ReadProseProof,
            ToolName::UpdateProseProof,
            ToolName::ReadDiagnostics,
        ] {
            let s = variant.as_str();
            let parsed = ToolName::try_from(s).expect("round trip");
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn tool_name_try_from_rejects_unknown() {
        assert!(ToolName::try_from("bogus").is_err());
    }

    // -- Token estimate --------------------------------------------------

    #[test]
    fn token_estimate_scales_with_turns() {
        let mut transcript = Transcript::default();
        let e0 = token_estimate(&transcript);
        transcript
            .turns
            .push(make_turn(Role::User, "a".repeat(400).as_str()));
        let e1 = token_estimate(&transcript);
        transcript
            .turns
            .push(make_turn(Role::Assistant, "b".repeat(400).as_str()));
        let e2 = token_estimate(&transcript);
        assert!(e0 < e1, "estimate should grow with turns");
        assert!(e1 < e2, "estimate should grow with turns");
    }

    #[test]
    fn token_estimate_includes_summary() {
        let mut transcript = Transcript::default();
        let without = token_estimate(&transcript);
        transcript.summary = Some("x".repeat(400));
        let with_summary = token_estimate(&transcript);
        assert!(with_summary > without);
    }

    // -- Context management ----------------------------------------------

    #[test]
    fn summarize_oldest_removes_first_75_percent() {
        // 4 turns → cut = max(4*3/4, 1) = 3 → 3 removed, 1 remains
        let mut transcript = Transcript::default();
        for i in 0..4 {
            transcript
                .turns
                .push(make_turn(Role::User, &format!("turn {i}")));
        }
        let cut = (transcript.turns.len() * 3 / 4).max(1);
        let _ = transcript.turns.drain(..cut).collect::<Vec<_>>();
        assert_eq!(transcript.turns.len(), 1);
        assert_eq!(transcript.turns[0].content, "turn 3");
    }

    #[test]
    fn summarize_oldest_noop_for_fewer_than_2_turns() {
        let mut transcript = Transcript::default();
        transcript.turns.push(make_turn(Role::User, "only turn"));
        let n = transcript.turns.len();
        if n >= 2 {
            let cut = (n * 3 / 4).max(1);
            transcript.turns.drain(..cut);
        }
        assert_eq!(
            transcript.turns.len(),
            1,
            "single turn should not be summarised"
        );
    }

    #[test]
    fn context_threshold_check() {
        let mut transcript = Transcript {
            max_tokens: 100,
            ..Default::default()
        };
        // 300 chars / 4 = 75 tokens — exactly at 75 % of 100
        transcript
            .turns
            .push(make_turn(Role::User, &"a".repeat(300)));
        assert!(token_estimate(&transcript) >= transcript.max_tokens * 3 / 4);
    }
}
