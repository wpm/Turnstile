//! LSP client: stdin/stdout JSON-RPC 2.0 transport for the Lean language server.
//!
//! Transport: Content-Length framing per the LSP spec.
//! The stdout reader runs on a dedicated thread (spawned in lib.rs) to avoid
//! blocking the async runtime. Responses to awaited requests are routed back
//! via the `pending` map; all other messages are dispatched to the caller's callback.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{json, Value};

use log::{debug, error, info, warn};

pub mod error;
pub use error::LspError;

// ── Public types for Tauri events ─────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct DiagnosticInfo {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub severity: u8, // 1 = error, 2 = warning, 3 = info, 4 = hint
    pub message: String,
}

#[derive(Clone, Serialize)]
pub struct SemanticToken {
    pub line: u32,
    pub col: u32,
    pub length: u32,
    pub token_type: String,
}

#[derive(Clone, Serialize)]
pub struct LspStatus {
    pub state: String, // "connected", "error", ""
    pub message: String,
}

/// A single completion candidate from `textDocument/completion`.
#[derive(Clone, Serialize)]
pub struct CompletionItem {
    /// The text shown in the completion menu.
    pub label: String,
    /// Brief type or kind information (e.g. "theorem", "def", "(def)").
    pub detail: Option<String>,
    /// The text inserted when the completion is accepted; falls back to `label`.
    pub insert_text: Option<String>,
}

/// A range of lines currently being elaborated by the Lean server.
/// Emitted via the `$/lean/fileProgress` notification.
#[derive(Clone, Serialize)]
pub struct FileProgressRange {
    pub start_line: u32, // 1-indexed (converted from 0-indexed LSP)
    pub end_line: u32,   // 1-indexed
}

/// Type information from `textDocument/hover` — types only, no docstrings.
#[derive(Clone, Debug, Serialize)]
pub struct HoverInfo {
    pub contents: String,
}

/// Result of `textDocument/definition` filtered to a single target location.
///
/// All positions are 0-indexed (raw LSP convention); the frontend converts
/// to CM6 offsets directly.
#[derive(Clone, Debug, Serialize)]
pub struct DefinitionLocation {
    pub uri: String,
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// A single text edit produced by a code action. Positions are 0-indexed.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TextEditDto {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub new_text: String,
}

/// A workspace edit, restricted to per-URI text edits.
///
/// Represented as a flat list of `(uri, edits)` pairs so the wire shape stays
/// stable across serde. Lean's `textDocument/codeAction` only targets document
/// edits in practice.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct WorkspaceEditDto {
    pub changes: Vec<(String, Vec<TextEditDto>)>,
}

/// A single code action from `textDocument/codeAction`.
#[derive(Clone, Debug, Serialize)]
pub struct CodeActionInfo {
    pub title: String,
    pub kind: Option<String>,
    /// Resolved workspace edit, if the server returned it inline.
    pub edit: Option<WorkspaceEditDto>,
    /// Raw opaque `data` field used by `codeAction/resolve` when `edit` is absent.
    pub resolve_data: Option<Value>,
}

/// A single document symbol from `textDocument/documentSymbol`. Positions are
/// 0-indexed. Children form a nested tree of symbols.
#[derive(Clone, Debug, Serialize)]
pub struct DocumentSymbolInfo {
    pub name: String,
    /// LSP `SymbolKind` numeric code (e.g. 12 = function, 5 = class).
    pub kind: u8,
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub children: Vec<DocumentSymbolInfo>,
}

// ── LSP Client ────────────────────────────────────────────────────────

pub struct LspClient {
    process: Child,
    pub next_id: Arc<AtomicI64>,
    /// Shared writer — clone the `Arc` to send messages from the reader thread via `send_raw`.
    pub writer: Arc<tokio::sync::Mutex<Box<dyn Write + Send>>>,
    /// Semantic token legend, populated during initialize (accessed from sync reader thread)
    pub token_types: Arc<Mutex<Vec<String>>>,
    /// Pending request registry: `request_id` → oneshot sender for the response.
    pub pending: Arc<Mutex<HashMap<i64, mpsc::SyncSender<Value>>>>,
}

impl LspClient {
    /// Spawn an LSP server process and return a client handle.
    ///
    /// # Errors
    /// Returns an error if the process cannot be spawned or its stdin cannot be captured.
    pub fn spawn(command: &str, args: &[&str], cwd: &Path) -> Result<Self, LspError> {
        info!(
            "Spawning LSP server: {command} {args:?} (cwd: {})",
            cwd.display()
        );

        let mut child = Command::new(command)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| LspError::SpawnFailed {
                command: command.to_string(),
                source,
            })?;

        let stdin = child.stdin.take().ok_or(LspError::StdinCaptureFailed)?;

        Ok(Self {
            process: child,
            next_id: Arc::new(AtomicI64::new(1)),
            writer: Arc::new(tokio::sync::Mutex::new(Box::new(stdin))),
            token_types: Arc::new(Mutex::new(Vec::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Send a JSON-RPC request and return the id used.
    ///
    /// # Errors
    /// Returns an error if serialization or writing to the server fails.
    pub async fn send_request(&self, method: &str, params: Value) -> Result<i64, LspError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send_message(&msg).await?;
        Ok(id)
    }

    /// Send a JSON-RPC request and block until the response arrives.
    /// Returns the `result` field of the response, or an error. Timeout: 10 seconds.
    ///
    /// # Errors
    /// Returns an error if the request cannot be sent or the response times out.
    pub async fn send_request_await(&self, method: &str, params: Value) -> Result<Value, LspError> {
        let (tx, rx) = mpsc::sync_channel::<Value>(1);
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| LspError::LockPoisoned { lock: "pending" })?;
            pending.insert(id, tx);
        }

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send_message(&msg).await?;

        let method_owned = method.to_owned();
        tokio::task::spawn_blocking(move || {
            rx.recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|_| LspError::Timeout {
                    method: method_owned,
                })
        })
        .await?
    }

    /// Send a JSON-RPC notification (no id, no response expected).
    ///
    /// # Errors
    /// Returns an error if serialization or writing to the server fails.
    pub async fn send_notification(&self, method: &str, params: Value) -> Result<(), LspError> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.send_message(&msg).await?;
        Ok(())
    }

    /// Read messages from stdout in a blocking loop. Call from a spawned thread.
    /// Routes responses to any registered pending senders; passes the rest to `on_message`.
    pub fn read_messages<F>(
        stdout: std::process::ChildStdout,
        pending: &Arc<Mutex<HashMap<i64, mpsc::SyncSender<Value>>>>,
        mut on_message: F,
    ) where
        F: FnMut(&Value),
    {
        let mut reader = BufReader::new(stdout);

        loop {
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        info!("LSP server stdout closed");
                        return;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            break;
                        }
                        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
                            if let Ok(len) = len_str.parse::<usize>() {
                                content_length = len;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error reading LSP stdout: {e}");
                        return;
                    }
                }
            }

            if content_length == 0 {
                warn!("Got LSP message with no Content-Length, skipping");
                continue;
            }

            let mut body = vec![0u8; content_length];
            if let Err(e) = std::io::Read::read_exact(&mut reader, &mut body) {
                error!("Error reading LSP message body: {e}");
                return;
            }

            match serde_json::from_slice::<Value>(&body) {
                Ok(msg) => {
                    debug!(
                        "LSP ← {}",
                        serde_json::to_string_pretty(&msg).unwrap_or_default()
                    );

                    if let Some(id_val) = msg.get("id") {
                        if let Some(id) = id_val.as_i64() {
                            let sender = pending.lock().ok().and_then(|mut p| p.remove(&id));
                            if let Some(tx) = sender {
                                let result = msg.get("result").cloned().unwrap_or(Value::Null);
                                let _ = tx.send(result);
                                continue;
                            }
                        }
                    }

                    on_message(&msg);
                }
                Err(e) => {
                    warn!("Failed to parse LSP message: {e}");
                }
            }
        }
    }

    /// Take stdout from the child process (can only be called once).
    pub const fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.process.stdout.take()
    }

    async fn send_message(&self, msg: &Value) -> Result<(), LspError> {
        let body = serde_json::to_string(msg)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        debug!(
            "LSP → {}",
            serde_json::to_string_pretty(msg).unwrap_or_default()
        );

        let mut writer = self.writer.lock().await;
        writer
            .write_all(header.as_bytes())
            .map_err(|source| LspError::Io {
                operation: "write header",
                source,
            })?;
        writer
            .write_all(body.as_bytes())
            .map_err(|source| LspError::Io {
                operation: "write body",
                source,
            })?;
        writer.flush().map_err(|source| LspError::Io {
            operation: "flush",
            source,
        })?;
        drop(writer);

        Ok(())
    }
}

// ── Shutdown ─────────────────────────────────────────────────────────

impl LspClient {
    /// Gracefully shut down the LSP server process.
    ///
    /// Sends a `shutdown` JSON-RPC request, waits briefly for the response,
    /// then sends an `exit` notification and waits for the child to exit.
    /// If the process doesn't exit within the timeout, it is killed.
    ///
    /// This is intentionally synchronous so it can be called from `Drop`.
    pub fn shutdown(&mut self) {
        info!("Initiating LSP server shutdown");

        // Send the `shutdown` request.
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let shutdown_msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "shutdown",
            "params": null,
        });
        if let Err(e) = self.send_message_sync(&shutdown_msg) {
            warn!("Failed to send shutdown request: {e}");
        } else {
            // Wait briefly for the server to acknowledge.
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        // Send the `exit` notification.
        let exit_msg = json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null,
        });
        if let Err(e) = self.send_message_sync(&exit_msg) {
            warn!("Failed to send exit notification: {e}");
        }

        // Wait for the child to exit, with a timeout.
        match self.process.try_wait().ok().flatten().or_else(|| {
            std::thread::sleep(std::time::Duration::from_secs(1));
            self.process.try_wait().ok().flatten()
        }) {
            Some(status) => info!("LSP server exited: {status}"),
            None => {
                warn!("LSP server did not exit in time, killing");
                if let Err(e) = self.process.kill() {
                    warn!("Failed to kill LSP server: {e}");
                } else {
                    // Reap the zombie.
                    let _ = self.process.wait();
                }
            }
        }
    }

    /// Write a JSON-RPC message synchronously using `try_lock`.
    fn send_message_sync(&self, msg: &Value) -> Result<(), LspError> {
        let body = serde_json::to_string(msg)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        debug!(
            "LSP → {}",
            serde_json::to_string_pretty(msg).unwrap_or_default()
        );

        let mut writer = self
            .writer
            .try_lock()
            .map_err(|_| LspError::WriterContended)?;
        writer
            .write_all(header.as_bytes())
            .map_err(|source| LspError::Io {
                operation: "write header",
                source,
            })?;
        writer
            .write_all(body.as_bytes())
            .map_err(|source| LspError::Io {
                operation: "write body",
                source,
            })?;
        writer.flush().map_err(|source| LspError::Io {
            operation: "flush",
            source,
        })?;
        Ok(())
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ── Utilities ─────────────────────────────────────────────────────────

/// Send a null-result response for a server→client request.
///
/// The LSP spec requires clients to respond to every server request. Lean sends
/// `workspace/semanticTokens/refresh`, `workspace/inlayHint/refresh`, and
/// `client/registerCapability` as requests; this acks them with `{ "result": null }`.
///
/// Intended for use from the sync reader thread — pass a clone of `client.writer`.
///
/// # Errors
/// Returns an error if serialization or writing to the server fails.
pub fn ack_request(
    writer: &Arc<tokio::sync::Mutex<Box<dyn Write + Send>>>,
    id: &Value,
) -> Result<(), LspError> {
    let msg = json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null });
    let body = serde_json::to_string(&msg)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    debug!(
        "LSP → {}",
        serde_json::to_string_pretty(&msg).unwrap_or_default()
    );
    let mut guard = writer.blocking_lock();
    guard
        .write_all(header.as_bytes())
        .map_err(|source| LspError::Io {
            operation: "write header",
            source,
        })?;
    guard
        .write_all(body.as_bytes())
        .map_err(|source| LspError::Io {
            operation: "write body",
            source,
        })?;
    guard.flush().map_err(|source| LspError::Io {
        operation: "flush",
        source,
    })?;
    drop(guard);
    Ok(())
}

/// Send a JSON-RPC request from the sync reader thread (fire-and-forget).
///
/// The response will arrive on the reader thread and be dispatched via the
/// `on_message` callback (not through `pending`), so the caller does not
/// await a result.
///
/// # Errors
/// Returns an error if serialization or writing to the server fails.
pub fn send_request_sync(
    writer: &Arc<tokio::sync::Mutex<Box<dyn Write + Send>>>,
    next_id: &Arc<AtomicI64>,
    method: &str,
    params: Value,
) -> Result<(), LspError> {
    let id = next_id.fetch_add(1, Ordering::SeqCst);
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    let body = serde_json::to_string(&msg)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    debug!(
        "LSP → {}",
        serde_json::to_string_pretty(&msg).unwrap_or_default()
    );
    let mut guard = writer.blocking_lock();
    guard
        .write_all(header.as_bytes())
        .map_err(|source| LspError::Io {
            operation: "write header",
            source,
        })?;
    guard
        .write_all(body.as_bytes())
        .map_err(|source| LspError::Io {
            operation: "write body",
            source,
        })?;
    guard.flush().map_err(|source| LspError::Io {
        operation: "flush",
        source,
    })?;
    drop(guard);
    Ok(())
}

// ── Initialize handshake ──────────────────────────────────────────────

/// Build the `initialize` request params.
pub fn initialize_params(root_uri: &str) -> Value {
    json!({
        "processId": std::process::id(),
        "capabilities": {
            "textDocument": {
                "synchronization": {
                    "dynamicRegistration": false,
                    "willSave": false,
                    "willSaveWaitUntil": false,
                    "didSave": true
                },
                "publishDiagnostics": {
                    "relatedInformation": true
                },
                "semanticTokens": {
                    "dynamicRegistration": false,
                    "requests": {
                        "full": true
                    },
                    "tokenTypes": [
                        "namespace", "type", "class", "enum", "interface",
                        "struct", "typeParameter", "parameter", "variable",
                        "property", "enumMember", "event", "function",
                        "method", "macro", "keyword", "modifier", "comment",
                        "string", "number", "regexp", "operator", "decorator"
                    ],
                    "tokenModifiers": [
                        "declaration", "definition", "readonly", "static",
                        "deprecated", "abstract", "async", "modification",
                        "documentation", "defaultLibrary"
                    ],
                    "formats": ["relative"],
                    "multilineTokenSupport": false,
                    "overlappingTokenSupport": false
                },
                "completion": {
                    "completionItem": {
                        "snippetSupport": false,
                        "documentationFormat": ["plaintext"]
                    },
                    "contextSupport": false
                },
                "hover": {
                    "dynamicRegistration": false,
                    "contentFormat": ["markdown", "plaintext"]
                },
                "definition": {
                    "dynamicRegistration": false,
                    "linkSupport": false
                },
                "codeAction": {
                    "dynamicRegistration": false,
                    "resolveSupport": { "properties": ["edit"] },
                    "dataSupport": true
                },
                "documentSymbol": {
                    "dynamicRegistration": false,
                    "hierarchicalDocumentSymbolSupport": true
                }
            },
            "experimental": {
                "plainGoal": true
            }
        },
        "rootUri": root_uri,
        "workspaceFolders": [{ "uri": root_uri, "name": "turnstile" }],
    })
}

/// Convert a filesystem path to a `file://` URI with full RFC 8089 percent-encoding.
pub fn path_to_file_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .map_or_else(|()| format!("file://{}", path.display()), |u| u.to_string())
}

/// Parse the semantic token legend from an initialize response.
pub fn parse_token_legend(result: &Value) -> Vec<String> {
    result
        .get("capabilities")
        .and_then(|c| c.get("semanticTokensProvider"))
        .and_then(|p| p.get("legend"))
        .and_then(|l| l.get("tokenTypes"))
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a `textDocument/publishDiagnostics` notification params.
pub fn parse_diagnostics(params: &Value) -> Vec<DiagnosticInfo> {
    let Some(diags) = params.get("diagnostics").and_then(|d| d.as_array()) else {
        return Vec::new();
    };

    diags
        .iter()
        .filter_map(|d| {
            let (sl, sc, el, ec) = parse_lsp_range(d.get("range")?)?;
            let severity =
                u8::try_from(d.get("severity").and_then(Value::as_u64).unwrap_or(1)).unwrap_or(1);
            let message = d
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            Some(DiagnosticInfo {
                start_line: sl + 1, // LSP is 0-indexed; frontend is 1-indexed
                start_col: sc,
                end_line: el + 1,
                end_col: ec,
                severity,
                message,
            })
        })
        .collect()
}

/// Decode delta-encoded semantic tokens into absolute positions.
///
/// The LSP response is a flat array of 5-tuples:
///   [deltaLine, deltaStart, length, tokenTypeIndex, tokenModifiers]
pub fn decode_semantic_tokens(data: &[u32], legend: &[String]) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    let mut line: u32 = 1; // 1-indexed for frontend
    let mut col: u32 = 0;

    for chunk in data.chunks_exact(5) {
        let delta_line = chunk[0];
        let delta_start = chunk[1];
        let length = chunk[2];
        let token_type_idx = chunk[3] as usize;

        if delta_line > 0 {
            line += delta_line;
            col = delta_start;
        } else {
            col += delta_start;
        }

        let token_type = legend
            .get(token_type_idx)
            .cloned()
            .unwrap_or_else(|| "variable".to_string());

        tokens.push(SemanticToken {
            line,
            col,
            length,
            token_type,
        });
    }

    tokens
}

/// Parse a `textDocument/completion` response into a list of completion items.
///
/// The LSP response is either a `CompletionList` (`{ items: [...] }`) or a bare
/// `CompletionItem[]`. Both shapes are handled here.
pub fn parse_completion_items(result: &Value) -> Vec<CompletionItem> {
    let items = result
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| result.as_array())
        .map(Vec::as_slice)
        .unwrap_or_default();

    items
        .iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_string();
            let detail = item.get("detail").and_then(Value::as_str).map(String::from);
            let insert_text = item
                .get("insertText")
                .and_then(Value::as_str)
                .map(String::from);
            Some(CompletionItem {
                label,
                detail,
                insert_text,
            })
        })
        .collect()
}

/// Parse a `$/lean/fileProgress` notification params into processing ranges.
pub fn parse_file_progress(params: &Value) -> Vec<FileProgressRange> {
    let Some(processing) = params.get("processing").and_then(|p| p.as_array()) else {
        return Vec::new();
    };

    processing
        .iter()
        .filter_map(|item| {
            let (sl, _sc, el, _ec) = parse_lsp_range(item.get("range")?)?;
            Some(FileProgressRange {
                start_line: sl + 1, // 0-indexed → 1-indexed
                end_line: el + 1,
            })
        })
        .collect()
}

/// Parse a `textDocument/hover` response, keeping only the type signature
/// (the first fenced Lean code block) and dropping any trailing documentation.
///
/// The Lean server returns markdown with a fenced lean block followed by an
/// optional docstring block. We keep the first block and discard the rest.
/// Returns `None` if the response is null or has no readable contents.
pub fn parse_hover(result: &Value) -> Option<HoverInfo> {
    if result.is_null() {
        return None;
    }
    let raw = hover_contents_string(result.get("contents")?)?;
    let contents = hover_strip_docstrings(&raw);
    if contents.trim().is_empty() {
        None
    } else {
        Some(HoverInfo { contents })
    }
}

/// Flatten a hover `contents` value into one markdown string.
///
/// LSP permits three shapes here: a bare string, a `MarkupContent`
/// (`{ kind, value }`), or an array of strings / `{ language, value }` /
/// `MarkupContent`. This helper normalizes all three to a single string.
fn hover_contents_string(contents: &Value) -> Option<String> {
    if let Some(s) = contents.as_str() {
        return Some(s.to_string());
    }
    if let Some(v) = contents.get("value").and_then(Value::as_str) {
        return Some(v.to_string());
    }
    if let Some(arr) = contents.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            if let Some(s) = item.as_str() {
                parts.push(s.to_string());
            } else if let Some(v) = item.get("value").and_then(Value::as_str) {
                parts.push(v.to_string());
            }
        }
        if parts.is_empty() {
            return None;
        }
        return Some(parts.join("\n\n"));
    }
    None
}

/// Keep only the first fenced `lean` code block from hover markdown; drop the
/// rest (which is typically a docstring). If no fenced block is present, fall
/// back to the untouched input.
fn hover_strip_docstrings(markdown: &str) -> String {
    let mut in_block = false;
    let mut collected: Vec<&str> = Vec::new();
    let mut saw_block = false;

    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_block {
                // End of the first code block — stop.
                saw_block = true;
                break;
            }
            in_block = true;
            continue;
        }
        if in_block {
            collected.push(line);
        }
    }

    if saw_block {
        collected.join("\n").trim().to_string()
    } else {
        markdown.trim().to_string()
    }
}

/// Parse a `textDocument/definition` response, keeping only the first target.
///
/// The Lean server may return any of:
///   - `Location` — `{ uri, range }`
///   - `Location[]`
///   - `LocationLink[]` — `{ targetUri, targetRange, targetSelectionRange, originSelectionRange }`
///   - `null`
///
/// We set `linkSupport: false` in the initialize params, but Lean sometimes
/// returns `LocationLink` anyway, so handle both shapes.
pub fn parse_definition(result: &Value) -> Option<DefinitionLocation> {
    if result.is_null() {
        return None;
    }
    // Either a single Location or an array — pick the first.
    let entry = if let Some(arr) = result.as_array() {
        arr.first()?
    } else {
        result
    };

    // LocationLink uses `targetUri`/`targetRange`; Location uses `uri`/`range`.
    let (uri, range) = if let Some(target_uri) = entry.get("targetUri").and_then(|v| v.as_str()) {
        let range = entry
            .get("targetSelectionRange")
            .or_else(|| entry.get("targetRange"))?;
        (target_uri.to_string(), range)
    } else {
        let uri = entry.get("uri")?.as_str()?.to_string();
        let range = entry.get("range")?;
        (uri, range)
    };

    let (sl, sc, el, ec) = parse_lsp_range(range)?;
    Some(DefinitionLocation {
        uri,
        line: sl,
        character: sc,
        end_line: el,
        end_character: ec,
    })
}

/// Parse a `textDocument/codeAction` response into our `CodeActionInfo` DTOs.
///
/// Each entry is either a `Command` (ignored — we only surface workspace
/// edits) or a `CodeAction` object. Inline edits are captured in
/// `CodeActionInfo.edit`; actions with only a `data` field (to be resolved
/// later via `codeAction/resolve`) carry `resolve_data`.
pub fn parse_code_actions(result: &Value) -> Vec<CodeActionInfo> {
    let Some(arr) = result.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            // Filter out plain Command items (no title + command string pair with edits).
            let title = item.get("title")?.as_str()?.to_string();
            let kind = item.get("kind").and_then(Value::as_str).map(String::from);
            let edit = item.get("edit").and_then(parse_workspace_edit);
            let resolve_data = item.get("data").cloned();
            Some(CodeActionInfo {
                title,
                kind,
                edit,
                resolve_data,
            })
        })
        .collect()
}

/// Parse a `WorkspaceEdit` object with only `changes` (the shape Lean emits).
///
/// Returns `None` if the value doesn't look like a `WorkspaceEdit` we can use.
pub fn parse_workspace_edit(edit: &Value) -> Option<WorkspaceEditDto> {
    let changes = edit.get("changes").and_then(Value::as_object)?;
    let mut out = Vec::new();
    for (uri, edits) in changes {
        let edits_arr = edits.as_array()?;
        let mut parsed = Vec::new();
        for text_edit in edits_arr {
            if let Some(te) = parse_text_edit(text_edit) {
                parsed.push(te);
            }
        }
        out.push((uri.clone(), parsed));
    }
    Some(WorkspaceEditDto { changes: out })
}

fn parse_text_edit(edit: &Value) -> Option<TextEditDto> {
    let (sl, sc, el, ec) = parse_lsp_range(edit.get("range")?)?;
    let new_text = edit.get("newText")?.as_str()?.to_string();
    Some(TextEditDto {
        start_line: sl,
        start_character: sc,
        end_line: el,
        end_character: ec,
        new_text,
    })
}

/// Parse a `textDocument/documentSymbol` response into a hierarchical tree.
///
/// Only the modern `DocumentSymbol[]` shape is handled — we set
/// `hierarchicalDocumentSymbolSupport: true` in the initialize handshake.
pub fn parse_document_symbols(result: &Value) -> Vec<DocumentSymbolInfo> {
    let Some(arr) = result.as_array() else {
        return Vec::new();
    };
    arr.iter().filter_map(parse_document_symbol).collect()
}

fn parse_document_symbol(item: &Value) -> Option<DocumentSymbolInfo> {
    let name = item.get("name")?.as_str()?.to_string();
    let kind = u8::try_from(item.get("kind")?.as_u64()?).ok()?;
    let (sl, sc, el, ec) = parse_lsp_range(item.get("range")?)?;
    let children = item
        .get("children")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(parse_document_symbol).collect())
        .unwrap_or_default();
    Some(DocumentSymbolInfo {
        name,
        kind,
        start_line: sl,
        start_character: sc,
        end_line: el,
        end_character: ec,
        children,
    })
}

/// Parse `{ "start": { "line": u32, "character": u32 }, "end": { ... } }`.
fn parse_lsp_range(range: &Value) -> Option<(u32, u32, u32, u32)> {
    let start = range.get("start")?;
    let end = range.get("end")?;
    Some((
        u32::try_from(start.get("line")?.as_u64()?).ok()?,
        u32::try_from(start.get("character")?.as_u64()?).ok()?,
        u32::try_from(end.get("line")?.as_u64()?).ok()?,
        u32::try_from(end.get("character")?.as_u64()?).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_to_file_uri_simple() {
        let path = Path::new("/home/user/project");
        let uri = path_to_file_uri(path);
        assert!(uri.starts_with("file://"));
        assert!(uri.contains("home/user/project"));
    }

    #[test]
    fn path_to_file_uri_encodes_spaces() {
        let path = Path::new("/home/user/my project");
        let uri = path_to_file_uri(path);
        assert!(
            uri.contains("%20"),
            "space should be percent-encoded, got: {uri}"
        );
        assert!(
            !uri.contains(' '),
            "raw space should not appear in URI, got: {uri}"
        );
    }

    #[test]
    fn parse_token_legend_returns_types() {
        let result = json!({
            "capabilities": {
                "semanticTokensProvider": {
                    "legend": {
                        "tokenTypes": ["keyword", "type", "variable"]
                    }
                }
            }
        });
        let legend = parse_token_legend(&result);
        assert_eq!(legend, vec!["keyword", "type", "variable"]);
    }

    #[test]
    fn parse_token_legend_missing_capabilities_returns_empty() {
        assert!(parse_token_legend(&json!({})).is_empty());
    }

    #[test]
    fn parse_diagnostics_empty_returns_empty() {
        let params = json!({ "diagnostics": [] });
        assert!(parse_diagnostics(&params).is_empty());
    }

    #[test]
    fn parse_diagnostics_parses_single_diagnostic() {
        let params = json!({
            "diagnostics": [{
                "range": {
                    "start": { "line": 2, "character": 4 },
                    "end":   { "line": 2, "character": 10 }
                },
                "severity": 1,
                "message": "unknown identifier"
            }]
        });
        let diags = parse_diagnostics(&params);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.start_line, 3); // LSP 0-indexed → 1-indexed
        assert_eq!(d.start_col, 4);
        assert_eq!(d.severity, 1);
        assert_eq!(d.message, "unknown identifier");
    }

    #[test]
    fn parse_diagnostics_converts_line_index() {
        let params = json!({
            "diagnostics": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end":   { "line": 0, "character": 1 }
                },
                "severity": 2,
                "message": "warning"
            }]
        });
        let diags = parse_diagnostics(&params);
        assert_eq!(diags[0].start_line, 1);
    }

    #[test]
    fn decode_semantic_tokens_single_token() {
        let legend = vec!["keyword".to_string(), "type".to_string()];
        let data = vec![0, 5, 3, 0, 0];
        let tokens = decode_semantic_tokens(&data, &legend);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[0].col, 5);
        assert_eq!(tokens[0].length, 3);
        assert_eq!(tokens[0].token_type, "keyword");
    }

    #[test]
    fn decode_semantic_tokens_line_advance() {
        let legend = vec!["keyword".to_string()];
        let data = vec![
            0, 0, 1, 0, 0, // token at line 1, col 0
            2, 4, 1, 0, 0, // delta line +2, col 4 → line 3, col 4
        ];
        let tokens = decode_semantic_tokens(&data, &legend);
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[1].line, 3);
        assert_eq!(tokens[1].col, 4);
    }

    #[test]
    fn initialize_params_contains_root_uri() {
        let uri = "file:///my/project";
        let params = initialize_params(uri);
        assert_eq!(params["rootUri"], uri);
        assert_eq!(params["workspaceFolders"][0]["uri"], uri);
    }

    #[test]
    fn initialize_params_declares_plain_goal_capability() {
        let params = initialize_params("file:///tmp");
        assert_eq!(params["capabilities"]["experimental"]["plainGoal"], true);
    }

    #[test]
    fn decode_semantic_tokens_col_resets_on_new_line() {
        let legend = vec!["keyword".to_string()];
        // First token: line 1, col 10. Second token: delta_line=1 → line 2, delta_start=3 → col 3 (not 10+3).
        let data = vec![0, 10, 1, 0, 0, 1, 3, 1, 0, 0];
        let tokens = decode_semantic_tokens(&data, &legend);
        assert_eq!(tokens[1].line, 2);
        assert_eq!(tokens[1].col, 3);
    }

    #[test]
    fn decode_semantic_tokens_unknown_type_falls_back_to_variable() {
        let legend = vec!["keyword".to_string()];
        let data = vec![0, 0, 5, 99, 0]; // index 99 out of bounds
        let tokens = decode_semantic_tokens(&data, &legend);
        assert_eq!(tokens[0].token_type, "variable");
    }

    #[test]
    fn parse_completion_items_from_list_object() {
        let result = json!({
            "isIncomplete": false,
            "items": [
                { "label": "theorem", "detail": "keyword", "insertText": "theorem" },
                { "label": "Nat.succ", "detail": "Nat → Nat" }
            ]
        });
        let items = parse_completion_items(&result);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "theorem");
        assert_eq!(items[0].detail.as_deref(), Some("keyword"));
        assert_eq!(items[0].insert_text.as_deref(), Some("theorem"));
        assert_eq!(items[1].label, "Nat.succ");
        assert_eq!(items[1].detail.as_deref(), Some("Nat → Nat"));
        assert!(items[1].insert_text.is_none());
    }

    #[test]
    fn parse_completion_items_from_bare_array() {
        let result = json!([
            { "label": "def" },
            { "label": "lemma", "detail": "keyword" }
        ]);
        let items = parse_completion_items(&result);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "def");
        assert!(items[0].detail.is_none());
        assert_eq!(items[1].label, "lemma");
    }

    #[test]
    fn parse_completion_items_empty_returns_empty() {
        let result = json!({ "items": [] });
        assert!(parse_completion_items(&result).is_empty());
    }

    #[test]
    fn parse_completion_items_null_returns_empty() {
        assert!(parse_completion_items(&json!(null)).is_empty());
    }

    #[test]
    fn parse_file_progress_single_range() {
        let params = json!({
            "textDocument": { "uri": "file:///test.lean" },
            "processing": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 10, "character": 0 }
                }
            }]
        });
        let ranges = parse_file_progress(&params);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 1); // 0-indexed → 1-indexed
        assert_eq!(ranges[0].end_line, 11);
    }

    #[test]
    fn parse_file_progress_multiple_ranges() {
        let params = json!({
            "processing": [
                { "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 5, "character": 0 } } },
                { "range": { "start": { "line": 8, "character": 0 }, "end": { "line": 12, "character": 0 } } }
            ]
        });
        let ranges = parse_file_progress(&params);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start_line, 1);
        assert_eq!(ranges[0].end_line, 6);
        assert_eq!(ranges[1].start_line, 9);
        assert_eq!(ranges[1].end_line, 13);
    }

    #[test]
    fn parse_file_progress_empty_processing_returns_empty() {
        let params = json!({ "processing": [] });
        assert!(parse_file_progress(&params).is_empty());
    }

    #[test]
    fn parse_file_progress_missing_processing_returns_empty() {
        assert!(parse_file_progress(&json!({})).is_empty());
    }

    #[test]
    fn initialize_params_declares_completion_capability() {
        let params = initialize_params("file:///tmp");
        assert_eq!(
            params["capabilities"]["textDocument"]["completion"]["completionItem"]
                ["snippetSupport"],
            false
        );
    }

    /// Helper: create an `LspClient` backed by `cat`, which simply reads stdin.
    /// This gives us a real child process whose stdin we can write to.
    fn spawn_cat_client() -> LspClient {
        LspClient::spawn("cat", &[], Path::new("/tmp")).expect("failed to spawn cat")
    }

    /// Helper: extract all JSON-RPC messages written to a byte buffer.
    /// Parses Content-Length framed messages.
    fn extract_jsonrpc_messages(buf: &[u8]) -> Vec<Value> {
        let mut msgs = Vec::new();
        let mut cursor = std::io::Cursor::new(buf);
        let mut reader = BufReader::new(&mut cursor);

        loop {
            let mut content_length: Option<usize> = None;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => return msgs,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            break;
                        }
                        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
                            content_length = len_str.parse::<usize>().ok();
                        }
                    }
                    Err(_) => return msgs,
                }
            }
            let Some(len) = content_length else {
                return msgs;
            };
            let mut body = vec![0u8; len];
            if std::io::Read::read_exact(&mut reader, &mut body).is_err() {
                return msgs;
            }
            if let Ok(msg) = serde_json::from_slice::<Value>(&body) {
                msgs.push(msg);
            }
        }
    }

    #[test]
    fn shutdown_sends_shutdown_request_and_exit_notification() {
        // Use a shared buffer as the writer so we can inspect the messages.
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

        // We need a real child process that will exit on its own when stdin closes.
        let child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn cat");

        // Build a writer that captures bytes.
        #[derive(Clone)]
        struct BufWriter(Arc<Mutex<Vec<u8>>>);
        impl Write for BufWriter {
            fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(data);
                Ok(data.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let writer = BufWriter(Arc::clone(&buf));
        let mut client = LspClient {
            process: child,
            next_id: Arc::new(AtomicI64::new(1)),
            writer: Arc::new(tokio::sync::Mutex::new(Box::new(writer))),
            token_types: Arc::new(Mutex::new(Vec::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
        };

        // Manually call shutdown (which will also be called by Drop, but that's fine).
        client.shutdown();

        let captured = buf.lock().unwrap();
        let msgs = extract_jsonrpc_messages(&captured);

        assert!(
            msgs.len() >= 2,
            "Expected at least 2 messages (shutdown + exit), got {}: {msgs:?}",
            msgs.len()
        );

        // First message: shutdown request (has id).
        assert_eq!(msgs[0]["method"], "shutdown");
        assert!(msgs[0].get("id").is_some(), "shutdown should have an id");

        // Second message: exit notification (no id).
        assert_eq!(msgs[1]["method"], "exit");
        assert!(
            msgs[1].get("id").is_none(),
            "exit should be a notification (no id)"
        );
    }

    #[test]
    fn drop_does_not_panic_when_process_already_dead() {
        let mut client = spawn_cat_client();
        // Kill the process before drop.
        let _ = client.process.kill();
        let _ = client.process.wait();
        // Drop should not panic.
        drop(client);
    }

    #[test]
    fn drop_does_not_panic_on_normal_client() {
        let client = spawn_cat_client();
        // Just dropping should work fine.
        drop(client);
    }

    #[test]
    fn path_to_file_uri_encodes_hash() {
        let path = Path::new("/home/user/my#project");
        let uri = path_to_file_uri(path);
        assert!(
            uri.contains("%23"),
            "hash should be percent-encoded, got: {uri}"
        );
    }

    #[test]
    fn parse_token_legend_missing_provider_returns_empty() {
        // capabilities present but no semanticTokensProvider key
        let result = json!({ "capabilities": { "textDocument": {} } });
        assert!(parse_token_legend(&result).is_empty());
    }

    #[test]
    fn parse_file_progress_skips_malformed_range() {
        let params = json!({
            "processing": [
                { "range": { "start": { "character": 0 }, "end": { "line": 5, "character": 0 } } },
                { "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 3, "character": 0 } } }
            ]
        });
        let ranges = parse_file_progress(&params);
        assert_eq!(ranges.len(), 1, "malformed entry should be skipped");
        assert_eq!(ranges[0].start_line, 2);
    }

    #[test]
    fn parse_diagnostics_defaults_severity_to_1() {
        let params = json!({
            "diagnostics": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end":   { "line": 0, "character": 1 }
                },
                "message": "err"
            }]
        });
        let diags = parse_diagnostics(&params);
        assert_eq!(diags[0].severity, 1);
    }

    #[test]
    fn parse_diagnostics_defaults_message_to_empty() {
        let params = json!({
            "diagnostics": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end":   { "line": 0, "character": 1 }
                },
                "severity": 2
            }]
        });
        let diags = parse_diagnostics(&params);
        assert_eq!(diags[0].message, "");
    }

    #[test]
    fn initialize_params_process_id_matches() {
        let params = initialize_params("file:///tmp");
        assert_eq!(params["processId"], std::process::id());
    }

    #[test]
    fn initialize_params_declares_hover_capability() {
        let params = initialize_params("file:///tmp");
        assert_eq!(
            params["capabilities"]["textDocument"]["hover"]["dynamicRegistration"],
            false
        );
    }

    #[test]
    fn initialize_params_declares_definition_capability() {
        let params = initialize_params("file:///tmp");
        assert_eq!(
            params["capabilities"]["textDocument"]["definition"]["dynamicRegistration"],
            false
        );
    }

    #[test]
    fn initialize_params_declares_code_action_capability() {
        let params = initialize_params("file:///tmp");
        assert!(params["capabilities"]["textDocument"]["codeAction"].is_object());
    }

    #[test]
    fn initialize_params_declares_document_symbol_capability() {
        let params = initialize_params("file:///tmp");
        assert_eq!(
            params["capabilities"]["textDocument"]["documentSymbol"]
                ["hierarchicalDocumentSymbolSupport"],
            true
        );
    }

    // ── Hover parsing ─────────────────────────────────────────────────

    #[test]
    fn parse_hover_markup_content_keeps_type_only() {
        let result = json!({
            "contents": {
                "kind": "markdown",
                "value": "```lean\ntheorem foo : True\n```\nDocumentation paragraph."
            }
        });
        let hover = parse_hover(&result).expect("hover should parse");
        assert_eq!(hover.contents, "theorem foo : True");
    }

    #[test]
    fn parse_hover_null_returns_none() {
        assert!(parse_hover(&Value::Null).is_none());
    }

    #[test]
    fn parse_hover_missing_contents_returns_none() {
        assert!(parse_hover(&json!({})).is_none());
    }

    #[test]
    fn parse_hover_bare_string() {
        let result = json!({ "contents": "inline type" });
        let hover = parse_hover(&result).expect("hover should parse");
        assert_eq!(hover.contents, "inline type");
    }

    #[test]
    fn parse_hover_array_contents() {
        let result = json!({
            "contents": [
                { "language": "lean", "value": "theorem foo : True" },
                "Some docs"
            ]
        });
        let hover = parse_hover(&result).expect("hover should parse");
        // Array is joined then the first fenced block extracted. Here, no fence
        // so the whole thing is preserved as the best-effort fallback.
        assert!(hover.contents.contains("theorem foo : True"));
    }

    #[test]
    fn parse_hover_empty_returns_none() {
        let result = json!({ "contents": { "kind": "markdown", "value": "" } });
        assert!(parse_hover(&result).is_none());
    }

    // ── Definition parsing ────────────────────────────────────────────

    #[test]
    fn parse_definition_single_location() {
        let result = json!({
            "uri": "file:///home/user/proof.lean",
            "range": {
                "start": { "line": 3, "character": 8 },
                "end": { "line": 3, "character": 11 }
            }
        });
        let def = parse_definition(&result).expect("definition should parse");
        assert_eq!(def.uri, "file:///home/user/proof.lean");
        assert_eq!(def.line, 3);
        assert_eq!(def.character, 8);
        assert_eq!(def.end_line, 3);
        assert_eq!(def.end_character, 11);
    }

    #[test]
    fn parse_definition_array_picks_first() {
        let result = json!([
            {
                "uri": "file:///a.lean",
                "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 3 } }
            },
            {
                "uri": "file:///b.lean",
                "range": { "start": { "line": 5, "character": 0 }, "end": { "line": 5, "character": 3 } }
            }
        ]);
        let def = parse_definition(&result).expect("definition should parse");
        assert_eq!(def.uri, "file:///a.lean");
    }

    #[test]
    fn parse_definition_null_returns_none() {
        assert!(parse_definition(&Value::Null).is_none());
    }

    #[test]
    fn parse_definition_empty_array_returns_none() {
        assert!(parse_definition(&json!([])).is_none());
    }

    #[test]
    fn parse_definition_location_link() {
        // Lean sometimes returns LocationLink even when linkSupport is false.
        let result = json!([{
            "originSelectionRange": {
                "start": { "line": 3, "character": 27 },
                "end": { "line": 3, "character": 37 }
            },
            "targetUri": "file:///proof.lean",
            "targetRange": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 20 }
            },
            "targetSelectionRange": {
                "start": { "line": 1, "character": 8 },
                "end": { "line": 1, "character": 18 }
            }
        }]);
        let def = parse_definition(&result).expect("should parse LocationLink");
        assert_eq!(def.uri, "file:///proof.lean");
        // Prefers targetSelectionRange over targetRange.
        assert_eq!(def.line, 1);
        assert_eq!(def.character, 8);
        assert_eq!(def.end_line, 1);
        assert_eq!(def.end_character, 18);
    }

    #[test]
    fn parse_definition_location_link_without_selection_range() {
        // Falls back to targetRange when targetSelectionRange is absent.
        let result = json!([{
            "targetUri": "file:///a.lean",
            "targetRange": {
                "start": { "line": 5, "character": 2 },
                "end": { "line": 5, "character": 10 }
            }
        }]);
        let def = parse_definition(&result).expect("should parse LocationLink");
        assert_eq!(def.uri, "file:///a.lean");
        assert_eq!(def.line, 5);
        assert_eq!(def.character, 2);
    }

    // ── Code action parsing ───────────────────────────────────────────

    #[test]
    fn parse_code_actions_with_inline_edit() {
        let result = json!([
            {
                "title": "Try this: exact rfl",
                "kind": "quickfix",
                "edit": {
                    "changes": {
                        "file:///proof.lean": [
                            {
                                "range": {
                                    "start": { "line": 2, "character": 2 },
                                    "end": { "line": 2, "character": 7 }
                                },
                                "newText": "exact rfl"
                            }
                        ]
                    }
                }
            }
        ]);
        let actions = parse_code_actions(&result);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Try this: exact rfl");
        assert_eq!(actions[0].kind.as_deref(), Some("quickfix"));
        let edit = actions[0].edit.as_ref().expect("should have edit");
        assert_eq!(edit.changes.len(), 1);
        let (uri, edits) = &edit.changes[0];
        assert_eq!(uri, "file:///proof.lean");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "exact rfl");
        assert_eq!(edits[0].start_line, 2);
        assert_eq!(edits[0].start_character, 2);
    }

    #[test]
    fn parse_code_actions_with_resolve_data() {
        let result = json!([
            {
                "title": "Lazy action",
                "data": { "token": 42 }
            }
        ]);
        let actions = parse_code_actions(&result);
        assert_eq!(actions.len(), 1);
        assert!(actions[0].edit.is_none());
        assert_eq!(actions[0].resolve_data, Some(json!({ "token": 42 })));
    }

    #[test]
    fn parse_code_actions_empty_array() {
        assert!(parse_code_actions(&json!([])).is_empty());
    }

    #[test]
    fn parse_code_actions_null() {
        assert!(parse_code_actions(&Value::Null).is_empty());
    }

    #[test]
    fn parse_code_actions_skips_commands_without_title() {
        // LSP `Command` items have `command`/`title`; our filter relies on title.
        let result = json!([
            { "command": "foo" },
            { "title": "Real action" }
        ]);
        let actions = parse_code_actions(&result);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Real action");
    }

    // ── Document symbol parsing ───────────────────────────────────────

    #[test]
    fn parse_document_symbols_flat_list() {
        let result = json!([
            {
                "name": "foo",
                "kind": 12,
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 2, "character": 0 }
                },
                "selectionRange": {
                    "start": { "line": 0, "character": 8 },
                    "end": { "line": 0, "character": 11 }
                }
            },
            {
                "name": "bar",
                "kind": 13,
                "range": {
                    "start": { "line": 3, "character": 0 },
                    "end": { "line": 4, "character": 0 }
                },
                "selectionRange": {
                    "start": { "line": 3, "character": 4 },
                    "end": { "line": 3, "character": 7 }
                }
            }
        ]);
        let symbols = parse_document_symbols(&result);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "foo");
        assert_eq!(symbols[0].kind, 12);
        assert_eq!(symbols[0].start_line, 0);
        assert!(symbols[0].children.is_empty());
        assert_eq!(symbols[1].name, "bar");
    }

    #[test]
    fn parse_document_symbols_hierarchical() {
        let result = json!([
            {
                "name": "Ns",
                "kind": 3,
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 10, "character": 0 }
                },
                "selectionRange": {
                    "start": { "line": 0, "character": 10 },
                    "end": { "line": 0, "character": 12 }
                },
                "children": [
                    {
                        "name": "inner",
                        "kind": 12,
                        "range": {
                            "start": { "line": 2, "character": 2 },
                            "end": { "line": 4, "character": 0 }
                        },
                        "selectionRange": {
                            "start": { "line": 2, "character": 10 },
                            "end": { "line": 2, "character": 15 }
                        }
                    }
                ]
            }
        ]);
        let symbols = parse_document_symbols(&result);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].children.len(), 1);
        assert_eq!(symbols[0].children[0].name, "inner");
        assert_eq!(symbols[0].children[0].start_line, 2);
    }

    #[test]
    fn parse_document_symbols_null_returns_empty() {
        assert!(parse_document_symbols(&Value::Null).is_empty());
    }

    #[test]
    fn parse_workspace_edit_multiple_uris() {
        let edit = json!({
            "changes": {
                "file:///a.lean": [
                    {
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 1 }
                        },
                        "newText": "a"
                    }
                ],
                "file:///b.lean": [
                    {
                        "range": {
                            "start": { "line": 1, "character": 1 },
                            "end": { "line": 1, "character": 2 }
                        },
                        "newText": "b"
                    }
                ]
            }
        });
        let ws = parse_workspace_edit(&edit).expect("parse");
        assert_eq!(ws.changes.len(), 2);
    }

    #[test]
    fn parse_workspace_edit_missing_changes_returns_none() {
        assert!(parse_workspace_edit(&json!({})).is_none());
    }
}
