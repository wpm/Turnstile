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

/// The Lean source buffer.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FormalProof {
    pub source: String,
}

/// The prose proof draft, tagged with the hash of the formal source that
/// produced it.  When the source hash diverges from the current source, the
/// prose is stale and a regeneration is queued.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProseProof {
    pub text: String,
    /// Hash of the formal source that produced this prose.  Empty if no prose
    /// has been generated yet.
    pub source_hash: String,
}

/// The goal state reported by the Lean LSP while elaborating the formal proof.
///
/// * `full` — what Lean reports at the end of the document (the "whole-proof"
///   goal state, independent of cursor position).
/// * `per_line` — one entry per line in the source, precomputed so the UI can
///   show the goal state at any tactic step without a round-trip.
///
/// Currently populated on-demand and delivered to the UI via the
/// `goal-state-updated` Tauri event; the in-memory `Proof.goal_state` field
/// is reserved for future readers (PA tool dispatch, session save) and is
/// not kept continuously in sync.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalState {
    pub full: String,
    pub per_line: Vec<String>,
}

/// A proof represented both formally (Lean) and in prose, together with the
/// live goal state from the LSP.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Proof {
    pub formal: FormalProof,
    pub prose: ProseProof,
    pub goal_state: GoalState,
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

/// Compute a fast hash of a string for change detection (not cryptographic).
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
    fn proof_default_has_empty_fields() {
        let p = Proof::default();
        assert!(p.formal.source.is_empty());
        assert!(p.prose.text.is_empty());
        assert!(p.prose.source_hash.is_empty());
        assert!(p.goal_state.full.is_empty());
        assert!(p.goal_state.per_line.is_empty());
    }

    #[test]
    fn proof_round_trips_through_json() {
        let proof = Proof {
            formal: FormalProof {
                source: "theorem foo : True := trivial".into(),
            },
            prose: ProseProof {
                text: "This proves True.".into(),
                source_hash: "abc".into(),
            },
            goal_state: GoalState {
                full: "⊢ True".into(),
                per_line: vec!["⊢ True".into(), "no goals".into()],
            },
        };
        let json = serde_json::to_string(&proof).unwrap();
        let restored: Proof = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, proof);
    }
}
