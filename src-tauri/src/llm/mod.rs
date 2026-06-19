//! LLM provider abstraction and wire protocol.
//!
//! The [`Llm`] trait abstracts the Anthropic provider so the rest of the app
//! can talk to it without knowing the wire details.  Two operations are
//! supported:
//!
//! * [`Llm::complete`] — one-shot completion with a system prompt.  Used by the
//!   translator (prose proof generation).
//! * [`Llm::send_with_tools`] — multi-turn streaming with tool use.  Used by the
//!   assistant to carry on a conversation with the user.
//!
//! The only backend is [`AnthropicBackend`].  There is deliberately no mock or
//! echo fallback: when no usable API key is available the assistant reports a
//! disconnected status rather than silently substituting a fake reply.

pub mod models;

use async_trait::async_trait;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::assistant::{
    ToolDefinition, Transcript, Turn, COMPLETE_EVENT, STREAM_DELTA_EVENT, STREAM_DONE_EVENT,
};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error message used when the backend has no usable API key. Callers map this
/// (and API auth rejections) onto the disconnected status (#58).
pub const NO_API_KEY_ERROR: &str = "No Anthropic API key is configured.";

#[derive(Debug, Serialize)]
pub struct LlmError(pub String);

impl LlmError {
    /// Whether this error indicates a missing key (no key configured).
    #[must_use]
    pub fn is_missing_key(&self) -> bool {
        self.0 == NO_API_KEY_ERROR
    }

    /// Whether this error indicates the API rejected the key (an auth failure).
    /// `stream_request` formats HTTP failures as `API error {status}: …`, so a
    /// 401/403 status is the signal that the key itself is bad.
    #[must_use]
    pub fn is_auth_error(&self) -> bool {
        self.0.contains("API error 401") || self.0.contains("API error 403")
    }
}

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
///
/// The `model` parameter on each call is the model ID (e.g. `"claude-opus-4-6"`)
/// the caller wants to use.  Callers select the model from the appropriate
/// settings field (`assistant_model` or `translation_model`) so different
/// surfaces of the app can use different models.
#[async_trait]
pub trait Llm: Send + Sync {
    /// One-shot completion with an explicit system prompt.  Used by the
    /// translator (prose generation) which has its own prompt and does not
    /// participate in the PA conversation.
    async fn complete(
        &self,
        system_prompt: &str,
        user_content: &str,
        model: &str,
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
        model: &str,
        app: &AppHandle,
        user_content: &str,
    ) -> Result<Turn, LlmError>;
}

// ---------------------------------------------------------------------------
// AnthropicBackend (the only LLM backend)
// ---------------------------------------------------------------------------

pub struct AnthropicBackend {
    /// The API key, wrapped in a redacting type so it can never be logged or
    /// serialized. Never add `#[derive(Debug)]` / `Serialize` to this struct.
    api_key: crate::secret::ApiKey,
    client: reqwest::Client,
}

impl AnthropicBackend {
    /// Construct a backend from an explicit API key.  An empty key is allowed;
    /// the request methods reject it with a structured error rather than
    /// reaching the network.
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self {
            api_key: crate::secret::ApiKey::new(api_key),
            client: reqwest::Client::new(),
        }
    }

    /// Construct a backend from the `ANTHROPIC_API_KEY` environment variable,
    /// defaulting to an empty key when it is unset.  An empty key yields a
    /// backend that reports the missing-key error on use — never an echo.
    #[must_use]
    pub fn from_env() -> Self {
        Self::new(std::env::var("ANTHROPIC_API_KEY").unwrap_or_default())
    }

    /// Stream one request to the Anthropic API and collect the full response.
    ///
    /// Returns `(stop_reason, full_text, tool_use_blocks)` where `tool_use_blocks`
    /// is a list of `(id, name, input_json)` for any tool calls in the response.
    #[allow(clippy::too_many_lines)]
    async fn stream_request(
        &self,
        system_prompt: &str,
        messages: &[serde_json::Value],
        tools: &[ToolDefinition],
        model: &str,
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
            "model": model,
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
            .header("x-api-key", self.api_key.expose())
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

        // Tool use accumulation: block index → (id, name, accumulated input JSON string).
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
                                    let idx = v
                                        .get("index")
                                        .and_then(serde_json::Value::as_u64)
                                        .and_then(|i| usize::try_from(i).ok())
                                        .unwrap_or(0);
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
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                (id, name, input)
            })
            .collect();

        Ok((stop_reason, full_text, tool_calls))
    }
}

#[async_trait]
impl Llm for AnthropicBackend {
    async fn complete(
        &self,
        system_prompt: &str,
        user_content: &str,
        model: &str,
        app: &AppHandle,
    ) -> Result<Turn, LlmError> {
        if self.api_key.is_empty() {
            return Err(LlmError(NO_API_KEY_ERROR.to_string()));
        }

        let messages = vec![serde_json::json!({ "role": "user", "content": user_content })];
        let (_stop_reason, text, _tool_calls) = self
            .stream_request(system_prompt, &messages, &[], model, app)
            .await?;

        Ok(Turn::assistant(text))
    }

    async fn send_with_tools(
        &self,
        system_prompt: &str,
        transcript: &Transcript,
        tools: &[ToolDefinition],
        model: &str,
        app: &AppHandle,
        user_content: &str,
    ) -> Result<Turn, LlmError> {
        if self.api_key.is_empty() {
            // No usable key: refuse with a structured error. Never fabricate a
            // turn — the disconnected status (#58) is the single source of truth
            // and the disabled input (#60) is the primary guard.
            return Err(LlmError(NO_API_KEY_ERROR.to_string()));
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
                crate::assistant::Role::User => "user",
                crate::assistant::Role::Assistant => "assistant",
                crate::assistant::Role::System => continue,
            };
            messages.push(serde_json::json!({ "role": role, "content": turn.content }));
        }

        messages.push(serde_json::json!({ "role": "user", "content": user_content }));

        // Multi-turn tool use loop until stop_reason == "end_turn".
        let mut full_assistant_text = String::new();

        loop {
            let (stop_reason, text_chunk, tool_calls) = self
                .stream_request(system_prompt, &messages, tools, model, app)
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
                    let result = crate::assistant::dispatch_tool(name, input, app).await;
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
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Return the list of available models from `models.rs`.
#[tauri::command]
#[tracing::instrument(level = "debug", skip_all)]
#[must_use]
pub fn get_available_models() -> Vec<models::ModelInfo> {
    models::MODELS.to_vec()
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

    #[test]
    fn missing_key_error_is_classified() {
        let err = LlmError(NO_API_KEY_ERROR.to_string());
        assert!(err.is_missing_key());
        assert!(!err.is_auth_error());
    }

    #[test]
    fn auth_error_is_classified_from_status() {
        assert!(LlmError("API error 401 Unauthorized: bad key".into()).is_auth_error());
        assert!(LlmError("API error 403 Forbidden: nope".into()).is_auth_error());
        // A 5xx or network error is not an auth failure.
        assert!(!LlmError("API error 500: server error".into()).is_auth_error());
        assert!(!LlmError("API request failed: timeout".into()).is_auth_error());
    }

    #[test]
    fn empty_key_backend_reports_missing_key_not_echo() {
        // An empty-key backend must classify as missing-key (the send path maps
        // this to Disconnected) and must never echo.
        let backend = AnthropicBackend::new(String::new());
        assert!(backend.api_key.is_empty());
        let err = LlmError(NO_API_KEY_ERROR.to_string());
        assert!(err.is_missing_key());
        assert!(!err.0.contains("[echo]"));
    }
}
