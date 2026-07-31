//! Text fingerprinting for duplicate detection.
//!
//! Computes a 64-bit `FNV-1a` hash over a whitespace-normalized form of the
//! input text. The live translation pipeline uses fingerprints to skip
//! re-translating text that has not meaningfully changed between frames: OCR
//! jitter often shifts only spaces or line breaks, and normalizing those away
//! before hashing makes the duplicate check robust to such jitter while still
//! distinguishing any real change in wording.
//!
//! The hash is *not* cryptographic; it is chosen for speed and a low
//! collision rate on short OCR snippets. See `crate::is_duplicate`.

use tracing::instrument;

use crate::normalizer::is_invisible_char;

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Computes the 64-bit FNV-1a hash of `data`.
///
/// Implements the standard FNV-1a algorithm (XOR-then-multiply per byte).
/// Kept internal so the crate exposes only the text-level fingerprint API.
#[must_use]
pub(crate) fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Normalizes `text` for fingerprinting.
///
/// Removes invisible characters (zero-width spaces, bidi controls, BOM,
/// soft hyphens, ...) and collapses every whitespace run - including line
/// breaks - into a single ASCII space. Leading and trailing whitespace is
/// dropped. Character case and wording are preserved so that any real
/// change in the text still produces a different fingerprint.
#[must_use]
pub(crate) fn normalize_for_fingerprint(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if is_invisible_char(ch) {
            continue;
        }
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        out.push(ch);
    }
    out
}

/// Computes the duplicate-detection fingerprint of `text`.
///
/// The input is normalized with `normalize_for_fingerprint` before hashing,
/// so texts that differ only in whitespace or line breaks share a fingerprint.
#[must_use]
pub(crate) fn fingerprint_text(text: &str) -> u64 {
    let normalized = normalize_for_fingerprint(text);
    fnv1a_64(normalized.as_bytes())
}

/// Returns `true` when two texts are duplicates, i.e. their fingerprints
/// are equal.
///
/// Comparison is whitespace-insensitive: `"hello  world"` and
/// `"hello\nworld"` are considered duplicates, while any change in wording
/// (for example a typo) is not.
///
/// # Example
///
/// ```
/// use vtrans_text::is_duplicate;
///
/// assert!(is_duplicate("Hello world", "  Hello  world  "));
/// assert!(!is_duplicate("Hello world", "Hello world!"));
/// ```
#[must_use]
#[instrument(skip_all)]
pub fn is_duplicate(a: &str, b: &str) -> bool {
    fingerprint_text(a) == fingerprint_text(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_empty_is_offset_basis() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn fnv1a_known_vectors() {
        // Reference values computed with an independent FNV-1a implementation.
        assert_eq!(fnv1a_64(b"hello"), 0xa430_d846_80aa_bd0b);
        assert_eq!(fnv1a_64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn fnv1a_differs_for_similar_inputs() {
        assert_ne!(fnv1a_64(b"hello"), fnv1a_64(b"hellp"));
        assert_ne!(fnv1a_64(b"Hello"), fnv1a_64(b"hello"));
    }

    #[test]
    fn same_text_same_fingerprint() {
        assert_eq!(
            fingerprint_text("こんにちは世界"),
            fingerprint_text("こんにちは世界")
        );
    }

    #[test]
    fn whitespace_only_differences_share_fingerprint() {
        let a = fingerprint_text("Hello  world");
        let b = fingerprint_text(" Hello world ");
        let c = fingerprint_text("Hello\nworld");
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn zero_width_characters_are_ignored() {
        assert_eq!(fingerprint_text("Hel\u{200b}lo"), fingerprint_text("Hello"));
        assert_eq!(
            fingerprint_text("\u{feff}Hello\u{feff}"),
            fingerprint_text("Hello")
        );
    }

    #[test]
    fn wording_changes_produce_different_fingerprints() {
        assert_ne!(
            fingerprint_text("Hello world"),
            fingerprint_text("Hello world!")
        );
        assert_ne!(
            fingerprint_text("こんにちは"),
            fingerprint_text("こんばんは")
        );
        assert_ne!(fingerprint_text("hello"), fingerprint_text("Hello"));
    }

    #[test]
    fn empty_and_blank_texts_share_fingerprint() {
        assert_eq!(fingerprint_text(""), fingerprint_text("   \n\t "));
    }

    #[test]
    fn unicode_fingerprint_is_stable() {
        let a = fingerprint_text("漢字とひらがなのミックス１２３");
        let b = fingerprint_text("漢字とひらがなのミックス１２３");
        assert_eq!(a, b);
    }

    #[test]
    fn duplicate_detection() {
        assert!(is_duplicate("same text", "same  text"));
        assert!(is_duplicate("", " "));
        assert!(!is_duplicate("same text", "same test"));
        assert!(!is_duplicate("text", ""));
    }

    #[test]
    fn normalization_keeps_wording() {
        assert_eq!(normalize_for_fingerprint("  a  b\n  c  "), "a b c");
        assert_eq!(normalize_for_fingerprint(""), "");
        assert_eq!(normalize_for_fingerprint("   "), "");
    }
}
