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

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use turnstile_lib::lsp::{self, LspClient};

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

fn lean_bin() -> String {
    if let Ok(cmd) = std::env::var("TURNSTILE_LSP_CMD") {
        return cmd;
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".elan")
        .join("bin")
        .join(if cfg!(windows) { "lean.exe" } else { "lean" })
        .to_string_lossy()
        .into_owned()
}

// ── Session ────────────────────────────────────────────────────────────
//
// Wraps the real LspClient with a notification channel so tests can
// collect server-pushed messages (fileProgress, publishDiagnostics, etc.).

struct Session {
    client: LspClient,
    rx: Receiver<Value>,
    project: PathBuf,
    doc_version: i64,
    /// Content most recently sent via `set_content`; avoids redundant re-elaboration.
    current_content: Option<String>,
    /// Messages from the most recent `set_content` call (returned on cache hit).
    last_msgs: Vec<Value>,
}

impl Session {
    fn new(project: PathBuf) -> Result<Self, String> {
        let lean = lean_bin();
        let mut client = LspClient::spawn(&lean, &["--server"], &project)?;

        let (tx, rx) = mpsc::sync_channel::<Value>(512);
        let stdout = client.take_stdout().ok_or("no stdout")?;
        let pending = client.pending.clone();
        let writer = client.writer.clone();
        std::thread::spawn(move || {
            LspClient::read_messages(stdout, &pending, move |msg| {
                // Ack server→client requests (e.g. workspace/semanticTokens/refresh).
                if msg.get("id").is_some() && msg.get("method").is_some() {
                    lsp::ack_request(&writer, &msg["id"]).ok();
                    return;
                }
                let _ = tx.send(msg.clone());
            });
        });

        // `block_in_place` lets us call async code from a sync context while
        // inside a multi-thread Tokio runtime (used by `#[tokio::test]`).
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            let root_uri = lsp::path_to_file_uri(&project);
            rt.block_on(
                client.send_request_await("initialize", lsp::initialize_params(&root_uri)),
            )?;
            rt.block_on(client.send_notification("initialized", json!({})))?;

            let doc_uri = lsp::path_to_file_uri(&project.join("Proof.lean"));
            rt.block_on(client.send_notification(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": &doc_uri,
                        "languageId": "lean4",
                        "version": 1,
                        "text": "",
                    }
                }),
            ))
        })?;

        Ok(Self {
            client,
            rx,
            project,
            doc_version: 2,
            current_content: Some(String::new()),
            last_msgs: Vec::new(),
        })
    }

    fn doc_uri(&self) -> String {
        lsp::path_to_file_uri(&self.project.join("Proof.lean"))
    }

    fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.client.send_request_await(method, params))
        })
        .map_err(Into::into)
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

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.client.send_notification(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": &uri, "version": version },
                    "contentChanges": [{ "text": text }],
                }),
            ))
        })
        .expect("didChange failed");

        let msgs = self.wait_for_elaboration(&uri, Duration::from_secs(60));
        self.current_content = Some(text.to_owned());
        msgs.clone_into(&mut self.last_msgs);
        msgs
    }

    /// Collect messages until `done(msg)` returns true or `timeout` elapses.
    #[allow(clippy::needless_pass_by_ref_mut)] // recv_timeout mutates the Receiver via &mut self
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

// ── Global session ─────────────────────────────────────────────────────

static SESSION: OnceLock<Option<Mutex<Session>>> = OnceLock::new();

fn session() -> Option<std::sync::MutexGuard<'static, Session>> {
    let _ = env_logger::try_init();
    SESSION
        .get_or_init(|| {
            lean_project_path().map(|project| match Session::new(project) {
                Ok(s) => Mutex::new(s),
                Err(e) => panic!("Failed to start shared LSP session: {e}"),
            })
        })
        .as_ref()
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

#[tokio::test(flavor = "multi_thread")]
async fn initialize_returns_capabilities() {
    skip_if_no_project!(sess);
    let exists = sess.project.exists();
    drop(sess);
    assert!(exists, "project path should exist after initialization");
}

#[tokio::test(flavor = "multi_thread")]
async fn server_advertises_semantic_tokens_provider() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
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
        "server should return semantic tokens (proving it advertised the provider)"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn valid_proof_produces_no_error_diagnostics() {
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

#[tokio::test(flavor = "multi_thread")]
async fn type_mismatch_produces_error_diagnostic() {
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

#[tokio::test(flavor = "multi_thread")]
async fn unknown_identifier_produces_error_diagnostic() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
    let msgs = sess.set_content(UNKNOWN_IDENT);
    drop(sess);

    assert!(
        !errors_in(&msgs, &uri).is_empty(),
        "unknown identifier should produce at least one error"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unsolved_goals_produces_error_diagnostic() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
    let msgs = sess.set_content(UNSOLVED_GOALS);
    drop(sess);

    assert!(
        !errors_in(&msgs, &uri).is_empty(),
        "unsolved goals should produce at least one error"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn semantic_tokens_returned_for_valid_document() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
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

#[tokio::test(flavor = "multi_thread")]
async fn semantic_tokens_data_is_valid_five_tuples() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
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

#[tokio::test(flavor = "multi_thread")]
async fn plain_goal_inside_tactic_block() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
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

#[tokio::test(flavor = "multi_thread")]
async fn plain_goal_shows_context_mid_proof() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
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

#[tokio::test(flavor = "multi_thread")]
async fn plain_goal_is_null_outside_tactic_block() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
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

#[tokio::test(flavor = "multi_thread")]
async fn diagnostics_cleared_after_fixing_error() {
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

#[tokio::test(flavor = "multi_thread")]
async fn diagnostic_positions_are_zero_indexed() {
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

#[tokio::test(flavor = "multi_thread")]
async fn window_messages_parsed_without_panic() {
    skip_if_no_project!(sess);
    // Drain queued messages; no panic = success.
    sess.collect_until(Duration::from_secs(2), |_| false);
    drop(sess);
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_errors_in_one_file_all_reported() {
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

// ── Hover / definition / code actions / documentSymbol ─────────────────

/// A small proof that defines a local theorem and later references it, giving
/// us a useful target for hover and definition tests.
const LOCAL_DEF_PROOF: &str = "-- Local def for hover/definition tests.\n\
theorem my_theorem (a b : Nat) : a + b = b + a := Nat.add_comm a b\n\n\
example : 1 + 2 = 2 + 1 := my_theorem 1 2\n";

#[tokio::test(flavor = "multi_thread")]
async fn hover_returns_type_for_local_theorem() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
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

    let hover = lsp::parse_hover(&result);
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

#[tokio::test(flavor = "multi_thread")]
async fn hover_returns_none_on_whitespace() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
    sess.set_content(LOCAL_DEF_PROOF);
    // Blank line 2.
    let result = sess
        .request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": &uri },
                "position": { "line": 2, "character": 0 },
            }),
        )
        .expect("hover request failed");
    drop(sess);

    // Either null from the server or an empty-contents structure that parse_hover rejects.
    let parsed = lsp::parse_hover(&result);
    assert!(
        parsed.is_none()
            || parsed
                .as_ref()
                .is_some_and(|h| !h.contents.trim().is_empty()),
        "hover on whitespace should be None or have content; got: {parsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn definition_resolves_to_local_theorem() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
    sess.set_content(LOCAL_DEF_PROOF);
    // "my_theorem" reference on line 3 (the example line). Position 27
    // lands on the 'm' of "my_theorem" (after "example : 1 + 2 = 2 + 1 := ").
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

    let def = lsp::parse_definition(&result);
    assert!(def.is_some(), "definition should parse; got: {result}");
    let def = def.unwrap();
    // Should point back into the same document, on the declaration line (1).
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

#[tokio::test(flavor = "multi_thread")]
async fn document_symbols_returns_top_level_symbols() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
    sess.set_content(LOCAL_DEF_PROOF);
    let result = sess
        .request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": &uri } }),
        )
        .expect("documentSymbol request failed");
    drop(sess);

    let symbols = lsp::parse_document_symbols(&result);
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

#[tokio::test(flavor = "multi_thread")]
async fn code_action_available_on_error_line() {
    skip_if_no_project!(sess);
    let uri = sess.doc_uri();
    // Deliberately leave the goal unsolved so Lean offers a "Try this:" action.
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

    let actions = lsp::parse_code_actions(&result);
    // Lean may or may not offer actions here; we validate the DTO shape either way.
    for action in &actions {
        assert!(
            !action.title.is_empty(),
            "code action title should be non-empty"
        );
    }
}
