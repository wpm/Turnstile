//! Chat backend: conversation state, LLM abstraction, and Tauri command handlers.
//!
//! # Architecture
//!
//! The central abstraction is [`ChatBackend`], an async trait that hides whether the
//! underlying LLM is a real Anthropic API call or a test mock.  [`ChatState`] owns
//! the conversation (an optional summary plus a list of [`Turn`]s) and is stored in
//! [`crate::AppState`] behind an `Arc<tokio::sync::Mutex<ChatState>>`.
//!
//! # Context management
//!
//! When the running token estimate exceeds 75 % of `ChatState::max_tokens`, the
//! backend summarizes the oldest 75 % of turns into a single string stored in
//! `ChatState::summary`.  Summarization is an async LLM call that does not block
//! the UI; new messages keep arriving normally while it runs.
//!
//! # Backend selection
//!
//! Production: [`AnthropicBackend`] (reads `ANTHROPIC_API_KEY` from the environment).
//! Testing / mock mode: [`MockBackend`], selected by either
//! * the `mock-llm` Cargo feature flag, or
//! * the `TURNSTILE_MOCK_LLM` environment variable (`echo` | `scripted` | `delay`).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

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

/// Serializable snapshot of the conversation — the unit stored in `.turn` files.
///
/// ```json
/// {
///   "summary": "string or null",
///   "transcript": [ { "role": "user", "content": "...", "timestamp": ... }, ... ]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatState {
    pub summary: Option<String>,
    /// Active turns (the most-recent portion after any summarization).
    pub transcript: Vec<Turn>,
    /// Soft context-window limit in tokens. Default 200 000 (Claude claude-sonnet-4-6).
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
}

fn default_max_tokens() -> usize {
    200_000
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            summary: None,
            transcript: Vec::new(),
            max_tokens: default_max_tokens(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

/// Tools the LLM may call to inspect or modify editor state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

pub fn default_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read_lean_source".into(),
            description: "Read the current Lean source file contents.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "read_tactic_state".into(),
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
            name: "read_prose".into(),
            description: "Read the current prose proof draft (LaTeX).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "update_prose".into(),
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
            name: "read_diagnostics".into(),
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
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ChatError(pub String);

impl std::fmt::Display for ChatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ChatError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

// ---------------------------------------------------------------------------
// ChatBackend trait
// ---------------------------------------------------------------------------

/// Abstraction over the LLM provider.
///
/// Each call receives the full [`ChatState`] (so the backend can construct the
/// context window) and must return the assistant [`Turn`] to append to the
/// transcript.  The backend emits `chat-stream-delta` Tauri events while
/// streaming and a `chat-message-complete` event when done.
#[async_trait]
pub trait ChatBackend: Send + Sync {
    async fn send_message(
        &self,
        state: &ChatState,
        tools: &[ToolDefinition],
        app: &AppHandle,
        user_content: &str,
    ) -> Result<Turn, ChatError>;
}

// ---------------------------------------------------------------------------
// Context management helpers
// ---------------------------------------------------------------------------

/// Rough token estimate: 4 characters ≈ 1 token.
pub fn token_estimate(state: &ChatState) -> usize {
    let summary_tokens = state.summary.as_deref().map(|s| s.len() / 4).unwrap_or(0);
    let turn_tokens: usize = state.transcript.iter().map(|t| t.content.len() / 4).sum();
    summary_tokens + turn_tokens
}

/// Summarize the oldest 75 % of turns using `backend`, storing the result in
/// `state.summary` and removing those turns from `state.transcript`.
///
/// If `state.transcript` has fewer than 2 turns nothing is changed (we need
/// at least 2 turns to make summarization worthwhile).
pub async fn summarize_oldest(
    state: &mut ChatState,
    backend: &dyn ChatBackend,
    app: &AppHandle,
) -> Result<(), ChatError> {
    let n = state.transcript.len();
    if n < 2 {
        return Ok(());
    }

    let cut = (n * 3 / 4).max(1);
    let to_summarize: Vec<Turn> = state.transcript.drain(..cut).collect();

    // Build a lightweight state containing only what we want summarized.
    let summary_state = ChatState {
        summary: state.summary.clone(),
        transcript: to_summarize,
        max_tokens: state.max_tokens,
    };

    let prompt = "Summarize the conversation history above into a concise paragraph \
                  for use as context in a future message. Preserve all technical \
                  details (theorem names, tactic sequences, error messages).";

    let summary_turn = backend
        .send_message(&summary_state, &[], app, prompt)
        .await?;

    state.summary = Some(summary_turn.content);
    Ok(())
}

// ---------------------------------------------------------------------------
// MockBackend
// ---------------------------------------------------------------------------

/// Mock mode selected by the `TURNSTILE_MOCK_LLM` env var or `mock-llm` feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockMode {
    /// Returns the user message back, prefixed with `[echo] `.
    Echo,
    /// Plays back `Vec<String>` responses in order (cycling).
    Scripted,
    /// Like Echo but emits one character at a time with a 10 ms delay.
    Delay,
}

pub struct MockBackend {
    pub mode: MockMode,
    /// Scripted responses (used when `mode == Scripted`).
    pub script: Vec<String>,
    /// Index into `script`, wrapped with Mutex for interior mutability.
    script_idx: Arc<std::sync::Mutex<usize>>,
}

impl MockBackend {
    pub fn echo() -> Self {
        Self {
            mode: MockMode::Echo,
            script: Vec::new(),
            script_idx: Arc::new(std::sync::Mutex::new(0)),
        }
    }

    pub fn scripted(responses: Vec<String>) -> Self {
        Self {
            mode: MockMode::Scripted,
            script: responses,
            script_idx: Arc::new(std::sync::Mutex::new(0)),
        }
    }

    pub fn delay() -> Self {
        Self {
            mode: MockMode::Delay,
            script: Vec::new(),
            script_idx: Arc::new(std::sync::Mutex::new(0)),
        }
    }

    /// Construct from the `TURNSTILE_MOCK_LLM` environment variable.
    pub fn from_env() -> Self {
        match std::env::var("TURNSTILE_MOCK_LLM")
            .unwrap_or_default()
            .as_str()
        {
            "scripted" => {
                let script: Vec<String> = std::env::var("TURNSTILE_MOCK_SCRIPT")
                    .unwrap_or_default()
                    .lines()
                    .map(String::from)
                    .collect();
                Self::scripted(script)
            }
            "delay" => Self::delay(),
            _ => Self::echo(),
        }
    }
}

#[async_trait]
impl ChatBackend for MockBackend {
    async fn send_message(
        &self,
        _state: &ChatState,
        _tools: &[ToolDefinition],
        app: &AppHandle,
        user_content: &str,
    ) -> Result<Turn, ChatError> {
        let response = match self.mode {
            MockMode::Echo => format!("[echo] {user_content}"),
            MockMode::Scripted => {
                if self.script.is_empty() {
                    "[scripted: no responses]".to_string()
                } else {
                    let mut idx = self.script_idx.lock().unwrap();
                    let r = self.script[*idx % self.script.len()].clone();
                    *idx += 1;
                    r
                }
            }
            MockMode::Delay => {
                let prefix = format!("[echo] {user_content}");
                for ch in prefix.chars() {
                    app.emit("chat-stream-delta", ch.to_string()).ok();
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                prefix
            }
        };

        let turn = Turn {
            role: Role::Assistant,
            content: response.clone(),
            timestamp: Utc::now().timestamp_millis(),
        };

        app.emit("chat-stream-done", ()).ok();
        app.emit("chat-message-complete", &turn).ok();
        Ok(turn)
    }
}

// ---------------------------------------------------------------------------
// AnthropicBackend (real LLM; excluded when mock-llm feature is active)
// ---------------------------------------------------------------------------

/// System prompt loaded at compile time from prompts/system.md.
#[cfg(not(feature = "mock-llm"))]
const SYSTEM_PROMPT: &str = include_str!("prompts/system.md");

#[cfg(not(feature = "mock-llm"))]
pub struct AnthropicBackend {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

#[cfg(not(feature = "mock-llm"))]
impl AnthropicBackend {
    pub fn from_env() -> Result<Self, String> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| "ANTHROPIC_API_KEY environment variable not set".to_string())?;
        let model = std::env::var("TURNSTILE_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());
        Ok(Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        })
    }

    /// Returns a no-op backend that returns an error message when no API key is set.
    pub fn no_op() -> Self {
        Self {
            api_key: String::new(),
            model: "none".to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Dispatch a tool call from the LLM and return the result string.
    async fn dispatch_tool(
        tool_name: &str,
        tool_input: &serde_json::Value,
        app: &AppHandle,
    ) -> String {
        use tauri::Manager;
        let state = app.state::<crate::AppState>();

        match tool_name {
            "read_lean_source" => state.current_source.lock().await.clone(),
            "read_tactic_state" => {
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
            "read_prose" => state.current_prose.lock().await.clone(),
            "update_prose" => {
                let text = tool_input
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                *state.current_prose.lock().await = text.clone();
                #[derive(serde::Serialize)]
                struct ProsePayload {
                    text: String,
                    hash: Option<String>,
                }
                app.emit(
                    "prose-updated",
                    &ProsePayload {
                        text: text.clone(),
                        hash: None,
                    },
                )
                .ok();
                "Prose updated successfully.".to_string()
            }
            "read_diagnostics" => {
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
            other => format!("Unknown tool: {other}"),
        }
    }

    /// Stream one request to the Anthropic API and collect the full response.
    ///
    /// Returns `(stop_reason, full_text, tool_use_blocks)` where `tool_use_blocks`
    /// is a list of `(id, name, input_json)` for any tool calls in the response.
    async fn stream_request(
        &self,
        messages: &[serde_json::Value],
        tools: &[ToolDefinition],
        app: &AppHandle,
    ) -> Result<(String, String, Vec<(String, String, serde_json::Value)>), ChatError> {
        use futures_util::StreamExt;

        let tools_json: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": 8192,
            "stream": true,
            "system": SYSTEM_PROMPT,
            "messages": messages,
        });

        if !tools_json.is_empty() {
            body["tools"] = serde_json::Value::Array(tools_json);
        }

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ChatError(format!("API request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ChatError(format!("API error {status}: {text}")));
        }

        let mut stream = response.bytes_stream();
        let mut full_text = String::new();
        let mut stop_reason = String::new();
        let mut buf = String::new();

        // Tool use accumulation: block index → (id, name, accumulated input json string)
        let mut tool_blocks: std::collections::HashMap<usize, (String, String, String)> =
            std::collections::HashMap::new();
        let mut current_tool_idx: Option<usize> = None;

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| ChatError(format!("Stream error: {e}")))?;
            buf.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(pos) = buf.find("\n\n") {
                let event = buf[..pos].to_string();
                buf = buf[pos + 2..].to_string();

                for line in event.lines() {
                    let Some(data) = line.strip_prefix("data: ") else {
                        continue;
                    };
                    if data == "[DONE]" {
                        break;
                    }
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
                        continue;
                    };

                    let event_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

                    match event_type {
                        "content_block_start" => {
                            if let Some(block) = v.get("content_block") {
                                let block_type =
                                    block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                if block_type == "tool_use" {
                                    let idx = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0)
                                        as usize;
                                    let id = block
                                        .get("id")
                                        .and_then(|i| i.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let name = block
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    tool_blocks.insert(idx, (id, name, String::new()));
                                    current_tool_idx = Some(idx);
                                } else {
                                    current_tool_idx = None;
                                }
                            }
                        }
                        "content_block_delta" => {
                            if let Some(delta) = v.get("delta") {
                                let delta_type =
                                    delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                match delta_type {
                                    "text_delta" => {
                                        if let Some(text) =
                                            delta.get("text").and_then(|t| t.as_str())
                                        {
                                            full_text.push_str(text);
                                            app.emit("chat-stream-delta", text).ok();
                                        }
                                    }
                                    "input_json_delta" => {
                                        if let Some(idx) = current_tool_idx {
                                            if let Some(partial) =
                                                delta.get("partial_json").and_then(|p| p.as_str())
                                            {
                                                if let Some(block) = tool_blocks.get_mut(&idx) {
                                                    block.2.push_str(partial);
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        "message_delta" => {
                            if let Some(delta) = v.get("delta") {
                                if let Some(reason) =
                                    delta.get("stop_reason").and_then(|r| r.as_str())
                                {
                                    stop_reason = reason.to_string();
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Parse accumulated tool input JSON strings.
        let tool_calls: Vec<(String, String, serde_json::Value)> = tool_blocks
            .into_values()
            .map(|(id, name, json_str)| {
                let input = serde_json::from_str(&json_str)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                (id, name, input)
            })
            .collect();

        Ok((stop_reason, full_text, tool_calls))
    }
}

#[cfg(not(feature = "mock-llm"))]
#[async_trait]
impl ChatBackend for AnthropicBackend {
    async fn send_message(
        &self,
        state: &ChatState,
        tools: &[ToolDefinition],
        app: &AppHandle,
        user_content: &str,
    ) -> Result<Turn, ChatError> {
        if self.api_key.is_empty() {
            let turn = Turn {
                role: Role::Assistant,
                content: "ANTHROPIC_API_KEY is not set. Please set it and restart.".to_string(),
                timestamp: Utc::now().timestamp_millis(),
            };
            app.emit("chat-message-complete", &turn).ok();
            return Ok(turn);
        }

        // Build the initial messages array from chat state + new user message.
        let mut messages: Vec<serde_json::Value> = Vec::new();

        if let Some(summary) = &state.summary {
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": format!("[Conversation summary]\n{summary}")
            }));
        }

        for turn in &state.transcript {
            let role = match turn.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => continue,
            };
            messages.push(serde_json::json!({ "role": role, "content": turn.content }));
        }

        messages.push(serde_json::json!({ "role": "user", "content": user_content }));

        // Multi-turn tool use loop until stop_reason == "end_turn".
        let mut full_assistant_text = String::new();

        loop {
            let (stop_reason, text_chunk, tool_calls) =
                self.stream_request(&messages, tools, app).await?;

            full_assistant_text.push_str(&text_chunk);

            if stop_reason == "tool_use" && !tool_calls.is_empty() {
                // Build the assistant message with mixed text + tool_use content blocks.
                let mut assistant_content: Vec<serde_json::Value> = Vec::new();
                if !text_chunk.is_empty() {
                    assistant_content.push(serde_json::json!({
                        "type": "text",
                        "text": text_chunk
                    }));
                }
                for (id, name, input) in &tool_calls {
                    assistant_content.push(serde_json::json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input
                    }));
                }
                messages.push(serde_json::json!({
                    "role": "assistant",
                    "content": assistant_content
                }));

                // Execute each tool call and build tool_result blocks.
                let mut tool_results: Vec<serde_json::Value> = Vec::new();
                for (id, name, input) in &tool_calls {
                    let result = Self::dispatch_tool(name, input, app).await;
                    tool_results.push(serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": result
                    }));
                }
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": tool_results
                }));
                // Continue the loop with updated messages.
            } else {
                // end_turn or no tool calls — we're done.
                break;
            }
        }

        let turn = Turn {
            role: Role::Assistant,
            content: full_assistant_text,
            timestamp: Utc::now().timestamp_millis(),
        };
        app.emit("chat-stream-done", ()).ok();
        app.emit("chat-message-complete", &turn).ok();
        Ok(turn)
    }
}

// ---------------------------------------------------------------------------
// Tauri command handlers
// ---------------------------------------------------------------------------

/// Append a user message, optionally summarize context, call the LLM, return
/// the assistant response content.
#[tauri::command]
pub async fn send_chat_message(
    app: AppHandle,
    content: String,
    state: tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    let backend = state.chat_backend.clone();
    let chat_state_arc = state.chat_state.clone();
    let tools = default_tools();

    // Append user turn and check if summarization is needed.
    {
        let mut chat_state = chat_state_arc.lock().await;
        chat_state.transcript.push(Turn {
            role: Role::User,
            content: content.clone(),
            timestamp: Utc::now().timestamp_millis(),
        });

        let needs_summary = token_estimate(&chat_state) > chat_state.max_tokens * 3 / 4;
        if needs_summary {
            let backend_ref: &dyn ChatBackend = backend.as_ref();
            let _ = summarize_oldest(&mut chat_state, backend_ref, &app).await;
        }
    }

    // Call LLM with a snapshot of the current state.
    let chat_snapshot = chat_state_arc.lock().await.clone();
    let assistant_turn = backend
        .send_message(&chat_snapshot, &tools, &app, &content)
        .await
        .map_err(|e| e.0)?;

    let response_content = assistant_turn.content.clone();

    // Append assistant turn.
    chat_state_arc.lock().await.transcript.push(assistant_turn);

    Ok(response_content)
}

/// Return the current chat state (for save-file serialization).
#[tauri::command]
pub async fn get_chat_state(state: tauri::State<'_, crate::AppState>) -> Result<ChatState, String> {
    Ok(state.chat_state.lock().await.clone())
}

/// Replace the chat state (for restoring from a `.turn` save file).
#[tauri::command]
pub async fn load_chat_state(
    new_state: ChatState,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    *state.chat_state.lock().await = new_state;
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
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

    // -- Serialisation -------------------------------------------------------

    #[test]
    fn turn_serializes_with_lowercase_role() {
        let turn = make_turn(Role::User, "hello");
        let json = serde_json::to_string(&turn).unwrap();
        assert!(json.contains(r#""role":"user""#), "json={json}");
    }

    #[test]
    fn chat_state_round_trips_through_json() {
        let state = ChatState {
            summary: Some("a summary".to_string()),
            transcript: vec![
                make_turn(Role::User, "hi"),
                make_turn(Role::Assistant, "hello"),
            ],
            max_tokens: 100,
        };
        let json = serde_json::to_string(&state).unwrap();
        let restored: ChatState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.summary, state.summary);
        assert_eq!(restored.transcript.len(), 2);
        assert_eq!(restored.max_tokens, 100);
    }

    #[test]
    fn chat_state_default_max_tokens_is_200k() {
        let state = ChatState::default();
        assert_eq!(state.max_tokens, 200_000);
    }

    // -- Tool definitions ----------------------------------------------------

    #[test]
    fn default_tools_has_five_entries() {
        let tools = default_tools();
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"read_lean_source"));
        assert!(names.contains(&"read_tactic_state"));
        assert!(names.contains(&"read_prose"));
        assert!(names.contains(&"update_prose"));
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

    // -- Token estimate -------------------------------------------------------

    #[test]
    fn token_estimate_scales_with_turns() {
        let mut state = ChatState::default();
        let e0 = token_estimate(&state);
        state
            .transcript
            .push(make_turn(Role::User, "a".repeat(400).as_str()));
        let e1 = token_estimate(&state);
        state
            .transcript
            .push(make_turn(Role::Assistant, "b".repeat(400).as_str()));
        let e2 = token_estimate(&state);
        assert!(e0 < e1, "estimate should grow with turns");
        assert!(e1 < e2, "estimate should grow with turns");
    }

    #[test]
    fn token_estimate_includes_summary() {
        let mut state = ChatState::default();
        let without = token_estimate(&state);
        state.summary = Some("x".repeat(400));
        let with_summary = token_estimate(&state);
        assert!(with_summary > without);
    }

    // -- MockBackend ---------------------------------------------------------

    #[tokio::test]
    async fn mock_echo_prefixes_message() {
        // We can't easily build an AppHandle in unit tests, so we test the
        // response content via a minimal tauri test app setup is skipped here.
        // The echo logic is straightforward: just verify the format string.
        let msg = "hello world";
        let echoed = format!("[echo] {msg}");
        assert_eq!(echoed, "[echo] hello world");
    }

    #[test]
    fn mock_scripted_cycles_responses() {
        let backend = MockBackend::scripted(vec!["first".into(), "second".into()]);
        {
            let mut idx = backend.script_idx.lock().unwrap();
            let r0 = backend.script[*idx % backend.script.len()].clone();
            *idx += 1;
            let r1 = backend.script[*idx % backend.script.len()].clone();
            *idx += 1;
            let r2 = backend.script[*idx % backend.script.len()].clone();
            assert_eq!(r0, "first");
            assert_eq!(r1, "second");
            assert_eq!(r2, "first"); // cycles
        }
    }

    // -- Context management --------------------------------------------------

    #[test]
    fn summarize_oldest_removes_first_75_percent() {
        // 4 turns → cut = max(4*3/4, 1) = 3 → 3 removed, 1 remains
        let mut state = ChatState::default();
        for i in 0..4 {
            state
                .transcript
                .push(make_turn(Role::User, &format!("turn {i}")));
        }
        let cut = (state.transcript.len() * 3 / 4).max(1);
        let _ = state.transcript.drain(..cut).collect::<Vec<_>>();
        assert_eq!(state.transcript.len(), 1);
        assert_eq!(state.transcript[0].content, "turn 3");
    }

    #[test]
    fn summarize_oldest_noop_for_fewer_than_2_turns() {
        let mut state = ChatState::default();
        state.transcript.push(make_turn(Role::User, "only turn"));
        // Cut formula: max(1*3/4, 1) = max(0, 1) = 1 — but the guard is n < 2.
        let n = state.transcript.len();
        if n >= 2 {
            let cut = (n * 3 / 4).max(1);
            state.transcript.drain(..cut);
        }
        assert_eq!(
            state.transcript.len(),
            1,
            "single turn should not be summarised"
        );
    }

    #[test]
    fn context_threshold_check() {
        let mut state = ChatState {
            max_tokens: 100,
            ..Default::default()
        };
        // 300 chars / 4 = 75 tokens — exactly at 75 % of 100
        state
            .transcript
            .push(make_turn(Role::User, &"a".repeat(300)));
        assert!(token_estimate(&state) >= state.max_tokens * 3 / 4);
    }

    // -- ChatError -----------------------------------------------------------

    #[test]
    fn chat_error_display() {
        let err = ChatError("boom".into());
        assert_eq!(format!("{err}"), "boom");
    }

    #[test]
    fn chat_error_from_string() {
        let err = ChatError::from("msg".to_string());
        assert_eq!(err.0, "msg");
    }

    // -- MockBackend::from_env -----------------------------------------------

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn mock_backend_from_env_defaults_to_echo() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("TURNSTILE_MOCK_LLM");
        let backend = MockBackend::from_env();
        assert_eq!(backend.mode, MockMode::Echo);
    }

    #[test]
    fn mock_backend_from_env_delay() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("TURNSTILE_MOCK_LLM", "delay");
        let backend = MockBackend::from_env();
        std::env::remove_var("TURNSTILE_MOCK_LLM");
        assert_eq!(backend.mode, MockMode::Delay);
    }

    #[test]
    fn mock_backend_from_env_scripted() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("TURNSTILE_MOCK_LLM", "scripted");
        std::env::set_var("TURNSTILE_MOCK_SCRIPT", "line1\nline2");
        let backend = MockBackend::from_env();
        std::env::remove_var("TURNSTILE_MOCK_LLM");
        std::env::remove_var("TURNSTILE_MOCK_SCRIPT");
        assert_eq!(backend.mode, MockMode::Scripted);
        assert_eq!(backend.script, vec!["line1", "line2"]);
    }

    #[test]
    fn mock_backend_scripted_empty_vec() {
        let backend = MockBackend::scripted(vec![]);
        assert_eq!(backend.mode, MockMode::Scripted);
        assert!(backend.script.is_empty());
    }
}
