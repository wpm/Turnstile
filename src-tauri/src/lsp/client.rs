//! LSP client handle and lifecycle state machine.
//!
//! [`LspClient`] owns the spawned `lean --server` process and provides the
//! async API for sending JSON-RPC requests and notifications. The protocol
//! lifecycle is tracked by [`LspLifecycle`], which follows the LSP 3.17 state
//! diagram: `WaitingForInitialize` → `Initializing` → `Initialized` →
//! `NormalOperation` → `ShuttingDown` → `ReadyToExit` → `Exited`.
//!
//! Guarded methods (`send_request_await`, `send_notification`) reject calls
//! made from the wrong state. The `_unchecked` variants (crate-internal) bypass
//! the guard for the initialization handshake, where the state is necessarily
//! still `Initializing` or `Initialized`.
//!
//! The stdout reader runs on a dedicated OS thread (see [`LspClient::receive_messages`]).
//! Responses to pending requests are routed back via per-request `mpsc` channels;
//! all other messages (notifications, server→client requests) are dispatched to
//! the caller-supplied callback.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use log::{debug, error, warn};
use serde_json::{json, Value};

use super::{ack_request, LspError};

/// Lifecycle states of the LSP protocol handshake.
///
/// Transitions follow the LSP 3.17 specification state diagram:
///
///   `WaitingForInitialize` → `Initializing` (initialize request sent)
///   `Initializing` → `Initialized` (initialize response received)
///   `Initialized` → `NormalOperation` (initialized notification sent)
///   `NormalOperation` → `ShuttingDown` (shutdown request sent)
///   `ShuttingDown` → `ReadyToExit` (shutdown response received)
///   `ReadyToExit` → `Exited` (exit notification sent)
///   any → `Error` (connection loss or protocol error)
///   `Initializing` → `WaitingForInitialize` (initialize error, can retry)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspLifecycle {
    WaitingForInitialize,
    Initializing,
    /// `initialize` response received; `initialized` notification not yet sent.
    Initialized,
    NormalOperation,
    ShuttingDown,
    ReadyToExit,
    Exited,
    Error(String),
}

impl std::fmt::Display for LspLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WaitingForInitialize => write!(f, "WaitingForInitialize"),
            Self::Initializing => write!(f, "Initializing"),
            Self::Initialized => write!(f, "Initialized"),
            Self::NormalOperation => write!(f, "NormalOperation"),
            Self::ShuttingDown => write!(f, "ShuttingDown"),
            Self::ReadyToExit => write!(f, "ReadyToExit"),
            Self::Exited => write!(f, "Exited"),
            Self::Error(msg) => write!(f, "Error({msg})"),
        }
    }
}

pub struct LspClient {
    process: Child,
    pub(crate) next_id: Arc<AtomicI64>,
    /// Cloned by the reader thread and transport helpers to write messages without going
    /// through `LspClient` (needed because the reader thread doesn't hold a client ref).
    pub(crate) writer: Arc<tokio::sync::Mutex<Box<dyn Write + Send>>>,
    token_types: Arc<Mutex<Vec<String>>>,
    token_modifiers: Arc<Mutex<Vec<String>>>,
    pub(crate) pending: Arc<Mutex<HashMap<i64, mpsc::SyncSender<Value>>>>,
    lifecycle: LspLifecycle,
}

impl LspClient {
    fn new(process: Child, writer: impl Write + Send + 'static) -> Self {
        Self {
            process,
            next_id: Arc::new(AtomicI64::new(1)),
            writer: Arc::new(tokio::sync::Mutex::new(Box::new(writer))),
            token_types: Arc::new(Mutex::new(Vec::new())),
            token_modifiers: Arc::new(Mutex::new(Vec::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            lifecycle: LspLifecycle::WaitingForInitialize,
        }
    }

    /// Spawn an LSP server process and return a client handle.
    ///
    /// # Errors
    /// Returns an error if the process cannot be spawned or its stdin cannot be captured.
    pub fn spawn(command: &str, args: &[&str], cwd: &Path) -> Result<Self, LspError> {
        debug!(
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

        Ok(Self::new(child, stdin))
    }

    /// Cloned `Arc` to the token type legend, for use outside the lock.
    #[must_use]
    pub fn token_types(&self) -> Arc<Mutex<Vec<String>>> {
        self.token_types.clone()
    }

    /// Cloned `Arc` to the token modifier legend, for use outside the lock.
    #[must_use]
    pub fn token_modifiers(&self) -> Arc<Mutex<Vec<String>>> {
        self.token_modifiers.clone()
    }

    /// Current lifecycle state.
    #[must_use]
    pub const fn lifecycle(&self) -> &LspLifecycle {
        &self.lifecycle
    }

    /// Advance the lifecycle to a new state.
    pub fn set_lifecycle(&mut self, state: LspLifecycle) {
        self.lifecycle = state;
    }

    /// Send a JSON-RPC request and return the id used.
    ///
    /// # Errors
    /// Returns an error if serialization or writing to the server fails.
    pub async fn send_request(&self, method: &str, params: Value) -> Result<i64, LspError> {
        self.ensure_normal_operation()?;
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
        self.ensure_normal_operation()?;
        self.send_request_await_impl(method, params).await
    }

    /// Send a JSON-RPC notification (no id, no response expected).
    ///
    /// # Errors
    /// Returns an error if serialization or writing to the server fails.
    pub async fn send_notification(&self, method: &str, params: Value) -> Result<(), LspError> {
        self.ensure_can_notify()?;
        self.send_notification_impl(method, params).await
    }

    /// Send a request bypassing lifecycle guards. Used during the LSP initialization
    /// handshake where the state is `Initializing`, not yet `NormalOperation`.
    ///
    /// # Errors
    /// Returns an error if the request cannot be sent or the response times out.
    pub(crate) async fn send_request_await_unchecked(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, LspError> {
        self.send_request_await_impl(method, params).await
    }

    /// Send a notification bypassing lifecycle guards. Used during the LSP initialization
    /// handshake to send the `initialized` notification while still in `Initialized` state.
    ///
    /// # Errors
    /// Returns an error if serialization or writing to the server fails.
    pub(crate) async fn send_notification_unchecked(
        &self,
        method: &str,
        params: Value,
    ) -> Result<(), LspError> {
        self.send_notification_impl(method, params).await
    }

    /// Receive messages from stdout in a blocking loop. Call from a spawned thread.
    /// Routes responses to any registered pending senders; passes the rest to `on_message`.
    pub fn receive_messages<F>(
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
                        debug!("LSP server stdout closed");
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
                        "Client ← LSP\n{}",
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

    /// Spawn the stdout reader thread, then perform the `initialize` / `initialized`
    /// handshake. On success the client is in `NormalOperation` state.
    ///
    /// `on_notification` is called for every server-pushed message that is not a
    /// response to a pending request. Use it to forward notifications to a channel.
    ///
    /// # Errors
    /// Returns an error if stdout is unavailable or the handshake fails.
    pub async fn initialize<F>(
        &mut self,
        root_uri: &str,
        on_notification: F,
    ) -> Result<Value, LspError>
    where
        F: FnMut(&Value) + Send + 'static,
    {
        let stdout = self.take_stdout().ok_or(LspError::StdinCaptureFailed)?;
        let pending = self.pending.clone();
        let writer = self.writer.clone();
        let mut on_notification = on_notification;
        std::thread::spawn(move || {
            Self::receive_messages(stdout, &pending, move |msg| {
                if msg.get("id").is_some() && msg.get("method").is_some() {
                    ack_request(&writer, &msg["id"]).ok();
                    return;
                }
                on_notification(msg);
            });
        });

        self.set_lifecycle(LspLifecycle::Initializing);
        let init_result = self
            .send_request_await_unchecked("initialize", super::initialize_params(root_uri))
            .await?;
        self.set_lifecycle(LspLifecycle::Initialized);
        self.send_notification_unchecked("initialized", json!({}))
            .await?;
        self.set_lifecycle(LspLifecycle::NormalOperation);
        Ok(init_result)
    }

    async fn send_message(&self, msg: &Value) -> Result<(), LspError> {
        debug!(
            "Client → LSP\n{}",
            serde_json::to_string_pretty(msg).unwrap_or_default()
        );
        let mut writer = self.writer.lock().await;
        super::transport::write_framed_message(&mut *writer, msg)
    }

    async fn send_request_await_impl(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, LspError> {
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

    async fn send_notification_impl(&self, method: &str, params: Value) -> Result<(), LspError> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.send_message(&msg).await
    }

    fn ensure_normal_operation(&self) -> Result<(), LspError> {
        if self.lifecycle == LspLifecycle::NormalOperation {
            Ok(())
        } else {
            Err(LspError::InvalidState {
                current: self.lifecycle.to_string(),
                expected: "NormalOperation".to_string(),
            })
        }
    }

    /// Permits `Initialized` (for the `initialized` handshake notification) and `NormalOperation`.
    fn ensure_can_notify(&self) -> Result<(), LspError> {
        match self.lifecycle {
            LspLifecycle::Initialized | LspLifecycle::NormalOperation => Ok(()),
            _ => Err(LspError::InvalidState {
                current: self.lifecycle.to_string(),
                expected: "Initialized or NormalOperation".to_string(),
            }),
        }
    }
}

// ── Shutdown ──────────────────────────────────────────────────────────

impl LspClient {
    /// Gracefully shut down the LSP server process.
    ///
    /// Sends a `shutdown` JSON-RPC request, waits briefly for the response,
    /// then sends an `exit` notification and waits for the child to exit.
    /// If the process doesn't exit within the timeout, it is killed.
    ///
    /// This is intentionally synchronous so it can be called from `Drop`.
    pub fn shutdown(&mut self) {
        debug!("Initiating LSP server shutdown");
        self.lifecycle = LspLifecycle::ShuttingDown;

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
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        self.lifecycle = LspLifecycle::ReadyToExit;

        let exit_msg = json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null,
        });
        if let Err(e) = self.send_message_sync(&exit_msg) {
            warn!("Failed to send exit notification: {e}");
        }
        self.lifecycle = LspLifecycle::Exited;

        if let Some(status) = self.process.try_wait().ok().flatten().or_else(|| {
            std::thread::sleep(std::time::Duration::from_secs(1));
            self.process.try_wait().ok().flatten()
        }) {
            if status.success() {
                debug!("LSP server exited: {status}");
            } else {
                warn!("LSP server exited with error: {status}");
            }
        } else {
            warn!("LSP server did not exit in time, killing");
            if let Err(e) = self.process.kill() {
                warn!("Failed to kill LSP server: {e}");
            } else {
                let _ = self.process.wait();
            }
        }
    }

    fn send_message_sync(&self, msg: &Value) -> Result<(), LspError> {
        debug!(
            "Client → LSP\n{}",
            serde_json::to_string_pretty(msg).unwrap_or_default()
        );
        let mut writer = self
            .writer
            .try_lock()
            .map_err(|_| LspError::WriterContended)?;
        super::transport::write_framed_message(&mut *writer, msg)
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::process::Stdio;

    fn spawn_cat_client() -> LspClient {
        LspClient::spawn("cat", &[], Path::new("/tmp")).expect("failed to spawn cat")
    }

    fn extract_jsonrpc_messages(buf: &[u8]) -> Vec<Value> {
        let mut msgs = Vec::new();
        let mut cursor = std::io::Cursor::new(buf);
        let mut reader = BufReader::new(&mut cursor);

        loop {
            let mut content_length: Option<usize> = None;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => return msgs,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            break;
                        }
                        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
                            content_length = len_str.parse::<usize>().ok();
                        }
                    }
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
    fn lifecycle_starts_at_waiting_for_initialize() {
        let client = spawn_cat_client();
        assert_eq!(client.lifecycle(), &LspLifecycle::WaitingForInitialize);
    }

    #[test]
    fn lifecycle_display() {
        assert_eq!(LspLifecycle::NormalOperation.to_string(), "NormalOperation");
        assert_eq!(
            LspLifecycle::Error("oops".to_string()).to_string(),
            "Error(oops)"
        );
    }

    #[test]
    fn shutdown_sends_shutdown_request_and_exit_notification() {
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

        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

        let child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn cat");

        let writer = BufWriter(Arc::clone(&buf));
        let mut client = LspClient::new(child, writer);
        client.lifecycle = LspLifecycle::NormalOperation;

        client.shutdown();

        let captured = buf.lock().unwrap();
        let msgs = extract_jsonrpc_messages(&captured);
        drop(captured);

        assert!(
            msgs.len() >= 2,
            "Expected at least 2 messages (shutdown + exit), got {}: {msgs:?}",
            msgs.len()
        );

        assert_eq!(msgs[0]["method"], "shutdown");
        assert!(msgs[0].get("id").is_some(), "shutdown should have an id");

        assert_eq!(msgs[1]["method"], "exit");
        assert!(
            msgs[1].get("id").is_none(),
            "exit should be a notification (no id)"
        );

        assert_eq!(client.lifecycle(), &LspLifecycle::Exited);
    }

    #[test]
    fn drop_does_not_panic_when_process_already_dead() {
        let mut client = spawn_cat_client();
        let _ = client.process.kill();
        let _ = client.process.wait();
        drop(client);
    }

    #[test]
    fn drop_does_not_panic_on_normal_client() {
        let client = spawn_cat_client();
        drop(client);
    }
}
