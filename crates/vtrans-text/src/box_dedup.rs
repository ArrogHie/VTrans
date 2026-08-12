//! Per-box fingerprint deduplication cache for multi-box live translation.
//!
//! In a multi-box live translation scenario, each translation box captures
//! its own screen region and runs OCR independently. Text from one box must
//! not influence the duplicate detection of another -- a screen that shows
//! the same text in two different regions should still be translated per
//! box, because each box's translation overlay needs its own result.
//!
//! [`BoxFingerprintCache`] provides a thread-safe, per-box dedup state so
//! that the pipeline can ask "has this box's text changed since the last
//! frame?" without mixing state across boxes. It uses the same FNV-1a
//! fingerprint as [`crate::TextNormalizer::fingerprint`] and
//! [`crate::is_duplicate`], so the dedup algorithm is identical -- only the
//! state is isolated per `box_id`.
//!
//! ## Concurrency
//!
//! The cache is `Send + Sync` via an internal `Mutex`. Multiple pipeline
//! tasks (one per box) can share a single `Arc<BoxFingerprintCache>` and
//! call [`is_duplicate`](BoxFingerprintCache::is_duplicate) concurrently.
//! The `Mutex` serializes access to the `HashMap` of per-box fingerprints.
//!
//! ## Example
//!
//! ```
//! use vtrans_text::BoxFingerprintCache;
//!
//! let cache = BoxFingerprintCache::new();
//!
//! // First sighting of text in box 0 -- not a duplicate.
//! assert!(!cache.is_duplicate(0, "hello world"));
//!
//! // Same text again -- duplicate.
//! assert!(cache.is_duplicate(0, "hello world"));
//!
//! // Box 1 is independent -- not a duplicate.
//! assert!(!cache.is_duplicate(1, "hello world"));
//!
//! // Reset box 0 -- next call is not a duplicate.
//! cache.clear_box(0);
//! assert!(!cache.is_duplicate(0, "hello world"));
//! ```

use std::collections::HashMap;
use std::sync::Mutex;

use tracing::{debug, instrument};

use crate::fingerprint::fingerprint_text;

/// Thread-safe, per-box fingerprint deduplication cache.
///
/// Each `box_id` (a `u32`, matching the `id` field of
/// `vtrans_config::TranslationBoxConfig`) maintains an independent
/// fingerprint. When [`is_duplicate`](Self::is_duplicate) is called, the
/// fingerprint of the new text is compared against the last recorded
/// fingerprint for that box. If they match, the text is a duplicate and
/// the translation step can be skipped; otherwise the new fingerprint is
/// recorded for the next comparison.
///
/// This mirrors the semantics of `vtrans_pipeline::dedup::TextDedup` but
/// isolates state per box and is safe to share across threads. Only the
/// *last* fingerprint is stored per box (not a history), so text that
/// changes back to a previously seen value is correctly re-translated.
///
/// # Example
///
/// ```
/// use vtrans_text::BoxFingerprintCache;
///
/// let cache = BoxFingerprintCache::new();
/// assert!(!cache.is_duplicate(0, "frame 1"));
/// assert!(cache.is_duplicate(0, "frame 1"));   // unchanged
/// assert!(!cache.is_duplicate(0, "frame 2"));  // changed
/// assert!(!cache.is_duplicate(0, "frame 1"));  // changed back
/// ```
#[derive(Debug, Default)]
pub struct BoxFingerprintCache {
    /// Per-box last-seen fingerprint. `box_id` to `u64`.
    boxes: Mutex<HashMap<u32, u64>>,
}

impl BoxFingerprintCache {
    /// Creates an empty cache with no recorded fingerprints.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_text::BoxFingerprintCache;
    ///
    /// let cache = BoxFingerprintCache::new();
    /// assert!(!cache.is_duplicate(0, "first text"));
    /// ```
    #[must_use]
    #[instrument]
    pub fn new() -> Self {
        Self {
            boxes: Mutex::new(HashMap::new()),
        }
    }

    /// Records the fingerprint of `text` for `box_id` and returns `true`
    /// if it duplicated the previously recorded text for that box.
    ///
    /// The fingerprint is whitespace-insensitive (spaces, line breaks,
    /// and invisible characters are normalized away before hashing), so
    /// OCR jitter between frames does not defeat deduplication. Any real
    /// change in wording produces a different fingerprint and is not a
    /// duplicate.
    ///
    /// Only the *last* fingerprint is stored per box. If the text changes
    /// from A to B and back to A, the third frame is **not** a duplicate
    /// (the stored fingerprint is B, which differs from A), so the
    /// translation is re-run -- matching the live-translation requirement
    /// that the overlay always reflects the current screen content.
    ///
    /// When the text is a duplicate, a `debug`-level log entry is emitted
    /// containing the `box_id`. The text itself is never logged.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_text::BoxFingerprintCache;
    ///
    /// let cache = BoxFingerprintCache::new();
    /// assert!(!cache.is_duplicate(0, "hello"));
    /// assert!(cache.is_duplicate(0, "  hello\n "));  // whitespace-insensitive
    /// assert!(!cache.is_duplicate(0, "world"));
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the internal `Mutex` is poisoned (a thread panicked
    /// while holding the lock).
    #[instrument(skip(self, text))]
    pub fn is_duplicate(&self, box_id: u32, text: &str) -> bool {
        let fp = fingerprint_text(text);
        let mut boxes = self
            .boxes
            .lock()
            .expect("BoxFingerprintCache mutex poisoned");
        let duplicate = boxes.get(&box_id).copied() == Some(fp);
        boxes.insert(box_id, fp);
        if duplicate {
            debug!(box_id, "dedup hit; skipping translation for this box");
        }
        duplicate
    }

    /// Resets the dedup state for `box_id`.
    ///
    /// After calling this, the next [`is_duplicate`](Self::is_duplicate)
    /// call for `box_id` will never report a duplicate, because the
    /// stored fingerprint is discarded.
    ///
    /// Use this when a box's region is updated or the screen content is
    /// known to have changed externally (e.g., after a manual refresh).
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_text::BoxFingerprintCache;
    ///
    /// let cache = BoxFingerprintCache::new();
    /// cache.is_duplicate(0, "hello");
    /// assert!(cache.is_duplicate(0, "hello"));
    /// cache.clear_box(0);
    /// assert!(!cache.is_duplicate(0, "hello"));
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the internal `Mutex` is poisoned (a thread panicked
    /// while holding the lock).
    #[instrument(skip(self))]
    pub fn clear_box(&self, box_id: u32) {
        let mut boxes = self
            .boxes
            .lock()
            .expect("BoxFingerprintCache mutex poisoned");
        boxes.remove(&box_id);
    }

    /// Removes `box_id` from the cache entirely, freeing its memory.
    ///
    /// Call this when a translation box is deleted to avoid a slow memory
    /// leak as boxes are added and removed over time.
    ///
    /// In the current implementation (last-fingerprint-per-box),
    /// [`clear_box`](Self::clear_box) and `remove_box` have the same
    /// effect -- the box's entry is removed from the `HashMap`. They are
    /// kept as separate methods so callers can express intent: use
    /// `clear_box` to *reset* a still-active box, and `remove_box` to
    /// *tear down* a box that no longer exists.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_text::BoxFingerprintCache;
    ///
    /// let cache = BoxFingerprintCache::new();
    /// cache.is_duplicate(0, "hello");
    /// cache.remove_box(0);  // box 0 deleted -- clean up
    /// assert!(!cache.is_duplicate(0, "hello"));
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the internal `Mutex` is poisoned (a thread panicked
    /// while holding the lock).
    #[instrument(skip(self))]
    pub fn remove_box(&self, box_id: u32) {
        let mut boxes = self
            .boxes
            .lock()
            .expect("BoxFingerprintCache mutex poisoned");
        boxes.remove(&box_id);
    }

    /// Resets the dedup state for **all** boxes.
    ///
    /// After calling this, every box's next `is_duplicate` call will
    /// report a non-duplicate. Use this when the entire capture session
    /// is restarted or when all regions are reconfigured.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_text::BoxFingerprintCache;
    ///
    /// let cache = BoxFingerprintCache::new();
    /// cache.is_duplicate(0, "a");
    /// cache.is_duplicate(1, "b");
    /// cache.clear_all();
    /// assert!(!cache.is_duplicate(0, "a"));
    /// assert!(!cache.is_duplicate(1, "b"));
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the internal `Mutex` is poisoned (a thread panicked
    /// while holding the lock).
    #[instrument(skip_all)]
    pub fn clear_all(&self) {
        let mut boxes = self
            .boxes
            .lock()
            .expect("BoxFingerprintCache mutex poisoned");
        boxes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- construction & defaults --

    #[test]
    fn new_creates_empty_cache() {
        let cache = BoxFingerprintCache::new();
        assert!(!cache.is_duplicate(0, "anything"));
    }

    #[test]
    fn default_equals_new() {
        let a = BoxFingerprintCache::new();
        let b = BoxFingerprintCache::default();
        assert!(!a.is_duplicate(0, "x"));
        assert!(!b.is_duplicate(0, "x"));
    }

    // -- basic dedup within a single box --

    #[test]
    fn first_text_is_not_duplicate() {
        let cache = BoxFingerprintCache::new();
        assert!(!cache.is_duplicate(0, "hello world"));
    }

    #[test]
    fn same_text_is_duplicate() {
        let cache = BoxFingerprintCache::new();
        cache.is_duplicate(0, "hello world");
        assert!(cache.is_duplicate(0, "hello world"));
    }

    #[test]
    fn different_text_is_not_duplicate() {
        let cache = BoxFingerprintCache::new();
        cache.is_duplicate(0, "hello");
        assert!(!cache.is_duplicate(0, "world"));
    }

    #[test]
    fn text_changing_back_is_not_duplicate() {
        // A to B to A: the third call is *not* a duplicate because the
        // stored fingerprint is B, not A. This ensures the overlay is
        // re-translated when the screen content genuinely reverts.
        let cache = BoxFingerprintCache::new();
        assert!(!cache.is_duplicate(0, "A"));
        assert!(!cache.is_duplicate(0, "B"));
        assert!(!cache.is_duplicate(0, "A"));
    }

    // -- whitespace-insensitive dedup (same algorithm as is_duplicate) --

    #[test]
    fn whitespace_insensitive_dedup() {
        let cache = BoxFingerprintCache::new();
        cache.is_duplicate(0, "hello  world");
        assert!(cache.is_duplicate(0, "  hello\nworld "));
    }

    #[test]
    fn zero_width_characters_are_ignored() {
        let cache = BoxFingerprintCache::new();
        cache.is_duplicate(0, "hello");
        assert!(cache.is_duplicate(0, "hel\u{200b}lo"));
    }

    #[test]
    fn empty_and_blank_texts_are_duplicates() {
        let cache = BoxFingerprintCache::new();
        cache.is_duplicate(0, "");
        assert!(cache.is_duplicate(0, "   \n\t "));
    }

    #[test]
    fn unicode_text_dedup() {
        let cache = BoxFingerprintCache::new();
        cache.is_duplicate(
            0,
            "\u{3053}\u{3093}\u{306b}\u{3061}\u{306f}\u{4e16}\u{754c}",
        );
        assert!(cache.is_duplicate(
            0,
            "\u{3053}\u{3093}\u{306b}\u{3061}\u{306f}\u{4e16}\u{754c}"
        ));
        assert!(!cache.is_duplicate(
            0,
            "\u{3053}\u{3093}\u{3070}\u{3093}\u{306f}\u{4e16}\u{754c}"
        ));
    }

    // -- multi-box isolation --

    #[test]
    fn boxes_are_isolated() {
        let cache = BoxFingerprintCache::new();
        assert!(!cache.is_duplicate(0, "shared text"));
        assert!(cache.is_duplicate(0, "shared text"));
        assert!(!cache.is_duplicate(1, "shared text"));
    }

    #[test]
    fn independent_box_lifecycles() {
        let cache = BoxFingerprintCache::new();
        assert!(!cache.is_duplicate(10, "alpha"));
        assert!(!cache.is_duplicate(20, "beta"));
        assert!(!cache.is_duplicate(10, "alpha2"));
        assert!(cache.is_duplicate(20, "beta"));
    }

    #[test]
    fn many_boxes_do_not_interfere() {
        let cache = BoxFingerprintCache::new();
        for id in 0..32u32 {
            assert!(!cache.is_duplicate(id, "same text"));
        }
        for id in 0..32u32 {
            assert!(cache.is_duplicate(id, "same text"));
        }
    }

    // -- clear_box --

    #[test]
    fn clear_box_resets_state() {
        let cache = BoxFingerprintCache::new();
        cache.is_duplicate(0, "hello");
        assert!(cache.is_duplicate(0, "hello"));
        cache.clear_box(0);
        assert!(!cache.is_duplicate(0, "hello"));
    }

    #[test]
    fn clear_box_does_not_affect_other_boxes() {
        let cache = BoxFingerprintCache::new();
        cache.is_duplicate(0, "hello");
        cache.is_duplicate(1, "hello");
        cache.clear_box(0);
        assert!(!cache.is_duplicate(0, "hello"));
        assert!(cache.is_duplicate(1, "hello"));
    }

    #[test]
    fn clear_nonexistent_box_is_noop() {
        let cache = BoxFingerprintCache::new();
        cache.clear_box(999);
    }

    // -- remove_box --

    #[test]
    fn remove_box_cleans_up() {
        let cache = BoxFingerprintCache::new();
        cache.is_duplicate(0, "hello");
        assert!(cache.is_duplicate(0, "hello"));
        cache.remove_box(0);
        assert!(!cache.is_duplicate(0, "hello"));
    }

    #[test]
    fn remove_box_does_not_affect_other_boxes() {
        let cache = BoxFingerprintCache::new();
        cache.is_duplicate(0, "hello");
        cache.is_duplicate(1, "hello");
        cache.remove_box(0);
        assert!(cache.is_duplicate(1, "hello"));
    }

    #[test]
    fn remove_nonexistent_box_is_noop() {
        let cache = BoxFingerprintCache::new();
        cache.remove_box(999);
    }

    // -- clear_all --

    #[test]
    fn clear_all_resets_everything() {
        let cache = BoxFingerprintCache::new();
        cache.is_duplicate(0, "a");
        cache.is_duplicate(1, "b");
        cache.is_duplicate(2, "c");
        cache.clear_all();
        assert!(!cache.is_duplicate(0, "a"));
        assert!(!cache.is_duplicate(1, "b"));
        assert!(!cache.is_duplicate(2, "c"));
    }

    #[test]
    fn clear_all_on_empty_cache_is_noop() {
        let cache = BoxFingerprintCache::new();
        cache.clear_all();
        assert!(!cache.is_duplicate(0, "x"));
    }

    // -- concurrency safety --

    #[test]
    fn concurrent_access_is_safe() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(BoxFingerprintCache::new());
        let mut handles = vec![];

        for box_id in 0..8u32 {
            let cache = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for i in 0..50 {
                    let text = format!("box {box_id} frame {i}");
                    assert!(!cache.is_duplicate(box_id, &text));
                    assert!(cache.is_duplicate(box_id, &text));
                }
            }));
        }

        for handle in handles {
            handle.join().expect("worker thread panicked");
        }
    }

    #[test]
    fn concurrent_same_box_exactly_one_non_duplicate() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::thread;

        const THREADS: usize = 16;
        let cache = Arc::new(BoxFingerprintCache::new());
        let non_dup_count = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];

        for _ in 0..THREADS {
            let cache = Arc::clone(&cache);
            let counter = Arc::clone(&non_dup_count);
            handles.push(thread::spawn(move || {
                if !cache.is_duplicate(0, "concurrent text") {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }

        for handle in handles {
            handle.join().expect("worker thread panicked");
        }

        assert_eq!(non_dup_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn concurrent_clear_and_dedup_does_not_panic() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(BoxFingerprintCache::new());
        let mut handles = vec![];

        let cache_w = Arc::clone(&cache);
        handles.push(thread::spawn(move || {
            for i in 0..100 {
                let text = format!("frame {i}");
                cache_w.is_duplicate(0, &text);
            }
        }));

        let cache_c = Arc::clone(&cache);
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                cache_c.clear_box(0);
            }
        }));

        for handle in handles {
            handle.join().expect("worker thread panicked");
        }

        cache.is_duplicate(0, "final text");
    }
}
