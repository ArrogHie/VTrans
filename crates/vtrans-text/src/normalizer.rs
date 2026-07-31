//! Text cleaning and OCR line merging.
//!
//! This module implements `TextNormalizer`, the entry point of the
//! `vtrans-text` crate:
//!
//! - `TextNormalizer::clean` removes invisible characters and abnormal
//!   whitespace and normalizes fullwidth ASCII forms. It is intentionally
//!   language-neutral; Japanese-specific punctuation rules live in
//!   `crate::japanese`.
//! - `TextNormalizer::merge_lines` merges OCR lines into paragraphs using
//!   their vertical positions, inserting spaces only where a natural word
//!   boundary exists.
//! - `TextNormalizer::fingerprint` and `crate::is_duplicate` support
//!   duplicate detection for live translation.
//! - `TextNormalizer::split_paragraphs` limits chunk length for
//!   translation providers.

use tracing::{debug, instrument};
use vtrans_core::{truncate_for_log, OcrLine};

use crate::fingerprint::fingerprint_text;
use crate::paragraph::{split_paragraphs, validate_length, DEFAULT_MAX_PARAGRAPH_LEN};
use crate::TextError;

/// Ratio of the average line height used as the maximum vertical gap for
/// two OCR lines to be considered part of the same paragraph.
///
/// A gap larger than this ratio times the average of the two lines' heights
/// starts a new paragraph. Smaller values split paragraphs more eagerly.
///
/// # Example
///
/// ```
/// use vtrans_text::normalizer::MERGE_LINE_GAP_RATIO;
///
/// assert_eq!(MERGE_LINE_GAP_RATIO, 0.75);
/// ```
pub const MERGE_LINE_GAP_RATIO: f32 = 0.75;

/// Smallest vertical gap (in pixels) that still merges two lines.
///
/// Prevents tiny lines from being split into separate paragraphs when the
/// ratio-based threshold would round to almost nothing.
const MIN_MERGE_GAP_PX: f32 = 2.0;

/// Vertical gap (in pixels) used when line heights cannot be determined
/// (degenerate or empty polygons).
const DEFAULT_MERGE_GAP_PX: f32 = 8.0;

/// The text normalizer.
///
/// All methods are pure functions; the struct is a stateless namespace that
/// groups the crate's normalization operations behind one name.
pub struct TextNormalizer;

impl TextNormalizer {
    /// Cleans raw text for translation.
    ///
    /// Performs, in order:
    /// 1. line-ending normalization (`\r\n` and `\r` become `\n`,
    ///    and Unicode line/paragraph separators become `\n`);
    /// 2. removal of invisible characters (zero-width spaces, bidi controls,
    ///    BOM, soft hyphens, control characters);
    /// 3. whitespace normalization: every Unicode space (including the
    ///    ideographic space U+3000) collapses to a single ASCII space, and
    ///    spaces around line breaks are trimmed;
    /// 4. fullwidth-to-halfwidth mapping for ASCII forms (letters, digits,
    ///    symbols). The fullwidth comma `，`, full stop `．`, and tilde
    ///    `～` are deliberately left untouched because their conventional
    ///    replacements depend on the source language - see
    ///    `crate::japanese::normalize_punctuation`.
    ///
    /// Proper nouns and word order are never modified.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_text::TextNormalizer;
    ///
    /// let raw = "ＨＰ\u{3000}１００  \u{200b}攻撃力\n  アップ";
    /// assert_eq!(TextNormalizer::clean(raw), "HP 100 攻撃力\nアップ");
    /// ```
    #[must_use]
    #[instrument(skip(raw), fields(sample = %truncate_for_log(raw)))]
    pub fn clean(raw: &str) -> String {
        let mut out = String::with_capacity(raw.len());
        for ch in normalize_line_endings(raw).chars() {
            match ch {
                '\n' | '\u{2028}' | '\u{2029}' | '\u{0085}' => out.push('\n'),
                c if is_invisible_char(c) => {}
                c if is_horizontal_space(c) => {
                    if !out.is_empty() && !out.ends_with(' ') && !out.ends_with('\n') {
                        out.push(' ');
                    }
                }
                c if is_removed_control(c) => {}
                c => out.push(fullwidth_to_ascii(c)),
            }
        }
        out.split('\n')
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    }

    /// Merges OCR lines into paragraphs, preserving necessary line breaks.
    ///
    /// Lines are ordered by `OcrLine::reading_order`. Consecutive lines
    /// whose vertical gap (difference between the bottom of one line's
    /// polygon and the top of the next) is at most
    /// `MERGE_LINE_GAP_RATIO` times the average line height belong to the
    /// same paragraph and are joined on one line. Otherwise a `\n`
    /// separates paragraphs.
    ///
    /// Within a paragraph, lines are joined without a space between CJK
    /// characters, with a space between ASCII words, and with a space after
    /// ASCII punctuation that is followed by an ASCII letter (so
    /// `"Hello," + "world"` becomes `"Hello, world"` while `"1," + "000"`
    /// stays `"1,000"`). Empty lines are skipped.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_core::OcrLine;
    /// use vtrans_text::TextNormalizer;
    ///
    /// // Two lines of the same paragraph, then one line of a new paragraph.
    /// let lines = vec![
    ///     OcrLine::new("Hello", 0.9, [[0., 10.], [40., 10.], [40., 30.], [0., 30.]], 0),
    ///     OcrLine::new("world", 0.9, [[45., 10.], [85., 10.], [85., 30.], [45., 30.]], 1),
    ///     OcrLine::new("Next", 0.9, [[0., 60.], [30., 60.], [30., 80.], [0., 80.]], 2),
    /// ];
    /// assert_eq!(TextNormalizer::merge_lines(&lines), "Hello world\nNext");
    /// ```
    #[must_use]
    #[instrument(skip(lines), fields(line_count = lines.len()))]
    pub fn merge_lines(lines: &[OcrLine]) -> String {
        let mut usable: Vec<&OcrLine> = lines
            .iter()
            .filter(|line| !line.text.trim().is_empty())
            .collect();
        usable.sort_by_key(|line| line.reading_order);
        if usable.is_empty() {
            return String::new();
        }

        let mut paragraphs: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut previous: Option<LineGeometry> = None;

        for line in usable {
            let text = line.text.trim().replace(['\r', '\n'], " ");
            let geometry = LineGeometry::from_polygon(&line.polygon);
            if let Some(prev) = previous {
                if !same_paragraph(&prev, &geometry) {
                    debug!(
                        paragraph = paragraphs.len() + 1,
                        "closing paragraph; vertical gap starts a new paragraph"
                    );
                    paragraphs.push(std::mem::take(&mut current));
                }
            }
            if let Some(prev_last) = current.chars().last() {
                if let Some(curr_first) = text.chars().next() {
                    if should_join_with_space(prev_last, curr_first) {
                        current.push(' ');
                    }
                }
            }
            current.push_str(&text);
            previous = Some(geometry);
        }
        if !current.is_empty() {
            paragraphs.push(current);
        }
        debug!(
            paragraph_count = paragraphs.len(),
            "merged OCR lines into paragraphs"
        );
        paragraphs.join("\n")
    }

    /// Computes the duplicate-detection fingerprint of `text`.
    ///
    /// Whitespace and line breaks do not affect the fingerprint, so OCR
    /// jitter between frames does not defeat deduplication; any change in
    /// wording does. See `crate::is_duplicate`.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_text::TextNormalizer;
    ///
    /// assert_eq!(
    ///     TextNormalizer::fingerprint("hello world"),
    ///     TextNormalizer::fingerprint(" hello\nworld ")
    /// );
    /// ```
    #[must_use]
    #[instrument(skip(text), fields(sample = %truncate_for_log(text)))]
    pub fn fingerprint(text: &str) -> u64 {
        fingerprint_text(text)
    }

    /// Splits `text` into paragraphs of at most `max_len` characters.
    ///
    /// See `crate::paragraph` for the splitting rules.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_text::TextNormalizer;
    ///
    /// assert_eq!(
    ///     TextNormalizer::split_paragraphs("abc def ghi", 7),
    ///     vec!["abc def", "ghi"]
    /// );
    /// ```
    #[must_use]
    #[instrument(skip(text), fields(sample = %truncate_for_log(text), max_len = max_len))]
    pub fn split_paragraphs(text: &str, max_len: usize) -> Vec<String> {
        split_paragraphs(text, max_len)
    }

    /// Splits `text` using the default maximum paragraph length.
    ///
    /// Equivalent to
    /// `split_paragraphs` with `DEFAULT_MAX_PARAGRAPH_LEN`.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_text::TextNormalizer;
    ///
    /// let chunks = TextNormalizer::split_paragraphs_default("hello");
    /// assert_eq!(chunks, vec!["hello"]);
    /// ```
    #[must_use]
    #[instrument(skip(text), fields(sample = %truncate_for_log(text)))]
    pub fn split_paragraphs_default(text: &str) -> Vec<String> {
        split_paragraphs(text, DEFAULT_MAX_PARAGRAPH_LEN)
    }

    /// Checks that `text` is at most `max_len` characters long.
    ///
    /// Useful as a guard before sending a single chunk to a translation
    /// provider.
    ///
    /// # Errors
    ///
    /// Returns `TextError::TooLong` with the actual character count when
    /// `text` exceeds `max_len`.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_text::TextNormalizer;
    ///
    /// assert!(TextNormalizer::validate_length("hello", 10).is_ok());
    /// assert!(TextNormalizer::validate_length("hello", 4).is_err());
    /// ```
    #[instrument(skip(text), fields(sample = %truncate_for_log(text), max_len = max_len))]
    pub fn validate_length(text: &str, max_len: usize) -> Result<(), TextError> {
        validate_length(text, max_len)
    }
}

/// Replaces every line ending with a single `\n`.
fn normalize_line_endings(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(ch);
        }
    }
    out
}

/// Returns `true` for characters that are invisible when rendered and
/// carry no meaning for translation or deduplication.
#[must_use]
pub(crate) fn is_invisible_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{00AD}'                 // soft hyphen
            | '\u{061C}'           // arabic letter mark
            | '\u{180E}'           // mongolian vowel separator
            | '\u{200B}'           // zero width space
            | '\u{200C}'           // zero width non-joiner
            | '\u{200D}'           // zero width joiner
            | '\u{200E}'           // left-to-right mark
            | '\u{200F}'           // right-to-left mark
            | '\u{202A}'..='\u{202E}' // bidi embeddings and overrides
            | '\u{2060}'           // word joiner
            | '\u{2061}'..='\u{2064}' // invisible operators
            | '\u{2066}'..='\u{2069}' // bidi isolates
            | '\u{FEFF}'           // zero width no-break space / BOM
    )
}

/// Returns `true` for horizontal whitespace (excluding line breaks).
#[must_use]
fn is_horizontal_space(ch: char) -> bool {
    ch.is_whitespace() && ch != '\n'
}

/// Returns `true` for control characters that are not meaningful in text.
///
/// `\n` and `\t` are handled by the whitespace/line-break rules;
/// `\r` is normalized to `\n` before this function is reached.
#[must_use]
fn is_removed_control(ch: char) -> bool {
    ch.is_control() && !matches!(ch, '\n' | '\r' | '\t')
}

/// Maps a fullwidth ASCII form to its halfwidth counterpart.
///
/// The fullwidth comma `，` (U+FF0C), full stop `．` (U+FF0E), and
/// tilde `～` (U+FF5E) are returned unchanged because their conventional
/// replacement depends on the source language (see
/// `crate::japanese::normalize_punctuation`). All other characters in
/// U+FF01..=U+FF5E map linearly onto U+0021..=U+007E.
///
/// `pub(crate)` so the paragraph splitter can reuse the same fullwidth
/// semantics when deciding whether a window boundary is mid-word.
#[must_use]
pub(crate) fn fullwidth_to_ascii(ch: char) -> char {
    let code = u32::from(ch);
    if matches!(code, 0xFF0C | 0xFF0E | 0xFF5E) {
        return ch;
    }
    if (0xFF01..=0xFF5E).contains(&code) {
        // SAFETY-free: every code in the range maps to a valid ASCII scalar
        // (0x21..=0x7E), so from_u32 always succeeds for these inputs.
        char::from_u32(code - 0xFEE0).unwrap_or(ch)
    } else {
        ch
    }
}

/// Classification of a character used to decide whether two OCR lines
/// should be joined with a space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    /// ASCII letter (`a`-`z`, `A`-`Z`).
    AsciiLetter,
    /// ASCII digit (`0`-`9`).
    AsciiDigit,
    /// ASCII punctuation.
    AsciiPunct,
    /// CJK word characters: ideographs and kana.
    CjkWord,
    /// CJK punctuation (ideographic comma, full stop, brackets, ...).
    CjkPunct,
    /// Anything else (symbols, emoji, ...).
    Other,
}

/// Classifies `ch`, treating fullwidth ASCII forms by their ASCII
/// meaning.
fn char_class(ch: char) -> CharClass {
    let ch = fullwidth_to_ascii(ch);
    if ch.is_ascii_alphanumeric() {
        if ch.is_ascii_digit() {
            CharClass::AsciiDigit
        } else {
            CharClass::AsciiLetter
        }
    } else if ch.is_ascii_punctuation() {
        CharClass::AsciiPunct
    } else if is_cjk_word(ch) {
        CharClass::CjkWord
    } else if is_cjk_punct(ch) {
        CharClass::CjkPunct
    } else {
        CharClass::Other
    }
}

/// Returns `true` for CJK word characters (ideographs and kana).
///
/// `pub(crate)` so the paragraph splitter can reuse the same CJK semantics
/// when deciding whether a window boundary is mid-word.
#[must_use]
pub(crate) fn is_cjk_word(ch: char) -> bool {
    matches!(
        ch,
        '\u{3040}'..='\u{309F}' // hiragana
            | '\u{30A0}'..='\u{30FF}' // katakana
            | '\u{31F0}'..='\u{31FF}' // katakana phonetic extensions
            | '\u{3400}'..='\u{4DBF}' // CJK unified ideographs extension A
            | '\u{4E00}'..='\u{9FFF}' // CJK unified ideographs
            | '\u{F900}'..='\u{FAFF}' // CJK compatibility ideographs
    )
}

/// Returns `true` for CJK punctuation.
#[must_use]
fn is_cjk_punct(ch: char) -> bool {
    matches!(
        ch,
        '\u{3001}'
            ..='\u{303F}' // ideographic comma, full stop, brackets, ...
            | '\u{FF0C}' | '\u{FF0E}' | '\u{FF5E}' // fullwidth comma / full stop / tilde
            | '\u{FF61}' | '\u{FF64}' // halfwidth ideographic full stop / comma
    )
}

/// Decides whether two adjacent line fragments should be joined with a
/// space. CJK text is joined directly; ASCII words are separated by spaces;
/// ASCII punctuation followed by an ASCII letter gets a space (but a comma
/// followed by a digit does not, to keep numbers like `1,000` intact).
fn should_join_with_space(prev_last: char, curr_first: char) -> bool {
    matches!(
        (char_class(prev_last), char_class(curr_first)),
        (
            CharClass::AsciiLetter | CharClass::AsciiDigit | CharClass::CjkWord,
            CharClass::AsciiLetter | CharClass::AsciiDigit,
        ) | (
            CharClass::AsciiLetter | CharClass::AsciiDigit,
            CharClass::CjkWord
        ) | (CharClass::AsciiPunct, CharClass::AsciiLetter)
    )
}

/// Vertical geometry of an OCR line, derived from its polygon.
#[derive(Debug, Clone, Copy)]
struct LineGeometry {
    /// Y of the polygon's top edge.
    top: f32,
    /// Y of the polygon's bottom edge.
    bottom: f32,
    /// Line height in pixels (`bottom - top`, never negative).
    height: f32,
}

impl LineGeometry {
    /// Computes the geometry from a polygon's Y coordinates.
    #[must_use]
    fn from_polygon(polygon: &[[f32; 2]; 4]) -> Self {
        let top = polygon
            .iter()
            .map(|point| point[1])
            .fold(f32::INFINITY, f32::min);
        let bottom = polygon
            .iter()
            .map(|point| point[1])
            .fold(f32::NEG_INFINITY, f32::max);
        let height = (bottom - top).max(0.0);
        if height == 0.0 {
            debug!("line has a degenerate polygon; falling back to the default merge gap");
        }
        Self {
            top,
            bottom,
            height,
        }
    }
}

/// Returns `true` when two consecutive lines belong to the same
/// paragraph.
fn same_paragraph(previous: &LineGeometry, current: &LineGeometry) -> bool {
    let average_height = (previous.height + current.height) / 2.0;
    let threshold = if average_height > 0.0 {
        (average_height * MERGE_LINE_GAP_RATIO).max(MIN_MERGE_GAP_PX)
    } else {
        DEFAULT_MERGE_GAP_PX
    };
    let gap = current.top - previous.bottom;
    gap <= threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Builds an OCR line with a simple horizontal box.
    fn line(text: &str, top: f32, height: f32, order: usize) -> OcrLine {
        let bottom = top + height;
        OcrLine::new(
            text,
            0.9,
            [[0., top], [50., top], [50., bottom], [0., bottom]],
            order,
        )
    }

    // ── clean ──

    #[test]
    fn clean_removes_zero_width_characters() {
        let raw = "Hel\u{200b}lo\u{feff}世界";
        assert_eq!(TextNormalizer::clean(raw), "Hello世界");
    }

    #[test]
    fn clean_converts_fullwidth_space_to_halfwidth() {
        assert_eq!(TextNormalizer::clean("あ\u{3000}い"), "あ い");
        assert_eq!(TextNormalizer::clean("a\u{3000}b"), "a b");
    }

    #[test]
    fn clean_collapses_multiple_spaces() {
        assert_eq!(TextNormalizer::clean("a   b\t\u{00a0}c"), "a b c");
    }

    #[test]
    fn clean_normalizes_line_endings() {
        assert_eq!(TextNormalizer::clean("a\r\nb"), "a\nb");
        assert_eq!(TextNormalizer::clean("a\rb"), "a\nb");
        assert_eq!(TextNormalizer::clean("a\u{2028}b"), "a\nb");
    }

    #[test]
    fn clean_removes_invisible_and_control_characters() {
        assert_eq!(TextNormalizer::clean("a\u{00ad}b"), "ab");
        assert_eq!(TextNormalizer::clean("a\u{200e}b"), "ab");
        assert_eq!(TextNormalizer::clean("a\u{0007}b"), "ab");
    }

    #[test]
    fn clean_converts_fullwidth_ascii_forms() {
        assert_eq!(TextNormalizer::clean("ＨＰ１００ＡＢＣ"), "HP100ABC");
        assert_eq!(TextNormalizer::clean("（注）！？"), "(注)!?");
    }

    #[test]
    fn clean_keeps_language_specific_fullwidth_punctuation() {
        assert_eq!(TextNormalizer::clean("あ，い．う～"), "あ，い．う～");
    }

    #[test]
    fn clean_trims_edges_and_keeps_blank_lines() {
        assert_eq!(TextNormalizer::clean("  a  "), "a");
        assert_eq!(TextNormalizer::clean("a \n b"), "a\nb");
        assert_eq!(TextNormalizer::clean("a\n\nb"), "a\n\nb");
    }

    #[test]
    fn clean_preserves_cjk_text_unchanged() {
        let text = "こんにちは、世界。";
        assert_eq!(TextNormalizer::clean(text), text);
    }

    #[test]
    fn clean_empty_and_whitespace_only() {
        assert_eq!(TextNormalizer::clean(""), "");
        assert_eq!(TextNormalizer::clean("   \n  "), "");
    }

    #[test]
    fn clean_is_idempotent() {
        let inputs = [
            "ＨＰ\u{3000}１００  \u{200b}攻撃力\n  アップ",
            "a\r\nb\rc",
            "  x  \t y  ",
            "あ，い．う～",
            "a\n\nb\n",
            "",
        ];
        for input in inputs {
            let once = TextNormalizer::clean(input);
            let twice = TextNormalizer::clean(&once);
            assert_eq!(once, twice, "clean is not idempotent for {input:?}");
        }
    }

    // ── merge_lines ──

    #[test]
    fn merge_lines_joins_ascii_lines_with_space() {
        let lines = vec![line("Hello", 0.0, 20.0, 0), line("world", 0.0, 20.0, 1)];
        assert_eq!(TextNormalizer::merge_lines(&lines), "Hello world");
    }

    #[test]
    fn merge_lines_joins_cjk_lines_without_space() {
        let lines = vec![line("こんにちは", 0.0, 20.0, 0), line("世界", 0.0, 20.0, 1)];
        assert_eq!(TextNormalizer::merge_lines(&lines), "こんにちは世界");
    }

    #[test]
    fn merge_lines_separates_paragraphs_by_vertical_gap() {
        let lines = vec![
            line("First", 0.0, 20.0, 0),
            line("Second", 0.0, 20.0, 1),
            line("Next paragraph", 60.0, 20.0, 2),
        ];
        assert_eq!(
            TextNormalizer::merge_lines(&lines),
            "First Second\nNext paragraph"
        );
    }

    #[test]
    fn merge_lines_respects_reading_order() {
        let lines = vec![line("second", 0.0, 20.0, 1), line("first", 0.0, 20.0, 0)];
        assert_eq!(TextNormalizer::merge_lines(&lines), "first second");
    }

    #[test]
    fn merge_lines_skips_empty_lines() {
        let lines = vec![
            line("   ", 0.0, 20.0, 0),
            line("keep", 0.0, 20.0, 1),
            line("", 10.0, 20.0, 2),
        ];
        assert_eq!(TextNormalizer::merge_lines(&lines), "keep");
    }

    #[test]
    fn merge_lines_empty_input() {
        assert_eq!(TextNormalizer::merge_lines(&[]), "");
    }

    #[test]
    fn merge_lines_adds_space_after_ascii_punctuation_for_letters() {
        let lines = vec![line("Hello,", 0.0, 20.0, 0), line("world", 0.0, 20.0, 1)];
        assert_eq!(TextNormalizer::merge_lines(&lines), "Hello, world");
    }

    #[test]
    fn merge_lines_does_not_split_numbers() {
        let lines = vec![line("1,", 0.0, 20.0, 0), line("000", 0.0, 20.0, 1)];
        assert_eq!(TextNormalizer::merge_lines(&lines), "1,000");
    }

    #[test]
    fn merge_lines_close_gap_merges_far_gap_splits() {
        // Line height 20 => threshold 15 px. A 5 px gap merges, a 25 px gap splits.
        let merged = vec![line("a", 0.0, 20.0, 0), line("b", 25.0, 20.0, 1)];
        assert_eq!(TextNormalizer::merge_lines(&merged), "a b");
        let split = vec![line("a", 0.0, 20.0, 0), line("b", 45.0, 20.0, 1)];
        assert_eq!(TextNormalizer::merge_lines(&split), "a\nb");
    }

    #[test]
    fn merge_lines_overlapping_boxes_merge() {
        // Slightly overlapping boxes are the same line, not two paragraphs.
        let lines = vec![line("a", 0.0, 20.0, 0), line("b", 15.0, 20.0, 1)];
        assert_eq!(TextNormalizer::merge_lines(&lines), "a b");
    }

    #[test]
    fn merge_lines_degenerate_polygons_use_default_gap() {
        // All polygons are zero: heights are unknown, so the default gap
        // threshold (8 px) applies; 5 px merges, 30 px splits.
        let near = vec![
            OcrLine::new("a", 0.9, [[0., 0.], [0., 0.], [0., 0.], [0., 0.]], 0),
            OcrLine::new("b", 0.9, [[0., 5.], [0., 5.], [0., 5.], [0., 5.]], 1),
        ];
        assert_eq!(TextNormalizer::merge_lines(&near), "a b");
        let far = vec![
            OcrLine::new("a", 0.9, [[0., 0.], [0., 0.], [0., 0.], [0., 0.]], 0),
            OcrLine::new("b", 0.9, [[0., 30.], [0., 30.], [0., 30.], [0., 30.]], 1),
        ];
        assert_eq!(TextNormalizer::merge_lines(&far), "a\nb");
    }

    // ── delegates ──

    #[test]
    fn fingerprint_delegates_to_fingerprint_module() {
        assert_eq!(
            TextNormalizer::fingerprint("a b"),
            TextNormalizer::fingerprint("a\nb")
        );
        assert_ne!(
            TextNormalizer::fingerprint("a"),
            TextNormalizer::fingerprint("b")
        );
    }

    #[test]
    fn split_paragraphs_delegates() {
        // "aa bb" fills the 5-character window exactly; "cc" remains.
        assert_eq!(
            TextNormalizer::split_paragraphs("aa bb cc", 5),
            vec!["aa bb", "cc"]
        );
        assert_eq!(
            TextNormalizer::split_paragraphs_default("hello"),
            vec!["hello"]
        );
    }

    #[test]
    fn validate_length_delegates() {
        assert!(TextNormalizer::validate_length("ok", 2).is_ok());
        assert!(matches!(
            TextNormalizer::validate_length("too long", 4),
            Err(TextError::TooLong(8))
        ));
    }

    // ── helpers ──

    #[test]
    fn normalize_line_endings_handles_all_forms() {
        assert_eq!(normalize_line_endings("a\r\nb\rc"), "a\nb\nc");
    }

    #[test]
    fn fullwidth_mapping_roundtrip() {
        assert_eq!(fullwidth_to_ascii('ａ'), 'a');
        assert_eq!(fullwidth_to_ascii('１'), '1');
        assert_eq!(fullwidth_to_ascii('（'), '(');
        assert_eq!(fullwidth_to_ascii('，'), '，');
        assert_eq!(fullwidth_to_ascii('．'), '．');
        assert_eq!(fullwidth_to_ascii('～'), '～');
        assert_eq!(fullwidth_to_ascii('日'), '日');
    }

    #[test]
    fn char_classification_drives_spacing() {
        assert_eq!(char_class('A'), CharClass::AsciiLetter);
        assert_eq!(char_class('9'), CharClass::AsciiDigit);
        assert_eq!(char_class(','), CharClass::AsciiPunct);
        assert_eq!(char_class('日'), CharClass::CjkWord);
        assert_eq!(char_class('あ'), CharClass::CjkWord);
        assert_eq!(char_class('。'), CharClass::CjkPunct);
        assert_eq!(char_class('😀'), CharClass::Other);
        // Fullwidth letters are classified by their ASCII meaning.
        assert_eq!(char_class('Ａ'), CharClass::AsciiLetter);
        // Fullwidth comma is deferred to the Japanese rules and stays CJK.
        assert_eq!(char_class('，'), CharClass::CjkPunct);
    }

    #[test]
    fn spacing_decisions() {
        assert!(should_join_with_space('a', 'b'));
        assert!(!should_join_with_space('日', '本'));
        assert!(should_join_with_space('a', '日'));
        assert!(should_join_with_space('日', 'a'));
        assert!(should_join_with_space(',', 'w'));
        assert!(!should_join_with_space(',', '0'));
        assert!(!should_join_with_space('。', '次'));
        // No space between a number and CJK punctuation.
        assert!(!should_join_with_space('０', '，'));
        assert!(!should_join_with_space('0', '。'));
    }

    #[test]
    fn geometry_from_polygon() {
        let geometry =
            LineGeometry::from_polygon(&[[10., 20.], [30., 20.], [30., 50.], [10., 50.]]);
        assert!((geometry.top - 20.0).abs() < f32::EPSILON);
        assert!((geometry.bottom - 50.0).abs() < f32::EPSILON);
        assert!((geometry.height - 30.0).abs() < f32::EPSILON);
    }
}
