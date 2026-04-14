//! Session persistence: read/write `.turn` files.
//!
//! A `.turn` file is a ZIP archive containing:
//!
//! - `meta.json`       — format version, timestamps, UI state
//! - `proof.lean`      — Lean source from the editor buffer
//! - `prose.json`      — prose text and tactic state hash for staleness detection
//! - `transcript.json` — recent window of chat dialog turns
//! - `summary.txt`     — optional LLM-generated conversation summary
//!
//! # Format version
//!
//! `meta.json` carries a `format_version` integer. [`load`] rejects files
//! with an unknown version. The current version is [`FORMAT_VERSION`].

use std::io::{Cursor, Read, Write};
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Current `.turn` format version. Increment when making breaking changes.
pub const FORMAT_VERSION: u32 = 1;

// ── Data types ────────────────────────────────────────────────────────

/// UI-restoration metadata stored in `meta.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    pub format_version: u32,
    /// ISO-8601 creation timestamp (set on first save, never updated).
    pub created_at: String,
    /// ISO-8601 last-saved timestamp.
    pub saved_at: String,
    /// Cursor position: 0-based line number.
    pub cursor_line: u32,
    /// Cursor position: 0-based column number.
    pub cursor_col: u32,
    /// Editor scroll offset in pixels.
    pub editor_scroll_top: f64,
    /// Chat/code split: chat panel width as a percentage (0–100).
    pub chat_width_pct: f64,
    /// Which proof view was active: `"formal"` or `"prose"`. Absent in older files.
    #[serde(default)]
    pub proof_view: Option<String>,
    /// Goal panel height as a percentage of the editor column. Absent in older files.
    #[serde(default)]
    pub goal_panel_pct: Option<f64>,
    /// Per-file word-wrap state for the editor. Absent means the editor default (off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_wrap: Option<bool>,
}

/// Prose content with a tactic state hash for staleness detection.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ProseData {
    /// LaTeX or plain-text prose content.
    pub text: String,
    /// Hash of the tactic state at the time of writing, for staleness detection.
    pub tactic_state_hash: Option<String>,
}

/// One turn in the chat transcript.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TranscriptTurn {
    /// `"user"` or `"assistant"`.
    pub role: String,
    /// Message content.
    pub content: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: i64,
}

/// Complete session state persisted in a `.turn` file.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionState {
    pub meta: Meta,
    /// Lean source text.
    pub proof_lean: String,
    /// Prose annotation data.
    pub prose: ProseData,
    /// Chat turns in the current transcript window (may be empty).
    pub transcript: Vec<TranscriptTurn>,
    /// LLM-generated summary of earlier conversation history, if present.
    pub summary: Option<String>,
}

impl SessionState {
    /// Return a blank session with sensible defaults.
    pub fn blank() -> Self {
        let now = current_timestamp();
        SessionState {
            meta: Meta {
                format_version: FORMAT_VERSION,
                created_at: now.clone(),
                saved_at: now,
                ..Default::default()
            },
            proof_lean: String::new(),
            prose: ProseData::default(),
            transcript: Vec::new(),
            summary: None,
        }
    }
}

// ── ZIP I/O ───────────────────────────────────────────────────────────

/// Write `state` to a `.turn` ZIP archive at `path`.
///
/// Overwrites any existing file.
pub fn save(state: &SessionState, path: &Path) -> Result<(), String> {
    let buf = build_zip(state)?;
    std::fs::write(path, buf).map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

/// Read a `.turn` ZIP archive from `path` and deserialize it.
///
/// Returns an error if:
/// - the file cannot be read or is not a valid ZIP,
/// - any required entry (`meta.json`, `proof.lean`, `prose.json`,
///   `transcript.json`) is missing or malformed,
/// - `meta.json` carries an unknown `format_version`.
pub fn load(path: &Path) -> Result<SessionState, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    parse_zip(&bytes)
}

// ── Internal helpers ──────────────────────────────────────────────────

fn current_timestamp() -> String {
    Utc::now().to_rfc3339()
}

pub(crate) fn build_zip(state: &SessionState) -> Result<Vec<u8>, String> {
    let buf = Vec::new();
    let cursor = Cursor::new(buf);
    let mut zip = zip::ZipWriter::new(cursor);

    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // meta.json
    let meta_json = serde_json::to_string_pretty(&state.meta)
        .map_err(|e| format!("Failed to serialize meta: {e}"))?;
    zip.start_file("meta.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(meta_json.as_bytes())
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // proof.lean
    zip.start_file("proof.lean", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(state.proof_lean.as_bytes())
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // prose.json
    let prose_json = serde_json::to_string_pretty(&state.prose)
        .map_err(|e| format!("Failed to serialize prose: {e}"))?;
    zip.start_file("prose.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(prose_json.as_bytes())
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // transcript.json
    let transcript_json = serde_json::to_string_pretty(&state.transcript)
        .map_err(|e| format!("Failed to serialize transcript: {e}"))?;
    zip.start_file("transcript.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(transcript_json.as_bytes())
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // summary.txt (optional)
    if let Some(ref summary) = state.summary {
        zip.start_file("summary.txt", options)
            .map_err(|e| format!("ZIP error: {e}"))?;
        zip.write_all(summary.as_bytes())
            .map_err(|e| format!("ZIP write error: {e}"))?;
    }

    let cursor = zip.finish().map_err(|e| format!("ZIP finish error: {e}"))?;
    Ok(cursor.into_inner())
}

pub(crate) fn parse_zip(bytes: &[u8]) -> Result<SessionState, String> {
    let cursor = Cursor::new(bytes);
    let mut zip =
        zip::ZipArchive::new(cursor).map_err(|e| format!("Not a valid ZIP archive: {e}"))?;

    // meta.json — required
    let meta: Meta = {
        let mut file = zip
            .by_name("meta.json")
            .map_err(|_| "Missing meta.json in archive".to_string())?;
        let mut s = String::new();
        file.read_to_string(&mut s)
            .map_err(|e| format!("Failed to read meta.json: {e}"))?;
        serde_json::from_str(&s).map_err(|e| format!("Failed to parse meta.json: {e}"))?
    };

    if meta.format_version != FORMAT_VERSION {
        return Err(format!(
            "Unknown format version {} (expected {FORMAT_VERSION})",
            meta.format_version
        ));
    }

    // proof.lean — required
    let proof_lean = {
        let mut file = zip
            .by_name("proof.lean")
            .map_err(|_| "Missing proof.lean in archive".to_string())?;
        let mut s = String::new();
        file.read_to_string(&mut s)
            .map_err(|e| format!("Failed to read proof.lean: {e}"))?;
        s
    };

    // prose.json — required
    let prose: ProseData = {
        let mut file = zip
            .by_name("prose.json")
            .map_err(|_| "Missing prose.json in archive".to_string())?;
        let mut s = String::new();
        file.read_to_string(&mut s)
            .map_err(|e| format!("Failed to read prose.json: {e}"))?;
        serde_json::from_str(&s).map_err(|e| format!("Failed to parse prose.json: {e}"))?
    };

    // transcript.json — required (may be empty array)
    let transcript: Vec<TranscriptTurn> = {
        let mut file = zip
            .by_name("transcript.json")
            .map_err(|_| "Missing transcript.json in archive".to_string())?;
        let mut s = String::new();
        file.read_to_string(&mut s)
            .map_err(|e| format!("Failed to read transcript.json: {e}"))?;
        serde_json::from_str(&s).map_err(|e| format!("Failed to parse transcript.json: {e}"))?
    };

    // summary.txt — optional
    let summary = match zip.by_name("summary.txt") {
        Ok(mut file) => {
            let mut s = String::new();
            file.read_to_string(&mut s)
                .map_err(|e| format!("Failed to read summary.txt: {e}"))?;
            Some(s)
        }
        Err(_) => None,
    };

    Ok(SessionState {
        meta,
        proof_lean,
        prose,
        transcript,
        summary,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn sample_state() -> SessionState {
        SessionState {
            meta: Meta {
                format_version: FORMAT_VERSION,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                saved_at: "2026-01-01T00:00:01Z".to_string(),
                cursor_line: 3,
                cursor_col: 7,
                editor_scroll_top: 120.5,
                chat_width_pct: 40.0,
                proof_view: None,
                goal_panel_pct: None,
                word_wrap: None,
            },
            proof_lean: "theorem foo : True := by\n  trivial\n".to_string(),
            prose: ProseData {
                text: "This proves True.".to_string(),
                tactic_state_hash: Some("abc123".to_string()),
            },
            transcript: vec![
                TranscriptTurn {
                    role: "user".to_string(),
                    content: "What is rfl?".to_string(),
                    timestamp: 1_000_000,
                },
                TranscriptTurn {
                    role: "assistant".to_string(),
                    content: "rfl proves reflexivity.".to_string(),
                    timestamp: 1_000_001,
                },
            ],
            summary: Some("Earlier we discussed rfl.".to_string()),
        }
    }

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn round_trip_full_state() -> TestResult {
        let original = sample_state();
        let tmp = NamedTempFile::new()?;

        save(&original, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded, original);
        Ok(())
    }

    #[test]
    fn round_trip_without_summary() -> TestResult {
        let mut state = sample_state();
        state.summary = None;

        let tmp = NamedTempFile::new()?;
        save(&state, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded, state);
        assert!(loaded.summary.is_none());
        Ok(())
    }

    #[test]
    fn round_trip_empty_transcript() -> TestResult {
        let mut state = sample_state();
        state.transcript = Vec::new();

        let tmp = NamedTempFile::new()?;
        save(&state, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded, state);
        assert!(loaded.transcript.is_empty());
        Ok(())
    }

    #[test]
    fn round_trip_empty_proof() -> TestResult {
        let mut state = sample_state();
        state.proof_lean = String::new();

        let tmp = NamedTempFile::new()?;
        save(&state, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded.proof_lean, "");
        Ok(())
    }

    #[test]
    fn round_trip_prose_without_hash() -> TestResult {
        let mut state = sample_state();
        state.prose = ProseData {
            text: "Some text".to_string(),
            tactic_state_hash: None,
        };

        let tmp = NamedTempFile::new()?;
        save(&state, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded.prose.text, "Some text");
        assert!(loaded.prose.tactic_state_hash.is_none());
        Ok(())
    }

    #[test]
    fn round_trip_transcript_timestamps() -> TestResult {
        let state = sample_state();
        let tmp = NamedTempFile::new()?;

        save(&state, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded.transcript[0].timestamp, 1_000_000);
        assert_eq!(loaded.transcript[1].timestamp, 1_000_001);
        Ok(())
    }

    #[test]
    fn format_version_rejection() -> TestResult {
        let mut state = sample_state();
        // Build a ZIP with an unknown format version
        state.meta.format_version = 999;

        let bytes = build_zip(&state)?;
        let err = parse_zip(&bytes).unwrap_err();

        assert!(err.contains("Unknown format version 999"), "got: {err}");
        Ok(())
    }

    #[test]
    fn rejects_non_zip_bytes() {
        let garbage = b"this is not a ZIP file";
        let err = parse_zip(garbage).unwrap_err();
        assert!(err.contains("Not a valid ZIP archive"), "got: {err}");
    }

    #[test]
    fn rejects_zip_missing_meta() -> TestResult {
        // Build a ZIP that has no meta.json
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("proof.lean", opts)?;
        zip.write_all(b"")?;
        let cursor = zip.finish()?;

        let err = parse_zip(&cursor.into_inner()).unwrap_err();
        assert!(err.contains("Missing meta.json"), "got: {err}");
        Ok(())
    }

    #[test]
    fn rejects_zip_missing_proof_lean() -> TestResult {
        // Build a ZIP that has meta.json but no proof.lean
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default();
        let meta = Meta {
            format_version: FORMAT_VERSION,
            ..Default::default()
        };
        let meta_json = serde_json::to_string(&meta)?;
        zip.start_file("meta.json", opts)?;
        zip.write_all(meta_json.as_bytes())?;
        let cursor = zip.finish()?;

        let err = parse_zip(&cursor.into_inner()).unwrap_err();
        assert!(err.contains("Missing proof.lean"), "got: {err}");
        Ok(())
    }

    #[test]
    fn blank_session_has_format_version() {
        let state = SessionState::blank();
        assert_eq!(state.meta.format_version, FORMAT_VERSION);
        assert!(state.proof_lean.is_empty());
        assert!(state.prose.text.is_empty());
        assert!(state.prose.tactic_state_hash.is_none());
        assert!(state.transcript.is_empty());
        assert!(state.summary.is_none());
    }

    #[test]
    fn proof_view_round_trip() -> TestResult {
        let mut state = sample_state();
        state.meta.proof_view = Some("prose".to_string());

        let bytes = build_zip(&state)?;
        let loaded = parse_zip(&bytes)?;

        assert_eq!(loaded.meta.proof_view, Some("prose".to_string()));
        Ok(())
    }

    #[test]
    fn proof_view_defaults_to_none() -> TestResult {
        // Old meta without proof_view should deserialize to None
        let json = r#"{
            "format_version": 1,
            "created_at": "",
            "saved_at": "",
            "cursor_line": 0,
            "cursor_col": 0,
            "editor_scroll_top": 0.0,
            "chat_width_pct": 25.0
        }"#;
        let meta: Meta = serde_json::from_str(json)?;
        assert!(meta.proof_view.is_none());
        Ok(())
    }

    #[test]
    fn word_wrap_round_trip_some_true() -> TestResult {
        let mut state = sample_state();
        state.meta.word_wrap = Some(true);

        let bytes = build_zip(&state)?;
        let loaded = parse_zip(&bytes)?;

        assert_eq!(loaded.meta.word_wrap, Some(true));
        Ok(())
    }

    #[test]
    fn word_wrap_round_trip_some_false() -> TestResult {
        let mut state = sample_state();
        state.meta.word_wrap = Some(false);

        let bytes = build_zip(&state)?;
        let loaded = parse_zip(&bytes)?;

        assert_eq!(loaded.meta.word_wrap, Some(false));
        Ok(())
    }

    #[test]
    fn word_wrap_defaults_to_none() -> TestResult {
        // Old meta without word_wrap should deserialize to None
        let json = r#"{
            "format_version": 1,
            "created_at": "",
            "saved_at": "",
            "cursor_line": 0,
            "cursor_col": 0,
            "editor_scroll_top": 0.0,
            "chat_width_pct": 25.0
        }"#;
        let meta: Meta = serde_json::from_str(json)?;
        assert!(meta.word_wrap.is_none());
        Ok(())
    }

    #[test]
    fn word_wrap_none_is_not_serialized() -> TestResult {
        // With skip_serializing_if, None should be omitted from output
        let mut state = sample_state();
        state.meta.word_wrap = None;
        let json = serde_json::to_string(&state.meta)?;
        assert!(!json.contains("word_wrap"), "got: {json}");
        Ok(())
    }

    #[test]
    fn meta_fields_preserved() -> TestResult {
        let original = sample_state();
        let bytes = build_zip(&original)?;
        let loaded = parse_zip(&bytes)?;

        assert_eq!(loaded.meta.cursor_line, 3);
        assert_eq!(loaded.meta.cursor_col, 7);
        assert!((loaded.meta.editor_scroll_top - 120.5).abs() < f64::EPSILON);
        assert!((loaded.meta.chat_width_pct - 40.0).abs() < f64::EPSILON);
        Ok(())
    }
}
