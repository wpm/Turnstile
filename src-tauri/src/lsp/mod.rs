//! LSP client: async-lsp based transport for the Lean language server.

pub mod events;

mod client;
pub use client::{FileProgressParams, LeanClient, PlainGoal, StartError, StopError};

mod parse;
pub use parse::{
    decode_semantic_tokens, parse_code_actions, parse_completion_items, parse_definition,
    parse_diagnostics, parse_document_symbols, parse_file_progress, parse_hover, parse_text_edits,
    parse_workspace_edit, CodeActionInfo, CompletionItem, DefinitionLocation, DiagnosticInfo,
    DocumentSymbolInfo, FileProgressRange, HoverInfo, HoverKind, LspStatus, SemanticToken,
    TextEditDto, WorkspaceEditDto,
};
