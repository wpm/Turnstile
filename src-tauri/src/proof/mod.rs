//! The central domain type: a [`Proof`] is one proof viewed two ways — as
//! formal Lean source and as textbook-style prose — together with the current
//! goal state that Lean reports while elaborating it.
//!
//! The proof travels through the app as a single unit: [`AppState`](crate::AppState)
//! holds an `Arc<Mutex<Proof>>`, sessions serialize it as [`Proof`] via
//! [`crate::session::SessionState`], and [`translator`] generates prose from
//! formal.

pub mod translator;

use serde::{Deserialize, Serialize};

use crate::lsp::{DiagnosticInfo, SemanticToken};

/// Semantic token type, mirroring the LSP token type legend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenType {
    Namespace,
    Type,
    Class,
    Enum,
    Interface,
    Struct,
    TypeParameter,
    Parameter,
    Variable,
    Property,
    EnumMember,
    Event,
    Function,
    Method,
    Macro,
    Keyword,
    Modifier,
    Comment,
    String,
    Number,
    Regexp,
    Operator,
    Decorator,
    #[serde(other)]
    Unknown,
}

impl std::str::FromStr for TokenType {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "namespace" => Self::Namespace,
            "type" => Self::Type,
            "class" => Self::Class,
            "enum" => Self::Enum,
            "interface" => Self::Interface,
            "struct" => Self::Struct,
            "typeParameter" => Self::TypeParameter,
            "parameter" => Self::Parameter,
            "variable" => Self::Variable,
            "property" => Self::Property,
            "enumMember" => Self::EnumMember,
            "event" => Self::Event,
            "function" => Self::Function,
            "method" => Self::Method,
            "macro" => Self::Macro,
            "keyword" => Self::Keyword,
            "modifier" => Self::Modifier,
            "comment" => Self::Comment,
            "string" => Self::String,
            "number" => Self::Number,
            "regexp" => Self::Regexp,
            "operator" => Self::Operator,
            "decorator" => Self::Decorator,
            _ => Self::Unknown,
        })
    }
}

/// Diagnostic severity level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

impl DiagnosticSeverity {
    #[must_use]
    pub const fn from_u8(n: u8) -> Self {
        match n {
            1 => Self::Error,
            2 => Self::Warning,
            3 => Self::Info,
            _ => Self::Hint,
        }
    }
}

/// A span annotation on the formal proof source.
///
/// Either a semantic token (for syntax highlighting) or a diagnostic
/// (for error/warning squiggles). All line numbers are 1-indexed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Annotation {
    #[serde(rename_all = "camelCase")]
    Token {
        line: u32,
        col: u32,
        length: u32,
        token_type: TokenType,
        modifiers: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Diagnostic {
        start_line: u32,
        start_col: u32,
        end_line: u32,
        end_col: u32,
        severity: DiagnosticSeverity,
        message: String,
    },
}

/// The complete set of span annotations on the formal proof, as last
/// reported by the LSP. Tokens and diagnostics are stored together and
/// replaced independently when new LSP responses arrive.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotations {
    pub items: Vec<Annotation>,
}

impl Annotations {
    /// Replace all `Token` annotations with those derived from `tokens`,
    /// leaving `Diagnostic` annotations untouched.
    pub fn set_tokens(&mut self, tokens: &[SemanticToken]) {
        self.items
            .retain(|a| !matches!(a, Annotation::Token { .. }));
        self.items.extend(tokens.iter().map(|t| Annotation::Token {
            line: t.line,
            col: t.col,
            length: t.length,
            token_type: t.token_type.parse().unwrap_or(TokenType::Unknown),
            modifiers: t.token_modifiers.clone(),
        }));
    }

    /// Replace all `Diagnostic` annotations with those derived from `diags`,
    /// leaving `Token` annotations untouched.
    pub fn set_diagnostics(&mut self, diags: &[DiagnosticInfo]) {
        self.items
            .retain(|a| !matches!(a, Annotation::Diagnostic { .. }));
        self.items
            .extend(diags.iter().map(|d| Annotation::Diagnostic {
                start_line: d.start_line,
                start_col: d.start_col,
                end_line: d.end_line,
                end_col: d.end_col,
                severity: DiagnosticSeverity::from_u8(d.severity),
                message: d.message.clone(),
            }));
    }
}

/// The Lean source buffer.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FormalProof {
    pub source: String,
}

/// The prose proof draft, tagged with the hash of the goal state that
/// produced it.  When the goal-state hash diverges from the current goal
/// state, the prose is stale and a regeneration is queued.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProseProof {
    pub text: String,
    /// Hash of the goal state that produced this prose.  Empty if no prose
    /// has been generated yet.
    pub goal_state_hash: String,
}

/// The goal state reported by the Lean LSP while elaborating the formal proof.
///
/// `full` is what Lean reports at the end of the document (the "whole-proof"
/// goal state, independent of cursor position).
///
/// Populated on-demand and delivered to the UI via the
/// [`GOAL_STATE_UPDATED_EVENT`] Tauri event.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalState {
    pub full: String,
}

/// A proof represented both formally (Lean) and in prose, together with the
/// live goal state and span annotations from the LSP.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Proof {
    pub formal: FormalProof,
    pub prose: ProseProof,
    pub goal_state: GoalState,
    pub annotations: Annotations,
}

/// Payload for the [`PROSE_UPDATED_EVENT`] Tauri event.
///
/// `hash` is `Some` when the prose was generated from a specific formal
/// source; `None` when the prose was written without a source reference
/// (e.g. a direct `update_prose_proof` tool call).
#[derive(Clone, Debug, Serialize)]
pub struct ProsePayload {
    pub text: String,
    pub hash: Option<String>,
}

/// Emitted when the prose draft changes — by the translator, by a PA tool
/// call, or by a session load.  Payload: [`ProsePayload`].
pub const PROSE_UPDATED_EVENT: &str = "prose-updated";

/// Emitted when the whole-proof goal state has been refreshed.
pub const GOAL_STATE_UPDATED_EVENT: &str = "goal-state-updated";

/// Emitted when span annotations (tokens or diagnostics) change.
/// Payload: the full [`Vec<Annotation>`] after the update.
pub const ANNOTATIONS_UPDATED_EVENT: &str = "annotations-updated";

/// Compute a fast hash of a string for change detection (not cryptographic).
#[must_use]
pub fn compute_source_hash(source: &str) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_hash_is_deterministic() {
        let h1 = compute_source_hash("theorem foo : True := by trivial");
        let h2 = compute_source_hash("theorem foo : True := by trivial");
        assert_eq!(h1, h2);
    }

    #[test]
    fn source_hash_differs_for_different_input() {
        let h1 = compute_source_hash("theorem foo : True := by trivial");
        let h2 = compute_source_hash("theorem bar : True := by trivial");
        assert_ne!(h1, h2);
    }

    #[test]
    fn source_hash_is_16_hex_chars() {
        let h = compute_source_hash("hello");
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn source_hash_of_empty_string_is_valid_hex() {
        let h = compute_source_hash("");
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn source_hash_distinguishes_whitespace_variants() {
        let h_empty = compute_source_hash("");
        let h_space = compute_source_hash(" ");
        let h_newline = compute_source_hash("\n");
        assert_ne!(h_empty, h_space);
        assert_ne!(h_empty, h_newline);
        assert_ne!(h_space, h_newline);
    }

    #[test]
    fn proof_default_has_empty_fields() {
        let p = Proof::default();
        assert!(p.formal.source.is_empty());
        assert!(p.prose.text.is_empty());
        assert!(p.prose.goal_state_hash.is_empty());
        assert!(p.goal_state.full.is_empty());
    }

    #[test]
    fn annotation_serializes_with_camel_case_fields() {
        let token = Annotation::Token {
            line: 1,
            col: 0,
            length: 7,
            token_type: TokenType::Keyword,
            modifiers: vec![],
        };
        let diag = Annotation::Diagnostic {
            start_line: 1,
            start_col: 7,
            end_line: 1,
            end_col: 7,
            severity: DiagnosticSeverity::Error,
            message: "expected identifier".into(),
        };
        let token_json = serde_json::to_string(&token).unwrap();
        let diag_json = serde_json::to_string(&diag).unwrap();
        assert!(
            token_json.contains("\"tokenType\""),
            "token_type must serialize as tokenType, got: {token_json}"
        );
        assert!(
            !token_json.contains("token_type"),
            "must not contain snake_case token_type"
        );
        assert!(
            diag_json.contains("\"startLine\""),
            "start_line must serialize as startLine, got: {diag_json}"
        );
        assert!(
            diag_json.contains("\"endLine\""),
            "end_line must serialize as endLine, got: {diag_json}"
        );
        assert!(
            !diag_json.contains("start_line"),
            "must not contain snake_case start_line"
        );
    }

    #[test]
    fn proof_round_trips_through_json() {
        let proof = Proof {
            formal: FormalProof {
                source: "theorem foo : True := trivial".into(),
            },
            prose: ProseProof {
                text: "This proves True.".into(),
                goal_state_hash: "abc".into(),
            },
            goal_state: GoalState {
                full: "⊢ True".into(),
            },
            annotations: Annotations::default(),
        };
        let json = serde_json::to_string(&proof).unwrap();
        let restored: Proof = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, proof);
    }
}
