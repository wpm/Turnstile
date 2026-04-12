//! Available LLM models.
//!
//! This module is the single source of truth for the model list shown in the
//! Settings UI.  When Anthropic releases new models, update `MODELS` here and
//! ship a Tauri update — no other code needs to change.

use serde::{Deserialize, Serialize};

/// A single model entry shown in the Settings UI.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    /// The model ID sent to the Anthropic API (e.g. `"claude-opus-4-6"`).
    pub id: &'static str,
    /// Human-readable display name shown in the dropdown.
    pub display_name: &'static str,
}

/// All models available for selection, in descending preference order.
/// The first entry is the default.
pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: "claude-opus-4-6",
        display_name: "Claude Opus 4.6",
    },
    ModelInfo {
        id: "claude-sonnet-4-6",
        display_name: "Claude Sonnet 4.6",
    },
    ModelInfo {
        id: "claude-haiku-4-5-20251001",
        display_name: "Claude Haiku 4.5",
    },
];

/// The default model ID (first entry in `MODELS`).
pub fn default_model_id() -> &'static str {
    MODELS[0].id
}

/// Return `true` if `id` is a known model ID.
pub fn is_valid_model_id(id: &str) -> bool {
    MODELS.iter().any(|m| m.id == id)
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_list_is_non_empty() {
        assert!(!MODELS.is_empty());
    }

    #[test]
    fn default_model_id_is_first_entry() {
        assert_eq!(default_model_id(), MODELS[0].id);
    }

    #[test]
    fn default_model_id_is_valid() {
        assert!(is_valid_model_id(default_model_id()));
    }

    #[test]
    fn is_valid_model_id_returns_true_for_known_ids() {
        for model in MODELS {
            assert!(
                is_valid_model_id(model.id),
                "expected {} to be valid",
                model.id
            );
        }
    }

    #[test]
    fn is_valid_model_id_returns_false_for_unknown_id() {
        assert!(!is_valid_model_id("gpt-4o"));
        assert!(!is_valid_model_id(""));
    }

    #[test]
    fn all_models_have_non_empty_display_name() {
        for model in MODELS {
            assert!(
                !model.display_name.is_empty(),
                "model {} has empty display name",
                model.id
            );
        }
    }

    #[test]
    fn model_ids_are_unique() {
        let mut ids: Vec<&str> = MODELS.iter().map(|m| m.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), MODELS.len(), "duplicate model IDs found");
    }
}
