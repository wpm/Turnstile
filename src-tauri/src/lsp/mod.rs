//! LSP client: stdin/stdout JSON-RPC 2.0 transport for the Lean language server.
//!
//! Transport: Content-Length framing per the LSP spec.
//! The stdout reader runs on a dedicated thread (spawned in lib.rs) to avoid
//! blocking the async runtime. Responses to awaited requests are routed back
//! via the `pending` map; all other messages are dispatched to the caller's callback.

pub mod error;
pub use error::LspError;

mod transport;
pub use transport::{ack_request, path_to_file_uri, send_request_sync};

mod client;
pub use client::{LspClient, LspLifecycle};

mod protocol;
pub use protocol::{initialize_params, parse_modifier_legend, parse_token_legend, LspNotification};

mod parse;
pub use parse::{
    decode_semantic_tokens, parse_code_actions, parse_completion_items, parse_definition,
    parse_diagnostics, parse_document_symbols, parse_file_progress, parse_hover, parse_text_edits,
    parse_workspace_edit, CodeActionInfo, CompletionItem, DefinitionLocation, DiagnosticInfo,
    DocumentSymbolInfo, FileProgressRange, HoverInfo, HoverKind, LspStatus, SemanticToken,
    TextEditDto, WorkspaceEditDto,
};
