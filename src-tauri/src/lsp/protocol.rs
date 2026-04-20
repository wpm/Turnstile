//! LSP protocol messages, capabilities, and notification dispatch.
//!
//! [`initialize_params`] builds the `initialize` request payload that declares
//! this client's capabilities to the Lean server. It is called once at startup
//! and the result is sent as the first JSON-RPC request. Capabilities are
//! constructed using `lsp-types` structs and then serialized to `Value`.
//!
//! [`LspNotification`] is a tagged enum over the server-pushed notifications
//! this client handles. Serde deserializes incoming messages by the `method`
//! field, giving exhaustive compile-time dispatch instead of string matching.
//! Unknown methods fail deserialization, which surfaces as an unhandled-message
//! log rather than a silent no-op.
//!
//! [`parse_token_legend`] and [`parse_modifier_legend`] extract the semantic
//! token type and modifier lists from the `initialize` response; the decoded
//! legend is stored in `LspClient` and passed to [`super::decode_semantic_tokens`]
//! on every `textDocument/semanticTokens` response.

use lsp_types::{
    ClientCapabilities, CodeActionClientCapabilities, CodeActionKindLiteralSupport,
    CodeActionLiteralSupport, CompletionClientCapabilities, CompletionItemCapability,
    DocumentFormattingClientCapabilities, DocumentSymbolClientCapabilities, GotoCapability,
    HoverClientCapabilities, InitializeParams, PublishDiagnosticsClientCapabilities,
    SemanticTokensClientCapabilities, SemanticTokensClientCapabilitiesRequests,
    SemanticTokensFullOptions, TextDocumentClientCapabilities, TextDocumentSyncClientCapabilities,
    Uri, WorkspaceFolder,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// Build the `initialize` request params.
///
/// # Panics
/// Panics if `root_uri` is not a valid URI.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn initialize_params(root_uri: &str) -> Value {
    let uri: Uri = root_uri.parse().expect("root_uri must be a valid URI");

    let params = InitializeParams {
        process_id: Some(std::process::id()),
        workspace_folders: Some(vec![WorkspaceFolder {
            uri,
            name: "turnstile".to_string(),
        }]),
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                synchronization: Some(TextDocumentSyncClientCapabilities {
                    dynamic_registration: Some(false),
                    will_save: Some(false),
                    will_save_wait_until: Some(false),
                    did_save: Some(true),
                }),
                publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                    related_information: Some(true),
                    ..Default::default()
                }),
                semantic_tokens: Some(SemanticTokensClientCapabilities {
                    dynamic_registration: Some(false),
                    requests: SemanticTokensClientCapabilitiesRequests {
                        full: Some(SemanticTokensFullOptions::Bool(true)),
                        ..Default::default()
                    },
                    token_types: vec![
                        "namespace".into(),
                        "type".into(),
                        "class".into(),
                        "enum".into(),
                        "interface".into(),
                        "struct".into(),
                        "typeParameter".into(),
                        "parameter".into(),
                        "variable".into(),
                        "property".into(),
                        "enumMember".into(),
                        "event".into(),
                        "function".into(),
                        "method".into(),
                        "macro".into(),
                        "keyword".into(),
                        "modifier".into(),
                        "comment".into(),
                        "string".into(),
                        "number".into(),
                        "regexp".into(),
                        "operator".into(),
                        "decorator".into(),
                    ],
                    token_modifiers: vec![
                        "declaration".into(),
                        "definition".into(),
                        "readonly".into(),
                        "static".into(),
                        "deprecated".into(),
                        "abstract".into(),
                        "async".into(),
                        "modification".into(),
                        "documentation".into(),
                        "defaultLibrary".into(),
                    ],
                    formats: vec![lsp_types::TokenFormat::RELATIVE],
                    multiline_token_support: Some(false),
                    overlapping_token_support: Some(false),
                    ..Default::default()
                }),
                completion: Some(CompletionClientCapabilities {
                    completion_item: Some(CompletionItemCapability {
                        snippet_support: Some(false),
                        documentation_format: Some(vec![lsp_types::MarkupKind::PlainText]),
                        ..Default::default()
                    }),
                    context_support: Some(false),
                    ..Default::default()
                }),
                hover: Some(HoverClientCapabilities {
                    dynamic_registration: Some(false),
                    content_format: Some(vec![
                        lsp_types::MarkupKind::Markdown,
                        lsp_types::MarkupKind::PlainText,
                    ]),
                }),
                definition: Some(GotoCapability {
                    dynamic_registration: Some(false),
                    link_support: Some(false),
                }),
                code_action: Some(CodeActionClientCapabilities {
                    dynamic_registration: Some(false),
                    resolve_support: Some(lsp_types::CodeActionCapabilityResolveSupport {
                        properties: vec!["edit".to_string()],
                    }),
                    code_action_literal_support: Some(CodeActionLiteralSupport {
                        code_action_kind: CodeActionKindLiteralSupport { value_set: vec![] },
                    }),
                    data_support: Some(true),
                    ..Default::default()
                }),
                document_symbol: Some(DocumentSymbolClientCapabilities {
                    dynamic_registration: Some(false),
                    hierarchical_document_symbol_support: Some(true),
                    ..Default::default()
                }),
                formatting: Some(DocumentFormattingClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                ..Default::default()
            }),
            experimental: Some(json!({ "plainGoal": true })),
            ..Default::default()
        },
        ..Default::default()
    };

    serde_json::to_value(params).expect("InitializeParams serialization is infallible")
}

/// Parse the semantic token type legend from an initialized response.
#[must_use]
pub fn parse_token_legend(result: &Value) -> Vec<String> {
    result
        .get("capabilities")
        .and_then(|c| c.get("semanticTokensProvider"))
        .and_then(|p| p.get("legend"))
        .and_then(|l| l.get("tokenTypes"))
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse the semantic token modifier legend from an initialized response.
#[must_use]
pub fn parse_modifier_legend(result: &Value) -> Vec<String> {
    result
        .get("capabilities")
        .and_then(|c| c.get("semanticTokensProvider"))
        .and_then(|p| p.get("legend"))
        .and_then(|l| l.get("tokenModifiers"))
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// A server-to-client LSP notification we know how to handle.
///
/// Serde dispatches on the JSON-RPC `method` field, which gives us three
/// compile-time guarantees that the previous string-match dispatch lacked:
/// method names cannot contain typos, the match over this enum is
/// exhaustive, and adding a new notification is a compile error until
/// every caller handles it.
///
/// `$/lean/fileProgress` is a Lean-specific extension with no `lsp-types`
/// analog, so its params stay as raw `Value`. All other variants use typed
/// `lsp-types` params directly.
#[derive(Debug, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum LspNotification {
    #[serde(rename = "textDocument/publishDiagnostics")]
    PublishDiagnostics(lsp_types::PublishDiagnosticsParams),
    #[serde(rename = "$/lean/fileProgress")]
    FileProgress(Value),
    #[serde(rename = "window/logMessage")]
    LogMessage(lsp_types::LogMessageParams),
    #[serde(rename = "window/showMessage")]
    ShowMessage(lsp_types::ShowMessageParams),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn initialize_params_contains_workspace_folder() {
        let uri = "file:///my/project";
        let params = initialize_params(uri);
        assert_eq!(params["workspaceFolders"][0]["uri"], uri);
        assert_eq!(params["workspaceFolders"][0]["name"], "turnstile");
    }

    #[test]
    fn initialize_params_declares_plain_goal_capability() {
        let params = initialize_params("file:///tmp");
        assert_eq!(params["capabilities"]["experimental"]["plainGoal"], true);
    }

    #[test]
    fn initialize_params_declares_completion_capability() {
        let params = initialize_params("file:///tmp");
        assert_eq!(
            params["capabilities"]["textDocument"]["completion"]["completionItem"]
                ["snippetSupport"],
            false
        );
    }

    #[test]
    fn initialize_params_process_id_matches() {
        let params = initialize_params("file:///tmp");
        assert_eq!(params["processId"], std::process::id());
    }

    #[test]
    fn initialize_params_declares_document_symbol_capability() {
        let params = initialize_params("file:///tmp");
        assert_eq!(
            params["capabilities"]["textDocument"]["documentSymbol"]
                ["hierarchicalDocumentSymbolSupport"],
            true
        );
    }

    #[test]
    fn parse_token_legend_returns_types() {
        let result = json!({
            "capabilities": {
                "semanticTokensProvider": {
                    "legend": {
                        "tokenTypes": ["keyword", "type", "variable"]
                    }
                }
            }
        });
        let legend = parse_token_legend(&result);
        assert_eq!(legend, vec!["keyword", "type", "variable"]);
    }

    #[test]
    fn parse_token_legend_missing_capabilities_returns_empty() {
        assert!(parse_token_legend(&json!({})).is_empty());
    }

    #[test]
    fn parse_modifier_legend_returns_modifiers() {
        let result = json!({
            "capabilities": {
                "semanticTokensProvider": {
                    "legend": {
                        "tokenTypes": ["keyword"],
                        "tokenModifiers": ["declaration", "readonly"]
                    }
                }
            }
        });
        let legend = parse_modifier_legend(&result);
        assert_eq!(legend, vec!["declaration", "readonly"]);
    }

    #[test]
    fn parse_modifier_legend_missing_returns_empty() {
        assert!(parse_modifier_legend(&json!({})).is_empty());
    }

    #[test]
    fn lsp_notification_parses_log_message() {
        let msg = json!({
            "method": "window/logMessage",
            "params": { "type": 3, "message": "hello" }
        });
        let parsed: LspNotification =
            serde_json::from_value(msg).expect("should deserialize as LogMessage");
        match parsed {
            LspNotification::LogMessage(p) => {
                assert_eq!(p.typ, lsp_types::MessageType::INFO);
                assert_eq!(p.message, "hello");
            }
            _ => panic!("expected LogMessage variant"),
        }
    }

    #[test]
    fn lsp_notification_rejects_unknown_method() {
        let msg = json!({
            "method": "some/unknownMethod",
            "params": {}
        });
        let result: Result<LspNotification, _> = serde_json::from_value(msg);
        assert!(
            result.is_err(),
            "unknown methods must fail deserialization, not fall through silently"
        );
    }
}
