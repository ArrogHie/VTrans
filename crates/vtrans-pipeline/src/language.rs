//! Source-language resolution for the translation stage.
//!
//! The local translation engines only accept concrete source languages
//! (`en` / `ja` / `zh-CN`); `Auto` must be resolved by the pipeline before
//! a request reaches a provider. Resolution follows a fixed order (see the
//! translation integration guide, §8):
//!
//! 1. the configured source language, when it is concrete;
//! 2. the OCR-detected language, when the configured source is `Auto` and
//!    the detection is concrete;
//! 3. a Unicode-script heuristic on the recognized text
//!    ([`heuristic_detect_language`]);
//! 4. `Auto` when none of the above applies (the provider then reports an
//!    unsupported-pair error, and the UI prompts the user to choose a
//!    language explicitly).
//!
//! Steps 1, 2 and 4 are implemented by [`resolve_translation_source`].
//! `resolve_effective_source` composes all four steps and is what the
//! single/live pipeline modes call after OCR.

use tracing::debug;
use vtrans_core::types::Language;

/// Resolves the actual translation source language.
///
/// A concrete configured source is returned unchanged. When the configured
/// source is [`Language::Auto`], the OCR-detected language is used if it
/// is concrete (`en` / `ja` / `zh-CN`); otherwise `Auto` is returned.
///
/// The Unicode-heuristic fallback (step 3) needs the recognized text, which
/// this two-argument function does not receive; the pipeline applies it
/// through `resolve_effective_source` before translating.
///
/// # Example
///
/// ```
/// use vtrans_core::Language;
/// use vtrans_pipeline::resolve_translation_source;
///
/// // A concrete configured source always wins.
/// assert_eq!(
///     resolve_translation_source(Some(Language::Japanese), Language::English),
///     Language::English
/// );
/// // Auto prefers the OCR detection.
/// assert_eq!(
///     resolve_translation_source(Some(Language::Japanese), Language::Auto),
///     Language::Japanese
/// );
/// // No usable detection leaves the source as Auto.
/// assert_eq!(resolve_translation_source(None, Language::Auto), Language::Auto);
/// ```
#[must_use]
pub fn resolve_translation_source(detected: Option<Language>, configured: Language) -> Language {
    if !configured.is_auto() {
        return configured;
    }
    match detected {
        Some(lang @ (Language::English | Language::Japanese | Language::ChineseSimplified)) => lang,
        _ => Language::Auto,
    }
}

/// Detects the language of `text` by Unicode script heuristics.
///
/// Returns [`Language::Japanese`] when the text contains any hiragana
/// (U+3040-U+309F), katakana (U+30A0-U+30FF), or halfwidth katakana
/// (U+FF65-U+FF9F). Otherwise returns [`Language::English`] when Latin
/// letters dominate the non-whitespace characters, and `None` when the
/// script signal is ambiguous (for example kanji-only text, digits, or
/// punctuation).
///
/// This is a cheap routing heuristic, not a language detector: kanji-only
/// text can be either Chinese or Japanese, so callers should prefer an
/// explicit user choice or OCR detection when available (guide §8).
///
/// # Example
///
/// ```
/// use vtrans_core::Language;
/// use vtrans_pipeline::heuristic_detect_language;
///
/// assert_eq!(heuristic_detect_language("こんにちは"), Some(Language::Japanese));
/// assert_eq!(heuristic_detect_language("Hello world"), Some(Language::English));
/// assert_eq!(heuristic_detect_language("12345"), None);
/// ```
#[must_use]
pub fn heuristic_detect_language(text: &str) -> Option<Language> {
    if text.chars().any(is_kana) {
        return Some(Language::Japanese);
    }
    let latin = text.chars().filter(char::is_ascii_alphabetic).count();
    let non_latin = text
        .chars()
        .filter(|ch| !ch.is_ascii_alphabetic() && !ch.is_whitespace())
        .count();
    if latin > 0 && latin > non_latin {
        Some(Language::English)
    } else {
        None
    }
}

/// Returns `true` for the kana scripts used to identify Japanese text.
///
/// Ranges follow the translation integration guide §8: hiragana
/// (U+3040-U+309F), katakana (U+30A0-U+30FF), and halfwidth katakana
/// (U+FF65-U+FF9F).
fn is_kana(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x3040..=0x309F | 0x30A0..=0x30FF | 0xFF65..=0xFF9F
    )
}

/// Resolves the effective translation source used by the pipeline.
///
/// Composes [`resolve_translation_source`] with the
/// [`heuristic_detect_language`] fallback: a concrete configured source
/// wins, then the OCR-detected language, then the heuristic over `text`,
/// and `Auto` remains when nothing could be decided. `text` should be the
/// OCR merged text; only script-level signals matter, so cleaning the text
/// first is not required.
pub(crate) fn resolve_effective_source(
    detected: Option<Language>,
    configured: Language,
    text: &str,
) -> Language {
    let resolved = resolve_translation_source(detected, configured);
    if !resolved.is_auto() {
        debug!(
            configured = %configured.code(),
            detected = ?detected,
            resolved = %resolved.code(),
            "translation source resolved"
        );
        return resolved;
    }
    let resolved = heuristic_detect_language(text).unwrap_or(Language::Auto);
    debug!(
        configured = %configured.code(),
        detected = ?detected,
        resolved = %resolved.code(),
        "translation source resolved"
    );
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_translation_source ──

    #[test]
    fn concrete_configured_source_passes_through() {
        for configured in [
            Language::English,
            Language::Japanese,
            Language::ChineseSimplified,
        ] {
            assert_eq!(
                resolve_translation_source(Some(Language::Japanese), configured),
                configured
            );
            assert_eq!(resolve_translation_source(None, configured), configured);
        }
    }

    #[test]
    fn auto_uses_ocr_detection_when_concrete() {
        assert_eq!(
            resolve_translation_source(Some(Language::English), Language::Auto),
            Language::English
        );
        assert_eq!(
            resolve_translation_source(Some(Language::Japanese), Language::Auto),
            Language::Japanese
        );
        assert_eq!(
            resolve_translation_source(Some(Language::ChineseSimplified), Language::Auto),
            Language::ChineseSimplified
        );
    }

    #[test]
    fn auto_ignores_non_concrete_detection() {
        assert_eq!(
            resolve_translation_source(Some(Language::Auto), Language::Auto),
            Language::Auto
        );
        assert_eq!(
            resolve_translation_source(None, Language::Auto),
            Language::Auto
        );
    }

    // ── heuristic_detect_language ──

    #[test]
    fn heuristic_detects_japanese_kana() {
        assert_eq!(
            heuristic_detect_language("こんにちは世界"),
            Some(Language::Japanese)
        );
        assert_eq!(
            heuristic_detect_language("カタカナのテキスト"),
            Some(Language::Japanese)
        );
        assert_eq!(
            heuristic_detect_language("ｶﾀｶﾅﾃﾞｽ"),
            Some(Language::Japanese)
        );
        assert_eq!(
            heuristic_detect_language("漢字とひらがなの混在"),
            Some(Language::Japanese)
        );
        // Kana beats Latin letters when both are present.
        assert_eq!(
            heuristic_detect_language("こんにちはHello"),
            Some(Language::Japanese)
        );
    }

    #[test]
    fn heuristic_detects_latin_dominant_english() {
        assert_eq!(
            heuristic_detect_language("Hello world"),
            Some(Language::English)
        );
        assert_eq!(
            heuristic_detect_language("Version 2.0 released"),
            Some(Language::English)
        );
        // Latin letters dominate the Chinese characters.
        assert_eq!(
            heuristic_detect_language("Hello 世界"),
            Some(Language::English)
        );
    }

    #[test]
    fn heuristic_returns_none_when_ambiguous() {
        assert_eq!(heuristic_detect_language(""), None);
        assert_eq!(heuristic_detect_language("   \n "), None);
        assert_eq!(heuristic_detect_language("12345"), None);
        assert_eq!(heuristic_detect_language("!!!???"), None);
        assert_eq!(heuristic_detect_language("世界"), None);
        // Letters and non-letters tie, so there is no dominant script.
        assert_eq!(heuristic_detect_language("abc123"), None);
    }

    // ── resolve_effective_source ──

    #[test]
    fn effective_source_prefers_configured_concrete_language() {
        assert_eq!(
            resolve_effective_source(Some(Language::Japanese), Language::English, "こんにちは"),
            Language::English
        );
        assert_eq!(
            resolve_effective_source(None, Language::Japanese, "Hello world"),
            Language::Japanese
        );
    }

    #[test]
    fn effective_source_prefers_detection_over_heuristic() {
        assert_eq!(
            resolve_effective_source(Some(Language::English), Language::Auto, "こんにちは"),
            Language::English
        );
        assert_eq!(
            resolve_effective_source(Some(Language::ChineseSimplified), Language::Auto, "Hello"),
            Language::ChineseSimplified
        );
    }

    #[test]
    fn effective_source_falls_back_to_heuristic_without_detection() {
        assert_eq!(
            resolve_effective_source(None, Language::Auto, "こんにちは"),
            Language::Japanese
        );
        assert_eq!(
            resolve_effective_source(None, Language::Auto, "Hello world"),
            Language::English
        );
        assert_eq!(
            resolve_effective_source(None, Language::Auto, "こんにちはHello"),
            Language::Japanese
        );
    }

    #[test]
    fn effective_source_stays_auto_when_undecidable() {
        assert_eq!(
            resolve_effective_source(None, Language::Auto, ""),
            Language::Auto
        );
        assert_eq!(
            resolve_effective_source(None, Language::Auto, "12345"),
            Language::Auto
        );
    }
}
