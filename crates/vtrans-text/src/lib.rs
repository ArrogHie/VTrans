//! `VTrans` text normalization module.
//!
//! Cleans abnormal whitespace and invisible characters from OCR output,
//! merges OCR lines that belong to the same paragraph, normalizes Japanese
//! punctuation, computes text fingerprints for duplicate detection, and
//! splits text into length-limited paragraphs for translation providers.
//! Proper nouns and wording are never modified.
//!
//! # Layout
//!
//! - `normalizer`: `TextNormalizer`, the crate's entry point
//!   (`clean`, `merge_lines`, `fingerprint`,
//!   `split_paragraphs`, `validate_length`);
//! - `fingerprint`: FNV-1a fingerprinting and `is_duplicate`;
//! - `japanese`: Japanese punctuation normalization;
//! - `paragraph`: paragraph splitting and length limiting.
//!
//! - `box_dedup`: per-box fingerprint deduplication cache
//!   (`BoxFingerprintCache`) for multi-box live translation.
//!
//! See `docs/modules/06-text.md` for the full module specification.

pub mod box_dedup;
pub mod fingerprint;
pub mod japanese;
pub mod normalizer;
pub mod paragraph;

pub use box_dedup::BoxFingerprintCache;
pub use fingerprint::is_duplicate;
pub use normalizer::TextNormalizer;
pub use paragraph::DEFAULT_MAX_PARAGRAPH_LEN;

use thiserror::Error;

/// Errors reported by the text normalization module.
///
/// `TooLong` is returned by
/// `TextNormalizer::validate_length` when text exceeds a length
/// limit. `Failed` is reserved for future fallible normalization
/// operations; the current API surface never constructs it.
///
/// # Example
///
/// ```
/// use vtrans_text::{TextError, TextNormalizer};
///
/// let error = TextNormalizer::validate_length("hello", 3).unwrap_err();
/// assert!(matches!(error, TextError::TooLong(5)));
/// assert_eq!(error.to_string(), "text too long: 5 chars");
/// ```
#[derive(Debug, Error)]
pub enum TextError {
    /// The text exceeds the configured length limit (character count).
    #[error("text too long: {0} chars")]
    TooLong(usize),

    /// A normalization operation failed.
    #[error("normalization failed: {0}")]
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_error_display_messages() {
        assert_eq!(
            TextError::TooLong(3000).to_string(),
            "text too long: 3000 chars"
        );
        assert_eq!(
            TextError::Failed("bad input".to_string()).to_string(),
            "normalization failed: bad input"
        );
    }

    #[test]
    fn text_error_is_debug_and_error() {
        let error: &dyn std::error::Error = &TextError::TooLong(1);
        assert_eq!(error.source().map(ToString::to_string), None);
        let debug = format!("{error:?}");
        assert!(debug.starts_with("TooLong(1)"));
    }

    #[test]
    fn crate_re_exports_are_consistent() {
        // The re-exported names resolve to the same items as the modules.
        assert_eq!(
            normalizer::TextNormalizer::fingerprint("a"),
            TextNormalizer::fingerprint("a")
        );
        assert!(fingerprint::is_duplicate("x y", "x\ny"));
        assert_eq!(DEFAULT_MAX_PARAGRAPH_LEN, 2000);
    }
}
