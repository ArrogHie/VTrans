//! Paragraph splitting and length limiting.
//!
//! `split_paragraphs` turns normalized text into chunks that each fit
//! within a maximum length, and `validate_length` is a cheap guard for
//! callers that send text without splitting. Both operate on character
//! counts (Unicode scalar values), which is the unit translation providers
//! typically bill by.

use tracing::warn;
use vtrans_core::truncate_for_log;

use crate::TextError;

/// Default maximum length of a single text chunk, in characters.
///
/// Used by `crate::TextNormalizer::split_paragraphs_default`. The pipeline
/// may override it per call through
/// `crate::TextNormalizer::split_paragraphs`.
///
/// # Example
///
/// ```
/// use vtrans_text::DEFAULT_MAX_PARAGRAPH_LEN;
///
/// assert_eq!(DEFAULT_MAX_PARAGRAPH_LEN, 2000);
/// ```
pub const DEFAULT_MAX_PARAGRAPH_LEN: usize = 2000;

/// Characters that mark a natural chunk boundary when a paragraph must be
/// split. Split points are preferred at these before falling back to
/// whitespace, then to a hard character boundary.
fn is_sentence_ender(ch: char) -> bool {
    matches!(ch, '。' | '！' | '？' | '…' | '；' | '.' | '!' | '?' | ';')
}

/// Splits `text` into paragraphs of at most `max_len` characters.
///
/// Every newline is treated as a paragraph separator (matching the output of
/// `TextNormalizer::merge_lines`), and blank lines are dropped. Each
/// non-empty paragraph is trimmed. A paragraph that still exceeds
/// `max_len` is split at sentence-ending punctuation, then at the last
/// whitespace inside the window, and finally at a hard character boundary.
///
/// Passing `0` as `max_len` disables length limiting: paragraphs are
/// returned as-is. This is convenient for callers that only want the
/// paragraph structure.
///
/// # Example
///
/// ```
/// use vtrans_text::TextNormalizer;
///
/// let chunks = TextNormalizer::split_paragraphs("aaaa bbbb cccc", 6);
/// assert_eq!(chunks, vec!["aaaa", "bbbb", "cccc"]);
///
/// let short = TextNormalizer::split_paragraphs("hello", 2000);
/// assert_eq!(short, vec!["hello"]);
/// ```
#[must_use]
pub(crate) fn split_paragraphs(text: &str, max_len: usize) -> Vec<String> {
    if max_len == 0 {
        warn!(
            text = %truncate_for_log(text),
            "split_paragraphs called with max_len = 0; treating as unlimited"
        );
    }
    let mut paragraphs = Vec::new();
    for raw in text.split('\n') {
        let paragraph = raw.trim();
        if paragraph.is_empty() {
            continue;
        }
        if max_len == 0 || paragraph.chars().count() <= max_len {
            paragraphs.push(paragraph.to_string());
        } else {
            paragraphs.extend(split_long_paragraph(paragraph, max_len));
        }
    }
    paragraphs
}

/// Splits one over-long paragraph into chunks of at most `max_len` chars.
fn split_long_paragraph(paragraph: &str, max_len: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut remaining = paragraph;
    while remaining.chars().count() > max_len {
        let (chunk, rest) = take_chunk(remaining, max_len);
        // trim_end guards the window-boundary cut: the window may end with a
        // whitespace character when the input has consecutive spaces.
        chunks.push(chunk.trim_end().to_string());
        remaining = rest.trim_start();
    }
    if !remaining.is_empty() {
        chunks.push(remaining.to_string());
    }
    chunks
}

/// Cuts off the next chunk from `paragraph`.
///
/// Returns `(chunk, rest)` where `chunk` has at most `max_len`
/// characters. When the window boundary lands in the middle of a word, the
/// cut prefers a sentence-ending punctuation boundary, then the last
/// whitespace inside the window, and falls back to a hard cut at the window
/// boundary. When the boundary already lands on a word end, the window is
/// used as-is. Soft boundaries are only used when they consume at least half
/// of the window, so that tiny fragments are not produced.
fn take_chunk(paragraph: &str, max_len: usize) -> (&str, &str) {
    debug_assert!(paragraph.chars().count() > max_len);

    let mut indices = paragraph.char_indices();
    // Byte offset just past the first max_len characters (the window).
    let window_end = indices
        .nth(max_len - 1)
        .map_or(paragraph.len(), |(idx, ch)| idx + ch.len_utf8());

    // The boundary is mid-word when the character right after the window
    // continues the word (ASCII letter/digit or CJK word character).
    let mid_word = paragraph[window_end..].chars().next().is_some_and(|ch| {
        let ch = crate::normalizer::fullwidth_to_ascii(ch);
        ch.is_ascii_alphanumeric() || crate::normalizer::is_cjk_word(ch)
    });

    if mid_word {
        let window = &paragraph[..window_end];
        let floor = max_len / 2;
        if let Some((byte_idx, ch)) = last_boundary(window, is_sentence_ender, floor) {
            // Include the sentence-ending punctuation in the chunk.
            let end = byte_idx + ch.len_utf8();
            return (&paragraph[..end], &paragraph[end..]);
        }
        if let Some((byte_idx, _)) = last_boundary(window, char::is_whitespace, floor) {
            // Cut before the whitespace; split_long_paragraph trims the rest.
            return (&paragraph[..byte_idx], &paragraph[byte_idx..]);
        }
    }
    (&paragraph[..window_end], &paragraph[window_end..])
}

/// Finds the last character in `window` matching `predicate` whose chunk
/// prefix would consume more than half of the window (at least
/// `floor + 1` characters), and returns its byte index together with the
/// character itself so the caller can decide whether to include it.
///
/// Returns `None` when no such boundary exists.
fn last_boundary(window: &str, predicate: fn(char) -> bool, floor: usize) -> Option<(usize, char)> {
    let mut result = None;
    for (char_idx, (byte_idx, ch)) in window.char_indices().enumerate() {
        if char_idx + 1 > floor && predicate(ch) {
            result = Some((byte_idx, ch));
        }
    }
    result
}

/// Checks that the normalized text does not exceed `max_len` characters.
///
/// This is a cheap guard for callers that send a single chunk to a
/// translation provider. It counts characters of `text` as given; run
/// `TextNormalizer::clean` first if the whitespace-normalized length is
/// what should be limited.
///
/// # Errors
///
/// Returns `TextError::TooLong` with the actual character count when
/// `text` exceeds `max_len`.
///
/// # Example
///
/// ```
/// use vtrans_text::{TextError, TextNormalizer};
///
/// assert!(TextNormalizer::validate_length("hello", 10).is_ok());
/// assert!(matches!(
///     TextNormalizer::validate_length("hello", 3),
///     Err(TextError::TooLong(5))
/// ));
/// ```
pub(crate) fn validate_length(text: &str, max_len: usize) -> Result<(), TextError> {
    let len = text.chars().count();
    if len > max_len {
        warn!(
            text = %truncate_for_log(text),
            length = len,
            max_len,
            "text exceeds length limit"
        );
        return Err(TextError::TooLong(len));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn short_text_is_returned_unchanged() {
        assert_eq!(split_paragraphs("hello", 2000), vec!["hello"]);
    }

    #[test]
    fn newlines_separate_paragraphs() {
        assert_eq!(
            split_paragraphs("para one\npara two\npara three", 2000),
            vec!["para one", "para two", "para three"]
        );
    }

    #[test]
    fn blank_lines_are_dropped() {
        assert_eq!(split_paragraphs("a\n\nb\n\n\nc", 2000), vec!["a", "b", "c"]);
    }

    #[test]
    fn paragraphs_are_trimmed() {
        assert_eq!(split_paragraphs("  a  \n  b  ", 2000), vec!["a", "b"]);
    }

    #[test]
    fn empty_input_yields_no_paragraphs() {
        assert!(split_paragraphs("", 2000).is_empty());
        assert!(split_paragraphs("\n  \n", 2000).is_empty());
    }

    #[test]
    fn hard_split_when_no_soft_boundary() {
        assert_eq!(split_paragraphs("abcdefgh", 3), vec!["abc", "def", "gh"]);
    }

    #[test]
    fn split_at_whitespace_boundary() {
        assert_eq!(
            split_paragraphs("aaaa bbbb cccc", 6),
            vec!["aaaa", "bbbb", "cccc"]
        );
    }

    #[test]
    fn consecutive_spaces_do_not_leave_trailing_whitespace() {
        assert_eq!(split_paragraphs("aaaa  bbbb", 5), vec!["aaaa", "bbbb"]);
    }

    #[test]
    fn split_at_sentence_ender() {
        assert_eq!(
            split_paragraphs("AAAA. BBBB. CCCC.", 6),
            vec!["AAAA.", "BBBB.", "CCCC."]
        );
    }

    #[test]
    fn cjk_sentence_enders_are_respected() {
        assert_eq!(
            split_paragraphs("こんにちは。また明日。さようなら。", 8),
            vec!["こんにちは。", "また明日。", "さようなら。"]
        );
    }

    #[test]
    fn hard_split_respects_unicode_boundaries() {
        assert_eq!(
            split_paragraphs("こんにちは世界", 3),
            vec!["こんに", "ちは世", "界"]
        );
    }

    #[test]
    fn every_chunk_stays_within_max_len() {
        let inputs = [
            "The quick brown fox jumps over the lazy dog. ",
            "あいうえおかきくけこさしすせそたちつてと",
            "a\nb\nccccc",
        ];
        for input in inputs {
            for max_len in [1, 2, 3, 7, 10, 2000] {
                for chunk in split_paragraphs(input, max_len) {
                    assert!(
                        chunk.chars().count() <= max_len,
                        "chunk too long: {chunk:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn split_never_panics_and_upholds_invariants() {
        // A deterministic stress sweep over pathological inputs: the splitter
        // must never panic and every chunk must be non-empty and within the
        // limit, for every limit from 1 upward.
        let inputs = [
            "こんにちは。また明日。さようなら。",
            "The quick brown fox jumps over the lazy dog. Really? Yes!",
            "a\tb\u{3000}c  \u{200b}d",
            "line1\nline2\n\nline3\n",
            "no-break-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "。！？；",
            "1234567890 1234567890 1234567890",
            "\u{feff}\u{200b}   \u{200c}",
        ];
        for input in inputs {
            for max_len in 1..=12 {
                let chunks = split_paragraphs(input, max_len);
                for chunk in &chunks {
                    assert!(!chunk.is_empty(), "empty chunk for {input:?}");
                    assert!(
                        chunk.chars().count() <= max_len,
                        "chunk too long for {input:?} with max_len {max_len}: {chunk:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn zero_max_len_is_unlimited() {
        assert_eq!(split_paragraphs("long text", 0), vec!["long text"]);
        assert_eq!(split_paragraphs("a\nb", 0), vec!["a", "b"]);
    }

    #[test]
    fn exactly_at_limit_is_single_chunk() {
        assert_eq!(split_paragraphs("abcd", 4), vec!["abcd"]);
    }

    #[test]
    fn validate_length_accepts_short_text() {
        assert!(validate_length("hello", 5).is_ok());
        assert!(validate_length("", 0).is_ok());
    }

    #[test]
    fn validate_length_rejects_long_text() {
        let err = validate_length("hello", 3).unwrap_err();
        assert!(matches!(err, TextError::TooLong(5)));
        assert_eq!(err.to_string(), "text too long: 5 chars");
    }

    #[test]
    fn take_chunk_prefers_sentence_ender_over_whitespace() {
        // "AAA. BBB" with max_len 7: window is "AAA. BB"; the sentence ender
        // (index 3) is preferred over the whitespace (index 4).
        let (chunk, rest) = take_chunk("AAA. BBB", 7);
        assert_eq!(chunk, "AAA.");
        assert_eq!(rest, " BBB");
    }
}
