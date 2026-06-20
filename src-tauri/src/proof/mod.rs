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
use ts_rs::TS;

use crate::lean::messages::turnstile::{DiagnosticInfo, SemanticToken};

/// Semantic token type, mirroring the LSP token type legend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/lib/")]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/lib/")]
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

/// The Lean LSP's view of the formal proof's visual annotations: the two
/// producer-shaped lists it delivers, kept as the single source of truth for
/// everything the editor renders over the source.
///
/// The LSP produces two distinct atom types on two independent schedules —
/// [`SemanticToken`]s (syntax colouring) via `semanticTokens`, and
/// [`DiagnosticInfo`]s (error/warning underlines) via `publishDiagnostics`. We
/// store them exactly as produced (1-indexed line/col, UTF-16 columns) and
/// derive every CodeMirror-facing shape on demand via [`LSPAnnotation::derive`].
///
/// Tokens and diagnostics refresh independently: assigning `tokens` leaves
/// `diagnostics` untouched and vice versa, which falls out structurally from
/// them being separate fields rather than a filtered sum type.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LSPAnnotation {
    pub tokens: Vec<SemanticToken>,
    pub diagnostics: Vec<DiagnosticInfo>,
}

impl LSPAnnotation {
    /// Resolve every annotation into `CodeMirror`-ready structures.
    ///
    /// Offsets are computed in UTF-16 code units against `source` —
    /// `CodeMirror`'s native coordinate system. This is the single producer of
    /// everything the editor renders; the frontend is a pure renderer that maps
    /// these straight onto decorations, gutter markers, and tooltips.
    #[must_use]
    pub fn derive(&self, source: &str) -> DerivedAnnotations {
        // Precompute the UTF-16 offset of the start of each line once, then share
        // it across every resolution below so the document is scanned a single
        // time per refresh.
        let line_starts = utf16_line_starts(source);

        let syntax_coloring = self
            .tokens
            .iter()
            .map(|t| {
                let from = resolve_offset(&line_starts, t.line, t.col);
                SyntaxColor {
                    from,
                    to: from + t.length,
                    token_type: t.token_type.parse().unwrap_or(TokenType::Unknown),
                }
            })
            .collect();

        let underlines = self
            .diagnostics
            .iter()
            .map(|d| Underline {
                from: resolve_offset(&line_starts, d.start_line, d.start_col),
                to: resolve_offset(&line_starts, d.end_line, d.end_col),
                severity: DiagnosticSeverity::from_u8(d.severity),
            })
            .collect();

        DerivedAnnotations {
            syntax_coloring,
            underlines,
            gutter: self.gutter(),
            hover: self.hover(&line_starts),
        }
    }

    /// One gutter mark per line that has a diagnostic, deduped so the most
    /// severe wins (error over warning). Info and hint diagnostics get no
    /// gutter mark. Lines stay 1-indexed; the gutter addresses whole lines, so
    /// no offset resolution is needed.
    fn gutter(&self) -> Vec<GutterMark> {
        let mut by_line: std::collections::BTreeMap<u32, DiagnosticSeverity> =
            std::collections::BTreeMap::new();
        for d in &self.diagnostics {
            let severity = DiagnosticSeverity::from_u8(d.severity);
            if !matches!(
                severity,
                DiagnosticSeverity::Error | DiagnosticSeverity::Warning
            ) {
                continue;
            }
            by_line
                .entry(d.start_line)
                .and_modify(|cur| {
                    if severity == DiagnosticSeverity::Error {
                        *cur = DiagnosticSeverity::Error;
                    }
                })
                .or_insert(severity);
        }
        by_line
            .into_iter()
            .map(|(line, severity)| GutterMark { line, severity })
            .collect()
    }

    /// One hover hit per diagnostic, with its span resolved to UTF-16 offsets
    /// against the shared `line_starts`. The renderer tests the cursor position
    /// against each hit's `[from, to]` and unions the messages of all hits under
    /// the cursor.
    fn hover(&self, line_starts: &[u32]) -> Vec<HoverHit> {
        self.diagnostics
            .iter()
            .map(|d| HoverHit {
                from: resolve_offset(line_starts, d.start_line, d.start_col),
                to: resolve_offset(line_starts, d.end_line, d.end_col),
                severity: DiagnosticSeverity::from_u8(d.severity),
                message: d.message.clone(),
            })
            .collect()
    }
}

/// One syntax-colouring span: a half-open `[from, to)` UTF-16 offset range and
/// the token type that selects its CSS class.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../src/lib/")]
#[serde(rename_all = "camelCase")]
pub struct SyntaxColor {
    pub from: u32,
    pub to: u32,
    pub token_type: TokenType,
}

/// One diagnostic underline: a half-open `[from, to)` UTF-16 offset range and
/// the severity that selects its CSS class.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../src/lib/")]
#[serde(rename_all = "camelCase")]
pub struct Underline {
    pub from: u32,
    pub to: u32,
    pub severity: DiagnosticSeverity,
}

/// One gutter mark, addressing a whole (1-indexed) line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../src/lib/")]
#[serde(rename_all = "camelCase")]
pub struct GutterMark {
    pub line: u32,
    pub severity: DiagnosticSeverity,
}

/// One diagnostic hover target: a `[from, to]` UTF-16 offset span and the
/// message to show when the cursor falls within it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../src/lib/")]
#[serde(rename_all = "camelCase")]
pub struct HoverHit {
    pub from: u32,
    pub to: u32,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

/// The complete set of `CodeMirror`-ready annotation views.
///
/// Derived from an [`LSPAnnotation`] against a specific source. This is the
/// payload the frontend renders; it carries no line/col, only resolved offsets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../src/lib/")]
#[serde(rename_all = "camelCase")]
pub struct DerivedAnnotations {
    pub syntax_coloring: Vec<SyntaxColor>,
    pub underlines: Vec<Underline>,
    pub gutter: Vec<GutterMark>,
    pub hover: Vec<HoverHit>,
}

/// The UTF-16 offset of the start of each line in `source`, indexed 0-based by
/// line number (so element 0 is line 1's start, always 0).
///
/// `CodeMirror` addresses the document in UTF-16 code units, and the LSP reports
/// columns in UTF-16, so resolving a (line, col) to an absolute offset is
/// `line_starts[line-1] + col`.
fn utf16_line_starts(source: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    let mut offset = 0u32;
    for ch in source.chars() {
        offset += u32::try_from(ch.len_utf16()).unwrap_or(2);
        if ch == '\n' {
            starts.push(offset);
        }
    }
    starts
}

/// Resolve a 1-indexed (line, col) to an absolute UTF-16 offset using the
/// precomputed `line_starts`. A line past the end clamps to the last line start.
///
/// Only the line start is bounded; `col` (and any `+ length` a caller adds) is
/// trusted as the LSP reported it, so the returned offset — and especially a
/// caller's `to` — may point past the end of the document if the annotation was
/// computed against a since-edited source. The frontend renderer clamps every
/// span to the live document before use, so out-of-range offsets are dropped
/// rather than trusted.
fn resolve_offset(line_starts: &[u32], line: u32, col: u32) -> u32 {
    line_starts
        .get((line as usize).saturating_sub(1))
        .map_or_else(|| line_starts.last().copied().unwrap_or(0), |s| s + col)
}

/// The Lean source buffer together with the LSP annotations that index into it.
///
/// Co-locating them makes "the text and its annotations are one coupled unit"
/// structural: annotations are always resolved against this exact `source`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FormalProof {
    pub source: String,
    pub annotations: LSPAnnotation,
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

/// Status of a row's cached goal (ADR-0004). Display renders a card for a row
/// **iff** its status is `Fresh`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/lib/")]
#[serde(rename_all = "camelCase")]
pub enum GoalRowStatus {
    /// Queried and current: the row's after-state is the `goals` it carries.
    Fresh,
    /// Invalidated by an edit at or above this row; awaiting re-query. A card
    /// on a stale row hides immediately, so no goal flickers mid-keystroke.
    #[default]
    Stale,
    /// Queried, but the row has no open goal — a non-tactic line, or a line
    /// that closed the proof. No card (a closed proof runs out of goals).
    Empty,
}

/// One open goal in structured form (ADR-0004 decision 6).
///
/// An optional `case` label, its hypotheses, and the `⊢` conclusion. Parsed
/// once in the backend from a `$/lean/plainGoal` `goals[]` entry, so the
/// frontend renders structure rather than re-splitting text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/lib/")]
#[serde(rename_all = "camelCase")]
pub struct GoalEntry {
    pub case_label: Option<String>,
    pub hypotheses: Vec<String>,
    pub conclusion: String,
}

/// A row's cached goal: its `status` and, when `Fresh`, its after-state goals.
///
/// The goals are taken at the row's **trailing** position. There is more than
/// one only in the brief unfocused window after a splitting tactic — the `+N`
/// moment. Internal cache storage; the wire shape is [`GoalCacheRow`] (which
/// adds the row number). Not exported to TypeScript.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowGoal {
    pub status: GoalRowStatus,
    pub goals: Vec<GoalEntry>,
}

impl RowGoal {
    /// Mark this row stale and drop any goal it was carrying, so a stale row
    /// never serializes a goal the card could render.
    fn mark_stale(&mut self) {
        self.status = GoalRowStatus::Stale;
        self.goals.clear();
    }
}

/// One row in a [`GoalCacheInfo`] snapshot: a 1-indexed line number with its
/// cached status and goals. Flattened from the cache for the wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../src/lib/")]
#[serde(rename_all = "camelCase")]
pub struct GoalCacheRow {
    /// 1-indexed line number (matching the `CodeMirror` / frontend convention).
    pub row: u32,
    pub status: GoalRowStatus,
    pub goals: Vec<GoalEntry>,
}

/// A full snapshot of the per-row goal cache (ADR-0004).
///
/// Delivered to the UI via
/// [`crate::lean::messages::turnstile::TurnstileMessage::GoalCacheUpdated`];
/// display is a pure function of `(cursor row, this snapshot)`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../src/lib/")]
pub struct GoalCacheInfo {
    /// Cached rows, ascending. `Stale`/`Empty` rows are included so the
    /// frontend can drop a card the instant its row goes stale.
    pub rows: Vec<GoalCacheRow>,
}

/// The goal state reported by the Lean LSP while elaborating the formal proof.
///
/// - `full` is what Lean reports at the end of the document (the "whole-proof"
///   goal state). It is retained as the input to **prose generation** and the
///   **Proof Assistant**, which summarize the whole proof, not a single row.
/// - `rows` is the per-row goal cache (ADR-0004): a 1-indexed row → after-state
///   map. Acquisition is edit-driven and `fileProgress`-gated; display is a
///   pure function of `(cursor row, rows)`.
///
/// Delivered to the UI via [`crate::lean::messages::turnstile::TurnstileMessage`]
/// (`GoalStateUpdated` for `full`, `GoalCacheUpdated` for `rows`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalState {
    pub full: String,
    pub rows: std::collections::BTreeMap<u32, RowGoal>,
}

impl GoalState {
    /// Mark rows `[from, to]` (1-indexed, inclusive) stale and drop any cached
    /// row beyond `to` (the document shrank past it). **Downward-inclusive**:
    /// an edit at row `from` stales `[from, to]` and never anything above,
    /// because every tactic depends on the ones before it.
    pub fn mark_stale(&mut self, from: u32, to: u32) {
        self.rows.retain(|&row, _| row <= to);
        for row in from..=to {
            self.rows.entry(row).or_default().mark_stale();
        }
    }

    /// The stale rows, ascending — exactly the set to re-query once
    /// `fileProgress` reports the changed tail complete.
    #[must_use]
    pub fn stale_rows(&self) -> Vec<u32> {
        self.rows
            .iter()
            .filter(|(_, r)| r.status == GoalRowStatus::Stale)
            .map(|(&row, _)| row)
            .collect()
    }

    /// Store a row's queried after-state. Empty `goals` records the row as
    /// `Empty` (a non-tactic or proof-closing line); a non-empty list as
    /// `Fresh`.
    pub fn set_row(&mut self, row: u32, goals: Vec<GoalEntry>) {
        let status = if goals.is_empty() {
            GoalRowStatus::Empty
        } else {
            GoalRowStatus::Fresh
        };
        self.rows.insert(row, RowGoal { status, goals });
    }

    /// Clear the per-row cache (the formal proof was deleted).
    pub fn clear_rows(&mut self) {
        self.rows.clear();
    }

    /// Flatten the cache into a wire snapshot, rows ascending.
    #[must_use]
    pub fn snapshot(&self) -> GoalCacheInfo {
        GoalCacheInfo {
            rows: self
                .rows
                .iter()
                .map(|(&row, r)| GoalCacheRow {
                    row,
                    status: r.status,
                    goals: r.goals.clone(),
                })
                .collect(),
        }
    }
}

/// Parse one `$/lean/plainGoal` `goals[]` entry into structured form.
///
/// Splits into an optional leading `case` label, the hypothesis lines above the
/// `⊢`, and the conclusion after it. A single entry is always one goal, so
/// unlike the old `parseGoalText` this never splits on blank lines.
#[must_use]
pub fn parse_goal_entry(entry: &str) -> GoalEntry {
    let mut lines: Vec<&str> = entry.split('\n').collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }

    let mut case_label = None;
    if lines.len() > 1 && lines[0].starts_with("case ") {
        case_label = Some(lines[0].trim().to_string());
        lines.remove(0);
    }

    let Some(goal_idx) = lines.iter().position(|l| l.trim_start().starts_with('⊢')) else {
        return GoalEntry {
            case_label,
            hypotheses: Vec::new(),
            conclusion: entry.trim().to_string(),
        };
    };

    let hypotheses = lines[..goal_idx]
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| (*l).to_string())
        .collect();

    let joined = lines[goal_idx..].join("\n");
    let conclusion = joined
        .find('⊢')
        .map_or_else(
            || joined.trim().to_string(),
            |pos| joined[pos + '⊢'.len_utf8()..].to_string(),
        )
        .trim_start_matches([' ', '\t'])
        .to_string();

    GoalEntry {
        case_label,
        hypotheses,
        conclusion,
    }
}

/// Parse a `$/lean/plainGoal` `goals[]` list into structured goals.
#[must_use]
pub fn parse_goal_entries(goals: &[String]) -> Vec<GoalEntry> {
    goals.iter().map(|g| parse_goal_entry(g)).collect()
}

/// A proof represented both formally (Lean) and in prose, together with the
/// live goal state and span annotations from the LSP.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Proof {
    pub formal: FormalProof,
    pub prose: ProseProof,
    pub goal_state: GoalState,
}

impl Proof {
    /// Clear everything derived from the formal source: the goal state, the
    /// prose proof, and the prose's source hash. Used when the formal proof is
    /// deleted, so the goal-state and prose panels don't keep showing output
    /// for source that no longer exists. Leaves `formal` untouched (the caller
    /// owns the source; the LSP annotations within it clear via the LSP).
    pub fn clear_derived(&mut self) {
        self.goal_state.full.clear();
        self.goal_state.clear_rows();
        self.prose.text.clear();
        self.prose.goal_state_hash.clear();
    }
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

/// Drives the Prose Proof busy indicator.
///
/// Emitted around a prose-translation LLM call so the UI can show a busy
/// indicator. Payload: a bare `bool` (`true` when generation starts, `false`
/// when it finishes — whether it succeeded, failed, or was superseded).
pub const PROSE_GENERATING_EVENT: &str = "prose-generating";

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
    fn clear_derived_empties_goal_and_prose_but_keeps_source() {
        let mut p = Proof::default();
        p.formal.source = "theorem t : True := trivial".to_string();
        p.goal_state.full = "⊢ True".to_string();
        p.goal_state.set_row(1, vec![GoalEntry::default()]);
        p.prose.text = "It is trivially true.".to_string();
        p.prose.goal_state_hash = "abc123".to_string();

        p.clear_derived();

        // Derived state is gone — the panels will show nothing.
        assert!(p.goal_state.full.is_empty());
        assert!(p.goal_state.rows.is_empty());
        assert!(p.prose.text.is_empty());
        assert!(p.prose.goal_state_hash.is_empty());
        // The formal source is the caller's; clear_derived must not touch it.
        assert_eq!(p.formal.source, "theorem t : True := trivial");
    }

    // ── Per-row goal cache (ADR-0004) ──────────────────────────────────

    #[test]
    fn parse_goal_entry_plain_conclusion() {
        let g = parse_goal_entry("⊢ True");
        assert_eq!(g.case_label, None);
        assert!(g.hypotheses.is_empty());
        assert_eq!(g.conclusion, "True");
    }

    #[test]
    fn parse_goal_entry_hypotheses_above_turnstile() {
        let g = parse_goal_entry("h : P\nh2 : Q\n⊢ P ∧ Q");
        assert_eq!(g.hypotheses, vec!["h : P", "h2 : Q"]);
        assert_eq!(g.conclusion, "P ∧ Q");
    }

    #[test]
    fn parse_goal_entry_case_label() {
        let g = parse_goal_entry("case inl\nh : P\n⊢ P ∨ Q");
        assert_eq!(g.case_label.as_deref(), Some("case inl"));
        assert_eq!(g.hypotheses, vec!["h : P"]);
        assert_eq!(g.conclusion, "P ∨ Q");
    }

    #[test]
    fn parse_goal_entry_strips_turnstile_even_when_indented() {
        // The conclusion is cleaned of the `⊢` regardless of leading indent.
        let g = parse_goal_entry("h : P\n  ⊢ P");
        assert_eq!(g.hypotheses, vec!["h : P"]);
        assert_eq!(g.conclusion, "P");
    }

    #[test]
    fn parse_goal_entry_keeps_multiline_conclusion() {
        let g = parse_goal_entry("⊢ (fun x =>\n  x + 1) 0 = 1");
        assert_eq!(g.conclusion, "(fun x =>\n  x + 1) 0 = 1");
    }

    #[test]
    fn parse_goal_entry_lone_case_line_is_not_a_label() {
        let g = parse_goal_entry("case inl");
        assert_eq!(g.case_label, None);
        assert_eq!(g.conclusion, "case inl");
    }

    #[test]
    fn parse_goal_entries_maps_each_entry() {
        let entries = parse_goal_entries(&["⊢ P".to_string(), "h : Q\n⊢ Q".to_string()]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].conclusion, "P");
        assert_eq!(entries[1].hypotheses, vec!["h : Q"]);
    }

    #[test]
    fn mark_stale_is_downward_inclusive() {
        let mut gs = GoalState::default();
        gs.set_row(1, vec![GoalEntry::default()]);
        gs.set_row(2, vec![GoalEntry::default()]);
        gs.set_row(3, vec![GoalEntry::default()]);

        // Editing row 2 stales [2, 3] and leaves row 1 fresh.
        gs.mark_stale(2, 3);
        assert_eq!(gs.rows[&1].status, GoalRowStatus::Fresh);
        assert_eq!(gs.rows[&2].status, GoalRowStatus::Stale);
        assert_eq!(gs.rows[&3].status, GoalRowStatus::Stale);
        assert_eq!(gs.stale_rows(), vec![2, 3]);
    }

    #[test]
    fn mark_stale_drops_rows_past_the_new_end() {
        let mut gs = GoalState::default();
        gs.set_row(1, vec![GoalEntry::default()]);
        gs.set_row(5, vec![GoalEntry::default()]);

        // The document shrank to 3 lines: row 5 is gone, not lingering.
        gs.mark_stale(1, 3);
        assert!(!gs.rows.contains_key(&5));
        assert_eq!(gs.rows.keys().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn mark_stale_clears_the_rows_goal() {
        let mut gs = GoalState::default();
        gs.set_row(1, vec![GoalEntry::default()]);
        gs.mark_stale(1, 1);
        assert!(gs.rows[&1].goals.is_empty());
    }

    #[test]
    fn set_row_empty_goals_is_empty_status() {
        let mut gs = GoalState::default();
        gs.set_row(1, Vec::new());
        assert_eq!(gs.rows[&1].status, GoalRowStatus::Empty);
        assert!(gs.stale_rows().is_empty());
    }

    #[test]
    fn snapshot_is_sorted_by_row() {
        let mut gs = GoalState::default();
        gs.set_row(3, vec![GoalEntry::default()]);
        gs.set_row(1, vec![GoalEntry::default()]);
        let snap = gs.snapshot();
        assert_eq!(
            snap.rows.iter().map(|r| r.row).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(snap.rows[0].status, GoalRowStatus::Fresh);
    }

    #[test]
    fn derived_annotations_serialize_camel_case() {
        let derived = DerivedAnnotations {
            syntax_coloring: vec![SyntaxColor {
                from: 0,
                to: 7,
                token_type: TokenType::Keyword,
            }],
            underlines: vec![Underline {
                from: 8,
                to: 11,
                severity: DiagnosticSeverity::Error,
            }],
            gutter: vec![GutterMark {
                line: 1,
                severity: DiagnosticSeverity::Error,
            }],
            hover: vec![HoverHit {
                from: 8,
                to: 11,
                severity: DiagnosticSeverity::Error,
                message: "boom".into(),
            }],
        };
        let json = serde_json::to_string(&derived).unwrap();
        // The wire shape the renderer consumes must be camelCase.
        assert!(
            json.contains("\"syntaxColoring\""),
            "must serialize syntax_coloring as syntaxColoring, got: {json}"
        );
        assert!(
            json.contains("\"tokenType\""),
            "must serialize token_type as tokenType, got: {json}"
        );
        assert!(
            !json.contains("syntax_coloring") && !json.contains("token_type"),
            "must not contain snake_case field names, got: {json}"
        );
    }

    #[test]
    fn token_type_from_str_maps_every_legend_name() {
        let cases = [
            ("namespace", TokenType::Namespace),
            ("type", TokenType::Type),
            ("class", TokenType::Class),
            ("enum", TokenType::Enum),
            ("interface", TokenType::Interface),
            ("struct", TokenType::Struct),
            ("typeParameter", TokenType::TypeParameter),
            ("parameter", TokenType::Parameter),
            ("variable", TokenType::Variable),
            ("property", TokenType::Property),
            ("enumMember", TokenType::EnumMember),
            ("event", TokenType::Event),
            ("function", TokenType::Function),
            ("method", TokenType::Method),
            ("macro", TokenType::Macro),
            ("keyword", TokenType::Keyword),
            ("modifier", TokenType::Modifier),
            ("comment", TokenType::Comment),
            ("string", TokenType::String),
            ("number", TokenType::Number),
            ("regexp", TokenType::Regexp),
            ("operator", TokenType::Operator),
            ("decorator", TokenType::Decorator),
        ];
        for (name, expected) in cases {
            assert_eq!(name.parse::<TokenType>().unwrap(), expected);
        }
        // Anything unrecognized falls back to Unknown.
        assert_eq!(
            "totallyBogus".parse::<TokenType>().unwrap(),
            TokenType::Unknown
        );
    }

    #[test]
    fn diagnostic_severity_from_u8_covers_all_levels() {
        assert_eq!(DiagnosticSeverity::from_u8(1), DiagnosticSeverity::Error);
        assert_eq!(DiagnosticSeverity::from_u8(2), DiagnosticSeverity::Warning);
        assert_eq!(DiagnosticSeverity::from_u8(3), DiagnosticSeverity::Info);
        assert_eq!(DiagnosticSeverity::from_u8(4), DiagnosticSeverity::Hint);
        // Out-of-range severities clamp to Hint.
        assert_eq!(DiagnosticSeverity::from_u8(99), DiagnosticSeverity::Hint);
    }

    fn sem_token(line: u32, ty: &str) -> SemanticToken {
        SemanticToken {
            line,
            col: 0,
            length: 3,
            token_type: ty.to_string(),
            token_modifiers: vec!["declaration".to_string()],
        }
    }

    fn diag_info(line: u32, severity: u8) -> DiagnosticInfo {
        DiagnosticInfo {
            start_line: line,
            start_col: 0,
            end_line: line,
            end_col: 3,
            severity,
            message: "m".to_string(),
        }
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)] // field-by-field assignment is the point
    fn tokens_and_diagnostics_assign_independently() {
        // Assigning one list leaves the other untouched — the independence the
        // old filtered sum type simulated is now structural.
        let mut anns = LSPAnnotation::default();
        anns.diagnostics = vec![diag_info(1, 1)];
        anns.tokens = vec![sem_token(2, "keyword"), sem_token(3, "bogusType")];
        assert_eq!(anns.tokens.len(), 2);
        assert_eq!(anns.diagnostics.len(), 1, "diagnostics survive a token set");

        // A second token assignment replaces, not appends, and leaves diagnostics.
        anns.tokens = vec![sem_token(5, "type")];
        assert_eq!(anns.tokens.len(), 1);
        assert_eq!(anns.diagnostics.len(), 1);
    }

    #[test]
    fn syntax_coloring_resolves_offsets_and_maps_unknown_type() {
        let anns = LSPAnnotation {
            tokens: vec![sem_token(1, "keyword"), sem_token(1, "bogusType")],
            diagnostics: vec![],
        };
        let derived = anns.derive("theorem foo");
        assert_eq!(derived.syntax_coloring.len(), 2);
        // sem_token uses col 0, length 3 → [0, 3).
        assert_eq!(derived.syntax_coloring[0].from, 0);
        assert_eq!(derived.syntax_coloring[0].to, 3);
        assert_eq!(derived.syntax_coloring[0].token_type, TokenType::Keyword);
        // Unrecognized legend names fall back to Unknown.
        assert_eq!(derived.syntax_coloring[1].token_type, TokenType::Unknown);
    }

    #[test]
    fn offsets_count_utf16_code_units_across_multibyte_glyphs() {
        // CodeMirror addresses the document in UTF-16. A token after the
        // 3-byte glyph "∀" (1 UTF-16 unit) must resolve by code units, not
        // bytes. Source: "∀ x" → a token at line 1, col 2, length 1 covers "x".
        let token = SemanticToken {
            line: 1,
            col: 2,
            length: 1,
            token_type: "variable".to_string(),
            token_modifiers: vec![],
        };
        let anns = LSPAnnotation {
            tokens: vec![token],
            diagnostics: vec![],
        };
        let derived = anns.derive("∀ x");
        // "∀"=1 unit, " "=1 unit → x starts at UTF-16 offset 2 (not byte 4).
        assert_eq!(derived.syntax_coloring[0].from, 2);
        assert_eq!(derived.syntax_coloring[0].to, 3);
    }

    #[test]
    fn offsets_resolve_on_later_lines() {
        // Token on line 2 must account for line 1's length plus the newline.
        let token = SemanticToken {
            line: 2,
            col: 0,
            length: 3,
            token_type: "keyword".to_string(),
            token_modifiers: vec![],
        };
        let anns = LSPAnnotation {
            tokens: vec![token],
            diagnostics: vec![],
        };
        // "ab\ncd" → line 2 starts at UTF-16 offset 3 ("ab"=2 + "\n"=1).
        let derived = anns.derive("ab\ncd");
        assert_eq!(derived.syntax_coloring[0].from, 3);
    }

    #[test]
    fn gutter_dedups_error_over_warning_and_excludes_info_hint() {
        let anns = LSPAnnotation {
            tokens: vec![],
            diagnostics: vec![
                diag_info(1, 2), // warning on line 1
                diag_info(1, 1), // error on line 1 — should win
                diag_info(2, 2), // warning on line 2
                diag_info(3, 3), // info — excluded
                diag_info(4, 4), // hint — excluded
            ],
        };
        let derived = anns.derive("a\nb\nc\nd");
        assert_eq!(derived.gutter.len(), 2, "only lines 1 and 2 get marks");
        assert_eq!(derived.gutter[0].line, 1);
        assert_eq!(derived.gutter[0].severity, DiagnosticSeverity::Error);
        assert_eq!(derived.gutter[1].line, 2);
        assert_eq!(derived.gutter[1].severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn hover_carries_each_diagnostic_with_resolved_span() {
        let anns = LSPAnnotation {
            tokens: vec![],
            diagnostics: vec![diag_info(1, 1)],
        };
        let derived = anns.derive("theorem foo");
        assert_eq!(derived.hover.len(), 1);
        assert_eq!(derived.hover[0].from, 0);
        assert_eq!(derived.hover[0].to, 3);
        assert_eq!(derived.hover[0].severity, DiagnosticSeverity::Error);
        assert_eq!(derived.hover[0].message, "m");
    }

    #[test]
    fn proof_round_trips_through_json() {
        let proof = Proof {
            formal: FormalProof {
                source: "theorem foo : True := trivial".into(),
                annotations: LSPAnnotation {
                    tokens: vec![sem_token(1, "keyword")],
                    diagnostics: vec![diag_info(1, 1)],
                },
            },
            prose: ProseProof {
                text: "This proves True.".into(),
                goal_state_hash: "abc".into(),
            },
            goal_state: GoalState {
                full: "⊢ True".into(),
                ..GoalState::default()
            },
        };
        let json = serde_json::to_string(&proof).unwrap();
        let restored: Proof = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, proof);
    }
}
