//! Low-level JSON-RPC 2.0 transport: Content-Length framing and fire-and-forget helpers.
//!
//! The core primitive is [`write_framed_message`], which serializes a JSON value
//! and writes it as a single Content-Length-framed LSP message to any `Write`
//! sink. Callers are responsible for acquiring the writer lock before calling it,
//! which lets them choose between async lock (`.await`), blocking lock, or
//! try-lock depending on context.
//!
//! [`ack_request`] and [`send_request_sync`] are convenience wrappers for the
//! two cases where the sync reader thread needs to write back to the server:
//! acknowledging server→client requests (required by the LSP spec) and issuing
//! fire-and-forget requests whose responses are handled by the `on_message` callback.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

use super::LspError;

/// Serialize `msg` and write it as a Content-Length-framed LSP message.
///
/// Takes an already-locked writer so callers control locking strategy
/// (async `.lock().await`, sync `.blocking_lock()`, or try-lock).
pub fn write_framed_message(writer: &mut dyn Write, msg: &Value) -> Result<(), LspError> {
    let body = serde_json::to_string(msg)?;
    let mut framed = format!("Content-Length: {}\r\n\r\n", body.len());
    framed.push_str(&body);
    writer
        .write_all(framed.as_bytes())
        .map_err(|source| LspError::Io {
            operation: "write message",
            source,
        })?;
    writer.flush().map_err(|source| LspError::Io {
        operation: "flush",
        source,
    })
}

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
    write_framed_message(&mut *writer.blocking_lock(), &msg)
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
    params: &Value,
) -> Result<(), LspError> {
    let id = next_id.fetch_add(1, Ordering::SeqCst);
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    write_framed_message(&mut *writer.blocking_lock(), &msg)
}

/// Convert a filesystem path to a `file://` URI with full RFC 8089 percent-encoding.
#[must_use]
pub fn path_to_file_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .map_or_else(|()| format!("file://{}", path.display()), |u| u.to_string())
}
