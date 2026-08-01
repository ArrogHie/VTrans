//! Frame difference detection and text fingerprint deduplication.
//!
//! The live translation loop must not re-run OCR on frames that have not
//! changed (idle screens) and must not re-translate text that is unchanged
//! between frames (OCR jitter). This module provides the two pure building
//! blocks for those checks:
//!
//! - [`FrameDiffer`] compares consecutive captured frames and reports
//!   whether the pixel content changed by more than a configurable ratio.
//! - [`TextDedup`] records the fingerprint of the last processed text and
//!   reports whether new text is a duplicate (whitespace-insensitive).
//!
//! Both types are single-threaded by design: the live capture loop owns the
//! [`FrameDiffer`] and the OCR worker owns the [`TextDedup`].

use vtrans_core::types::CapturedImage;
use vtrans_text::TextNormalizer;

/// Default ratio of differing pixels that triggers OCR in live mode.
///
/// A frame is considered "changed" when more than 2% of its pixels differ
/// from the previously processed frame.
pub const DEFAULT_DIFFERENCE_THRESHOLD: f32 = 0.02;

/// Detects whether a frame changed enough to warrant a new OCR pass.
///
/// The differ keeps a copy of the last frame it was asked about. It is not
/// `Sync` and is intended to be used from a single task (the live capture
/// loop).
///
/// # Example
///
/// ```
/// use vtrans_core::{CapturedImage, PixelFormat};
/// use vtrans_pipeline::dedup::FrameDiffer;
///
/// let image = |byte| CapturedImage {
///     width: 1,
///     height: 1,
///     format: PixelFormat::Rgba8,
///     data: vec![byte; 4],
/// };
///
/// let mut differ = FrameDiffer::new(0.0);
/// assert!(differ.is_changed(&image(0)));   // first frame always processed
/// assert!(!differ.is_changed(&image(0)));  // identical frame is skipped
/// assert!(differ.is_changed(&image(1)));   // different frame is processed
/// ```
#[derive(Debug)]
pub struct FrameDiffer {
    previous: Option<CapturedImage>,
    threshold: f32,
}

impl FrameDiffer {
    /// Creates a differ that reports "changed" when the differing-pixel
    /// ratio exceeds `threshold`.
    ///
    /// `threshold` is clamped into the `0.0..=1.0` range.
    #[must_use]
    pub fn new(threshold: f32) -> Self {
        Self {
            previous: None,
            threshold: threshold.clamp(0.0, 1.0),
        }
    }

    /// Returns the current difference threshold.
    #[must_use]
    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    /// Replaces the difference threshold, clamped into `0.0..=1.0`.
    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold.clamp(0.0, 1.0);
    }

    /// Returns `true` when `frame` differs from the previously seen frame
    /// by more than the configured ratio.
    ///
    /// The first frame ever seen is always considered changed. Frames with
    /// a different size or pixel format are considered changed (the ratio
    /// is undefined across such frames).
    pub fn is_changed(&mut self, frame: &CapturedImage) -> bool {
        let changed = match &self.previous {
            Some(previous) => {
                Self::diff_ratio(previous, frame).map_or(true, |ratio| ratio > self.threshold)
            }
            None => true,
        };
        self.previous = Some(frame.clone());
        changed
    }

    /// Clears the stored previous frame; the next frame is treated as
    /// changed.
    pub fn reset(&mut self) {
        self.previous = None;
    }

    /// Computes the ratio of pixels that differ between two frames.
    ///
    /// Returns `None` when the frames have different dimensions or pixel
    /// formats, because the ratio is not meaningful across such frames.
    /// Pixels are compared byte-wise across all four channels.
    #[must_use]
    pub fn diff_ratio(a: &CapturedImage, b: &CapturedImage) -> Option<f32> {
        if a.width != b.width || a.height != b.height || a.format != b.format {
            return None;
        }
        let total_pixels = a.width as usize * a.height as usize;
        if total_pixels == 0 {
            return None;
        }
        let differing_pixels = a
            .data
            .chunks_exact(4)
            .zip(b.data.chunks_exact(4))
            .filter(|(left, right)| left != right)
            .count();
        // The ratio is bounded by 1.0 and used only for a `>` comparison
        // against a small threshold; f32 precision is more than adequate.
        #[allow(clippy::cast_precision_loss)]
        Some(differing_pixels as f32 / total_pixels as f32)
    }
}

impl Default for FrameDiffer {
    fn default() -> Self {
        Self::new(DEFAULT_DIFFERENCE_THRESHOLD)
    }
}

/// Deduplicates text between frames using whitespace-insensitive
/// fingerprints.
///
/// The live OCR worker records the fingerprint of every processed frame.
/// When a new frame's text fingerprints the same as the previous one, the
/// translation step is skipped, which avoids re-translating unchanged text
/// when only OCR jitter (spaces, line breaks) differs.
///
/// # Example
///
/// ```
/// use vtrans_pipeline::dedup::TextDedup;
///
/// let mut dedup = TextDedup::new();
/// assert!(!dedup.record("hello world"));
/// assert!(dedup.record("  hello\nworld "));
/// assert!(!dedup.record("hello world!"));
/// ```
#[derive(Debug, Default)]
pub struct TextDedup {
    last_fingerprint: Option<u64>,
}

impl TextDedup {
    /// Creates an empty deduplicator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            last_fingerprint: None,
        }
    }

    /// Returns `true` when `text` fingerprints the same as the last
    /// recorded text, without updating the recorded state.
    #[must_use]
    pub fn is_duplicate(&self, text: &str) -> bool {
        self.last_fingerprint == Some(TextNormalizer::fingerprint(text))
    }

    /// Records the fingerprint of `text` and returns `true` when it
    /// duplicated the previously recorded text.
    pub fn record(&mut self, text: &str) -> bool {
        let fingerprint = TextNormalizer::fingerprint(text);
        let duplicate = self.last_fingerprint == Some(fingerprint);
        self.last_fingerprint = Some(fingerprint);
        duplicate
    }

    /// Clears the recorded fingerprint; the next recorded text is never a
    /// duplicate.
    pub fn reset(&mut self) {
        self.last_fingerprint = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtrans_core::PixelFormat;

    fn image(width: u32, height: u32, format: PixelFormat, byte: u8) -> CapturedImage {
        let len = usize::try_from(width * height * 4).unwrap();
        CapturedImage {
            width,
            height,
            format,
            data: vec![byte; len],
        }
    }

    // ── FrameDiffer ──

    #[test]
    fn diff_ratio_identical_frames_is_zero() {
        let a = image(4, 4, PixelFormat::Rgba8, 7);
        assert_eq!(FrameDiffer::diff_ratio(&a, &a), Some(0.0));
    }

    #[test]
    fn diff_ratio_all_different_is_one() {
        let a = image(2, 2, PixelFormat::Rgba8, 0);
        let b = image(2, 2, PixelFormat::Rgba8, 255);
        assert_eq!(FrameDiffer::diff_ratio(&a, &b), Some(1.0));
    }

    #[test]
    fn diff_ratio_counts_pixels_not_bytes() {
        let a = image(1, 2, PixelFormat::Rgba8, 0);
        let mut b = image(1, 2, PixelFormat::Rgba8, 0);
        // One differing pixel out of two, even though it differs in only
        // one byte.
        b.data[0] = 1;
        assert_eq!(FrameDiffer::diff_ratio(&a, &b), Some(0.5));
        assert_eq!(FrameDiffer::diff_ratio(&b, &a), Some(0.5));
    }

    #[test]
    fn diff_ratio_returns_none_for_dimension_mismatch() {
        let a = image(2, 2, PixelFormat::Rgba8, 0);
        let b = image(2, 3, PixelFormat::Rgba8, 0);
        assert_eq!(FrameDiffer::diff_ratio(&a, &b), None);
    }

    #[test]
    fn diff_ratio_returns_none_for_format_mismatch() {
        let a = image(2, 2, PixelFormat::Rgba8, 0);
        let b = image(2, 2, PixelFormat::Bgra8, 0);
        assert_eq!(FrameDiffer::diff_ratio(&a, &b), None);
    }

    #[test]
    fn is_changed_first_frame_always_true() {
        let mut differ = FrameDiffer::new(0.5);
        assert!(differ.is_changed(&image(2, 2, PixelFormat::Rgba8, 1)));
    }

    #[test]
    fn is_changed_identical_frame_false() {
        let mut differ = FrameDiffer::new(0.0);
        let frame = image(2, 2, PixelFormat::Rgba8, 1);
        assert!(differ.is_changed(&frame));
        assert!(!differ.is_changed(&frame));
    }

    #[test]
    fn is_changed_small_diff_below_threshold_false() {
        let mut differ = FrameDiffer::new(0.75);
        let mut a = image(4, 1, PixelFormat::Rgba8, 0);
        assert!(differ.is_changed(&a));
        // One of four pixels differs => ratio 0.25, below 0.75.
        a.data[0] = 255;
        assert!(!differ.is_changed(&a));
    }

    #[test]
    fn is_changed_large_diff_above_threshold_true() {
        let mut differ = FrameDiffer::new(0.5);
        let mut a = image(4, 1, PixelFormat::Rgba8, 0);
        assert!(differ.is_changed(&a));
        // Three of four pixels differ => ratio 0.75, above 0.5.
        for byte in &mut a.data[..12] {
            *byte = 255;
        }
        assert!(differ.is_changed(&a));
    }

    #[test]
    fn is_changed_dimension_change_considered_changed() {
        let mut differ = FrameDiffer::new(1.0);
        assert!(differ.is_changed(&image(1, 1, PixelFormat::Rgba8, 0)));
        assert!(differ.is_changed(&image(2, 1, PixelFormat::Rgba8, 0)));
    }

    #[test]
    fn reset_forces_next_frame_changed() {
        let mut differ = FrameDiffer::new(0.0);
        let frame = image(2, 2, PixelFormat::Rgba8, 3);
        assert!(differ.is_changed(&frame));
        assert!(!differ.is_changed(&frame));
        differ.reset();
        assert!(differ.is_changed(&frame));
    }

    #[test]
    fn threshold_is_clamped() {
        assert!(FrameDiffer::new(-1.0).threshold().abs() < f32::EPSILON);
        assert!((FrameDiffer::new(2.0).threshold() - 1.0).abs() < f32::EPSILON);
        let mut differ = FrameDiffer::new(0.5);
        differ.set_threshold(-3.0);
        assert!(differ.threshold().abs() < f32::EPSILON);
    }

    #[test]
    fn default_threshold_is_constant() {
        assert!(
            (FrameDiffer::default().threshold() - DEFAULT_DIFFERENCE_THRESHOLD).abs()
                < f32::EPSILON
        );
    }

    // ── TextDedup ──

    #[test]
    fn record_reports_whitespace_insensitive_duplicates() {
        let mut dedup = TextDedup::new();
        assert!(!dedup.record("Hello world"));
        assert!(dedup.record("  Hello\nworld  "));
        assert!(!dedup.record("Hello world!"));
    }

    #[test]
    fn record_empty_text_duplicates() {
        let mut dedup = TextDedup::new();
        assert!(!dedup.record(""));
        assert!(dedup.record("   \n "));
    }

    #[test]
    fn is_duplicate_does_not_update_state() {
        let mut dedup = TextDedup::new();
        assert!(!dedup.is_duplicate("a"));
        assert!(!dedup.is_duplicate("a"));
        dedup.record("a");
        assert!(dedup.is_duplicate("a"));
    }

    #[test]
    fn reset_clears_state() {
        let mut dedup = TextDedup::new();
        dedup.record("same");
        assert!(dedup.is_duplicate("same"));
        dedup.reset();
        assert!(!dedup.is_duplicate("same"));
        assert!(!dedup.record("same"));
    }
}
