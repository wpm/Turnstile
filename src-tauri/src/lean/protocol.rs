//! [`Protocol`]: document-level LSP state for a single `Proof.lean` session.

#![allow(dead_code)]
//!
//! Owns the document version counter and the staleness filters for both
//! server-pushed notifications (`keep_notification`) and request responses
//! (`fresh`). Position-arithmetic helpers live here too — they are the only
//! place in Turnstile that converts between UTF-16 columns and byte offsets.

use std::future::Future;
use std::sync::atomic::{AtomicI32, Ordering};

use lsp_types::{Position, Range};

use crate::lean::messages::lean::LeanMessage;

/// Owns all document-level protocol state for a single `Proof.lean` open in
/// `lean --server`. Created by `Client::start` and held for the lifetime of
/// the `Client`.
pub struct Protocol {
    /// Version of the most recent `did_change` (or 0 immediately after
    /// `did_open`). Monotonic. Bumped only by `next_version`.
    version: AtomicI32,
}

impl Protocol {
    /// Construct a fresh `Protocol` matching a `did_open` at version 0.
    pub const fn new() -> Self {
        Self {
            version: AtomicI32::new(0),
        }
    }

    /// Allocate the version number for the next outgoing `did_change`.
    /// Strictly monotonic; the first call after `new()` returns 1.
    pub fn next_version(&self) -> i32 {
        self.version.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// The document version most recently sent to the server.
    pub fn current_version(&self) -> i32 {
        self.version.load(Ordering::Acquire)
    }

    /// Decide whether a server-pushed notification still reflects the current
    /// document version. Returns `true` for messages that should be forwarded
    /// to the frontend, `false` for stale messages that should be dropped.
    ///
    /// Notifications without a version field (`LogMessage`, `ShowMessage`,
    /// `TokenRefresh`) are always kept.
    pub fn keep_notification(&self, msg: &LeanMessage) -> bool {
        let cur = self.current_version();
        match msg {
            LeanMessage::Diagnostics(p) => p.version.is_none_or(|v| v >= cur),
            LeanMessage::FileProgress(p) => p.version().is_none_or(|v| v >= cur),
            LeanMessage::LogMessage(_)
            | LeanMessage::ShowMessage(_)
            | LeanMessage::TokenRefresh => true,
        }
    }

    /// Wrap a request future so its result is discarded if the document
    /// version has advanced between request dispatch and response arrival.
    /// Returns `Ok(None)` for a stale response, `Ok(Some(value))` otherwise.
    /// Errors from the underlying request are propagated unchanged.
    pub async fn fresh<F, T, E>(&self, fut: F) -> Result<Option<T>, E>
    where
        F: Future<Output = Result<T, E>>,
    {
        let asked_at = self.current_version();
        let result = fut.await?;
        if self.current_version() > asked_at {
            Ok(None)
        } else {
            Ok(Some(result))
        }
    }

    /// Convert a UTF-16 column on `line` to a byte offset within `line`.
    /// Out-of-range columns clamp to `line.len()`.
    pub fn utf16_col_to_byte(line: &str, utf16_col: u32) -> usize {
        let mut utf16_pos = 0u32;
        for (byte, ch) in line.char_indices() {
            if utf16_pos >= utf16_col {
                return byte;
            }
            utf16_pos += u32::try_from(ch.len_utf16()).unwrap_or(u32::MAX);
        }
        line.len()
    }

    /// Convert a byte offset within `line` to a UTF-16 column.
    pub fn byte_to_utf16_col(line: &str, byte: usize) -> u32 {
        line[..byte.min(line.len())]
            .chars()
            .map(|c| u32::try_from(c.len_utf16()).unwrap_or(u32::MAX))
            .sum()
    }

    /// Convert an LSP `Position` (line + UTF-16 column) to a byte offset
    /// within the full document `text`. Returns `text.len()` if `pos` is
    /// past the end.
    pub fn position_to_byte(text: &str, pos: Position) -> usize {
        let mut offset = 0;
        for (i, line) in text.split('\n').enumerate() {
            if i == pos.line as usize {
                return offset + Self::utf16_col_to_byte(line, pos.character);
            }
            offset += line.len() + 1; // +1 for the '\n'
        }
        text.len()
    }

    /// Inverse of `position_to_byte`.
    pub fn byte_to_position(text: &str, byte: usize) -> Position {
        let byte = byte.min(text.len());
        let prefix = &text[..byte];
        let line =
            u32::try_from(prefix.bytes().filter(|&b| b == b'\n').count()).unwrap_or(u32::MAX);
        let line_start = prefix.rfind('\n').map_or(0, |i| i + 1);
        let character = Self::byte_to_utf16_col(&text[line_start..byte], byte - line_start);
        Position { line, character }
    }

    /// Slice `text` by a UTF-16-encoded `Range`. Used for tests that extract
    /// the text covered by a semantic token or a diagnostic range.
    pub fn slice_range(text: &str, range: Range) -> &str {
        let start = Self::position_to_byte(text, range.start);
        let end = Self::position_to_byte(text, range.end);
        &text[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // 1. Version monotonicity
    #[test]
    fn version_monotonic() {
        let p = Protocol::new();
        assert_eq!(p.current_version(), 0);
        assert_eq!(p.next_version(), 1);
        assert_eq!(p.current_version(), 1);
        assert_eq!(p.next_version(), 2);
        assert_eq!(p.next_version(), 3);
        assert_eq!(p.current_version(), 3);
    }

    // 2. Notification filter — stale diagnostics dropped
    #[test]
    fn keep_notification_diagnostics() {
        use lsp_types::PublishDiagnosticsParams;
        let p = Protocol::new();
        for _ in 0..5 {
            p.next_version();
        }
        assert_eq!(p.current_version(), 5);

        let make = |v: Option<i32>| {
            LeanMessage::Diagnostics(PublishDiagnosticsParams {
                uri: "file:///Proof.lean".parse().unwrap(),
                version: v,
                diagnostics: vec![],
            })
        };

        assert!(
            !p.keep_notification(&make(Some(3))),
            "stale v3 should be dropped"
        );
        assert!(
            p.keep_notification(&make(Some(5))),
            "current v5 should be kept"
        );
        assert!(
            p.keep_notification(&make(Some(6))),
            "future v6 should be kept"
        );
        assert!(
            p.keep_notification(&make(None)),
            "versionless should be kept"
        );
    }

    // 3. Notification filter — stale fileProgress dropped
    #[test]
    fn keep_notification_file_progress() {
        use crate::lean::server::{
            FileProgressInterval, FileProgressParams, FileProgressTextDocument,
        };
        use lsp_types::Range;
        let p = Protocol::new();
        for _ in 0..5 {
            p.next_version();
        }

        let make = |v: Option<i32>| {
            LeanMessage::FileProgress(FileProgressParams {
                text_document: FileProgressTextDocument {
                    uri: "file:///Proof.lean".parse().unwrap(),
                    version: v,
                },
                processing: vec![FileProgressInterval {
                    range: Range::default(),
                }],
            })
        };

        assert!(
            !p.keep_notification(&make(Some(3))),
            "stale v3 should be dropped"
        );
        assert!(
            p.keep_notification(&make(Some(5))),
            "current v5 should be kept"
        );
        assert!(
            p.keep_notification(&make(None)),
            "versionless should be kept"
        );
    }

    // 4. Notification filter — versionless messages always kept
    #[test]
    fn keep_notification_versionless() {
        use lsp_types::{LogMessageParams, MessageType, ShowMessageParams};
        let p = Protocol::new();
        for _ in 0..3 {
            p.next_version();
        }

        assert!(
            p.keep_notification(&LeanMessage::LogMessage(LogMessageParams {
                typ: MessageType::LOG,
                message: "test".into(),
            }))
        );
        assert!(
            p.keep_notification(&LeanMessage::ShowMessage(ShowMessageParams {
                typ: MessageType::INFO,
                message: "test".into(),
            }))
        );
        assert!(p.keep_notification(&LeanMessage::TokenRefresh));
    }

    // 5. fresh wrapper — non-stale
    #[tokio::test]
    async fn fresh_non_stale() {
        let p = Protocol::new();
        let result = p.fresh(async { Ok::<_, ()>(42) }).await;
        assert_eq!(result, Ok(Some(42)));
    }

    // 6. fresh wrapper — stale (version bumped during await)
    #[tokio::test]
    async fn fresh_stale() {
        let p = Arc::new(Protocol::new());
        let p2 = Arc::clone(&p);
        let result = p
            .fresh(async move {
                tokio::task::yield_now().await;
                p2.next_version();
                Ok::<_, ()>(42)
            })
            .await;
        assert_eq!(result, Ok(None));
    }

    // 7. Position arithmetic — ASCII
    #[test]
    fn position_arithmetic_ascii() {
        assert_eq!(Protocol::byte_to_utf16_col("hello", 3), 3);
        assert_eq!(Protocol::utf16_col_to_byte("hello", 3), 3);
    }

    // 8. Position arithmetic — multi-byte UTF-8 / single UTF-16 unit
    // "p ∧ q": p=0, ' '=1, ∧=2-4 (3 UTF-8 bytes, 1 UTF-16 unit), ' '=5, q=6
    // UTF-16 columns: p=0, ' '=1, ∧=2, ' '=3, q=4
    #[test]
    fn position_arithmetic_multibyte_utf8() {
        let s = "p ∧ q";
        // q is at byte 6 (p=1, space=1, ∧=3, space=1 → total 6)
        assert_eq!(
            Protocol::byte_to_utf16_col(s, 6),
            4,
            "q should be at UTF-16 col 4"
        );
        assert_eq!(
            Protocol::utf16_col_to_byte(s, 4),
            6,
            "UTF-16 col 4 should be byte 6"
        );
    }

    // 9. Position arithmetic — surrogate pair (U+1D54A = 𝕊, 4 UTF-8 bytes, 2 UTF-16 units)
    #[test]
    fn position_arithmetic_surrogate_pair() {
        let s = "𝕊x";
        // 𝕊 is 4 UTF-8 bytes, 2 UTF-16 units; 'x' starts at byte 4, UTF-16 col 2
        assert_eq!(
            Protocol::utf16_col_to_byte(s, 2),
            4,
            "x should start at byte 4"
        );
        assert_eq!(
            Protocol::byte_to_utf16_col(s, 4),
            2,
            "byte 4 should be UTF-16 col 2"
        );
    }

    // 10. Position arithmetic — clamping
    #[test]
    fn position_arithmetic_clamping() {
        assert_eq!(Protocol::utf16_col_to_byte("hi", 999), 2);
    }

    // 11. position_to_byte and byte_to_position round-trip
    #[test]
    fn position_roundtrip() {
        let text = "hello\nwörld\n𝕊end";
        for byte in 0..=text.len() {
            // skip positions mid-char
            if !text.is_char_boundary(byte) {
                continue;
            }
            let pos = Protocol::byte_to_position(text, byte);
            let back = Protocol::position_to_byte(text, pos);
            assert_eq!(back, byte, "round-trip failed at byte {byte}");
        }
    }
}
