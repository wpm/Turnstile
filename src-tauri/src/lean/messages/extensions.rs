//! Lean-specific LSP protocol extensions.
//!
//! Defines request and response types for methods that are not part of the
//! standard LSP spec but are provided by `lean --server`:
//!
//! - `$/lean/plainGoal` — the tactic goal state at a position.
//! - `$/lean/plainTermGoal` — the expected type at a position in a term-mode proof.

use async_lsp::lsp_types::request::Request;
use lsp_types::TextDocumentPositionParams;
use serde::{Deserialize, Serialize};

/// Response type for the `$/lean/plainGoal` request.
#[derive(Debug, Serialize, Deserialize)]
pub struct PlainGoal {
    /// The whole goal state rendered as one block (all goals, blank-line
    /// separated). Kept as the input to prose/assistant.
    pub rendered: String,
    /// Each open goal rendered individually (case label, hypotheses, and `⊢`
    /// conclusion). Carries the per-goal structure the cursor-row card and the
    /// All-Goals panel render, so the backend never re-splits `rendered`.
    /// Defaults to empty when the server omits it (older servers / fakes).
    #[serde(default)]
    pub goals: Vec<String>,
}

pub(super) enum PlainGoalMethod {}
impl Request for PlainGoalMethod {
    type Params = TextDocumentPositionParams;
    type Result = Option<PlainGoal>;
    const METHOD: &'static str = "$/lean/plainGoal";
}

/// Response type for the `$/lean/plainTermGoal` request.
#[derive(Debug, Serialize, Deserialize)]
pub struct PlainTermGoal {
    pub goal: String,
}

pub(super) enum PlainTermGoalMethod {}
impl Request for PlainTermGoalMethod {
    type Params = TextDocumentPositionParams;
    type Result = Option<PlainTermGoal>;
    const METHOD: &'static str = "$/lean/plainTermGoal";
}
