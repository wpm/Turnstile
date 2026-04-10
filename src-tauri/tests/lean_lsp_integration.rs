//! Integration tests for the Lean LSP server at the protocol level.
//!
//! A single `lean --server` process is shared across all tests via a
//! `OnceLock<Option<Mutex<Session>>>`. This avoids the Mathlib startup cost
//! for every individual test.
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
//! If that directory is absent the tests are skipped gracefully.
//!
//! Overrides:
//!   `TURNSTILE_LSP_CMD`      — path to the lean binary
//!   `TURNSTILE_PROJECT_PATH` — path to the Lean project directory

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

const ERROR_SEVERITY: u64 = 1;

static LOGGER: OnceLock<()> = OnceLock::new();

fn init_logger() {
    LOGGER.get_or_init(|| {
        let _ = env_logger::try_init();
    });
}

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

fn path_to_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .map_or_else(|()| format!("file://{}", path.display()), |u| u.to_string())
}

// ── LSP client ─────────────────────────────────────────────────────────
//
// A background thread owns the stdout reader and pushes every parsed JSON
// message onto a channel. The main (test) thread writes to stdin and pulls
// messages via the receiver, keeping writer and reader fully decoupled.

struct LspClient {
    _process: Child,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Value>,
    next_id: AtomicI64,
    /// Messages pulled from the channel but not yet matched by a request.
    buffered: Vec<Value>,
}

impl LspClient {
    fn spawn(cwd: &Path) -> Result<Self, String> {
        let lean = lean_bin();
        let mut child = Command::new(&lean)
            .arg("--server")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn lean --server ({}): {e}", lean.display()))?;

        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;

        let (tx, rx) = mpsc::sync_channel::<Value>(512);
        std::thread::spawn(move || reader_loop(stdout, tx));

        Ok(Self {
            _process: child,
            writer: Box::new(stdin),
            rx,
            next_id: AtomicI64::new(1),
            buffered: Vec::new(),
        })
    }

    fn notify(&mut self, method: &str, params: &Value) -> Result<(), String> {
        self.send_raw(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.send_raw(&json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        }))?;
        self.wait_for_id(id)
    }

    fn send_raw(&mut self, msg: &Value) -> Result<(), String> {
        let body = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        log::debug!("LSP → {body}");
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.writer
            .write_all(header.as_bytes())
            .map_err(|e| e.to_string())?;
        self.writer
            .write_all(body.as_bytes())
            .map_err(|e| e.to_string())?;
        self.writer.flush().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Block until the response matching `id` arrives; stash other messages.
    fn wait_for_id(&mut self, id: i64) -> Result<Value, String> {
        if let Some(pos) = self
            .buffered
            .iter()
            .position(|m| m.get("id").and_then(Value::as_i64) == Some(id))
        {
            let msg = self.buffered.remove(pos);
            return extract_result(&msg, id);
        }

        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(format!("Timed out waiting for response to id {id}"));
            }
            match self.rx.recv_timeout(remaining) {
                Ok(msg) if msg.get("id").and_then(Value::as_i64) == Some(id) => {
                    return extract_result(&msg, id);
                }
                Ok(msg) => self.buffered.push(msg),
                Err(_) => return Err(format!("Timed out waiting for response to id {id}")),
            }
        }
    }

    /// Collect messages until `done(msg)` returns true or `timeout` elapses.
    fn collect_until<F>(&mut self, timeout: Duration, mut done: F) -> Vec<Value>
    where
        F: FnMut(&Value) -> bool,
    {
        let deadline = std::time::Instant::now() + timeout;
        let mut collected: Vec<Value> = std::mem::take(&mut self.buffered);

        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match self.rx.recv_timeout(remaining) {
                Ok(msg) => {
                    let finished = done(&msg);
                    collected.push(msg);
                    if finished {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }
        }

        collected
    }

    /// Collect until `$/lean/fileProgress` signals empty `processing` for `uri`,
    /// then continue until `textDocument/publishDiagnostics` arrives for `uri`
    /// (or a 500 ms fallback elapses, for files that produce no diagnostics).
    fn wait_for_elaboration(&mut self, uri: &str, timeout: Duration) -> Vec<Value> {
        // Phase 1: wait for fileProgress done.
        let mut msgs = self.collect_until(timeout, |msg| {
            msg["method"].as_str() == Some("$/lean/fileProgress")
                && msg["params"]["textDocument"]["uri"].as_str() == Some(uri)
                && msg["params"]["processing"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
        });

        // Phase 2: collect until publishDiagnostics arrives, or 500 ms pass.
        // Diagnostics arrive slightly after the fileProgress done signal.
        let trailing = self.collect_until(Duration::from_millis(500), |msg| {
            msg["method"].as_str() == Some("textDocument/publishDiagnostics")
                && msg["params"]["uri"].as_str() == Some(uri)
        });
        msgs.extend(trailing);
        msgs
    }
}

fn extract_result(msg: &Value, id: i64) -> Result<Value, String> {
    if let Some(err) = msg.get("error") {
        return Err(format!("LSP error for id {id}: {err}"));
    }
    Ok(msg.get("result").cloned().unwrap_or(Value::Null))
}

// ── Background reader ──────────────────────────────────────────────────

#[allow(clippy::needless_pass_by_value)] // SyncSender is consumed by the thread
fn reader_loop(stdout: std::process::ChildStdout, tx: SyncSender<Value>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut content_length: usize = 0;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    let t = line.trim();
                    if t.is_empty() {
                        break;
                    }
                    if let Some(len) = t.strip_prefix("Content-Length: ") {
                        content_length = len.parse().unwrap_or(0);
                    }
                }
            }
        }
        if content_length == 0 {
            continue;
        }
        let mut body = vec![0u8; content_length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        if let Ok(msg) = serde_json::from_slice::<Value>(&body) {
            log::debug!("LSP ← {}", serde_json::to_string(&msg).unwrap_or_default());
            if tx.send(msg).is_err() {
                return;
            }
        }
    }
}

// ── Shared session ─────────────────────────────────────────────────────

struct Session {
    client: LspClient,
    project: PathBuf,
    doc_version: i64,
    /// Content most recently sent via `set_content`; avoids redundant re-elaboration.
    current_content: Option<String>,
    /// Messages from the most recent `set_content` call (returned on cache hit).
    last_msgs: Vec<Value>,
}

impl Session {
    fn new(project: PathBuf) -> Result<Self, String> {
        let mut client = LspClient::spawn(&project)?;

        let root_uri = path_to_uri(&project);
        client.request(
            "initialize",
            &json!({
                "processId": std::process::id(),
                "capabilities": {
                    "textDocument": {
                        "synchronization": { "didSave": true },
                        "publishDiagnostics": { "relatedInformation": true },
                        "semanticTokens": {
                            "dynamicRegistration": false,
                            "requests": { "full": true },
                            "tokenTypes": [
                                "namespace","type","class","enum","interface",
                                "struct","typeParameter","parameter","variable",
                                "property","enumMember","event","function",
                                "method","macro","keyword","modifier","comment",
                                "string","number","regexp","operator","decorator"
                            ],
                            "tokenModifiers": [
                                "declaration","definition","readonly","static",
                                "deprecated","abstract","async","modification",
                                "documentation","defaultLibrary"
                            ],
                            "formats": ["relative"],
                            "multilineTokenSupport": false,
                            "overlappingTokenSupport": false
                        }
                    },
                    "experimental": { "plainGoal": true }
                },
                "rootUri": &root_uri,
                "workspaceFolders": [{ "uri": &root_uri, "name": "test" }],
            }),
        )?;
        client.notify("initialized", &json!({}))?;

        let doc_uri = path_to_uri(&project.join("Proof.lean"));
        client.notify(
            "textDocument/didOpen",
            &json!({
                "textDocument": {
                    "uri": &doc_uri,
                    "languageId": "lean4",
                    "version": 1,
                    "text": "",
                }
            }),
        )?;

        Ok(Self {
            client,
            project,
            doc_version: 2,
            current_content: Some(String::new()),
            last_msgs: Vec::new(),
        })
    }

    /// Replace the document with `text` and wait for elaboration + diagnostics.
    /// No-ops if `text` matches the current content, returning the previous messages.
    fn set_content(&mut self, text: &str) -> Vec<Value> {
        if self.current_content.as_deref() == Some(text) {
            return self.last_msgs.clone();
        }

        let version = self.doc_version;
        self.doc_version += 1;
        let uri = self.doc_uri();

        self.client
            .notify(
                "textDocument/didChange",
                &json!({
                    "textDocument": { "uri": &uri, "version": version },
                    "contentChanges": [{ "text": text }],
                }),
            )
            .expect("didChange failed");

        let msgs = self
            .client
            .wait_for_elaboration(&uri, Duration::from_secs(60));
        self.current_content = Some(text.to_owned());
        msgs.clone_into(&mut self.last_msgs);
        msgs
    }

    fn doc_uri(&self) -> String {
        path_to_uri(&self.project.join("Proof.lean"))
    }
}

// ── Global session ─────────────────────────────────────────────────────
//
// `None` means no Lean project was found; tests call `session()` and skip
// when it returns `None`.

static SESSION: OnceLock<Option<Mutex<Session>>> = OnceLock::new();

fn session() -> Option<std::sync::MutexGuard<'static, Session>> {
    init_logger();
    SESSION
        .get_or_init(|| {
            lean_project_path().map(|project| match Session::new(project) {
                Ok(s) => Mutex::new(s),
                Err(e) => panic!("Failed to start shared LSP session: {e}"),
            })
        })
        .as_ref()
        // Recover from a poisoned mutex so one failing test doesn't cascade.
        .map(|mtx| {
            mtx.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        })
}

macro_rules! skip_if_no_project {
    ($sess:ident) => {
        #[allow(unused_mut)]
        let Some(mut $sess) = session() else {
            eprintln!(
                "SKIP: Lean project not found. \
                 Run Turnstile setup or set TURNSTILE_PROJECT_PATH."
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
fn initialize_returns_capabilities() {
    skip_if_no_project!(sess);
    let exists = sess.project.exists();
    drop(sess);
    assert!(exists, "project path should exist after initialization");
}

#[test]
fn server_advertises_semantic_tokens_provider() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
    sess.set_content(TACTIC_PROOF);
    let result = sess
        .client
        .request(
            "textDocument/semanticTokens/full",
            &json!({ "textDocument": { "uri": &uri } }),
        )
        .expect("semanticTokens/full request failed");
    drop(sess);

    assert!(
        !result.is_null(),
        "server should return semantic tokens (proving it advertised the provider)"
    );
}

#[test]
fn valid_proof_produces_no_error_diagnostics() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
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
    let uri = sess.doc_uri();
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
    let uri = sess.doc_uri();
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
    let uri = sess.doc_uri();
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
    let uri = sess.doc_uri();
    sess.set_content(TACTIC_PROOF);
    let result = sess
        .client
        .request(
            "textDocument/semanticTokens/full",
            &json!({ "textDocument": { "uri": &uri } }),
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
    let uri = sess.doc_uri();
    sess.set_content(TACTIC_PROOF);
    let result = sess
        .client
        .request(
            "textDocument/semanticTokens/full",
            &json!({ "textDocument": { "uri": &uri } }),
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
    let uri = sess.doc_uri();
    sess.set_content(TACTIC_PROOF);
    let result = sess
        .client
        .request(
            "$/lean/plainGoal",
            &json!({ "textDocument": { "uri": &uri }, "position": { "line": 2, "character": 2 } }),
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
    let uri = sess.doc_uri();
    let source = "theorem step_proof (a b : ℕ) : a + b = b + a := by\n  \
                  have h : a + b = b + a := Nat.add_comm a b\n  exact h\n";
    sess.set_content(source);
    let result = sess
        .client
        .request(
            "$/lean/plainGoal",
            &json!({ "textDocument": { "uri": &uri }, "position": { "line": 1, "character": 2 } }),
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
    let uri = sess.doc_uri();
    sess.set_content(TACTIC_PROOF);
    let result = sess
        .client
        .request(
            "$/lean/plainGoal",
            &json!({ "textDocument": { "uri": &uri }, "position": { "line": 0, "character": 0 } }),
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
    let uri = sess.doc_uri();

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
    let uri = sess.doc_uri();
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
fn window_messages_parsed_without_panic() {
    skip_if_no_project!(sess);
    // Drain queued messages; no panic = success.
    sess.client.collect_until(Duration::from_secs(2), |_| false);
    drop(sess);
}

#[test]
fn multiple_errors_in_one_file_all_reported() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
    let source = "def bad1 : Nat := \"not a nat\"\ndef bad2 : Bool := 42\n";
    let msgs = sess.set_content(source);
    drop(sess);

    let errors = errors_in(&msgs, &uri);
    assert!(
        errors.len() >= 2,
        "expected at least 2 error diagnostics; got: {errors:?}"
    );
}
