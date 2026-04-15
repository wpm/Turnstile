//! LLM provider abstraction and wire protocol.
//!
//! The [`Llm`] trait hides whether the underlying provider is a real Anthropic
//! API call or a test mock.  Two operations are supported:
//!
//! * [`Llm::complete`] — one-shot completion with a system prompt.  Used by the
//!   translator (prose proof generation).
//! * [`Llm::send_with_tools`] — multi-turn streaming with tool use.  Used by the
//!   proof assistant to carry on a conversation with the user.
//!
//! # Backend selection
//!
//! Production: [`AnthropicBackend`] (reads `ANTHROPIC_API_KEY` from the environment).
//! Testing / mock mode: [`MockBackend`], selected by either
//! * the `mock-llm` Cargo feature flag, or
//! * the `TURNSTILE_MOCK_LLM` environment variable (`echo` | `scripted` | `delay`).

pub mod models;

use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::proof_assistant::{
    ToolDefinition, Transcript, Turn, COMPLETE_EVENT, STREAM_DELTA_EVENT, STREAM_DONE_EVENT,
};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct LlmError(pub String);

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for LlmError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

// ---------------------------------------------------------------------------
// Llm trait
// ---------------------------------------------------------------------------

/// Abstraction over the LLM provider.
///
/// Backends emit [`STREAM_DELTA_EVENT`] Tauri events while streaming and a
/// [`COMPLETE_EVENT`] event when the multi-turn loop finishes.
#[async_trait]
pub trait Llm: Send + Sync {
    /// One-shot completion with an explicit system prompt.  Used by the
    /// translator (prose generation) which has its own prompt and does not
    /// participate in the PA conversation.
    async fn complete(
        &self,
        system_prompt: &str,
        user_content: &str,
        app: &AppHandle,
    ) -> Result<Turn, LlmError>;

    /// Multi-turn completion with tool use.  Receives the current
    /// [`Transcript`] (so the backend can construct the context window) and
    /// returns the assistant [`Turn`] to append to it.  The caller is
    /// responsible for composing `system_prompt` — this trait is
    /// provider-agnostic and does not reach into app settings.
    async fn send_with_tools(
        &self,
        system_prompt: &str,
        transcript: &Transcript,
        tools: &[ToolDefinition],
        app: &AppHandle,
        user_content: &str,
    ) -> Result<Turn, LlmError>;
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
impl Llm for MockBackend {
    async fn send_with_tools(
        &self,
        _system_prompt: &str,
        _transcript: &Transcript,
        _tools: &[ToolDefinition],
        app: &AppHandle,
        user_content: &str,
    ) -> Result<Turn, LlmError> {
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
                    app.emit(STREAM_DELTA_EVENT, ch.to_string()).ok();
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                prefix
            }
        };

        let turn = Turn::assistant(response);

        app.emit(STREAM_DONE_EVENT, ()).ok();
        app.emit(COMPLETE_EVENT, &turn).ok();
        Ok(turn)
    }

    async fn complete(
        &self,
        _system_prompt: &str,
        user_content: &str,
        app: &AppHandle,
    ) -> Result<Turn, LlmError> {
        let turn = Turn::assistant(format!("[echo] {user_content}"));
        app.emit(STREAM_DONE_EVENT, ()).ok();
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
        let model = std::env::var("TURNSTILE_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());
        Ok(Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        })
    }

    /// Stream one request to the Anthropic API and collect the full response.
    ///
    /// Returns `(stop_reason, full_text, tool_use_blocks)` where `tool_use_blocks`
    /// is a list of `(id, name, input_json)` for any tool calls in the response.
    async fn stream_request(
        &self,
        system_prompt: &str,
        messages: &[serde_json::Value],
        tools: &[ToolDefinition],
        app: &AppHandle,
    ) -> Result<(String, String, Vec<(String, String, serde_json::Value)>), LlmError> {
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
            "system": system_prompt,
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
            .map_err(|e| LlmError(format!("API request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(LlmError(format!("API error {status}: {text}")));
        }

        let mut stream = response.bytes_stream();
        let mut full_text = String::new();
        let mut stop_reason = String::new();
        let mut buf = String::new();

        // Tool use accumulation: block index → (id, name, accumulated input json string).
        // BTreeMap preserves insertion order by key, which matches the Anthropic stream's
        // monotonically increasing `index` values.  The API requires tool_result blocks to
        // appear in the same order as the corresponding tool_use blocks.
        let mut tool_blocks: std::collections::BTreeMap<usize, (String, String, String)> =
            std::collections::BTreeMap::new();
        let mut current_tool_idx: Option<usize> = None;

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| LlmError(format!("Stream error: {e}")))?;
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
                                            app.emit(STREAM_DELTA_EVENT, text).ok();
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
impl Llm for AnthropicBackend {
    async fn send_with_tools(
        &self,
        system_prompt: &str,
        transcript: &Transcript,
        tools: &[ToolDefinition],
        app: &AppHandle,
        user_content: &str,
    ) -> Result<Turn, LlmError> {
        if self.api_key.is_empty() {
            let turn = Turn::assistant(
                "ANTHROPIC_API_KEY is not set. Please set it and restart.".to_string(),
            );
            app.emit(COMPLETE_EVENT, &turn).ok();
            return Ok(turn);
        }

        // Build the initial messages array from transcript + new user message.
        let mut messages: Vec<serde_json::Value> = Vec::new();

        if let Some(summary) = &transcript.summary {
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": format!("[Conversation summary]\n{summary}")
            }));
        }

        for turn in &transcript.turns {
            let role = match turn.role {
                crate::proof_assistant::Role::User => "user",
                crate::proof_assistant::Role::Assistant => "assistant",
                crate::proof_assistant::Role::System => continue,
            };
            messages.push(serde_json::json!({ "role": role, "content": turn.content }));
        }

        messages.push(serde_json::json!({ "role": "user", "content": user_content }));

        // Multi-turn tool use loop until stop_reason == "end_turn".
        let mut full_assistant_text = String::new();

        loop {
            let (stop_reason, text_chunk, tool_calls) = self
                .stream_request(system_prompt, &messages, tools, app)
                .await?;

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
                    let result = crate::proof_assistant::dispatch_tool(name, input, app).await;
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
            } else {
                break;
            }
        }

        let turn = Turn::assistant(full_assistant_text);
        app.emit(STREAM_DONE_EVENT, ()).ok();
        app.emit(COMPLETE_EVENT, &turn).ok();
        Ok(turn)
    }

    async fn complete(
        &self,
        system_prompt: &str,
        user_content: &str,
        app: &AppHandle,
    ) -> Result<Turn, LlmError> {
        if self.api_key.is_empty() {
            return Err(LlmError(
                "ANTHROPIC_API_KEY is not set. Please set it and restart.".to_string(),
            ));
        }

        let messages = vec![serde_json::json!({ "role": "user", "content": user_content })];
        let (_stop_reason, text, _tool_calls) = self
            .stream_request(system_prompt, &messages, &[], app)
            .await?;

        Ok(Turn::assistant(text))
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Return the list of available models from `models.rs`.
#[tauri::command]
pub fn get_available_models() -> Vec<models::ModelInfo> {
    models::MODELS.to_vec()
}

/// Update the selected model in settings and persist to disk.
#[tauri::command]
pub async fn set_model(app: AppHandle, model_id: String) -> Result<(), String> {
    use tauri::Manager;

    if !models::is_valid_model_id(&model_id) {
        return Err(format!("Unknown model ID: {model_id}"));
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;

    let state = app.state::<crate::AppState>();
    let mut lock = state.settings.lock().await;
    lock.model = Some(model_id);
    let updated = lock.clone();
    drop(lock);

    crate::settings::save_settings_to_disk(&updated, &app_data_dir)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- LlmError --------------------------------------------------------

    #[test]
    fn llm_error_display() {
        let err = LlmError("boom".into());
        assert_eq!(format!("{err}"), "boom");
    }

    #[test]
    fn llm_error_from_string() {
        let err = LlmError::from("msg".to_string());
        assert_eq!(err.0, "msg");
    }

    // -- MockBackend -----------------------------------------------------

    #[test]
    fn mock_echo_prefixes_message() {
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

    // -- MockBackend::from_env -------------------------------------------

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
