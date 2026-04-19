//! Structured error type for the LSP client.
//!
//! `LspError` replaces the prior `Result<T, String>` return shape across
//! `lsp.rs`. Each variant encodes a distinct failure mode so callers, logs,
//! and tests can pattern-match instead of string-comparing. The Tauri command
//! boundary still speaks `Result<T, String>` to the frontend — the
//! `From<LspError> for String` impl keeps that wire format intact.

use std::io;

/// Every failure mode a call into the LSP client can produce.
#[derive(Debug, thiserror::Error)]
pub enum LspError {
    /// The LSP server process could not be spawned (e.g. binary not found).
    #[error("failed to spawn LSP server '{command}': {source}")]
    SpawnFailed {
        command: String,
        #[source]
        source: io::Error,
    },

    /// `Command::spawn` succeeded but the child's stdin was not piped.
    #[error("failed to capture LSP server stdin")]
    StdinCaptureFailed,

    /// A Tauri command was invoked before `start_lsp` completed.
    #[error("not connected to LSP server")]
    NotConnected,

    /// An awaited JSON-RPC response did not arrive within the timeout.
    #[error("timed out waiting for LSP response to {method}")]
    Timeout { method: String },

    /// JSON (de)serialization of a JSON-RPC message failed.
    #[error("JSON serialization failed")]
    Serde(#[from] serde_json::Error),

    /// A write or flush to the server's stdin failed.
    #[error("I/O error during {operation}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },

    /// A `std::sync::Mutex` guarding LSP client state was poisoned.
    #[error("{lock} lock poisoned")]
    LockPoisoned { lock: &'static str },

    /// `send_message_sync` could not acquire the writer lock during shutdown.
    #[error("writer lock contended during shutdown")]
    WriterContended,

    /// A `tokio::task::spawn_blocking` handle failed to join.
    #[error("spawn_blocking join failed")]
    JoinError(#[from] tokio::task::JoinError),

    /// The document revision advanced while a per-line goal-state fetch was
    /// in flight. Sentinel used by `fetch_per_line_goal_states` to bail out
    /// without emitting a stale result.
    #[error("stale request (document revision advanced)")]
    Stale,
}

impl From<LspError> for String {
    fn from(err: LspError) -> Self {
        err.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;
    use std::io::ErrorKind;

    #[test]
    fn display_spawn_failed_includes_command_and_os_error() {
        let err = LspError::SpawnFailed {
            command: "lean".to_string(),
            source: io::Error::new(ErrorKind::NotFound, "no such file"),
        };
        let msg = err.to_string();
        assert!(msg.contains("lean"), "missing command: {msg}");
        assert!(msg.contains("no such file"), "missing os error: {msg}");
    }

    #[test]
    fn display_stdin_capture_failed() {
        assert_eq!(
            LspError::StdinCaptureFailed.to_string(),
            "failed to capture LSP server stdin"
        );
    }

    #[test]
    fn display_not_connected() {
        assert_eq!(
            LspError::NotConnected.to_string(),
            "not connected to LSP server"
        );
    }

    #[test]
    fn display_timeout_includes_method() {
        let err = LspError::Timeout {
            method: "textDocument/hover".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "timed out waiting for LSP response to textDocument/hover"
        );
    }

    #[test]
    fn display_io_includes_operation() {
        let err = LspError::Io {
            operation: "write header",
            source: io::Error::new(ErrorKind::BrokenPipe, "pipe closed"),
        };
        let msg = err.to_string();
        assert!(msg.contains("write header"), "missing op: {msg}");
    }

    #[test]
    fn display_lock_poisoned_includes_name() {
        assert_eq!(
            LspError::LockPoisoned { lock: "pending" }.to_string(),
            "pending lock poisoned"
        );
    }

    #[test]
    fn display_writer_contended() {
        assert_eq!(
            LspError::WriterContended.to_string(),
            "writer lock contended during shutdown"
        );
    }

    #[test]
    fn display_stale() {
        assert_eq!(
            LspError::Stale.to_string(),
            "stale request (document revision advanced)"
        );
    }

    #[test]
    fn from_lsp_error_for_string_matches_display() {
        let err = LspError::Timeout {
            method: "$/lean/plainGoal".to_string(),
        };
        let expected = err.to_string();
        let got: String = err.into();
        assert_eq!(got, expected);
    }

    #[test]
    fn from_serde_error_via_question_mark() {
        fn inner() -> Result<(), LspError> {
            let _: serde_json::Value = serde_json::from_str("not json")?;
            Ok(())
        }
        let err = inner().unwrap_err();
        assert!(matches!(err, LspError::Serde(_)));
    }

    #[test]
    fn serde_error_source_is_populated() {
        let parse_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err: LspError = parse_err.into();
        assert!(err.source().is_some(), "Serde variant should expose source");
    }

    #[test]
    fn spawn_failed_source_is_populated() {
        let err = LspError::SpawnFailed {
            command: "lean".to_string(),
            source: io::Error::new(ErrorKind::NotFound, "no such file"),
        };
        assert!(
            err.source().is_some(),
            "SpawnFailed should expose io::Error source"
        );
    }

    #[test]
    fn io_variant_source_is_populated() {
        let err = LspError::Io {
            operation: "flush",
            source: io::Error::new(ErrorKind::BrokenPipe, "pipe closed"),
        };
        assert!(
            err.source().is_some(),
            "Io variant should expose io::Error source"
        );
    }

    #[test]
    fn simple_variants_have_no_source() {
        assert!(LspError::NotConnected.source().is_none());
        assert!(LspError::StdinCaptureFailed.source().is_none());
        assert!(LspError::WriterContended.source().is_none());
        assert!(LspError::Stale.source().is_none());
        assert!(LspError::LockPoisoned { lock: "x" }.source().is_none());
        assert!(LspError::Timeout {
            method: "m".to_string(),
        }
        .source()
        .is_none());
    }
}
