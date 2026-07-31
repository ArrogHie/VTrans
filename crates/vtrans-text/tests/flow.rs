//! End-to-end integration tests for `vtrans-text`.
//!
//! These tests exercise the crate through its public API exactly the way
//! the pipeline (module 09) does: merge OCR lines, clean the result, apply
//! Japanese punctuation rules when the source is Japanese, fingerprint for
//! deduplication, then split into length-limited chunks.

use vtrans_core::OcrLine;
use vtrans_text::{is_duplicate, japanese, TextError, TextNormalizer};

/// Builds a horizontal polygon box starting at `y` with the given height.
fn box_at(y: f32, height: f32) -> [[f32; 2]; 4] {
    let bottom = y + height;
    [[0., y], [60., y], [60., bottom], [0., bottom]]
}

/// Builds an OCR line with a horizontal box.
fn ocr_line(text: &str, y: f32, height: f32, order: usize) -> OcrLine {
    OcrLine::new(text, 0.9, box_at(y, height), order)
}

#[test]
fn ocr_lines_to_translation_chunks() {
    // Two lines of one paragraph and one line of a second paragraph, as
    // produced by a typical OCR pass.
    let lines = vec![
        ocr_line("The quick", 0.0, 20.0, 0),
        ocr_line("brown fox", 0.0, 20.0, 1),
        ocr_line("jumps over", 40.0, 20.0, 2),
        ocr_line("the lazy dog", 40.0, 20.0, 3),
    ];

    let merged = TextNormalizer::merge_lines(&lines);
    assert_eq!(merged, "The quick brown fox\njumps over the lazy dog");

    let cleaned = TextNormalizer::clean(&merged);
    assert_eq!(cleaned, merged);

    // The whole text fits in one chunk at the default limit.
    let chunks = TextNormalizer::split_paragraphs_default(&cleaned);
    assert_eq!(
        chunks,
        vec!["The quick brown fox", "jumps over the lazy dog"]
    );

    // Each chunk passes the length guard.
    for chunk in &chunks {
        assert!(TextNormalizer::validate_length(chunk, 2000).is_ok());
    }
}

#[test]
fn japanese_flow_normalizes_punctuation_and_merges_cjk_lines() {
    let lines = vec![
        ocr_line("ＨＰ １００", 0.0, 20.0, 0),
        ocr_line("，攻撃力アップ．", 0.0, 20.0, 1),
    ];

    let merged = TextNormalizer::merge_lines(&lines);
    assert_eq!(merged, "ＨＰ １００，攻撃力アップ．");

    let cleaned = TextNormalizer::clean(&merged);
    assert_eq!(cleaned, "HP 100，攻撃力アップ．");

    // Japanese source text: apply the Japanese punctuation rules on top.
    let normalized = japanese::normalize_punctuation(&cleaned);
    assert_eq!(normalized, "HP 100、攻撃力アップ。");

    let chunks = TextNormalizer::split_paragraphs(&normalized, 2000);
    assert_eq!(chunks, vec![normalized]);
}

#[test]
fn fingerprint_deduplicates_across_frames() {
    // The same logical text with OCR jitter (zero-width characters,
    // leading/trailing spaces, different line breaks) is a duplicate...
    let frame_a = TextNormalizer::merge_lines(&[
        ocr_line("こんにちは", 0.0, 20.0, 0),
        ocr_line("世界", 0.0, 20.0, 1),
    ]);
    let frame_b = " \u{200b}こんにちは世界\n ";

    let fingerprint_a = TextNormalizer::fingerprint(&frame_a);
    let fingerprint_b = TextNormalizer::fingerprint(frame_b);
    assert_eq!(fingerprint_a, fingerprint_b);
    assert!(is_duplicate(&frame_a, frame_b));

    // ...but a real wording change is not.
    let frame_c = "こんにちは、世界";
    assert!(!is_duplicate(&frame_a, frame_c));
}

#[test]
fn overlong_text_is_split_within_limit() {
    // A single 40-character paragraph must be split into chunks no longer
    // than the configured limit.
    let text = "a".repeat(40);
    let chunks = TextNormalizer::split_paragraphs(&text, 16);
    for chunk in &chunks {
        assert!(chunk.chars().count() <= 16);
    }
    assert_eq!(chunks.concat(), text);
}

#[test]
fn length_guard_reports_oversized_text() {
    let error = TextNormalizer::validate_length("123456789", 8).unwrap_err();
    assert!(matches!(error, TextError::TooLong(9)));
}
