//! Japanese punctuation normalization rules.
//!
//! OCR engines frequently emit punctuation forms that differ from the
//! conventional Japanese glyphs. This module maps those forms to their
//! standard Japanese equivalents:
//!
//! | Input | Name | Output | Name |
//! |-------|------|--------|------|
//! | `，` (U+FF0C) | fullwidth comma | `、` (U+3001) | ideographic comma |
//! | `．` (U+FF0E) | fullwidth full stop | `。` (U+3002) | ideographic full stop |
//! | `､` (U+FF64) | halfwidth ideographic comma | `、` (U+3001) | ideographic comma |
//! | `｡` (U+FF61) | halfwidth ideographic full stop | `。` (U+3002) | ideographic full stop |
//! | `～` (U+FF5E) | fullwidth tilde | `〜` (U+301C) | wave dash |
//!
//! The rules are character-level and perform no language detection. Apply
//! them only when the source text is known to be Japanese; applying them to
//! Chinese text would rewrite its `，` into `、` and change its meaning.

use tracing::instrument;
use vtrans_core::truncate_for_log;

/// Normalizes Japanese punctuation in `text`.
///
/// See the [module documentation](self) for the exact character mappings.
/// Non-Japanese characters and already-correct punctuation are left
/// untouched. The intended composition with the locale-agnostic cleaner is:
///
/// ```
/// use vtrans_text::{TextNormalizer, japanese};
///
/// let cleaned = TextNormalizer::clean("ＨＰ １００，攻撃力アップ．");
/// let text = japanese::normalize_punctuation(&cleaned);
/// assert_eq!(text, "HP 100、攻撃力アップ。");
/// ```
#[must_use]
#[instrument(skip(text), fields(sample = %truncate_for_log(text)))]
pub fn normalize_punctuation(text: &str) -> String {
    text.chars().map(normalize_char).collect()
}

/// Maps a single character through the Japanese punctuation table.
fn normalize_char(ch: char) -> char {
    match ch {
        // Fullwidth and halfwidth forms of the same glyph map together.
        '\u{FF0C}' | '\u{FF64}' => '\u{3001}', // comma -> ideographic comma
        '\u{FF0E}' | '\u{FF61}' => '\u{3002}', // full stop -> ideographic full stop
        '\u{FF5E}' => '\u{301C}',              // fullwidth tilde -> wave dash
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn fullwidth_comma_to_ideographic_comma() {
        assert_eq!(normalize_punctuation("あ，い"), "あ、い");
    }

    #[test]
    fn fullwidth_full_stop_to_ideographic_full_stop() {
        assert_eq!(normalize_punctuation("文．"), "文。");
    }

    #[test]
    fn halfwidth_punctuation_to_fullwidth() {
        assert_eq!(normalize_punctuation("｡､"), "。、");
    }

    #[test]
    fn fullwidth_tilde_to_wave_dash() {
        assert_eq!(normalize_punctuation("あ〜い"), "あ〜い");
        assert_eq!(normalize_punctuation("あ～い"), "あ〜い");
    }

    #[test]
    fn already_normalized_text_is_unchanged() {
        let text = "こんにちは、世界。";
        assert_eq!(normalize_punctuation(text), text);
    }

    #[test]
    fn ascii_text_is_unchanged() {
        assert_eq!(normalize_punctuation("Hello, world!"), "Hello, world!");
    }

    #[test]
    fn empty_string() {
        assert_eq!(normalize_punctuation(""), "");
    }

    #[test]
    fn mixed_text_normalizes_only_japanese_forms() {
        let input = "Lv.５，ＨＰ １００／３００．";
        let expected = "Lv.５、ＨＰ １００／３００。";
        assert_eq!(normalize_punctuation(input), expected);
    }

    #[test]
    fn composes_with_clean() {
        let cleaned = crate::TextNormalizer::clean("ＨＰ １００，攻撃力アップ．");
        assert_eq!(normalize_punctuation(&cleaned), "HP 100、攻撃力アップ。");
    }
}
