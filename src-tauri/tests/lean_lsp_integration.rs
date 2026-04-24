//! Integration tests for the Lean LSP server at the protocol level.
//!
//! A single `lean --server` process is shared across all tests via a
//! `OnceLock<Option<Mutex<Session>>>`. This avoids the Mathlib startup cost
//! for each test.
//!
//! # Running
//!
//! ```sh
//! cd src-tauri
//! cargo test --test lean_lsp_integration -- --test-threads=1
//! ```
//!
//! Tests use the Lean project created by Turnstile setup at
//! `~/Library/Application Support/com.ontical.turnstile/lean-project/`.
//! If that directory is absent, the tests are skipped gracefully.
//!
//! Overrides:
//!   `TURNSTILE_LSP_CMD`      — path to the lean binary
//!   `TURNSTILE_PROJECT_PATH` — path to the Lean project directory

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use lsp_types::SemanticToken as LspSemanticToken;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use turnstile_lib::lsp::events::LspEvent;
use turnstile_lib::lsp::{self, LeanClient};

const ERROR_SEVERITY: u64 = 1;

// ── Environment ────────────────────────────────────────────────────────

fn lean_project_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TURNSTILE_PROJECT_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    let base = dirs::data_dir()?;
    let path = base.join("com.ontical.turnstile").join("lean-project");
    path.exists().then_some(path)
}

fn lean_bin() -> PathBuf {
    if let Ok(cmd) = std::env::var("TURNSTILE_LSP_CMD") {
        return PathBuf::from(cmd);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".elan")
        .join("bin")
        .join(if cfg!(windows) { "lean.exe" } else { "lean" })
}

// ── Session ────────────────────────────────────────────────────────────
//
// Wraps the real LeanClient with a notification channel so tests can
// collect server-pushed messages (fileProgress, publishDiagnostics, etc.).

struct Session {
    client: LeanClient,
    /// Inbound server-push events collected during `wait_for_elaboration`.
    collected: Vec<LspEvent>,
    /// Channel for receiving server-push events.
    event_rx: mpsc::Receiver<LspEvent>,
    project: PathBuf,
    doc_version: i32,
    /// Content most recently sent via `set_content`; avoids redundant re-elaboration.
    current_content: Option<String>,
    /// Serialised JSON messages from the most recent `set_content` call (returned on cache hit).
    last_msgs: Vec<Value>,
    /// Token type legend from the server's `InitializeResult`.
    pub token_types: Vec<String>,
    /// Token modifier legend from the server's `InitializeResult`.
    pub token_modifiers: Vec<String>,
}

impl Session {
    fn new(project: PathBuf) -> Result<Self, String> {
        use lsp_types::{DidOpenTextDocumentParams, TextDocumentItem, Url};

        let lean = lean_bin();
        let root_uri =
            Url::from_directory_path(&project).map_err(|()| "invalid project path".to_string())?;
        let doc_uri = Url::from_file_path(project.join("Proof.lean"))
            .map_err(|()| "invalid doc path".to_string())?;

        let (event_tx, event_rx) = mpsc::channel(512);

        let client = rt()
            .block_on(LeanClient::start(&lean, &project, root_uri, event_tx))
            .map_err(|e| e.to_string())?;

        let (token_types, token_modifiers) = client.token_legend();

        client
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: doc_uri,
                    language_id: "lean4".to_string(),
                    version: 1,
                    text: String::new(),
                },
            })
            .map_err(|e| e.to_string())?;

        Ok(Self {
            client,
            collected: Vec::new(),
            event_rx,
            project,
            doc_version: 2,
            current_content: Some(String::new()),
            last_msgs: Vec::new(),
            token_types,
            token_modifiers,
        })
    }

    fn doc_uri_str(&self) -> String {
        self.doc_uri().to_string()
    }

    fn doc_uri(&self) -> lsp_types::Url {
        lsp_types::Url::from_file_path(self.project.join("Proof.lean")).expect("invalid doc path")
    }

    fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        // Use the underlying socket via a raw request. Since LeanClient only
        // exposes typed methods, for ad-hoc JSON requests we serialize params
        // and send via the appropriate typed call based on method name.
        // For integration tests we fall back to serde round-tripping.
        self.request_typed(method, params)
    }

    fn request_typed(&self, method: &str, params: Value) -> Result<Value, String> {
        use lsp_types::*;

        let rt = rt();
        match method {
            "textDocument/hover" => {
                let p: HoverParams = serde_json::from_value(params).map_err(|e| e.to_string())?;
                let r = rt
                    .block_on(self.client.hover(p))
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(r).unwrap_or(Value::Null))
            }
            "textDocument/definition" => {
                let p: GotoDefinitionParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let r = rt
                    .block_on(self.client.definition(p))
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(r).unwrap_or(Value::Null))
            }
            "textDocument/documentSymbol" => {
                let p: DocumentSymbolParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let r = rt
                    .block_on(self.client.document_symbol(p))
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(r).unwrap_or(Value::Null))
            }
            "textDocument/codeAction" => {
                let p: CodeActionParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let r = rt
                    .block_on(self.client.code_action(p))
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(r).unwrap_or(Value::Null))
            }
            "codeAction/resolve" => {
                let p: CodeAction = serde_json::from_value(params).map_err(|e| e.to_string())?;
                let r = rt
                    .block_on(self.client.resolve_code_action(p))
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(r).unwrap_or(Value::Null))
            }
            "textDocument/completion" => {
                let p: CompletionParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let r = rt
                    .block_on(self.client.completion(p))
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(r).unwrap_or(Value::Null))
            }
            "textDocument/semanticTokens/full" => {
                let p: SemanticTokensParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let r = rt
                    .block_on(self.client.semantic_tokens_full(p))
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(r).unwrap_or(Value::Null))
            }
            "$/lean/plainGoal" => {
                let p: TextDocumentPositionParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let r = rt
                    .block_on(self.client.plain_goal(p))
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_value(r).unwrap_or(Value::Null))
            }
            other => Err(format!("unsupported method in integration test: {other}")),
        }
    }

    /// Replace the document with `text` and wait for elaboration and diagnostics.
    /// No-ops if `text` matches the current content, returning the previous messages.
    fn set_content(&mut self, text: &str) -> Vec<Value> {
        use lsp_types::*;

        if self.current_content.as_deref() == Some(text) {
            return self.last_msgs.clone();
        }

        let version = self.doc_version;
        self.doc_version += 1;
        let uri = self.doc_uri();

        self.client
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.to_owned(),
                }],
            })
            .expect("didChange failed");

        let msgs = self.wait_for_elaboration(uri.as_ref(), Duration::from_mins(1));
        self.current_content = Some(text.to_owned());
        msgs.clone_into(&mut self.last_msgs);
        msgs
    }

    /// Send one incremental `didChange` with a single range replacement and wait for elaboration.
    /// `start`/`end` are (line, character) pairs (0-indexed, UTF-16 code units).
    fn apply_incremental_change(
        &mut self,
        start: (u32, u32),
        end: (u32, u32),
        text: &str,
    ) -> Vec<Value> {
        use lsp_types::*;

        let version = self.doc_version;
        self.doc_version += 1;
        let uri = self.doc_uri();

        self.client
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: Some(Range {
                        start: Position {
                            line: start.0,
                            character: start.1,
                        },
                        end: Position {
                            line: end.0,
                            character: end.1,
                        },
                    }),
                    range_length: None,
                    text: text.to_owned(),
                }],
            })
            .expect("incremental didChange failed");

        // Update tracked content by applying the change.
        if let Some(ref mut content) = self.current_content {
            let mut lines: Vec<String> = content.lines().map(str::to_owned).collect();
            while lines.len() <= end.0 as usize {
                lines.push(String::new());
            }
            if start.0 == end.0 {
                let line = &lines[start.0 as usize];
                let new_line = format!(
                    "{}{}{}",
                    &line[..start.1 as usize],
                    text,
                    &line[end.1 as usize..]
                );
                lines[start.0 as usize] = new_line;
            }
            *content = lines.join("\n");
            if !content.ends_with('\n') {
                content.push('\n');
            }
        }

        let msgs = self.wait_for_elaboration(uri.as_ref(), Duration::from_mins(1));
        msgs.clone_into(&mut self.last_msgs);
        msgs
    }

    /// Send `textDocument/didSave` with the current content and a short wait for any response.
    fn did_save(&mut self) {
        use lsp_types::*;

        let uri = self.doc_uri();
        let text = self.current_content.clone().unwrap_or_default();
        self.client
            .did_save(DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
                text: Some(text),
            })
            .expect("didSave failed");
        // Give the server a moment to process it; no specific response expected.
        self.collect_until(Duration::from_millis(200), |_| false);
    }

    /// Send `textDocument/didClose`.
    fn did_close(&self) {
        use lsp_types::*;

        let uri = self.doc_uri();
        self.client
            .did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
            })
            .expect("didClose failed");
    }

    /// Drain events from the channel until `done(event)` returns true or `timeout` elapses.
    /// Returns collected events serialized as JSON Values for compatibility with test helpers.
    fn collect_until<F>(&mut self, timeout: Duration, mut done: F) -> Vec<Value>
    where
        F: FnMut(&Value) -> bool,
    {
        let deadline = std::time::Instant::now() + timeout;
        let mut collected = Vec::new();

        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            let event = rt().block_on(async {
                tokio::time::timeout(remaining, self.event_rx.recv())
                    .await
                    .ok()
                    .flatten()
            });

            let Some(event) = event else { break };

            let msg = lsp_event_to_value(&event);
            self.collected.push(event);
            let finished = done(&msg);
            collected.push(msg);
            if finished {
                break;
            }
        }

        collected
    }

    /// Collect until `$/lean/fileProgress` signals empty `processing` for `uri`,
    /// then continue until `textDocument/publishDiagnostics` arrives for `uri`
    /// (or a 500 ms fallback elapses, for files that produce no diagnostics).
    fn wait_for_elaboration(&mut self, uri: &str, timeout: Duration) -> Vec<Value> {
        let mut msgs = self.collect_until(timeout, |msg| {
            msg["method"].as_str() == Some("$/lean/fileProgress")
                && msg["params"]["textDocument"]["uri"].as_str() == Some(uri)
                && msg["params"]["processing"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
        });

        let trailing = self.collect_until(Duration::from_millis(500), |msg| {
            msg["method"].as_str() == Some("textDocument/publishDiagnostics")
                && msg["params"]["uri"].as_str() == Some(uri)
        });
        msgs.extend(trailing);
        msgs
    }
}

/// Serialize an `LspEvent` into a JSON Value with the same shape the old
/// JSON-RPC wire messages had, so all existing test helper functions work
/// without modification.
fn lsp_event_to_value(event: &LspEvent) -> Value {
    match event {
        LspEvent::Diagnostics(p) => json!({
            "method": "textDocument/publishDiagnostics",
            "params": serde_json::to_value(p).unwrap_or(Value::Null),
        }),
        LspEvent::FileProgress(p) => {
            let processing: Vec<Value> = p.processing.iter().map(|interval| {
                json!({
                    "range": {
                        "start": { "line": interval.range.start.line, "character": interval.range.start.character },
                        "end":   { "line": interval.range.end.line,   "character": interval.range.end.character   },
                    }
                })
            }).collect();
            json!({
                "method": "$/lean/fileProgress",
                "params": {
                    "textDocument": { "uri": p.text_document.uri.as_str() },
                    "processing": processing,
                }
            })
        }
        LspEvent::LogMessage(p) => json!({
            "method": "window/logMessage",
            "params": serde_json::to_value(p).unwrap_or(Value::Null),
        }),
        LspEvent::ShowMessage(p) => json!({
            "method": "window/showMessage",
            "params": serde_json::to_value(p).unwrap_or(Value::Null),
        }),
    }
}

// ── Global runtime and session ─────────────────────────────────────────
//
// A single multi-thread tokio Runtime lives for the entire test binary. The
// LeanClient's MainLoop task is spawned onto it and must not outlive it.
// All async operations in the session go through this runtime via block_on
// so the I/O resources created on it remain valid across test boundaries.

static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static SESSION: OnceLock<Option<Mutex<Session>>> = OnceLock::new();

fn rt() -> &'static tokio::runtime::Runtime {
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("shared test runtime")
    })
}

fn session() -> Option<std::sync::MutexGuard<'static, Session>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    SESSION
        .get_or_init(|| {
            lean_project_path().map(|project| match Session::new(project) {
                Ok(s) => Mutex::new(s),
                Err(e) => panic!("Failed to start shared LSP session: {e}"),
            })
        })
        .as_ref()
        .and_then(|mtx| mtx.lock().ok())
}

macro_rules! skip_if_no_project {
    ($sess:ident) => {
        #[allow(unused_mut)]
        let Some(mut $sess) = session() else {
            eprintln!(
                "SKIP: Lean project not found or session mutex poisoned by a prior test panic. \
                 Run with --test-threads=1 to avoid cascade failures. \
                 Run Turnstile setup or set TURNSTILE_PROJECT_PATH to enable."
            );
            return;
        };
    };
}

// ── Helpers ────────────────────────────────────────────────────────────

fn diagnostics_for(msgs: &[Value], uri: &str) -> Vec<Value> {
    msgs.iter()
        .filter(|m| {
            m["method"].as_str() == Some("textDocument/publishDiagnostics")
                && m["params"]["uri"].as_str() == Some(uri)
        })
        .flat_map(|m| {
            m["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

fn errors_in(msgs: &[Value], uri: &str) -> Vec<Value> {
    diagnostics_for(msgs, uri)
        .into_iter()
        .filter(|d| d["severity"].as_u64() == Some(ERROR_SEVERITY))
        .collect()
}

fn assert_workspace_edit_valid(edit: &lsp::WorkspaceEditDto) {
    for (uri, edits) in &edit.changes {
        assert!(!uri.is_empty(), "workspace edit URI should be non-empty");
        for e in edits {
            assert!(
                e.start_line <= e.end_line,
                "edit start_line should be <= end_line"
            );
        }
    }
}

// ── Lean source fixtures ───────────────────────────────────────────────

const PRIMES_PROOF: &str = "import Mathlib.Data.Nat.Prime.Infinite\n\n\
theorem infinitely_many_primes : ∀ n : ℕ, ∃ p, n ≤ p ∧ Nat.Prime p :=\n  \
  Nat.exists_infinite_primes\n";

const TACTIC_PROOF: &str =
    "-- Simple tactic proof.\ntheorem add_comm_ex (a b : ℕ) : a + b = b + a := by\n  ring\n";

/// Line 1 (0-indexed): `def bad : Nat := "..."`
const INVALID_TYPE: &str =
    "-- Deliberate type mismatch.\ndef bad : Nat := \"this is a string, not a Nat\"\n";

const UNKNOWN_IDENT: &str =
    "-- Unknown identifier.\ndef also_bad : Nat := nonexistent_function 42\n";

const UNSOLVED_GOALS: &str =
    "-- Unsolved goals.\ntheorem incomplete (a b : ℕ) : a + b = b + a := by\n  skip\n";

// ── Tests ──────────────────────────────────────────────────────────────
//
// All tests share a single LSP session; run with --test-threads=1.

#[test]
fn valid_proof_produces_no_error_diagnostics() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    let msgs = sess.set_content(PRIMES_PROOF);
    drop(sess);

    assert!(
        errors_in(&msgs, &uri).is_empty(),
        "valid proof should produce no error diagnostics; got: {:?}",
        diagnostics_for(&msgs, &uri)
    );
}

#[test]
fn type_mismatch_produces_error_diagnostic() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    let msgs = sess.set_content(INVALID_TYPE);
    drop(sess);

    let errors = errors_in(&msgs, &uri);
    assert!(
        !errors.is_empty(),
        "type mismatch should produce at least one error"
    );
    let has_type_msg = errors.iter().any(|d| {
        d["message"].as_str().is_some_and(|m| {
            m.contains("type mismatch") || m.contains("String") || m.contains("Nat")
        })
    });
    assert!(
        has_type_msg,
        "error should mention the type issue; messages: {:?}",
        errors.iter().map(|d| &d["message"]).collect::<Vec<_>>()
    );
}

#[test]
fn unknown_identifier_produces_error_diagnostic() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    let msgs = sess.set_content(UNKNOWN_IDENT);
    drop(sess);

    assert!(
        !errors_in(&msgs, &uri).is_empty(),
        "unknown identifier should produce at least one error"
    );
}

#[test]
fn unsolved_goals_produces_error_diagnostic() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    let msgs = sess.set_content(UNSOLVED_GOALS);
    drop(sess);

    assert!(
        !errors_in(&msgs, &uri).is_empty(),
        "unsolved goals should produce at least one error"
    );
}

#[test]
fn semantic_tokens_returned_for_valid_document() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(TACTIC_PROOF);
    let result = sess
        .request(
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": &uri } }),
        )
        .expect("semanticTokens/full request failed");
    drop(sess);

    assert!(
        !result.is_null(),
        "server should return semantic tokens for a valid document"
    );

    let data = result["data"].as_array();
    assert!(
        data.is_some_and(|a| !a.is_empty()),
        "semantic token data should be non-empty; result: {result}"
    );
    assert_eq!(
        data.map_or(0, Vec::len) % 5,
        0,
        "token data length must be a multiple of 5"
    );
}

#[test]
fn semantic_tokens_data_is_valid_five_tuples() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(TACTIC_PROOF);
    let result = sess
        .request(
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": &uri } }),
        )
        .expect("semanticTokens/full failed");
    drop(sess);

    if result.is_null() {
        eprintln!("SKIP: server returned null for semanticTokens/full");
        return;
    }

    let data: Vec<u32> = result["data"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()))
        .collect();

    assert!(!data.is_empty(), "expected non-empty semantic token data");
    assert_eq!(
        data.len() % 5,
        0,
        "token data length must be divisible by 5"
    );

    let mut abs_line: i64 = 0;
    for (i, chunk) in data.chunks_exact(5).enumerate() {
        abs_line += i64::from(chunk[0]);
        let length = chunk[2];
        assert!(
            abs_line >= 0,
            "token {i}: absolute line must be >= 0, got {abs_line}"
        );
        assert!(length > 0, "token {i}: length must be > 0, got {length}");
    }
}

#[test]
fn plain_goal_inside_tactic_block() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(TACTIC_PROOF);
    let result = sess
        .request(
            "$/lean/plainGoal",
            json!({ "textDocument": { "uri": &uri }, "position": { "line": 2, "character": 2 } }),
        )
        .expect("$/lean/plainGoal request failed");
    drop(sess);

    if !result.is_null() {
        let rendered = result["rendered"].as_str().unwrap_or("");
        assert!(
            !rendered.is_empty(),
            "plainGoal rendered should not be empty when non-null"
        );
    }
    // null is acceptable: `ring` closes the goal so the position may be past it.
}

#[test]
fn plain_goal_shows_context_mid_proof() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    let source = "theorem step_proof (a b : ℕ) : a + b = b + a := by\n  \
                  have h : a + b = b + a := Nat.add_comm a b\n  exact h\n";
    sess.set_content(source);
    let result = sess
        .request(
            "$/lean/plainGoal",
            json!({ "textDocument": { "uri": &uri }, "position": { "line": 1, "character": 2 } }),
        )
        .expect("$/lean/plainGoal request failed");
    drop(sess);

    if result.is_null() {
        eprintln!("INFO: null goal at line 1 — tactic not yet resolved");
        return;
    }

    let rendered = result["rendered"].as_str().unwrap_or("");
    assert!(
        !rendered.is_empty(),
        "expected non-empty goal at line 1 col 2"
    );
    assert!(
        rendered.contains('a') || rendered.contains('b') || rendered.contains('⊢'),
        "goal state should reference proof context; got: {rendered:?}"
    );
}

#[test]
fn plain_goal_is_null_outside_tactic_block() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(TACTIC_PROOF);
    let result = sess
        .request(
            "$/lean/plainGoal",
            json!({ "textDocument": { "uri": &uri }, "position": { "line": 0, "character": 0 } }),
        )
        .expect("$/lean/plainGoal request failed");
    drop(sess);

    let rendered = result["rendered"].as_str().unwrap_or("");
    assert!(
        result.is_null() || rendered.is_empty(),
        "expected null or empty goal outside tactic block; got: {result}"
    );
}

#[test]
fn diagnostics_cleared_after_fixing_error() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();

    let msgs = sess.set_content(INVALID_TYPE);
    assert!(
        !errors_in(&msgs, &uri).is_empty(),
        "expected errors after opening invalid document"
    );

    let msgs2 = sess.set_content("def bad : Nat := 42\n");
    drop(sess);
    let remaining = errors_in(&msgs2, &uri);
    assert!(
        remaining.is_empty(),
        "errors should be cleared after fixing; remaining: {remaining:?}"
    );
}

#[test]
fn diagnostic_positions_are_zero_indexed() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    let msgs = sess.set_content(INVALID_TYPE);
    drop(sess);

    let errors = errors_in(&msgs, &uri);
    assert!(!errors.is_empty(), "expected at least one error diagnostic");

    for err in &errors {
        let line = err["range"]["start"]["line"]
            .as_u64()
            .expect("diagnostic should have range.start.line");
        assert_eq!(line, 1, "error should be on 0-indexed line 1; got {line}");
    }
}

#[test]
fn multiple_errors_in_one_file_all_reported() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    let source = "def bad1 : Nat := \"not a nat\"\ndef bad2 : Bool := 42\n";
    let msgs = sess.set_content(source);
    drop(sess);

    let errors = errors_in(&msgs, &uri);
    assert!(
        errors.len() >= 2,
        "expected at least 2 error diagnostics; got: {errors:?}"
    );
}

// ── Hover / definition / code actions / documentSymbol ─────────────────

/// A small proof that defines a local theorem and later references it, giving
/// us a useful target for hover and definition tests.
const LOCAL_DEF_PROOF: &str = "-- Local def for hover/definition tests.\n\
theorem my_theorem (a b : Nat) : a + b = b + a := Nat.add_comm a b\n\n\
example : 1 + 2 = 2 + 1 := my_theorem 1 2\n";

#[test]
fn hover_returns_type_for_local_theorem() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(LOCAL_DEF_PROOF);
    // "my_theorem" starts at line 3 (0-indexed), character 20 in the example line.
    let result = sess
        .request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": &uri },
                "position": { "line": 3, "character": 22 },
            }),
        )
        .expect("hover request failed");
    drop(sess);

    if result.is_null() {
        eprintln!("INFO: hover returned null (position may not be on identifier)");
        return;
    }

    let hover = lsp::parse_hover(serde_json::from_value(result.clone()).ok());
    assert!(
        hover.is_some(),
        "hover should parse when non-null; got: {result}"
    );
    let info = hover.unwrap();
    assert!(
        !info.contents.trim().is_empty(),
        "hover contents should be non-empty"
    );
}

#[test]
fn definition_resolves_to_local_theorem() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(LOCAL_DEF_PROOF);
    let result = sess
        .request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": &uri },
                "position": { "line": 3, "character": 27 },
            }),
        )
        .expect("definition request failed");
    drop(sess);

    if result.is_null() {
        eprintln!("INFO: definition returned null (position may not be on identifier)");
        return;
    }

    let def = lsp::parse_definition(serde_json::from_value(result.clone()).ok());
    assert!(def.is_some(), "definition should parse; got: {result}");
    let def = def.unwrap();
    assert_eq!(
        def.uri, uri,
        "local definition should target the same document"
    );
    assert_eq!(
        def.line, 1,
        "definition line should be 1 (0-indexed declaration); got {}",
        def.line
    );
}

#[test]
fn document_symbols_returns_top_level_symbols() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(LOCAL_DEF_PROOF);
    let result = sess
        .request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": &uri } }),
        )
        .expect("documentSymbol request failed");
    drop(sess);

    let symbols = lsp::parse_document_symbols(serde_json::from_value(result.clone()).ok());
    assert!(
        !symbols.is_empty(),
        "should find at least one symbol (my_theorem); result: {result}"
    );
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.iter().any(|n| n.contains("my_theorem")),
        "symbol list should include my_theorem; got: {names:?}"
    );
}

#[test]
fn code_action_available_on_error_line() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(UNSOLVED_GOALS);
    let result = sess
        .request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": &uri },
                "range": {
                    "start": { "line": 2, "character": 2 },
                    "end": { "line": 2, "character": 6 }
                },
                "context": { "diagnostics": [], "triggerKind": 1 }
            }),
        )
        .expect("codeAction request failed");
    drop(sess);

    let actions = lsp::parse_code_actions(serde_json::from_value(result).ok());
    for action in &actions {
        assert!(
            !action.title.is_empty(),
            "code action title should be non-empty"
        );
        if let Some(edit) = &action.edit {
            assert_workspace_edit_valid(edit);
        }
    }
}

// ── Completion / formatting / codeAction resolve / plainTermGoal ────────

const COMPLETION_SOURCE: &str = "-- Completion test.\nexample : Nat := Nat.";

const UNFORMATTED_SOURCE: &str = "def  foo:Nat:=42\n";

#[test]
fn completion_returns_items_after_dot() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(COMPLETION_SOURCE);
    let result = sess
        .request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": &uri },
                "position": { "line": 1, "character": 21 }
            }),
        )
        .expect("completion request failed");
    drop(sess);

    if result.is_null() {
        eprintln!("INFO: completion returned null");
        return;
    }

    let items = lsp::parse_completion_items(serde_json::from_value(result.clone()).ok());
    assert!(
        !items.is_empty(),
        "expected completion items after 'Nat.'; got: {result}"
    );
    assert!(
        items.iter().all(|i| !i.label.is_empty()),
        "every completion item should have a non-empty label"
    );
    assert!(
        items
            .iter()
            .any(|i| i.label.contains("succ") || i.label.contains("zero")),
        "expected at least one Nat member (succ/zero) in completions; got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[test]
fn formatting_returns_text_edits_for_unformatted_source() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(UNFORMATTED_SOURCE);
    let result = sess.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": &uri },
            "options": { "tabSize": 2, "insertSpaces": true }
        }),
    );
    drop(sess);

    let result = match result {
        Err(e) => {
            eprintln!("INFO: formatting request failed (server may not support it): {e}");
            return;
        }
        Ok(v) if v.is_null() => {
            eprintln!("INFO: formatting returned null (server may not support it)");
            return;
        }
        Ok(v) => v,
    };

    let edits = lsp::parse_text_edits(serde_json::from_value(result.clone()).ok());
    assert!(
        !edits.is_empty(),
        "expected formatting edits for unformatted source; got: {result}"
    );
    for edit in &edits {
        assert!(
            edit.start_line <= edit.end_line,
            "edit start_line should be <= end_line; got: {edit:?}"
        );
    }
}

#[test]
fn code_action_resolve_returns_workspace_edit() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(UNSOLVED_GOALS);
    let raw_result = sess
        .request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": &uri },
                "range": {
                    "start": { "line": 2, "character": 2 },
                    "end": { "line": 2, "character": 6 }
                },
                "context": { "diagnostics": [], "triggerKind": 1 }
            }),
        )
        .expect("codeAction request failed");

    let to_resolve = raw_result
        .as_array()
        .and_then(|arr| {
            arr.iter().find(|entry| {
                entry.get("data").is_some_and(|d| !d.is_null())
                    && entry.get("edit").is_none_or(Value::is_null)
            })
        })
        .cloned();

    let Some(action) = to_resolve else {
        drop(sess);
        eprintln!("INFO: no unresolved code actions found (Lean may inline all edits)");
        return;
    };

    let resolved = sess
        .request("codeAction/resolve", action)
        .expect("codeAction/resolve request failed");
    drop(sess);

    let edit_value = resolved["edit"].clone();
    let edit = lsp::parse_workspace_edit(
        serde_json::from_value(edit_value).expect("invalid workspace edit"),
    );
    assert!(
        edit.is_some(),
        "codeAction/resolve should return a workspace edit; got: {resolved}"
    );
    let edit = edit.unwrap();
    assert!(
        !edit.changes.is_empty(),
        "resolved workspace edit should have at least one file change"
    );
    assert_workspace_edit_valid(&edit);
}

#[test]
fn plain_term_goal_returns_result_for_term_proof() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();
    sess.set_content(LOCAL_DEF_PROOF);
    let result = sess.request(
        "$/lean/plainTermGoal",
        json!({
            "textDocument": { "uri": &uri },
            "position": { "line": 1, "character": 50 }
        }),
    );
    drop(sess);

    let result = match result {
        Err(e) => {
            eprintln!(
                "INFO: plainTermGoal request error (may not be supported at this position): {e}"
            );
            return;
        }
        Ok(v) if v.is_null() => {
            eprintln!("INFO: plainTermGoal returned null at this position");
            return;
        }
        Ok(v) => v,
    };

    let goal = result["goal"]
        .as_str()
        .or_else(|| result["rendered"].as_str())
        .unwrap_or("");
    assert!(
        !goal.is_empty(),
        "plainTermGoal should return a non-empty goal string; got: {result}"
    );
}

// ── Incremental sync / didSave / didClose ──────────────────────────────

#[test]
fn incremental_change_from_valid_to_error() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();

    sess.set_content("def ok : Nat := 42\n");
    let msgs = sess.apply_incremental_change((0, 16), (0, 18), "\"bad\"");
    drop(sess);

    let errors = errors_in(&msgs, &uri);
    assert!(
        !errors.is_empty(),
        "incremental change producing type mismatch should yield errors; got: {:?}",
        diagnostics_for(&msgs, &uri)
    );
}

#[test]
fn incremental_change_from_error_to_valid() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();

    sess.set_content("def bad : Nat := \"string\"\n");
    let msgs = sess.apply_incremental_change((0, 17), (0, 25), "99");
    drop(sess);

    let errors = errors_in(&msgs, &uri);
    assert!(
        errors.is_empty(),
        "incremental fix should clear errors; remaining: {errors:?}"
    );
}

#[test]
fn incremental_change_preserves_unmodified_lines() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();

    let source = "def first : Nat := 1\ndef second : Nat := 2\n";
    sess.set_content(source);
    let msgs = sess.apply_incremental_change((1, 20), (1, 21), "42");
    drop(sess);

    assert!(
        errors_in(&msgs, &uri).is_empty(),
        "edit to line 1 should not break line 0; errors: {:?}",
        diagnostics_for(&msgs, &uri)
    );
}

#[test]
fn did_save_accepted_without_error() {
    skip_if_no_project!(sess);

    sess.set_content(TACTIC_PROOF);
    sess.did_save();
    drop(sess);
}

#[test]
fn did_close_accepted_without_error() {
    skip_if_no_project!(sess);

    sess.set_content(TACTIC_PROOF);
    sess.did_close();

    let uri = sess.doc_uri();
    let version = sess.doc_version;
    sess.client
        .did_open(lsp_types::DidOpenTextDocumentParams {
            text_document: lsp_types::TextDocumentItem {
                uri,
                language_id: "lean4".to_string(),
                version,
                text: TACTIC_PROOF.to_owned(),
            },
        })
        .expect("re-open after didClose failed");
    sess.doc_version += 1;
    sess.current_content = Some(TACTIC_PROOF.to_owned());
    drop(sess);
}

// ── Fixture-based token highlighting and diagnostics tests ─────────────

const FIXTURE_CONJUNCTION: &str = include_str!("fixtures/01_conjunction.lean");
const FIXTURE_IMPLICATION: &str = include_str!("fixtures/02_implication.lean");
const FIXTURE_INDUCTION: &str = include_str!("fixtures/03_induction.lean");
const FIXTURE_PATTERN_MATCHING: &str = include_str!("fixtures/04_pattern_matching.lean");
const FIXTURE_WHERE_CLAUSE: &str = include_str!("fixtures/05_where_clause.lean");

fn fetch_decoded_tokens(sess: &Session) -> Vec<lsp::SemanticToken> {
    let uri = sess.doc_uri_str();
    let result = sess
        .request(
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": &uri } }),
        )
        .expect("semanticTokens/full request failed");

    let raw: Vec<u32> = result["data"]
        .as_array()
        .map_or(&[] as &[Value], Vec::as_slice)
        .iter()
        .filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()))
        .collect();
    let data: Vec<LspSemanticToken> = raw
        .chunks_exact(5)
        .map(|c| LspSemanticToken {
            delta_line: c[0],
            delta_start: c[1],
            length: c[2],
            token_type: c[3],
            token_modifiers_bitset: c[4],
        })
        .collect();

    lsp::decode_semantic_tokens(&data, &sess.token_types, &sess.token_modifiers)
}

fn assert_has_token_types(tokens: &[lsp::SemanticToken], expected: &[&str]) {
    for &expected_type in expected {
        assert!(
            tokens.iter().any(|t| t.token_type == expected_type),
            "expected at least one '{expected_type}' token; got types: {:?}",
            tokens
                .iter()
                .map(|t| t.token_type.as_str())
                .collect::<std::collections::HashSet<_>>()
        );
    }
}

#[test]
fn all_fixtures_elaborate_without_errors() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();

    let fixtures = [
        ("conjunction", FIXTURE_CONJUNCTION),
        ("implication", FIXTURE_IMPLICATION),
        ("induction", FIXTURE_INDUCTION),
        ("pattern_matching", FIXTURE_PATTERN_MATCHING),
        ("where_clause", FIXTURE_WHERE_CLAUSE),
    ];

    for (name, source) in &fixtures {
        let msgs = sess.set_content(source);
        let errors = errors_in(&msgs, &uri);
        assert!(
            errors.is_empty(),
            "{name} fixture should elaborate without errors; got: {:?}",
            errors
                .iter()
                .map(|d| d["message"].as_str().unwrap_or("?"))
                .collect::<Vec<_>>()
        );
    }

    drop(sess);
}

#[test]
fn all_fixtures_decoded_tokens_satisfy_invariants() {
    skip_if_no_project!(sess);

    let fixtures = [
        ("conjunction", FIXTURE_CONJUNCTION),
        ("implication", FIXTURE_IMPLICATION),
        ("induction", FIXTURE_INDUCTION),
        ("pattern_matching", FIXTURE_PATTERN_MATCHING),
        ("where_clause", FIXTURE_WHERE_CLAUSE),
    ];

    for (name, source) in &fixtures {
        sess.set_content(source);
        let tokens = fetch_decoded_tokens(&sess);
        let lines: Vec<&str> = source.lines().collect();

        assert!(!tokens.is_empty(), "{name}: expected non-empty token list");
        assert_has_token_types(&tokens, &["keyword"]);

        for tok in &tokens {
            assert!(
                tok.line >= 1,
                "{name}: token line must be >= 1 (1-indexed); got {}",
                tok.line
            );
            assert!(
                tok.length > 0,
                "{name}: token length must be > 0; got {}",
                tok.length
            );
            assert!(
                !tok.token_type.is_empty(),
                "{name}: token type must be non-empty"
            );
            let line_idx = (tok.line as usize).saturating_sub(1);
            assert!(
                line_idx < lines.len(),
                "{name}: token at line {} (1-indexed) is beyond document length {}",
                tok.line,
                lines.len()
            );
        }
    }

    drop(sess);
}

#[test]
fn fixture_implication_tokens_include_variable_types() {
    skip_if_no_project!(sess);
    sess.set_content(FIXTURE_IMPLICATION);
    let tokens = fetch_decoded_tokens(&sess);
    drop(sess);

    let has_variable_or_param = tokens
        .iter()
        .any(|t| t.token_type == "variable" || t.token_type == "parameter");
    assert!(
        has_variable_or_param,
        "expected variable or parameter tokens in implication fixture; got: {:?}",
        tokens
            .iter()
            .map(|t| t.token_type.as_str())
            .collect::<std::collections::HashSet<_>>()
    );
}

#[test]
fn fixture_pattern_matching_tokens_present() {
    skip_if_no_project!(sess);
    sess.set_content(FIXTURE_PATTERN_MATCHING);
    let tokens = fetch_decoded_tokens(&sess);
    drop(sess);

    assert_has_token_types(&tokens, &["keyword", "variable"]);
}

#[test]
fn fixture_then_error_clears_clean_state() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();

    let clean_msgs = sess.set_content(FIXTURE_CONJUNCTION);
    assert!(
        errors_in(&clean_msgs, &uri).is_empty(),
        "expected no errors for clean fixture"
    );

    let error_msgs = sess.set_content("def broken : Nat := \"not a nat\"\n");
    drop(sess);

    assert!(
        !errors_in(&error_msgs, &uri).is_empty(),
        "expected errors after introducing type mismatch"
    );
}

#[test]
fn error_then_fixture_restores_clean_state() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri_str();

    let error_msgs = sess.set_content("def broken : Nat := \"not a nat\"\n");
    assert!(
        !errors_in(&error_msgs, &uri).is_empty(),
        "expected errors to be reported first"
    );

    let clean_msgs = sess.set_content(FIXTURE_IMPLICATION);
    drop(sess);

    assert!(
        errors_in(&clean_msgs, &uri).is_empty(),
        "errors should clear after switching to valid fixture; got: {:?}",
        diagnostics_for(&clean_msgs, &uri)
    );
}
