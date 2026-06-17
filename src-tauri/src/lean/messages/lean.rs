//! Display helpers for Lean LSP params types.
//!
//! Rust's orphan rule prevents implementing `std::fmt::Display` directly on
//! foreign `lsp_types` structs, so each type gets a `display()` method via
//! `DisplayLspParams` that returns an `impl fmt::Display` wrapper.

use crate::lean::server::FileProgressParams;
use lsp_types::{LogMessageParams, PublishDiagnosticsParams, ShowMessageParams};
use std::fmt;

fn digit_count(n: u32) -> usize {
    n.checked_ilog10().unwrap_or(0) as usize + 1
}

fn pos_width(line: u32, character: u32) -> usize {
    digit_count(line) + 1 + digit_count(character)
}

fn url_filename(url: &lsp_types::Url) -> &str {
    url.path().rsplit('/').next().unwrap_or("")
}

fn format_range(r: &lsp_types::Range) -> String {
    format!(
        "{}:{}-{}:{}",
        r.start.line + 1,
        r.start.character,
        r.end.line + 1,
        r.end.character,
    )
}

fn display_uri(uri: &lsp_types::Url) -> impl fmt::Display + '_ {
    struct D<'a>(&'a lsp_types::Url);
    impl fmt::Display for D<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", url_filename(self.0))
        }
    }
    D(uri)
}

fn display_tdp(p: &lsp_types::TextDocumentPositionParams) -> impl fmt::Display + '_ {
    struct D<'a>(&'a lsp_types::TextDocumentPositionParams);
    impl fmt::Display for D<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{} {}:{}",
                url_filename(&self.0.text_document.uri),
                self.0.position.line + 1,
                self.0.position.character
            )
        }
    }
    D(p)
}

/// Human-readable `Display` for an LSP params type.
pub trait DisplayLspParams {
    fn display(&self) -> impl fmt::Display + '_;
}

// ── Implementations ────────────────────────────────────────────────────

impl DisplayLspParams for () {
    fn display(&self) -> impl fmt::Display + '_ {
        ""
    }
}

impl DisplayLspParams for lsp_types::DidOpenTextDocumentParams {
    fn display(&self) -> impl fmt::Display + '_ {
        struct D<'a>(&'a lsp_types::DidOpenTextDocumentParams);
        impl fmt::Display for D<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "open {} v{} ({} chars)",
                    url_filename(&self.0.text_document.uri),
                    self.0.text_document.version,
                    self.0.text_document.text.len()
                )
            }
        }
        D(self)
    }
}

impl DisplayLspParams for lsp_types::DidCloseTextDocumentParams {
    fn display(&self) -> impl fmt::Display + '_ {
        struct D<'a>(&'a lsp_types::DidCloseTextDocumentParams);
        impl fmt::Display for D<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "close {}", display_uri(&self.0.text_document.uri))
            }
        }
        D(self)
    }
}

impl DisplayLspParams for lsp_types::DidSaveTextDocumentParams {
    fn display(&self) -> impl fmt::Display + '_ {
        struct D<'a>(&'a lsp_types::DidSaveTextDocumentParams);
        impl fmt::Display for D<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "save {}", display_uri(&self.0.text_document.uri))
            }
        }
        D(self)
    }
}

impl DisplayLspParams for lsp_types::DocumentSymbolParams {
    fn display(&self) -> impl fmt::Display + '_ {
        display_uri(&self.text_document.uri)
    }
}

impl DisplayLspParams for lsp_types::SemanticTokensParams {
    fn display(&self) -> impl fmt::Display + '_ {
        display_uri(&self.text_document.uri)
    }
}

impl DisplayLspParams for lsp_types::DidChangeTextDocumentParams {
    fn display(&self) -> impl fmt::Display + '_ {
        struct D<'a>(&'a lsp_types::DidChangeTextDocumentParams);
        impl fmt::Display for D<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "change {} v{} ({} edits)",
                    url_filename(&self.0.text_document.uri),
                    self.0.text_document.version,
                    self.0.content_changes.len()
                )?;
                for change in &self.0.content_changes {
                    match &change.range {
                        Some(r) => write!(
                            f,
                            "\n  {}:{} \u{2013} {}:{} \u{2190} {:?}",
                            r.start.line + 1,
                            r.start.character,
                            r.end.line + 1,
                            r.end.character,
                            change.text
                        )?,
                        None => write!(f, "\n  (full replace, {} chars)", change.text.len())?,
                    }
                }
                Ok(())
            }
        }
        D(self)
    }
}

impl DisplayLspParams for lsp_types::HoverParams {
    fn display(&self) -> impl fmt::Display + '_ {
        display_tdp(&self.text_document_position_params)
    }
}

impl DisplayLspParams for lsp_types::CompletionParams {
    fn display(&self) -> impl fmt::Display + '_ {
        display_tdp(&self.text_document_position)
    }
}

impl DisplayLspParams for lsp_types::GotoDefinitionParams {
    fn display(&self) -> impl fmt::Display + '_ {
        display_tdp(&self.text_document_position_params)
    }
}

impl DisplayLspParams for lsp_types::CodeActionParams {
    fn display(&self) -> impl fmt::Display + '_ {
        struct D<'a>(&'a lsp_types::CodeActionParams);
        impl fmt::Display for D<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "{} {}",
                    url_filename(&self.0.text_document.uri),
                    format_range(&self.0.range)
                )
            }
        }
        D(self)
    }
}

impl DisplayLspParams for lsp_types::CodeAction {
    fn display(&self) -> impl fmt::Display + '_ {
        &self.title as &str
    }
}

impl DisplayLspParams for lsp_types::TextDocumentPositionParams {
    fn display(&self) -> impl fmt::Display + '_ {
        display_tdp(self)
    }
}

// ── Server→client notification types ──────────────────────────────────

fn display_message<'a>(
    label: &'a str,
    typ: lsp_types::MessageType,
    message: &'a str,
) -> impl fmt::Display + 'a {
    struct D<'a>(&'a str, lsp_types::MessageType, &'a str);
    impl fmt::Display for D<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{} {:?}: {}", self.0, self.1, self.2)
        }
    }
    D(label, typ, message)
}

impl DisplayLspParams for ShowMessageParams {
    fn display(&self) -> impl fmt::Display + '_ {
        display_message("showMessage", self.typ, &self.message)
    }
}

impl DisplayLspParams for LogMessageParams {
    fn display(&self) -> impl fmt::Display + '_ {
        display_message("logMessage", self.typ, &self.message)
    }
}

impl DisplayLspParams for FileProgressParams {
    fn display(&self) -> impl fmt::Display + '_ {
        struct D<'a>(&'a FileProgressParams);
        impl fmt::Display for D<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.0.version() {
                    Some(v) => write!(
                        f,
                        "fileProgress {} v{} ({} ranges)",
                        url_filename(&self.0.text_document.uri),
                        v,
                        self.0.processing.len()
                    )?,
                    None => write!(
                        f,
                        "fileProgress {} ({} ranges)",
                        url_filename(&self.0.text_document.uri),
                        self.0.processing.len()
                    )?,
                }
                if !self.0.processing.is_empty() {
                    let col_w = self
                        .0
                        .processing
                        .iter()
                        .map(|i| pos_width(i.range.start.line + 1, i.range.start.character))
                        .max()
                        .unwrap_or(0);
                    for interval in &self.0.processing {
                        let sl = interval.range.start.line + 1;
                        let sc = interval.range.start.character;
                        let pad = col_w - pos_width(sl, sc);
                        write!(
                            f,
                            "\n  {sl}:{sc}{:pad$}  \u{2013}  {}:{}",
                            "",
                            interval.range.end.line + 1,
                            interval.range.end.character,
                            pad = pad,
                        )?;
                    }
                }
                Ok(())
            }
        }
        D(self)
    }
}

impl DisplayLspParams for PublishDiagnosticsParams {
    fn display(&self) -> impl fmt::Display + '_ {
        struct D<'a>(&'a PublishDiagnosticsParams);
        impl fmt::Display for D<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.0.version {
                    Some(v) => write!(f, "diagnostics {} v{}", url_filename(&self.0.uri), v)?,
                    None => write!(f, "diagnostics {}", url_filename(&self.0.uri))?,
                }
                write!(f, " [")?;
                for (i, d) in self.0.diagnostics.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    let severity = match d.severity {
                        Some(lsp_types::DiagnosticSeverity::ERROR) => "error",
                        Some(lsp_types::DiagnosticSeverity::WARNING) => "warning",
                        Some(lsp_types::DiagnosticSeverity::INFORMATION) => "info",
                        Some(lsp_types::DiagnosticSeverity::HINT) => "hint",
                        _ => "unknown",
                    };
                    write!(f, "{} {severity}", format_range(&d.range))?;
                    if let Some(code) = &d.code {
                        match code {
                            lsp_types::NumberOrString::Number(n) => write!(f, ":{n}")?,
                            lsp_types::NumberOrString::String(s) => write!(f, ":{s}")?,
                        }
                    }
                    if let Some(desc) = &d.code_description {
                        write!(f, ":{}", desc.href)?;
                    }
                }
                write!(f, "]")
            }
        }
        D(self)
    }
}

/// A message pushed from `lean --server` to the client.
#[derive(Debug)]
pub enum LeanMessage {
    Diagnostics(PublishDiagnosticsParams),
    FileProgress(FileProgressParams),
    LogMessage(LogMessageParams),
    ShowMessage(ShowMessageParams),
    TokenRefresh,
}

impl fmt::Display for LeanMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diagnostics(p) => write!(f, "{}", p.display()),
            Self::FileProgress(p) => write!(f, "{}", p.display()),
            Self::LogMessage(p) => write!(f, "{}", p.display()),
            Self::ShowMessage(p) => write!(f, "{}", p.display()),
            Self::TokenRefresh => write!(f, "tokenRefresh"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lean::server::{FileProgressInterval, FileProgressTextDocument};
    use lsp_types::{
        CodeAction, CodeActionContext, CodeActionParams, CompletionParams, Diagnostic,
        DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
        DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentSymbolParams,
        GotoDefinitionParams, HoverParams, MessageType, NumberOrString, PartialResultParams,
        Position, Range, SemanticTokensParams, TextDocumentContentChangeEvent,
        TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
        VersionedTextDocumentIdentifier, WorkDoneProgressParams,
    };

    fn uri() -> Url {
        Url::parse("file:///home/user/lean-project/Proof.lean").unwrap()
    }

    fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Range {
        Range {
            start: Position {
                line: sl,
                character: sc,
            },
            end: Position {
                line: el,
                character: ec,
            },
        }
    }

    fn tdi() -> TextDocumentIdentifier {
        TextDocumentIdentifier { uri: uri() }
    }

    fn tdp(line: u32, character: u32) -> TextDocumentPositionParams {
        TextDocumentPositionParams {
            text_document: tdi(),
            position: Position { line, character },
        }
    }

    // ── helper functions ──────────────────────────────────────────────

    #[test]
    fn digit_count_handles_zero_and_powers_of_ten() {
        assert_eq!(digit_count(0), 1);
        assert_eq!(digit_count(9), 1);
        assert_eq!(digit_count(10), 2);
        assert_eq!(digit_count(99), 2);
        assert_eq!(digit_count(100), 3);
    }

    #[test]
    fn pos_width_counts_line_colon_character() {
        // "1:0" => 1 + 1 + 1
        assert_eq!(pos_width(1, 0), 3);
        // "100:5" => 3 + 1 + 1
        assert_eq!(pos_width(100, 5), 5);
    }

    #[test]
    fn url_filename_returns_last_segment_or_empty() {
        assert_eq!(url_filename(&uri()), "Proof.lean");
        let root = Url::parse("file:///").unwrap();
        assert_eq!(url_filename(&root), "");
    }

    #[test]
    fn format_range_is_one_indexed_lines() {
        assert_eq!(format_range(&range(0, 0, 0, 5)), "1:0-1:5");
        assert_eq!(format_range(&range(2, 3, 4, 1)), "3:3-5:1");
    }

    // ── DisplayLspParams impls ────────────────────────────────────────

    #[test]
    fn unit_display_is_empty() {
        assert_eq!(().display().to_string(), "");
    }

    #[test]
    fn did_open_display() {
        let p = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri(),
                language_id: "lean4".to_string(),
                version: 3,
                text: "abc".to_string(),
            },
        };
        assert_eq!(p.display().to_string(), "open Proof.lean v3 (3 chars)");
    }

    #[test]
    fn did_close_and_save_display() {
        let close = DidCloseTextDocumentParams {
            text_document: tdi(),
        };
        assert_eq!(close.display().to_string(), "close Proof.lean");

        let save = DidSaveTextDocumentParams {
            text_document: tdi(),
            text: None,
        };
        assert_eq!(save.display().to_string(), "save Proof.lean");
    }

    #[test]
    fn document_symbol_and_semantic_tokens_display_filename() {
        let ds = DocumentSymbolParams {
            text_document: tdi(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        assert_eq!(ds.display().to_string(), "Proof.lean");

        let st = SemanticTokensParams {
            text_document: tdi(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        assert_eq!(st.display().to_string(), "Proof.lean");
    }

    #[test]
    fn did_change_display_with_ranges_and_full_replace() {
        let with_range = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri(),
                version: 7,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: Some(range(0, 1, 0, 2)),
                range_length: None,
                text: "x".to_string(),
            }],
        };
        let s = with_range.display().to_string();
        assert!(s.starts_with("change Proof.lean v7 (1 edits)"));
        assert!(s.contains("1:1"));
        assert!(s.contains("\"x\""));

        let full = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri(),
                version: 1,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "abcd".to_string(),
            }],
        };
        assert!(full
            .display()
            .to_string()
            .contains("(full replace, 4 chars)"));
    }

    #[test]
    fn position_param_displays_use_one_indexed_line() {
        let hover = HoverParams {
            text_document_position_params: tdp(4, 2),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        assert_eq!(hover.display().to_string(), "Proof.lean 5:2");

        let completion = CompletionParams {
            text_document_position: tdp(0, 0),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };
        assert_eq!(completion.display().to_string(), "Proof.lean 1:0");

        let goto = GotoDefinitionParams {
            text_document_position_params: tdp(1, 1),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        assert_eq!(goto.display().to_string(), "Proof.lean 2:1");

        assert_eq!(tdp(9, 3).display().to_string(), "Proof.lean 10:3");
    }

    #[test]
    fn code_action_params_and_code_action_display() {
        let params = CodeActionParams {
            text_document: tdi(),
            range: range(0, 0, 0, 4),
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        assert_eq!(params.display().to_string(), "Proof.lean 1:0-1:4");

        let action = CodeAction {
            title: "apply omega".to_string(),
            ..Default::default()
        };
        assert_eq!(action.display().to_string(), "apply omega");
    }

    #[test]
    fn show_and_log_message_display() {
        let show = ShowMessageParams {
            typ: MessageType::ERROR,
            message: "boom".to_string(),
        };
        assert_eq!(show.display().to_string(), "showMessage Error: boom");

        let log = LogMessageParams {
            typ: MessageType::INFO,
            message: "fyi".to_string(),
        };
        assert_eq!(log.display().to_string(), "logMessage Info: fyi");
    }

    fn fpp(version: Option<i32>, intervals: Vec<FileProgressInterval>) -> FileProgressParams {
        FileProgressParams {
            text_document: FileProgressTextDocument {
                uri: uri(),
                version,
            },
            processing: intervals,
        }
    }

    #[test]
    fn file_progress_version_accessor() {
        assert_eq!(fpp(Some(9), vec![]).version(), Some(9));
        assert_eq!(fpp(None, vec![]).version(), None);
    }

    #[test]
    fn file_progress_display_versioned_empty() {
        let p = fpp(Some(2), vec![]);
        assert_eq!(
            p.display().to_string(),
            "fileProgress Proof.lean v2 (0 ranges)"
        );
    }

    #[test]
    fn file_progress_display_unversioned_empty() {
        let p = fpp(None, vec![]);
        assert_eq!(
            p.display().to_string(),
            "fileProgress Proof.lean (0 ranges)"
        );
    }

    #[test]
    fn file_progress_display_aligns_columns_across_intervals() {
        // Two intervals with different start-position widths exercise the
        // padding branch (col_w - pos_width).
        let p = fpp(
            Some(1),
            vec![
                FileProgressInterval {
                    range: range(0, 0, 5, 0),
                },
                FileProgressInterval {
                    range: range(99, 5, 120, 0),
                },
            ],
        );
        let s = p.display().to_string();
        assert!(s.contains("(2 ranges)"));
        // 1-indexed start lines.
        assert!(s.contains("1:0"));
        assert!(s.contains("100:5"));
        // End positions are also 1-indexed.
        assert!(s.contains("6:0"));
        assert!(s.contains("121:0"));
    }

    fn diag(severity: Option<DiagnosticSeverity>, code: Option<NumberOrString>) -> Diagnostic {
        Diagnostic {
            range: range(0, 0, 0, 3),
            severity,
            code,
            message: "msg".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn publish_diagnostics_display_all_severities() {
        let p = PublishDiagnosticsParams {
            uri: uri(),
            version: Some(4),
            diagnostics: vec![
                diag(Some(DiagnosticSeverity::ERROR), None),
                diag(Some(DiagnosticSeverity::WARNING), None),
                diag(Some(DiagnosticSeverity::INFORMATION), None),
                diag(Some(DiagnosticSeverity::HINT), None),
                diag(None, None),
            ],
        };
        let s = p.display().to_string();
        assert!(s.starts_with("diagnostics Proof.lean v4 ["));
        assert!(s.contains("error"));
        assert!(s.contains("warning"));
        assert!(s.contains("info"));
        assert!(s.contains("hint"));
        assert!(s.contains("unknown"));
        // Multiple entries are comma-separated.
        assert!(s.contains(", "));
        assert!(s.ends_with(']'));
    }

    #[test]
    fn publish_diagnostics_display_unversioned_with_codes() {
        let p = PublishDiagnosticsParams {
            uri: uri(),
            version: None,
            diagnostics: vec![
                diag(
                    Some(DiagnosticSeverity::ERROR),
                    Some(NumberOrString::Number(42)),
                ),
                diag(
                    Some(DiagnosticSeverity::WARNING),
                    Some(NumberOrString::String("lint".to_string())),
                ),
            ],
        };
        let s = p.display().to_string();
        assert!(s.starts_with("diagnostics Proof.lean ["));
        assert!(s.contains(":42"));
        assert!(s.contains(":lint"));
    }

    #[test]
    fn publish_diagnostics_display_with_code_description() {
        let mut d = diag(Some(DiagnosticSeverity::ERROR), None);
        d.code_description = Some(lsp_types::CodeDescription {
            href: Url::parse("https://example.com/e1").unwrap(),
        });
        let p = PublishDiagnosticsParams {
            uri: uri(),
            version: None,
            diagnostics: vec![d],
        };
        assert!(p.display().to_string().contains("https://example.com/e1"));
    }

    #[test]
    fn lean_message_display_covers_every_variant() {
        assert!(LeanMessage::Diagnostics(PublishDiagnosticsParams {
            uri: uri(),
            version: None,
            diagnostics: vec![],
        })
        .to_string()
        .starts_with("diagnostics"));
        assert!(LeanMessage::FileProgress(fpp(None, vec![]))
            .to_string()
            .starts_with("fileProgress"));
        assert!(LeanMessage::LogMessage(LogMessageParams {
            typ: MessageType::LOG,
            message: "l".to_string(),
        })
        .to_string()
        .starts_with("logMessage"));
        assert!(LeanMessage::ShowMessage(ShowMessageParams {
            typ: MessageType::WARNING,
            message: "w".to_string(),
        })
        .to_string()
        .starts_with("showMessage"));
        assert_eq!(LeanMessage::TokenRefresh.to_string(), "tokenRefresh");
    }
}
