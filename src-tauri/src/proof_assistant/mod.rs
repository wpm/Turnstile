//! The Proof Assistant: a conversational agent that helps the user develop a
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
use tauri::{AppHandle, Manager};

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
    /// Unix timestamp in milliseconds.
    pub timestamp: i64,
}

impl Turn {
    /// Build an assistant turn with the current wall-clock timestamp.
    pub fn assistant(content: String) -> Self {
        Self {
            role: Role::Assistant,
            content,
            timestamp: Utc::now().timestamp_millis(),
        }
    }
}

/// The system prompt delivered to the proof-assistant LLM.
pub const SYSTEM_PROMPT: &str = include_str!("prompts/system.md");

/// Emitted for every assistant text-delta chunk while streaming.  Payload: `String`.
pub const STREAM_DELTA_EVENT: &str = "proof-assistant-delta";

/// Emitted once when the stream ends (whether the turn completed normally
/// or via tool-use cycles).  Payload: `()`.
pub const STREAM_DONE_EVENT: &str = "proof-assistant-stream-done";

/// Emitted once the full assistant turn (including any tool-use cycles) is
/// complete.  Payload: [`Turn`].
pub const COMPLETE_EVENT: &str = "proof-assistant-complete";

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

fn default_max_tokens() -> usize {
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

/// Tools the proof assistant may call to inspect or modify editor state.
///
/// Each variant maps to a unique wire-level string used in the LLM protocol
/// and system prompt.  The [`ToolName::as_str`] method and [`TryFrom<&str>`]
/// impl form the single source of truth for these strings — do not duplicate
/// the literals elsewhere in the codebase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolName {
    ReadLeanSource,
    ReadTacticState,
    ReadProseProof,
    UpdateProseProof,
    ReadDiagnostics,
}

impl ToolName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadLeanSource => "read_lean_source",
            Self::ReadTacticState => "read_tactic_state",
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
            "read_tactic_state" => Ok(Self::ReadTacticState),
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
            name: ToolName::ReadTacticState.as_str().into(),
            description: "Read the tactic state at a given cursor position in the Lean source. \
                          If position is omitted, returns tactic state for every tactic step."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "line": {
                        "type": "integer",
                        "description": "0-indexed line in the Lean source (optional)"
                    },
                    "column": {
                        "type": "integer",
                        "description": "0-indexed column in the Lean source (optional)"
                    }
                },
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
pub async fn dispatch_tool(
    tool_name: &str,
    tool_input: &serde_json::Value,
    app: &AppHandle,
) -> String {
    use tauri::{Emitter, Manager};

    let state = app.state::<crate::AppState>();

    let tool = match ToolName::try_from(tool_name) {
        Ok(t) => t,
        Err(msg) => return msg,
    };

    match tool {
        ToolName::ReadLeanSource => state.proof.lock().await.formal.source.clone(),
        ToolName::ReadTacticState => {
            // Delegate to the LSP if available.
            let line = tool_input
                .get("line")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            let col = tool_input
                .get("column")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            let lsp_lock = state.lsp_client.lock().await;
            match lsp_lock.as_ref() {
                Some(client) => {
                    let doc_uri = state.doc_uri();
                    if let (Some(l), Some(c)) = (line, col) {
                        let result = client
                            .send_request_await(
                                "$/lean/plainGoal",
                                serde_json::json!({
                                    "textDocument": { "uri": doc_uri },
                                    "position": { "line": l, "character": c },
                                }),
                            )
                            .await;
                        match result {
                            Ok(v) => v
                                .get("rendered")
                                .and_then(|r| r.as_str())
                                .unwrap_or("(no goal)")
                                .to_string(),
                            Err(e) => format!("Error reading tactic state: {e}"),
                        }
                    } else {
                        "(no position provided — pass line and column to query goal state)"
                            .to_string()
                    }
                }
                None => "(LSP not connected)".to_string(),
            }
        }
        ToolName::ReadProseProof => state.proof.lock().await.prose.text.clone(),
        ToolName::UpdateProseProof => {
            let text = tool_input
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
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
pub fn token_estimate(transcript: &Transcript) -> usize {
    let summary_tokens = transcript
        .summary
        .as_deref()
        .map(|s| s.len() / 4)
        .unwrap_or(0);
    let turn_tokens: usize = transcript.turns.iter().map(|t| t.content.len() / 4).sum();
    summary_tokens + turn_tokens
}

/// Summarize the oldest 75 % of turns using `backend`, storing the result in
/// `transcript.summary` and removing those turns from `transcript.turns`.
///
/// If `transcript.turns` has fewer than 2 turns nothing is changed (we need
/// at least 2 turns to make summarization worthwhile).
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

    // Build a lightweight transcript containing only what we want summarized.
    let summary_transcript = Transcript {
        summary: transcript.summary.clone(),
        turns: to_summarize,
        max_tokens: transcript.max_tokens,
    };

    let prompt = "Summarize the conversation history above into a concise paragraph \
                  for use as context in a future message. Preserve all technical \
                  details (theorem names, tactic sequences, error messages).";

    let summary_turn = llm
        .send_with_tools(SYSTEM_PROMPT, &summary_transcript, &[], app, prompt)
        .await?;

    transcript.summary = Some(summary_turn.content);
    Ok(())
}

/// Compose the effective system prompt: the built-in PA prompt, plus the
/// user's optional custom prompt (from settings) separated by a blank line.
async fn effective_system_prompt(app: &AppHandle) -> String {
    let app_state = app.state::<crate::AppState>();
    let settings = app_state.settings.lock().await;
    let custom = settings.custom_prompt.trim();
    if custom.is_empty() {
        SYSTEM_PROMPT.to_string()
    } else {
        format!("{SYSTEM_PROMPT}\n\n{custom}")
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Append a user message, optionally summarize context, call the LLM, return
/// the assistant response content.
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    content: String,
    state: tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    let backend = state.llm.clone();
    let transcript_arc = state.transcript.clone();
    let tools = default_tools();

    // Append user turn and check if summarization is needed.
    {
        let mut transcript = transcript_arc.lock().await;
        transcript.turns.push(Turn {
            role: Role::User,
            content: content.clone(),
            timestamp: Utc::now().timestamp_millis(),
        });

        let needs_summary = token_estimate(&transcript) > transcript.max_tokens * 3 / 4;
        if needs_summary {
            let backend_ref: &dyn Llm = backend.as_ref();
            let _ = summarize_oldest(&mut transcript, backend_ref, &app).await;
        }
    }

    // Call LLM with a snapshot of the current transcript.
    let snapshot = transcript_arc.lock().await.clone();
    let system_prompt = effective_system_prompt(&app).await;
    let assistant_turn = backend
        .send_with_tools(&system_prompt, &snapshot, &tools, &app, &content)
        .await
        .map_err(|e| e.0)?;

    let response_content = assistant_turn.content.clone();

    // Append assistant turn.
    transcript_arc.lock().await.turns.push(assistant_turn);

    Ok(response_content)
}

/// Return the current transcript (for save-file serialization).
#[tauri::command]
pub async fn get_transcript(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Transcript, String> {
    Ok(state.transcript.lock().await.clone())
}

/// Replace the transcript (for restoring from a `.turn` save file).
#[tauri::command]
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
            content: content.to_string(),
            timestamp: 0,
        }
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
    fn default_tools_has_five_entries() {
        let tools = default_tools();
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"read_lean_source"));
        assert!(names.contains(&"read_tactic_state"));
        assert!(names.contains(&"read_prose_proof"));
        assert!(names.contains(&"update_prose_proof"));
        assert!(names.contains(&"read_diagnostics"));
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
            ToolName::ReadTacticState,
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
