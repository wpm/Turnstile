pub mod client;
pub mod messages;
pub mod protocol;
pub mod server;

#[cfg(test)]
mod tests {
    use crate::lean::client::Client;
    use crate::lean::messages::turnstile::{
        decode_semantic_tokens, SemanticToken, TurnstileMessage,
    };
    use lsp_types::{
        HoverParams, SemanticTokensParams, SemanticTokensResult, TextDocumentIdentifier,
        TextDocumentPositionParams,
    };

    trait ExtractText {
        fn extract_text<'a>(&self, source: &'a str) -> &'a str;
    }

    impl ExtractText for SemanticToken {
        fn extract_text<'a>(&self, source: &'a str) -> &'a str {
            use crate::lean::protocol::Protocol;
            let line = source.lines().nth(self.line as usize - 1).unwrap_or("");
            let start = Protocol::utf16_col_to_byte(line, self.col);
            let end = Protocol::utf16_col_to_byte(line, self.col + self.length);
            &line[start..end]
        }
    }
    use std::ops::Deref;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    async fn lean_client_with(content: &str) -> (Client, Arc<Mutex<Vec<TurnstileMessage>>>) {
        let messages = Arc::new(Mutex::new(Vec::new()));
        let messages_clone = Arc::clone(&messages);
        let client = Client::start(content, move |msg| {
            messages_clone.lock().unwrap().push(msg);
        })
        .await
        .expect("lean process start failed");
        (client, messages)
    }

    async fn wait_for_elaboration(messages: &Arc<Mutex<Vec<TurnstileMessage>>>) -> bool {
        wait_for(
            messages,
            |msgs| {
                msgs.iter()
                    .any(|m| matches!(m, TurnstileMessage::ElaborationDone))
            },
            Duration::from_mins(2),
        )
        .await
    }

    /// Poll until `predicate` returns true or `timeout` elapses.
    async fn wait_for(
        messages: &Arc<Mutex<Vec<TurnstileMessage>>>,
        predicate: impl Fn(&[TurnstileMessage]) -> bool + Send + Sync,
        timeout: Duration,
    ) -> bool {
        tokio::time::timeout(timeout, async {
            loop {
                if predicate(&messages.lock().unwrap()) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .is_ok()
    }

    /// RAII wrapper that calls `Client::stop` on drop, ensuring clean shutdown
    /// even when a test panics before reaching its explicit `stop()` call.
    struct ClientGuard(Option<Client>);

    impl ClientGuard {
        fn new(client: Client) -> Self {
            Self(Some(client))
        }

        async fn stop(mut self) {
            if let Some(client) = self.0.take() {
                client.stop().await.expect("stop failed");
            }
        }
    }

    impl Deref for ClientGuard {
        type Target = Client;
        fn deref(&self) -> &Client {
            self.0.as_ref().unwrap()
        }
    }

    impl Drop for ClientGuard {
        fn drop(&mut self) {
            if let Some(client) = self.0.take() {
                // Only reached when a test panics before calling stop() explicitly.
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.block_on(async { client.stop().await.ok() });
                }
            }
        }
    }

    /// Wait for `ElaborationDone` then return true if a `Diagnostics` message
    /// with at least one item whose message contains `needle` was received.
    async fn assert_has_diagnostic(
        messages: &Arc<Mutex<Vec<TurnstileMessage>>>,
        needle: &str,
    ) -> bool {
        assert!(
            wait_for_elaboration(messages).await,
            "timed out waiting for ElaborationDone"
        );
        messages.lock().unwrap().iter().any(|m| {
            if let TurnstileMessage::Diagnostics { items: diags } = m {
                diags.iter().any(|d| d.message.contains(needle))
            } else {
                false
            }
        })
    }

    fn assert_elaborated_cleanly(msgs: &[TurnstileMessage]) {
        let last_diagnostics = msgs
            .iter()
            .filter_map(|m| {
                if let TurnstileMessage::Diagnostics { items: d } = m {
                    Some(d)
                } else {
                    None
                }
            })
            .next_back();
        assert!(
            last_diagnostics.is_some(),
            "expected at least one Diagnostics message"
        );
        assert!(
            last_diagnostics.unwrap().is_empty(),
            "expected no diagnostics for valid proof, got: {:?}",
            last_diagnostics.unwrap()
        );
        assert!(
            msgs.iter()
                .any(|m| matches!(m, TurnstileMessage::FileProgress { .. })),
            "expected at least one FileProgress message"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn starts_and_stops_cleanly() {
        crate::init_tracing!();
        let (client, _messages) = lean_client_with("").await;
        let guard = ClientGuard::new(client);

        assert!(guard.init_result.capabilities.text_document_sync.is_some());

        guard.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_document_hover_returns_without_error() {
        crate::init_tracing!();
        let (client, _messages) = lean_client_with("").await;
        let uri = client.proof_uri.clone();
        let guard = ClientGuard::new(client);

        let result = guard
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: lsp_types::Position {
                        line: 0,
                        character: 0,
                    },
                },
                work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            })
            .await;

        assert!(result.is_ok(), "hover on empty document should not error");

        guard.stop().await;
    }

    async fn assert_tokens(source: &str, expected: &[(&str, &str)]) {
        let (client, messages) = lean_client_with(source).await;
        let guard = ClientGuard::new(client);
        assert!(
            wait_for_elaboration(&messages).await,
            "timed out waiting for ElaborationDone"
        );
        assert_elaborated_cleanly(&messages.lock().unwrap());

        let (type_legend, modifier_legend) = guard.token_legend();
        let uri = guard.proof_uri.clone();
        let result = guard
            .semantic_tokens_full(SemanticTokensParams {
                text_document: TextDocumentIdentifier { uri },
                work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
                partial_result_params: lsp_types::PartialResultParams::default(),
            })
            .await
            .expect("semantic_tokens_full failed")
            .expect("doc unchanged during test");
        let tokens = match result {
            SemanticTokensResult::Tokens(t) => {
                decode_semantic_tokens(&t.data, &type_legend, &modifier_legend)
            }
            other @ SemanticTokensResult::Partial(_) => {
                panic!("expected Tokens result, got {other:?}")
            }
        };
        let annotated: Vec<(&str, &str)> = tokens
            .iter()
            .map(|t| (t.extract_text(source), t.token_type.as_str()))
            .collect();
        assert_eq!(annotated, expected);
        guard.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn and_comm_tokens() {
        crate::init_tracing!();
        #[rustfmt::skip]
        assert_tokens(include_str!("fixtures/01_and_comm.lean"), &[
            ("theorem", "keyword"),
            ("p", "variable"), ("q", "variable"),          // type params
            ("h", "variable"),                              // hypothesis
            ("p", "variable"), ("q", "variable"),           // p ∧ q
            ("q", "variable"), ("p", "variable"),           // q ∧ p
            ("h", "variable"), ("2", "property"),           // h.2
            ("h", "variable"), ("1", "property"),           // h.1
        ]).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn and_assoc_tokens() {
        crate::init_tracing!();
        #[rustfmt::skip]
        assert_tokens(include_str!("fixtures/02_and_assoc.lean"), &[
            ("theorem", "keyword"),
            ("p",  "variable"), ("q",  "variable"), ("r",  "variable"), // type params
            ("h",  "variable"),                                          // hypothesis
            ("p",  "variable"), ("q",  "variable"), ("r",  "variable"), // p ∧ q ∧ r
            ("p",  "variable"), ("q",  "variable"), ("r",  "variable"), // (p ∧ q) ∧ r
            ("h",  "variable"), ("1",  "property"),                     // h.1
            ("h",  "variable"), ("2",  "property"), ("1",  "property"), // h.2.1
            ("h",  "variable"), ("2",  "property"), ("2",  "property"), // h.2.2
        ]).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn imp_trans_tokens() {
        crate::init_tracing!();
        #[rustfmt::skip]
        assert_tokens(include_str!("fixtures/03_imp_trans.lean"), &[
            ("theorem", "keyword"),
            ("p",   "variable"), ("q",   "variable"), ("r",   "variable"), // type params
            ("hpq", "variable"), ("p",   "variable"), ("q",   "variable"), // hpq : p → q
            ("hqr", "variable"), ("q",   "variable"), ("r",   "variable"), // hqr : q → r
            ("p",   "variable"), ("r",   "variable"),                       // : p → r
            ("by",  "keyword"),
            ("intro",  "keyword"), ("hp", "variable"),
            ("have",   "keyword"), ("hq", "variable"), ("q",  "variable"),
            ("hpq",    "variable"), ("hp", "variable"),
            ("exact",  "keyword"), ("hqr", "variable"), ("hq", "variable"),
        ]).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn imp_chain_tokens() {
        crate::init_tracing!();
        #[rustfmt::skip]
        assert_tokens(include_str!("fixtures/04_imp_chain.lean"), &[
            ("theorem", "keyword"),
            ("p",  "variable"), ("q",  "variable"), ("r",  "variable"), ("s",  "variable"), // type params
            ("h1", "variable"), ("p",  "variable"), ("q",  "variable"), // h1 : p → q
            ("h2", "variable"), ("q",  "variable"), ("r",  "variable"), // h2 : q → r
            ("h3", "variable"), ("r",  "variable"), ("s",  "variable"), // h3 : r → s
            ("p",  "variable"), ("s",  "variable"),                      // : p → s
            ("by", "keyword"),
            ("intro", "keyword"), ("hp", "variable"),
            ("apply", "keyword"), ("h3", "variable"),
            ("apply", "keyword"), ("h2", "variable"),
            ("exact", "keyword"), ("h1", "variable"), ("hp", "variable"),
        ]).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn zero_add_tokens() {
        crate::init_tracing!();
        #[rustfmt::skip]
        assert_tokens(include_str!("fixtures/05_zero_add.lean"), &[
            ("theorem",   "keyword"),
            ("n",         "variable"), // binder
            ("n",         "variable"), ("n", "variable"), // 0 + n = n
            ("by",        "keyword"),
            ("induction", "keyword"), ("n", "variable"), ("with", "keyword"),
            ("rfl",       "keyword"),                    // | zero
            ("n",         "variable"), ("ih", "variable"), // | succ n ih
            ("exact",     "keyword"), ("ih", "variable"),
        ]).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pred_succ_tokens() {
        crate::init_tracing!();
        #[rustfmt::skip]
        assert_tokens(include_str!("fixtures/06_pred_succ.lean"), &[
            ("theorem",   "keyword"),
            ("n",         "variable"), // binder
            ("n",         "variable"), ("n", "variable"), // Nat.pred (Nat.succ n) = n
            ("by",        "keyword"),
            ("induction", "keyword"), ("n", "variable"), ("with", "keyword"),
            ("rfl",       "keyword"),              // | zero
            ("n",         "variable"), ("_", "keyword"), // | succ n _
            ("rfl",       "keyword"),
        ]).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn not_not_tokens() {
        crate::init_tracing!();
        #[rustfmt::skip]
        assert_tokens(include_str!("fixtures/07_not_not.lean"), &[
            ("def",     "keyword"),                          // def myNot
            ("theorem", "keyword"),
            ("b",       "variable"),                         // binder
            ("b",       "variable"), ("b", "variable"),      // myNot (myNot b) = b
            ("by",      "keyword"),
            ("cases",   "keyword"), ("b", "variable"),
            ("rfl",     "keyword"),
        ]).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn not_eq_not_tokens() {
        crate::init_tracing!();
        #[rustfmt::skip]
        assert_tokens(include_str!("fixtures/08_not_eq_not.lean"), &[
            ("def",     "keyword"),                          // def myNot
            ("theorem", "keyword"),
            ("b",       "variable"),                         // binder
            ("b",       "variable"), ("b", "variable"),      // myNot b = !b
            ("by",      "keyword"),
            ("cases",   "keyword"), ("b", "variable"),
            ("rfl",     "keyword"),
        ]).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn map_id_tokens() {
        crate::init_tracing!();
        #[rustfmt::skip]
        assert_tokens(include_str!("fixtures/09_map_id.lean"), &[
            ("theorem", "keyword"),
            ("α",    "variable"), ("xs",  "variable"), ("α", "variable"), // {α : Type} (xs : List α)
            ("xs",   "variable"), ("map", "property"),                     // xs.map
            ("fun",  "keyword"),  ("x",   "variable"), ("x", "variable"),  // fun x => x
            ("xs",   "variable"),                                           // = xs
            ("xs",   "variable"),                                           // aux xs
            ("where", "keyword"),
            ("aux",  "function"),
            ("ys",   "variable"), ("α",   "variable"),                     // ∀ ys : List α
            ("ys",   "variable"), ("map", "property"),                     // ys.map
            ("fun",  "keyword"),  ("x",   "variable"), ("x", "variable"),  // fun x => x
            ("ys",   "variable"),                                           // = ys
            ("y",    "variable"), ("ys",  "variable"),                     // | y :: ys
            ("y",    "variable"), ("ys",  "variable"),                     // congrArg (y :: ·) (aux ys)
        ]).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn syntax_error_produces_diagnostic() {
        crate::init_tracing!();
        let source = "blorp";
        let (client, messages) = lean_client_with(source).await;
        let guard = ClientGuard::new(client);
        assert!(
            assert_has_diagnostic(&messages, "expected").await,
            "expected a diagnostic containing 'expected' for syntax error"
        );
        guard.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn type_error_produces_diagnostic() {
        crate::init_tracing!();
        let source = r#"def x : Nat := "hello""#;
        let (client, messages) = lean_client_with(source).await;
        let guard = ClientGuard::new(client);
        assert!(
            assert_has_diagnostic(&messages, "Type mismatch").await,
            "expected a diagnostic containing 'Type mismatch' for type error"
        );
        guard.stop().await;
    }

    // ── Integration tests for Protocol / replace_document ─────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn replace_document_bumps_version() {
        crate::init_tracing!();
        let (client, messages) = lean_client_with("").await;
        let guard = ClientGuard::new(client);

        let v1 = guard
            .replace_document("def x : Nat := 0".into())
            .expect("replace_document failed");
        assert_eq!(v1, 1);

        assert!(
            wait_for_elaboration(&messages).await,
            "timed out waiting for ElaborationDone after v1"
        );

        let v2 = guard
            .replace_document("def y : Nat := 1".into())
            .expect("replace_document failed");
        assert_eq!(v2, 2);

        let elaborated2 = wait_for(
            &messages,
            |msgs| {
                msgs.iter()
                    .filter(|m| matches!(m, TurnstileMessage::ElaborationDone))
                    .count()
                    >= 2
            },
            Duration::from_mins(2),
        )
        .await;
        assert!(
            elaborated2,
            "timed out waiting for ElaborationDone after v2"
        );

        guard.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn edit_then_fix_clears_diagnostics() {
        crate::init_tracing!();
        let source = r#"def x : Nat := "hello""#;
        let (client, messages) = lean_client_with(source).await;
        let guard = ClientGuard::new(client);

        assert!(
            assert_has_diagnostic(&messages, "Type mismatch").await,
            "expected type mismatch diagnostic on initial content"
        );

        guard
            .replace_document("def x : Nat := 0".into())
            .expect("replace_document failed");

        // Wait for an empty Diagnostics message to arrive (v1's publish).
        let cleared = wait_for(
            &messages,
            |msgs| {
                msgs.iter().any(|m| {
                    if let TurnstileMessage::Diagnostics { items: d } = m {
                        d.is_empty()
                    } else {
                        false
                    }
                })
            },
            Duration::from_mins(2),
        )
        .await;
        assert!(cleared, "timed out waiting for empty Diagnostics after fix");

        guard.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn edit_then_introduce_shows_diagnostic() {
        crate::init_tracing!();
        let (client, messages) = lean_client_with("def x : Nat := 0").await;
        let guard = ClientGuard::new(client);

        // Wait for first elaboration of the valid content.
        assert!(
            wait_for_elaboration(&messages).await,
            "timed out waiting for first ElaborationDone"
        );

        // Snapshot length so we can wait for a new ElaborationDone after the edit.
        let count_before = messages.lock().unwrap().len();

        guard
            .replace_document(r#"def x : Nat := "hello""#.into())
            .expect("replace_document failed");

        // Wait for the type mismatch diagnostic itself. Praxis note
        // (praxis/recordings/sqrt2-session.json): Lean can publish the final
        // diagnostics *after* the empty fileProgress that signals elaboration
        // done — observed ~1.5 s late in recorded traffic — so waiting for
        // ElaborationDone and then asserting on already-received diagnostics
        // is a race, not a guarantee.
        let found = wait_for(
            &messages,
            |msgs| {
                msgs[count_before..].iter().any(|m| {
                    if let TurnstileMessage::Diagnostics { items: diags } = m {
                        diags.iter().any(|d| d.message.contains("Type mismatch"))
                    } else {
                        false
                    }
                })
            },
            Duration::from_mins(2),
        )
        .await;
        assert!(
            found,
            "expected type mismatch diagnostic after introducing error"
        );

        guard.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stale_file_progress_smoke() {
        crate::init_tracing!();
        let (client, _messages) = lean_client_with("def x : Nat := 0").await;
        let guard = ClientGuard::new(client);

        for i in 0..10u32 {
            guard
                .replace_document(format!("def x : Nat := {i}"))
                .expect("replace_document failed");
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(guard.current_version(), 10);

        guard.stop().await;
    }

    // timing-sensitive: if hover responds before replace_document dispatches, result may not be stale
    #[tokio::test(flavor = "multi_thread")]
    async fn fresh_wrapper_end_to_end() {
        use std::sync::Arc;

        crate::init_tracing!();
        let long_source = include_str!("fixtures/09_map_id.lean");
        let (client, messages) = lean_client_with(long_source).await;

        assert!(
            wait_for_elaboration(&messages).await,
            "timed out waiting for ElaborationDone"
        );

        let client = Arc::new(client);
        let client2 = Arc::clone(&client);
        let uri = client.proof_uri.clone();

        let hover_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            client2
                .hover(HoverParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position: lsp_types::Position {
                            line: 0,
                            character: 0,
                        },
                    },
                    work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
                })
                .await
        });

        client
            .replace_document("def z : Nat := 99".into())
            .expect("replace_document failed");

        let result = hover_task.await.expect("hover task panicked");
        assert!(result.is_ok(), "hover should not error");

        let client = Arc::try_unwrap(client).unwrap_or_else(|_| panic!("arc still held"));
        client.stop().await.expect("stop failed");
    }
}
