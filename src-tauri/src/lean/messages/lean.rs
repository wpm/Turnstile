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
                    "{} {}:{} \u{2013} {}:{}",
                    url_filename(&self.0.text_document.uri),
                    self.0.range.start.line + 1,
                    self.0.range.start.character,
                    self.0.range.end.line + 1,
                    self.0.range.end.character
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
                match self.0.version {
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
                let items: Vec<String> = self
                    .0
                    .diagnostics
                    .iter()
                    .map(|d| {
                        let pos = format!("{}:{}", d.range.start.line + 1, d.range.start.character);
                        match &d.code {
                            Some(lsp_types::NumberOrString::Number(n)) => format!("{pos} {n}"),
                            Some(lsp_types::NumberOrString::String(s)) => format!("{pos} {s}"),
                            None => pos,
                        }
                    })
                    .collect();
                write!(f, " [{}]", items.join(", "))
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
