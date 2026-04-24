//! Frontend DTO types and LSP→DTO mappers.
//!
//! This module sits at the boundary between the typed `lsp-types` structs
//! (returned by `LeanClient`) and the Tauri frontend. Its two responsibilities
//! are:
//!
//! **DTO types** — Lean-specific structs (`DiagnosticInfo`, `HoverInfo`,
//! `SemanticToken`, etc.) that are serialized to JSON and emitted as Tauri
//! events. They are deliberately separate from the raw LSP types: they use
//! 1-indexed line numbers (matching the frontend's `CodeMirror` convention),
//! flatten nested LSP shapes into flat structs, and carry only the fields the
//! UI actually uses.
//!
//! **Map functions** — Each `parse_*` function accepts a typed `lsp-types`
//! value (already deserialized by `LeanClient`) and maps it to the
//! corresponding DTO. They are the single source of truth for each
//! LSP→DTO conversion and are unit-tested against real LSP payloads.
//!
//! [`decode_semantic_tokens`] accepts a `&[lsp_types::SemanticToken]` slice and
//! implements the LSP delta-encoding algorithm to reconstruct absolute token
//! positions from 5-tuple deltas.

use lsp_types::{
    CodeActionOrCommand, CodeActionResponse, CompletionResponse, DocumentSymbol,
    DocumentSymbolResponse, GotoDefinitionResponse, Hover, HoverContents, Location, LocationLink,
    MarkedString, MarkupKind, PublishDiagnosticsParams, TextEdit, WorkspaceEdit,
};
use serde::Serialize;
use serde_json::Value;

use crate::lsp::client::FileProgressParams;

// ── Public DTO types for Tauri events ─────────────────────────────────

#[derive(Clone, Serialize)]
pub struct DiagnosticInfo {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub severity: u8, // 1 = error, 2 = warning, 3 = info, 4 = hint
    pub message: String,
}

#[derive(Clone, Serialize)]
pub struct SemanticToken {
    pub line: u32,
    pub col: u32,
    pub length: u32,
    pub token_type: String,
    pub token_modifiers: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct LspStatus {
    pub state: String, // "connected", "error", ""
    pub message: String,
}

/// A single completion candidate from `textDocument/completion`.
#[derive(Clone, Serialize)]
pub struct CompletionItem {
    /// The text shown in the completion menu.
    pub label: String,
    /// Brief type or kind information (e.g. "theorem", "def", "(def)").
    pub detail: Option<String>,
    /// The text inserted when the completion is accepted; falls back to `label`.
    pub insert_text: Option<String>,
}

/// A range of lines currently being elaborated by the Lean server.
/// Emitted via the `$/lean/fileProgress` notification.
#[derive(Clone, Serialize)]
pub struct FileProgressRange {
    pub start_line: u32, // 1-indexed (converted from 0-indexed LSP)
    pub end_line: u32,   // 1-indexed
}

/// Markup kind for hover contents — mirrors LSP's `MarkupKind`.
///
/// Serialized as lowercase strings (`"markdown"` / `"plaintext"`) so the
/// frontend can branch on the payload without a separate mapping step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HoverKind {
    Markdown,
    Plaintext,
}

/// Content from `textDocument/hover`, paired with its markup kind so the
/// frontend can render markdown (fenced Lean blocks, docstrings, LaTeX) as
/// rich text and plaintext as preformatted text.
#[derive(Clone, Debug, Serialize)]
pub struct HoverInfo {
    pub contents: String,
    pub kind: HoverKind,
}

/// Result of `textDocument/definition` filtered to a single target location.
///
/// All positions are 0-indexed (raw LSP convention); the frontend converts
/// to CM6 offsets directly.
#[derive(Clone, Debug, Serialize)]
pub struct DefinitionLocation {
    pub uri: String,
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// A single text edit produced by a code action. Positions are 0-indexed.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TextEditDto {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub new_text: String,
}

/// A workspace edit, restricted to per-URI text edits.
///
/// Represented as a flat list of `(uri, edits)` pairs so that the wire shape stays
/// stable across serde. Lean's `textDocument/codeAction` only targets document
/// edits in practice.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct WorkspaceEditDto {
    pub changes: Vec<(String, Vec<TextEditDto>)>,
}

/// A single code action from `textDocument/codeAction`.
#[derive(Clone, Debug, Serialize)]
pub struct CodeActionInfo {
    pub title: String,
    pub kind: Option<String>,
    /// Resolved workspace edit, if the server returned it inline.
    pub edit: Option<WorkspaceEditDto>,
    /// Raw opaque `data` field used by `codeAction/resolve` when `edit` is absent.
    pub resolve_data: Option<Value>,
}

/// A single document symbol from `textDocument/documentSymbol`. Positions are
/// 0-indexed. Children form a nested tree of symbols.
#[derive(Clone, Debug, Serialize)]
pub struct DocumentSymbolInfo {
    pub name: String,
    /// LSP `SymbolKind` numeric code (e.g. 12 = function, 5 = class).
    pub kind: u8,
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub children: Vec<Self>,
}

// ── Private helpers ────────────────────────────────────────────────────

// lsp-types newtypes (DiagnosticSeverity, SymbolKind, …) are transparent `i32` wrappers
// with private fields. Serialize to extract the integer rather than pattern-matching
// all the named constants.
fn newtype_as_u8(v: impl Serialize) -> Option<u8> {
    u8::try_from(serde_json::to_value(v).ok()?.as_i64()?).ok()
}

fn fenced_code_block(language: &str, code: &str) -> String {
    format!("```{language}\n{code}\n```")
}

// ── Parse functions ────────────────────────────────────────────────────

/// Parse a `textDocument/publishDiagnostics` notification params.
#[must_use]
pub fn parse_diagnostics(params: PublishDiagnosticsParams) -> Vec<DiagnosticInfo> {
    params
        .diagnostics
        .into_iter()
        .map(|d| DiagnosticInfo {
            start_line: d.range.start.line + 1, // LSP is 0-indexed; frontend is 1-indexed
            start_col: d.range.start.character,
            end_line: d.range.end.line + 1,
            end_col: d.range.end.character,
            severity: d.severity.and_then(newtype_as_u8).unwrap_or(1),
            message: d.message,
        })
        .collect()
}

/// Decode delta-encoded semantic tokens into absolute positions.
///
/// The LSP response is a flat array of 5-tuples:
///   [deltaLine, deltaStart, length, tokenTypeIndex, tokenModifiers]
#[must_use]
pub fn decode_semantic_tokens(
    data: &[lsp_types::SemanticToken],
    type_legend: &[String],
    modifier_legend: &[String],
) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    let mut line: u32 = 1; // 1-indexed for frontend
    let mut col: u32 = 0;

    for token in data {
        let delta_line = token.delta_line;
        let delta_start = token.delta_start;
        let length = token.length;
        let token_type_idx = token.token_type as usize;
        let modifier_bits = token.token_modifiers_bitset;

        if delta_line > 0 {
            line += delta_line;
            col = delta_start;
        } else {
            col += delta_start;
        }

        let token_type = type_legend
            .get(token_type_idx)
            .cloned()
            .unwrap_or_else(|| "variable".to_string());

        let mut token_modifiers = Vec::new();
        for (i, name) in modifier_legend.iter().enumerate() {
            if modifier_bits & (1 << i) != 0 {
                token_modifiers.push(name.clone());
            }
        }

        tokens.push(SemanticToken {
            line,
            col,
            length,
            token_type,
            token_modifiers,
        });
    }

    tokens
}

/// Map a `textDocument/completion` response into a list of completion items.
///
/// The LSP response is either a `CompletionList` (`{ items: [...] }`) or a bare
/// `CompletionItem[]`. Both shapes are handled here.
#[must_use]
pub fn parse_completion_items(result: Option<CompletionResponse>) -> Vec<CompletionItem> {
    let Some(resp) = result else {
        return Vec::new();
    };
    let items = match resp {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    items
        .into_iter()
        .map(|item| CompletionItem {
            label: item.label,
            detail: item.detail,
            insert_text: item.insert_text,
        })
        .collect()
}

/// Map a `$/lean/fileProgress` notification params into processing ranges.
#[must_use]
pub fn parse_file_progress(params: FileProgressParams) -> Vec<FileProgressRange> {
    params
        .processing
        .into_iter()
        .map(|interval| FileProgressRange {
            start_line: interval.range.start.line + 1, // 0-indexed → 1-indexed
            end_line: interval.range.end.line + 1,
        })
        .collect()
}

/// Map a `textDocument/hover` response into `HoverInfo`.
///
/// Preserves the server's markup `kind` so the frontend can render markdown
/// (fenced Lean blocks, docstrings, inline code, LaTeX) as rich text and
/// plaintext as preformatted text. Returns `None` if the response is null,
/// has no readable contents, or the contents are empty.
#[must_use]
pub fn parse_hover(result: Option<Hover>) -> Option<HoverInfo> {
    let (contents, kind) = hover_contents(result?.contents)?;
    if contents.trim().is_empty() {
        None
    } else {
        Some(HoverInfo { contents, kind })
    }
}

fn hover_contents(contents: HoverContents) -> Option<(String, HoverKind)> {
    match contents {
        HoverContents::Scalar(MarkedString::String(s)) => Some((s, HoverKind::Plaintext)),
        HoverContents::Scalar(MarkedString::LanguageString(ls)) => Some((
            fenced_code_block(&ls.language, &ls.value),
            HoverKind::Markdown,
        )),
        HoverContents::Markup(markup) => {
            let kind = match markup.kind {
                MarkupKind::PlainText => HoverKind::Plaintext,
                MarkupKind::Markdown => HoverKind::Markdown,
            };
            Some((markup.value, kind))
        }
        HoverContents::Array(arr) => {
            if arr.is_empty() {
                return None;
            }
            let mut parts = Vec::new();
            let mut any_markdown = false;
            for item in arr {
                match item {
                    MarkedString::String(s) => parts.push(s),
                    MarkedString::LanguageString(ls) => {
                        parts.push(fenced_code_block(&ls.language, &ls.value));
                        any_markdown = true;
                    }
                }
            }
            if parts.is_empty() {
                return None;
            }
            let kind = if any_markdown {
                HoverKind::Markdown
            } else {
                HoverKind::Plaintext
            };
            Some((parts.join("\n\n"), kind))
        }
    }
}

/// Map a `textDocument/definition` response, keeping only the first target.
///
/// The Lean server may return any of:
///   - `Location` — `{ uri, range }`
///   - `Location[]`
///   - `LocationLink[]` — `{ targetUri, targetRange, targetSelectionRange, originSelectionRange }`
///   - `null`
///
/// We set `linkSupport: false` in the initialize params, but Lean sometimes
/// returns `LocationLink` anyway, so handle both shapes.
#[must_use]
pub fn parse_definition(result: Option<GotoDefinitionResponse>) -> Option<DefinitionLocation> {
    match result? {
        GotoDefinitionResponse::Scalar(ref loc) => Some(location_to_dto(loc)),
        GotoDefinitionResponse::Array(ref locs) => locs.first().map(location_to_dto),
        GotoDefinitionResponse::Link(ref links) => links.first().map(location_link_to_dto),
    }
}

fn location_to_dto(loc: &Location) -> DefinitionLocation {
    DefinitionLocation {
        uri: loc.uri.to_string(),
        line: loc.range.start.line,
        character: loc.range.start.character,
        end_line: loc.range.end.line,
        end_character: loc.range.end.character,
    }
}

fn location_link_to_dto(link: &LocationLink) -> DefinitionLocation {
    let range = link.target_selection_range;
    DefinitionLocation {
        uri: link.target_uri.to_string(),
        line: range.start.line,
        character: range.start.character,
        end_line: range.end.line,
        end_character: range.end.character,
    }
}

/// Map a `textDocument/codeAction` response into our `CodeActionInfo` DTOs.
///
/// Each entry is either a `Command` (ignored — we only surface workspace
/// edits) or a `CodeAction` object. Inline edits are captured in
/// `CodeActionInfo.edit`; actions with only a `data` field (to be resolved
/// later via `codeAction/resolve`) carry `resolve_data`.
#[must_use]
pub fn parse_code_actions(result: Option<CodeActionResponse>) -> Vec<CodeActionInfo> {
    let Some(resp) = result else {
        return Vec::new();
    };
    resp.into_iter()
        .filter_map(|item| match item {
            CodeActionOrCommand::CodeAction(action) => Some(CodeActionInfo {
                title: action.title,
                kind: action.kind.map(|k| k.as_str().to_string()),
                edit: action.edit.and_then(workspace_edit_to_dto),
                resolve_data: action.data,
            }),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect()
}

#[allow(clippy::mutable_key_type)]
fn workspace_edit_to_dto(edit: WorkspaceEdit) -> Option<WorkspaceEditDto> {
    let changes = edit.changes?;
    let out = changes
        .into_iter()
        .map(|(uri, edits)| {
            let dtos = edits.into_iter().map(text_edit_to_dto).collect();
            (uri.to_string(), dtos)
        })
        .collect();
    Some(WorkspaceEditDto { changes: out })
}

/// Map a `WorkspaceEdit` into our `WorkspaceEditDto`.
///
/// Returns `None` if there are no `changes` (the only shape Lean emits).
#[must_use]
pub fn parse_workspace_edit(edit: WorkspaceEdit) -> Option<WorkspaceEditDto> {
    workspace_edit_to_dto(edit)
}

/// Map a `textDocument/formatting` response into a list of text edits.
#[must_use]
pub fn parse_text_edits(result: Option<Vec<TextEdit>>) -> Vec<TextEditDto> {
    result
        .unwrap_or_default()
        .into_iter()
        .map(text_edit_to_dto)
        .collect()
}

fn text_edit_to_dto(edit: TextEdit) -> TextEditDto {
    TextEditDto {
        start_line: edit.range.start.line,
        start_character: edit.range.start.character,
        end_line: edit.range.end.line,
        end_character: edit.range.end.character,
        new_text: edit.new_text,
    }
}

/// Map a `textDocument/documentSymbol` response into a hierarchical tree.
///
/// Only the modern `DocumentSymbol[]` shape is handled — we set
/// `hierarchicalDocumentSymbolSupport: true` in the initialize handshake.
pub fn parse_document_symbols(result: Option<DocumentSymbolResponse>) -> Vec<DocumentSymbolInfo> {
    match result {
        Some(DocumentSymbolResponse::Nested(symbols)) => {
            symbols.into_iter().map(document_symbol_to_dto).collect()
        }
        _ => Vec::new(),
    }
}

fn document_symbol_to_dto(sym: DocumentSymbol) -> DocumentSymbolInfo {
    DocumentSymbolInfo {
        name: sym.name,
        kind: newtype_as_u8(sym.kind).unwrap_or(0),
        start_line: sym.range.start.line,
        start_character: sym.range.start.character,
        end_line: sym.range.end.line,
        end_character: sym.range.end.character,
        children: sym
            .children
            .unwrap_or_default()
            .into_iter()
            .map(document_symbol_to_dto)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{DocumentSymbolResponse, GotoDefinitionResponse, Hover};
    use serde_json::json;

    fn tokens_from_raw(data: &[u32]) -> Vec<lsp_types::SemanticToken> {
        data.chunks_exact(5)
            .map(|c| lsp_types::SemanticToken {
                delta_line: c[0],
                delta_start: c[1],
                length: c[2],
                token_type: c[3],
                token_modifiers_bitset: c[4],
            })
            .collect()
    }

    fn diags_from_value(v: Value) -> Vec<DiagnosticInfo> {
        parse_diagnostics(serde_json::from_value(v).expect("invalid PublishDiagnosticsParams"))
    }

    fn completion_from_value(v: Value) -> Vec<CompletionItem> {
        parse_completion_items(serde_json::from_value(v).ok())
    }

    fn file_progress_from_value(v: Value) -> Vec<FileProgressRange> {
        parse_file_progress(serde_json::from_value(v).expect("invalid FileProgressParams"))
    }

    fn hover_from_value(v: Value) -> Option<HoverInfo> {
        parse_hover(serde_json::from_value::<Hover>(v).ok())
    }

    fn definition_from_value(v: Value) -> Option<DefinitionLocation> {
        parse_definition(serde_json::from_value::<GotoDefinitionResponse>(v).ok())
    }

    fn code_actions_from_value(v: Value) -> Vec<CodeActionInfo> {
        parse_code_actions(serde_json::from_value(v).ok())
    }

    fn doc_symbols_from_value(v: Value) -> Vec<DocumentSymbolInfo> {
        parse_document_symbols(serde_json::from_value::<DocumentSymbolResponse>(v).ok())
    }

    fn workspace_edit_from_value(v: Value) -> Option<WorkspaceEditDto> {
        parse_workspace_edit(serde_json::from_value(v).expect("invalid WorkspaceEdit"))
    }

    fn text_edits_from_value(v: Value) -> Vec<TextEditDto> {
        parse_text_edits(serde_json::from_value(v).ok())
    }

    #[test]
    fn parse_diagnostics_empty_returns_empty() {
        let params = json!({ "uri": "file:///test.lean", "diagnostics": [] });
        assert!(diags_from_value(params).is_empty());
    }

    #[test]
    fn parse_diagnostics_parses_single_diagnostic() {
        let params = json!({
            "uri": "file:///test.lean",
            "diagnostics": [{
                "range": {
                    "start": { "line": 2, "character": 4 },
                    "end":   { "line": 2, "character": 10 }
                },
                "severity": 1,
                "message": "unknown identifier"
            }]
        });
        let diags = diags_from_value(params);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.start_line, 3); // LSP 0-indexed → 1-indexed
        assert_eq!(d.start_col, 4);
        assert_eq!(d.severity, 1);
        assert_eq!(d.message, "unknown identifier");
    }

    #[test]
    fn decode_semantic_tokens_single_token() {
        let types = vec!["keyword".to_string(), "type".to_string()];
        let modifiers = vec!["declaration".to_string()];
        let data = tokens_from_raw(&[0, 5, 3, 0, 0]);
        let tokens = decode_semantic_tokens(&data, &types, &modifiers);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[0].col, 5);
        assert_eq!(tokens[0].length, 3);
        assert_eq!(tokens[0].token_type, "keyword");
        assert!(tokens[0].token_modifiers.is_empty());
    }

    #[test]
    fn decode_semantic_tokens_with_modifiers() {
        let types = vec!["keyword".to_string(), "variable".to_string()];
        let modifiers = vec!["declaration".to_string(), "readonly".to_string()];
        let data = tokens_from_raw(&[0, 0, 5, 1, 0b11]); // variable with declaration+readonly
        let tokens = decode_semantic_tokens(&data, &types, &modifiers);
        assert_eq!(tokens[0].token_type, "variable");
        assert_eq!(tokens[0].token_modifiers, vec!["declaration", "readonly"]);
    }

    #[test]
    fn decode_semantic_tokens_line_advance() {
        let types = vec!["keyword".to_string()];
        let modifiers: Vec<String> = vec![];
        let data = tokens_from_raw(&[
            0, 0, 1, 0, 0, // token at line 1, col 0
            2, 4, 1, 0, 0, // delta line +2, col 4 → line 3, col 4
        ]);
        let tokens = decode_semantic_tokens(&data, &types, &modifiers);
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[1].line, 3);
        assert_eq!(tokens[1].col, 4);
    }

    #[test]
    fn decode_semantic_tokens_col_resets_on_new_line() {
        let types = vec!["keyword".to_string()];
        let modifiers: Vec<String> = vec![];
        // First token: line 1, col 10. Second token: delta_line=1 → line 2, delta_start=3 → col 3 (not 10+3).
        let data = tokens_from_raw(&[0, 10, 1, 0, 0, 1, 3, 1, 0, 0]);
        let tokens = decode_semantic_tokens(&data, &types, &modifiers);
        assert_eq!(tokens[1].line, 2);
        assert_eq!(tokens[1].col, 3);
    }

    #[test]
    fn decode_semantic_tokens_unknown_type_falls_back_to_variable() {
        let types = vec!["keyword".to_string()];
        let modifiers: Vec<String> = vec![];
        let data = tokens_from_raw(&[0, 0, 5, 99, 0]); // index 99 out of bounds
        let tokens = decode_semantic_tokens(&data, &types, &modifiers);
        assert_eq!(tokens[0].token_type, "variable");
    }

    #[test]
    fn parse_completion_items_from_list_object() {
        let result = json!({
            "isIncomplete": false,
            "items": [
                { "label": "theorem", "detail": "keyword", "insertText": "theorem" },
                { "label": "Nat.succ", "detail": "Nat → Nat" }
            ]
        });
        let items = completion_from_value(result);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "theorem");
        assert_eq!(items[0].detail.as_deref(), Some("keyword"));
        assert_eq!(items[0].insert_text.as_deref(), Some("theorem"));
        assert_eq!(items[1].label, "Nat.succ");
        assert_eq!(items[1].detail.as_deref(), Some("Nat → Nat"));
        assert!(items[1].insert_text.is_none());
    }

    #[test]
    fn parse_completion_items_from_bare_array() {
        let result = json!([
            { "label": "def" },
            { "label": "lemma", "detail": "keyword" }
        ]);
        let items = completion_from_value(result);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "def");
        assert!(items[0].detail.is_none());
        assert_eq!(items[1].label, "lemma");
    }

    #[test]
    fn parse_completion_items_empty_returns_empty() {
        let result = json!({ "isIncomplete": false, "items": [] });
        assert!(completion_from_value(result).is_empty());
    }

    #[test]
    fn parse_completion_items_none_returns_empty() {
        assert!(parse_completion_items(None).is_empty());
    }

    #[test]
    fn parse_file_progress_single_range() {
        let params = json!({
            "textDocument": { "uri": "file:///test.lean" },
            "processing": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 10, "character": 0 }
                }
            }]
        });
        let ranges = file_progress_from_value(params);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 1); // 0-indexed → 1-indexed
        assert_eq!(ranges[0].end_line, 11);
    }

    #[test]
    fn parse_file_progress_multiple_ranges() {
        let params = json!({
            "textDocument": { "uri": "file:///test.lean" },
            "processing": [
                { "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 5, "character": 0 } } },
                { "range": { "start": { "line": 8, "character": 0 }, "end": { "line": 12, "character": 0 } } }
            ]
        });
        let ranges = file_progress_from_value(params);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start_line, 1);
        assert_eq!(ranges[0].end_line, 6);
        assert_eq!(ranges[1].start_line, 9);
        assert_eq!(ranges[1].end_line, 13);
    }

    #[test]
    fn parse_file_progress_empty_processing_returns_empty() {
        let params = json!({
            "textDocument": { "uri": "file:///test.lean" },
            "processing": []
        });
        assert!(file_progress_from_value(params).is_empty());
    }

    #[test]
    fn parse_diagnostics_defaults_severity_to_1() {
        let params = json!({
            "uri": "file:///test.lean",
            "diagnostics": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end":   { "line": 0, "character": 1 }
                },
                "message": "err"
            }]
        });
        let diags = diags_from_value(params);
        assert_eq!(diags[0].severity, 1);
    }

    // ── Hover parsing ──────────────────────────────────────────────────

    #[test]
    fn parse_hover_markdown_preserves_docstring() {
        let raw = "```lean\ntheorem foo : True\n```\nDocumentation paragraph.";
        let result = json!({ "contents": { "kind": "markdown", "value": raw } });
        let hover = hover_from_value(result).expect("hover should parse");
        assert_eq!(hover.contents, raw);
        assert_eq!(hover.kind, HoverKind::Markdown);
    }

    #[test]
    fn parse_hover_plaintext_kind_roundtrips() {
        let result = json!({ "contents": { "kind": "plaintext", "value": "x : Nat" } });
        let hover = hover_from_value(result).expect("hover should parse");
        assert_eq!(hover.contents, "x : Nat");
        assert_eq!(hover.kind, HoverKind::Plaintext);
    }

    #[test]
    fn parse_hover_none_returns_none() {
        assert!(parse_hover(None).is_none());
    }

    #[test]
    fn parse_hover_bare_string_is_plaintext() {
        let result = json!({ "contents": "inline type" });
        let hover = hover_from_value(result).expect("hover should parse");
        assert_eq!(hover.contents, "inline type");
        assert_eq!(hover.kind, HoverKind::Plaintext);
    }

    #[test]
    fn parse_hover_array_with_marked_string_is_markdown() {
        let result = json!({
            "contents": [
                { "language": "lean", "value": "theorem foo : True" },
                "Some docs"
            ]
        });
        let hover = hover_from_value(result).expect("hover should parse");
        assert_eq!(hover.kind, HoverKind::Markdown);
        assert!(hover.contents.contains("```lean\ntheorem foo : True\n```"));
        assert!(hover.contents.contains("Some docs"));
    }

    #[test]
    fn parse_hover_array_with_plain_strings_joined() {
        // Per LSP spec, array contents are Vec<MarkedString>. A bare string ["one", "two"]
        // is deserialized by lsp-types as a LanguageString (positional serde matching),
        // which renders as a fenced code block with language="one" and value="two".
        // This test documents that actual behavior.
        let result = json!({ "contents": ["lean", "theorem foo : True"] });
        let hover = hover_from_value(result).expect("hover should parse");
        assert_eq!(hover.kind, HoverKind::Markdown);
        assert!(hover.contents.contains("```lean\ntheorem foo : True\n```"));
    }

    #[test]
    fn parse_hover_empty_returns_none() {
        let result = json!({ "contents": { "kind": "markdown", "value": "" } });
        assert!(hover_from_value(result).is_none());
    }

    // ── Definition parsing ─────────────────────────────────────────────

    #[test]
    fn parse_definition_single_location() {
        let result = json!({
            "uri": "file:///home/user/proof.lean",
            "range": {
                "start": { "line": 3, "character": 8 },
                "end": { "line": 3, "character": 11 }
            }
        });
        let def = definition_from_value(result).expect("definition should parse");
        assert_eq!(def.uri, "file:///home/user/proof.lean");
        assert_eq!(def.line, 3);
        assert_eq!(def.character, 8);
        assert_eq!(def.end_line, 3);
        assert_eq!(def.end_character, 11);
    }

    #[test]
    fn parse_definition_array_picks_first() {
        let result = json!([
            {
                "uri": "file:///a.lean",
                "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 3 } }
            },
            {
                "uri": "file:///b.lean",
                "range": { "start": { "line": 5, "character": 0 }, "end": { "line": 5, "character": 3 } }
            }
        ]);
        let def = definition_from_value(result).expect("definition should parse");
        assert_eq!(def.uri, "file:///a.lean");
    }

    #[test]
    fn parse_definition_none_returns_none() {
        assert!(parse_definition(None).is_none());
    }

    #[test]
    fn parse_definition_empty_array_returns_none() {
        let result = json!([]);
        assert!(definition_from_value(result).is_none());
    }

    #[test]
    fn parse_definition_location_link() {
        let result = json!([{
            "originSelectionRange": {
                "start": { "line": 3, "character": 27 },
                "end": { "line": 3, "character": 37 }
            },
            "targetUri": "file:///proof.lean",
            "targetRange": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 20 }
            },
            "targetSelectionRange": {
                "start": { "line": 1, "character": 8 },
                "end": { "line": 1, "character": 18 }
            }
        }]);
        let def = definition_from_value(result).expect("should parse LocationLink");
        assert_eq!(def.uri, "file:///proof.lean");
        assert_eq!(def.line, 1);
        assert_eq!(def.character, 8);
        assert_eq!(def.end_line, 1);
        assert_eq!(def.end_character, 18);
    }

    #[test]
    fn parse_definition_location_link_selection_range_used() {
        // targetSelectionRange is required per LSP spec; targetRange is the full body range.
        // We always use targetSelectionRange (the name span), not targetRange.
        let result = json!([{
            "targetUri": "file:///a.lean",
            "targetRange": {
                "start": { "line": 5, "character": 0 },
                "end": { "line": 5, "character": 50 }
            },
            "targetSelectionRange": {
                "start": { "line": 5, "character": 2 },
                "end": { "line": 5, "character": 10 }
            }
        }]);
        let def = definition_from_value(result).expect("should parse LocationLink");
        assert_eq!(def.uri, "file:///a.lean");
        assert_eq!(def.line, 5);
        assert_eq!(def.character, 2);
    }

    // ── Code action parsing ────────────────────────────────────────────

    #[test]
    fn parse_code_actions_with_inline_edit() {
        let result = json!([
            {
                "title": "Try this: exact rfl",
                "kind": "quickfix",
                "edit": {
                    "changes": {
                        "file:///proof.lean": [
                            {
                                "range": {
                                    "start": { "line": 2, "character": 2 },
                                    "end": { "line": 2, "character": 7 }
                                },
                                "newText": "exact rfl"
                            }
                        ]
                    }
                }
            }
        ]);
        let actions = code_actions_from_value(result);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Try this: exact rfl");
        assert_eq!(actions[0].kind.as_deref(), Some("quickfix"));
        let edit = actions[0].edit.as_ref().expect("should have edit");
        assert_eq!(edit.changes.len(), 1);
        let (uri, edits) = &edit.changes[0];
        assert_eq!(uri, "file:///proof.lean");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "exact rfl");
        assert_eq!(edits[0].start_line, 2);
        assert_eq!(edits[0].start_character, 2);
    }

    #[test]
    fn parse_code_actions_with_resolve_data() {
        let result = json!([
            {
                "title": "Lazy action",
                "data": { "token": 42 }
            }
        ]);
        let actions = code_actions_from_value(result);
        assert_eq!(actions.len(), 1);
        assert!(actions[0].edit.is_none());
        assert_eq!(actions[0].resolve_data, Some(json!({ "token": 42 })));
    }

    #[test]
    fn parse_code_actions_empty_returns_empty() {
        assert!(code_actions_from_value(json!([])).is_empty());
    }

    #[test]
    fn parse_code_actions_none_returns_empty() {
        assert!(parse_code_actions(None).is_empty());
    }

    #[test]
    fn parse_code_actions_skips_commands_without_title() {
        let result = json!([
            { "command": "foo", "title": "a command" },
            { "title": "Real action" }
        ]);
        let actions = code_actions_from_value(result);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Real action");
    }

    // ── Document symbol parsing ────────────────────────────────────────

    #[test]
    fn parse_document_symbols_flat_list() {
        let result = json!([
            {
                "name": "foo",
                "kind": 12,
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 2, "character": 0 }
                },
                "selectionRange": {
                    "start": { "line": 0, "character": 8 },
                    "end": { "line": 0, "character": 11 }
                }
            },
            {
                "name": "bar",
                "kind": 13,
                "range": {
                    "start": { "line": 3, "character": 0 },
                    "end": { "line": 4, "character": 0 }
                },
                "selectionRange": {
                    "start": { "line": 3, "character": 4 },
                    "end": { "line": 3, "character": 7 }
                }
            }
        ]);
        let symbols = doc_symbols_from_value(result);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "foo");
        assert_eq!(symbols[0].kind, 12);
        assert_eq!(symbols[0].start_line, 0);
        assert!(symbols[0].children.is_empty());
        assert_eq!(symbols[1].name, "bar");
    }

    #[test]
    fn parse_document_symbols_hierarchical() {
        let result = json!([
            {
                "name": "Ns",
                "kind": 3,
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 10, "character": 0 }
                },
                "selectionRange": {
                    "start": { "line": 0, "character": 10 },
                    "end": { "line": 0, "character": 12 }
                },
                "children": [
                    {
                        "name": "inner",
                        "kind": 12,
                        "range": {
                            "start": { "line": 2, "character": 2 },
                            "end": { "line": 4, "character": 0 }
                        },
                        "selectionRange": {
                            "start": { "line": 2, "character": 10 },
                            "end": { "line": 2, "character": 15 }
                        }
                    }
                ]
            }
        ]);
        let symbols = doc_symbols_from_value(result);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].children.len(), 1);
        assert_eq!(symbols[0].children[0].name, "inner");
        assert_eq!(symbols[0].children[0].start_line, 2);
    }

    #[test]
    fn parse_document_symbols_none_returns_empty() {
        assert!(parse_document_symbols(None).is_empty());
    }

    #[test]
    fn parse_workspace_edit_multiple_uris() {
        let edit = json!({
            "changes": {
                "file:///a.lean": [
                    {
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 1 }
                        },
                        "newText": "a"
                    }
                ],
                "file:///b.lean": [
                    {
                        "range": {
                            "start": { "line": 1, "character": 1 },
                            "end": { "line": 1, "character": 2 }
                        },
                        "newText": "b"
                    }
                ]
            }
        });
        let ws = workspace_edit_from_value(edit).expect("parse");
        assert_eq!(ws.changes.len(), 2);
    }

    #[test]
    fn parse_workspace_edit_missing_changes_returns_none() {
        assert!(workspace_edit_from_value(json!({})).is_none());
    }

    // ── Text edit parsing ──────────────────────────────────────────────

    #[test]
    fn parse_text_edits_single_edit() {
        let result = json!([{
            "range": {
                "start": { "line": 0, "character": 4 },
                "end":   { "line": 0, "character": 9 }
            },
            "newText": "hello"
        }]);
        let edits = text_edits_from_value(result);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].start_line, 0);
        assert_eq!(edits[0].start_character, 4);
        assert_eq!(edits[0].end_line, 0);
        assert_eq!(edits[0].end_character, 9);
        assert_eq!(edits[0].new_text, "hello");
    }

    #[test]
    fn parse_text_edits_multiple_edits_preserves_order() {
        let result = json!([
            {
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 3 } },
                "newText": "def"
            },
            {
                "range": { "start": { "line": 1, "character": 2 }, "end": { "line": 1, "character": 2 } },
                "newText": "  "
            }
        ]);
        let edits = text_edits_from_value(result);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].new_text, "def");
        assert_eq!(edits[1].start_line, 1);
        assert_eq!(edits[1].new_text, "  ");
    }

    #[test]
    fn parse_text_edits_empty_array_returns_empty() {
        assert!(text_edits_from_value(json!([])).is_empty());
    }

    #[test]
    fn parse_text_edits_none_returns_empty() {
        assert!(parse_text_edits(None).is_empty());
    }

    #[test]
    fn parse_text_edits_insertion_has_equal_start_and_end() {
        let result = json!([{
            "range": {
                "start": { "line": 3, "character": 0 },
                "end":   { "line": 3, "character": 0 }
            },
            "newText": "import Mathlib\n"
        }]);
        let edits = text_edits_from_value(result);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].start_line, edits[0].end_line);
        assert_eq!(edits[0].start_character, edits[0].end_character);
        assert_eq!(edits[0].new_text, "import Mathlib\n");
    }
}
