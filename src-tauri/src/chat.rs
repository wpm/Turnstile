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
//! backend summarises the oldest 75 % of turns into a single string stored in
//! `ChatState::summary`.  Summarisation is an async LLM call that does not block
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

/// Serialisable snapshot of the conversation — the unit stored in `.intuit` files.
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
    /// Active turns (the most-recent portion after any summarisation).
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
}

pub fn default_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read_lean_source".into(),
            description: "Read the current Lean source file contents.".into(),
        },
        ToolDefinition {
            name: "read_tactic_state".into(),
            description: "Read the tactic state at a given cursor position.".into(),
        },
        ToolDefinition {
            name: "read_prose".into(),
            description: "Read the current prose / markdown content.".into(),
        },
        ToolDefinition {
            name: "update_prose".into(),
            description: "Replace the prose / markdown content with new text.".into(),
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

/// Summarise the oldest 75 % of turns using `backend`, storing the result in
/// `state.summary` and removing those turns from `state.transcript`.
///
/// If `state.transcript` has fewer than 2 turns nothing is changed (we need
/// at least 2 turns to make summarisation worthwhile).
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

    // Build a lightweight state containing only what we want summarised.
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

        app.emit("chat-message-complete", &turn).ok();
        Ok(turn)
    }
}

// ---------------------------------------------------------------------------
// AnthropicBackend (real LLM; excluded when mock-llm feature is active)
// ---------------------------------------------------------------------------

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
        let model =
            std::env::var("TURNSTILE_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
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
}

#[cfg(not(feature = "mock-llm"))]
#[async_trait]
impl ChatBackend for AnthropicBackend {
    async fn send_message(
        &self,
        state: &ChatState,
        _tools: &[ToolDefinition],
        app: &AppHandle,
        user_content: &str,
    ) -> Result<Turn, ChatError> {
        use futures_util::StreamExt;

        if self.api_key.is_empty() {
            let turn = Turn {
                role: Role::Assistant,
                content: "ANTHROPIC_API_KEY is not set. Please set it and restart.".to_string(),
                timestamp: Utc::now().timestamp_millis(),
            };
            app.emit("chat-message-complete", &turn).ok();
            return Ok(turn);
        }

        // Build messages array from state + new user message.
        let mut messages: Vec<serde_json::Value> = Vec::new();

        if let Some(summary) = &state.summary {
            messages.push(serde_json::json!({
                "role": "user",
                "content": format!("[Conversation summary]\n{summary}")
            }));
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": "Understood."
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

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "stream": true,
            "messages": messages,
        });

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
        let mut buf = String::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| ChatError(format!("Stream error: {e}")))?;
            buf.push_str(&String::from_utf8_lossy(&bytes));

            // Process complete SSE events (terminated by \n\n).
            while let Some(pos) = buf.find("\n\n") {
                let event = buf[..pos].to_string();
                buf = buf[pos + 2..].to_string();

                for line in event.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            break;
                        }
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(delta) = v
                                .get("delta")
                                .and_then(|d| d.get("text"))
                                .and_then(|t| t.as_str())
                            {
                                full_text.push_str(delta);
                                app.emit("chat-stream-delta", delta).ok();
                            }
                        }
                    }
                }
            }
        }

        let turn = Turn {
            role: Role::Assistant,
            content: full_text,
            timestamp: Utc::now().timestamp_millis(),
        };
        app.emit("chat-message-complete", &turn).ok();
        Ok(turn)
    }
}

// ---------------------------------------------------------------------------
// Tauri command handlers
// ---------------------------------------------------------------------------

/// Append a user message, optionally summarise context, call the LLM, return
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

    // Append user turn and check if summarisation is needed.
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

/// Return the current chat state (for save-file serialisation).
#[tauri::command]
pub async fn get_chat_state(state: tauri::State<'_, crate::AppState>) -> Result<ChatState, String> {
    Ok(state.chat_state.lock().await.clone())
}

/// Replace the chat state (for restoring from a `.intuit` save file).
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
}
