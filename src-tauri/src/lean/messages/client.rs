//! Named LSP send methods on [`Client`].
//!
//! Each method is a thin, typed wrapper around [`Client::request`] or
//! [`Client::notify`], delegating to the appropriate `lsp_types` request or
//! notification marker type. Lifecycle concerns (spawn, shutdown, drop) live
//! in [`crate::lean::client`].

use async_lsp::Result;
use lsp_types::notification::{DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument};
use lsp_types::request::{
    CodeActionRequest, CodeActionResolveRequest, Completion, DocumentSymbolRequest, GotoDefinition,
    HoverRequest, SemanticTokensFullRequest,
};
use lsp_types::{
    CodeAction, CodeActionParams, CodeActionResponse, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse,
    Hover, HoverParams, SemanticTokensParams, SemanticTokensResult, TextDocumentPositionParams,
};

use super::extensions::{PlainGoal, PlainGoalMethod, PlainTermGoal, PlainTermGoalMethod};
use crate::lean::client::{Client, LspHandle};

// Notifications stay on `Client`: they are synchronous (`notify` returns
// immediately), so there is no `.await` to hold a lock across.
impl Client {
    pub fn did_open(&self, params: DidOpenTextDocumentParams) -> Result<()> {
        self.notify::<DidOpenTextDocument>(params)
    }

    pub fn did_close(&self, params: DidCloseTextDocumentParams) -> Result<()> {
        self.notify::<DidCloseTextDocument>(params)
    }

    pub fn did_change(&self, params: DidChangeTextDocumentParams) -> Result<()> {
        self.notify::<DidChangeTextDocument>(params)
    }
}

// Requests live on `LspHandle` so callers can drop the `Client` mutex guard
// before awaiting a server round-trip. `Client` keeps thin delegators (via
// `self.handle()`) so existing in-process callers and tests are unaffected;
// the deadlock-prone paths in lib.rs go through the handle directly.
macro_rules! request_methods {
    ($T:ty) => {
        impl $T {
            pub async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
                self.protocol
                    .fresh(self.request::<HoverRequest>(params))
                    .await
                    .map(Option::flatten)
            }

            pub async fn completion(
                &self,
                params: CompletionParams,
            ) -> Result<Option<CompletionResponse>> {
                self.protocol
                    .fresh(self.request::<Completion>(params))
                    .await
                    .map(Option::flatten)
            }

            pub async fn definition(
                &self,
                params: GotoDefinitionParams,
            ) -> Result<Option<GotoDefinitionResponse>> {
                self.protocol
                    .fresh(self.request::<GotoDefinition>(params))
                    .await
                    .map(Option::flatten)
            }

            pub async fn code_action(
                &self,
                params: CodeActionParams,
            ) -> Result<Option<CodeActionResponse>> {
                self.protocol
                    .fresh(self.request::<CodeActionRequest>(params))
                    .await
                    .map(Option::flatten)
            }

            pub async fn resolve_code_action(
                &self,
                params: CodeAction,
            ) -> Result<Option<CodeAction>> {
                self.protocol
                    .fresh(self.request::<CodeActionResolveRequest>(params))
                    .await
            }

            pub async fn document_symbol(
                &self,
                params: DocumentSymbolParams,
            ) -> Result<Option<DocumentSymbolResponse>> {
                self.protocol
                    .fresh(self.request::<DocumentSymbolRequest>(params))
                    .await
                    .map(Option::flatten)
            }

            pub async fn semantic_tokens_full(
                &self,
                params: SemanticTokensParams,
            ) -> Result<Option<SemanticTokensResult>> {
                self.protocol
                    .fresh(self.request::<SemanticTokensFullRequest>(params))
                    .await
                    .map(Option::flatten)
            }

            pub async fn plain_goal(
                &self,
                params: TextDocumentPositionParams,
            ) -> Result<Option<PlainGoal>> {
                self.protocol
                    .fresh(self.request::<PlainGoalMethod>(params))
                    .await
                    .map(Option::flatten)
            }

            pub async fn plain_term_goal(
                &self,
                params: TextDocumentPositionParams,
            ) -> Result<Option<PlainTermGoal>> {
                self.protocol
                    .fresh(self.request::<PlainTermGoalMethod>(params))
                    .await
                    .map(Option::flatten)
            }
        }
    };
}

request_methods!(Client);
request_methods!(LspHandle);
