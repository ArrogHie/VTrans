//! Integration tests for multi-box fingerprint deduplication.
//!
//! These tests exercise [`BoxFingerprintCache`] through the crate's public
//! API in a way that mirrors how the multi-box live translation pipeline
//! (module 09) would use it: each box produces OCR text, the cache decides
//! whether to skip translation, and boxes are added and removed at runtime.

use vtrans_text::{BoxFingerprintCache, TextNormalizer};

/// Simulates one OCR frame for a box: returns the text and tells the cache
/// whether it is a duplicate (translation should be skipped).
fn ocr_frame(cache: &BoxFingerprintCache, box_id: u32, text: &str) -> bool {
    cache.is_duplicate(box_id, text)
}

#[test]
fn multi_box_live_translation_dedup() {
    let cache = BoxFingerprintCache::new();

    // --- Frame 1: both boxes see new text, both get translated. ---
    assert!(!ocr_frame(&cache, 0, "Hello"));
    assert!(!ocr_frame(&cache, 1, "World"));

    // --- Frame 2: nothing changed, both are duplicates. ---
    assert!(ocr_frame(&cache, 0, "Hello"));
    assert!(ocr_frame(&cache, 1, "World"));

    // --- Frame 3: box 0 changed, box 1 did not. ---
    assert!(!ocr_frame(&cache, 0, "Hello!"));
    assert!(ocr_frame(&cache, 1, "World"));
}

#[test]
fn box_isolation_independent_state() {
    let cache = BoxFingerprintCache::new();

    // Box A and Box B both see the same text.
    assert!(!cache.is_duplicate(0, "same text"));
    assert!(!cache.is_duplicate(1, "same text"));

    // Box A sees it again -> duplicate.
    assert!(cache.is_duplicate(0, "same text"));

    // Box B still has its own independent state.
    // If Box A's state leaked into Box B, this would be a false positive.
    assert!(cache.is_duplicate(1, "same text"));

    // Clearing Box A must not affect Box B.
    cache.clear_box(0);
    assert!(!cache.is_duplicate(0, "same text")); // Box A reset
    assert!(cache.is_duplicate(1, "same text")); // Box B unchanged
}

#[test]
fn box_removal_releases_state() {
    let cache = BoxFingerprintCache::new();

    // Box 5 records some text.
    cache.is_duplicate(5, "some text");
    assert!(cache.is_duplicate(5, "some text"));

    // Box 5 is deleted.
    cache.remove_box(5);

    // Re-adding box 5 (or reusing the id) starts fresh.
    assert!(!cache.is_duplicate(5, "some text"));
}

#[test]
fn clear_all_resets_session() {
    let cache = BoxFingerprintCache::new();

    for id in 0..4u32 {
        cache.is_duplicate(id, "frame 1");
    }

    // Session restart: all boxes should see their next text as new.
    cache.clear_all();

    for id in 0..4u32 {
        assert!(!cache.is_duplicate(id, "frame 1"));
    }
}

#[test]
fn fingerprint_consistent_with_text_normalizer() {
    // BoxFingerprintCache uses the same FNV-1a fingerprint as
    // TextNormalizer::fingerprint. Verify that two texts with the same
    // TextNormalizer fingerprint are also duplicates in the cache.
    let cache = BoxFingerprintCache::new();

    let text_a = "Hello  World";
    let text_b = "Hello\nWorld";

    assert_eq!(
        TextNormalizer::fingerprint(text_a),
        TextNormalizer::fingerprint(text_b)
    );

    cache.is_duplicate(0, text_a);
    assert!(cache.is_duplicate(0, text_b));
}

#[test]
fn text_reverting_is_re_translated() {
    // When text changes A -> B -> A, the third frame is NOT a duplicate
    // because the stored fingerprint is B. This matches the live-translation
    // requirement: the overlay must reflect the current screen content.
    let cache = BoxFingerprintCache::new();

    assert!(!cache.is_duplicate(0, "A"));
    assert!(!cache.is_duplicate(0, "B"));
    assert!(!cache.is_duplicate(0, "A"));
}
