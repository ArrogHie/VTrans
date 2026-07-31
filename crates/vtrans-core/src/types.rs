//! Core data types shared across all `VTrans` crates.
//!
//! This module defines the fundamental data structures used throughout the
//! `VTrans` project: language identifiers, screen regions, captured images,
//! OCR results, translation requests/results, and pipeline configuration.
//!
//! All types derive `Debug` and `Clone`. Most also derive `Serialize` and
//! `Deserialize` for IPC with the frontend, except [`CapturedImage`] which
//! intentionally omits serialization to prevent image data from crossing
//! the IPC boundary as JSON/Base64.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Supported languages for OCR and translation.
///
/// Serialized as short string codes (`"auto"`, `"zh-CN"`, `"ja"`, `"en"`)
/// for stable JSON representation across frontend and backend.
///
/// # Example
///
/// ```
/// use vtrans_core::types::Language;
///
/// let lang = Language::Japanese;
/// assert_eq!(lang.code(), "ja");
/// assert_eq!(Language::from_code("ja"), Some(Language::Japanese));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    /// Automatic language detection.
    #[serde(rename = "auto")]
    Auto,
    /// Simplified Chinese (zh-CN).
    #[serde(rename = "zh-CN")]
    ChineseSimplified,
    /// Japanese.
    #[serde(rename = "ja")]
    Japanese,
    /// English.
    #[serde(rename = "en")]
    English,
}

impl Language {
    /// Returns the short string code used in serialization and IPC.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ChineseSimplified => "zh-CN",
            Self::Japanese => "ja",
            Self::English => "en",
        }
    }

    /// Parses a language from its string code.
    ///
    /// Returns `None` if the code is not recognized.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "auto" => Some(Self::Auto),
            "zh-CN" => Some(Self::ChineseSimplified),
            "ja" => Some(Self::Japanese),
            "en" => Some(Self::English),
            _ => None,
        }
    }

    /// Returns `true` if this is [`Language::Auto`].
    #[must_use]
    pub const fn is_auto(self) -> bool {
        matches!(self, Self::Auto)
    }

    /// Returns a human-readable display name for logging and UI.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::ChineseSimplified => "Chinese (Simplified)",
            Self::Japanese => "Japanese",
            Self::English => "English",
        }
    }

    /// Returns all concrete (non-auto) languages supported by `VTrans`.
    #[must_use]
    pub const fn all_concrete() -> &'static [Self] {
        &[Self::ChineseSimplified, Self::Japanese, Self::English]
    }
}

/// A rectangular region on a specific monitor.
///
/// Coordinates are in physical pixels relative to the monitor's top-left
/// corner. The `monitor_id` identifies which display the region belongs to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenRegion {
    /// Identifier of the monitor this region belongs to.
    pub monitor_id: String,
    /// X offset from the monitor's left edge, in physical pixels.
    pub x: i32,
    /// Y offset from the monitor's top edge, in physical pixels.
    pub y: i32,
    /// Region width in physical pixels. Must be greater than zero.
    pub width: u32,
    /// Region height in physical pixels. Must be greater than zero.
    pub height: u32,
}

impl ScreenRegion {
    /// Creates a new screen region.
    #[must_use]
    pub fn new(monitor_id: impl Into<String>, x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            monitor_id: monitor_id.into(),
            x,
            y,
            width,
            height,
        }
    }

    /// Validates that the region has non-zero dimensions.
    ///
    /// # Errors
    ///
    /// Returns `Err(CoreError::InvalidRegion)` if `width` or `height` is zero.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.width == 0 || self.height == 0 {
            return Err(CoreError::InvalidRegion(format!(
                "region has zero dimension: {}x{}",
                self.width, self.height
            )));
        }
        Ok(())
    }

    /// Returns `true` if the region has non-zero dimensions.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

/// Pixel format of a [`CapturedImage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// RGBA, 8 bits per channel, 4 bytes per pixel.
    Rgba8,
    /// BGRA, 8 bits per channel, 4 bytes per pixel.
    Bgra8,
}

impl PixelFormat {
    /// Returns the number of bytes per pixel.
    #[must_use]
    pub const fn bytes_per_pixel(self) -> usize {
        4
    }

    /// Returns the number of color channels (always 4, including alpha).
    #[must_use]
    pub const fn channels(self) -> usize {
        4
    }
}

/// A captured screen frame in CPU memory.
///
/// Intentionally does **not** derive `Serialize` to prevent image data
/// from crossing the IPC boundary as JSON/Base64.
#[derive(Debug, Clone)]
pub struct CapturedImage {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Pixel format of the `data` buffer.
    pub format: PixelFormat,
    /// Raw pixel data, row-major, top-to-bottom.
    pub data: Vec<u8>,
}

impl CapturedImage {
    /// Creates a new captured image, validating that the data buffer
    /// matches the expected size for the given dimensions and format.
    ///
    /// # Errors
    /// Returns `CoreError::InvalidRegion` if width or height is zero,
    /// or if the data length does not equal `width * height * bytes_per_pixel`.
    pub fn new(
        width: u32,
        height: u32,
        format: PixelFormat,
        data: Vec<u8>,
    ) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::InvalidRegion(format!(
                "image has zero dimension: {width}x{height}"
            )));
        }
        let expected = Self::expected_data_len(width, height, format);
        if data.len() != expected {
            return Err(CoreError::InvalidRegion(format!(
                "data length mismatch: expected {expected} bytes, got {}",
                data.len()
            )));
        }
        Ok(Self {
            width,
            height,
            format,
            data,
        })
    }

    /// Computes the expected data buffer length for the given dimensions and pixel format.
    #[must_use]
    pub const fn expected_data_len(width: u32, height: u32, format: PixelFormat) -> usize {
        (width as usize) * (height as usize) * format.bytes_per_pixel()
    }

    /// Returns the expected data buffer length for this image.
    #[must_use]
    pub fn data_len(&self) -> usize {
        Self::expected_data_len(self.width, self.height, self.format)
    }

    /// Checks that the image's pixel format matches the expected format.
    ///
    /// # Errors
    /// Returns `CoreError::FormatMismatch` if formats differ.
    pub fn check_format(&self, expected: PixelFormat) -> Result<(), CoreError> {
        if self.format != expected {
            return Err(CoreError::FormatMismatch {
                expected,
                actual: self.format,
            });
        }
        Ok(())
    }

    /// Validates that the data buffer length matches the image dimensions.
    ///
    /// # Errors
    /// Returns `CoreError::InvalidRegion` if the data length is incorrect.
    pub fn validate(&self) -> Result<(), CoreError> {
        let expected = self.data_len();
        if self.data.len() != expected {
            return Err(CoreError::InvalidRegion(format!(
                "data length mismatch: expected {expected} bytes, got {}",
                self.data.len()
            )));
        }
        Ok(())
    }
}
/// A single line of recognized text from OCR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrLine {
    /// The recognized text content.
    pub text: String,
    /// Recognition confidence, 0.0 to 1.0.
    pub confidence: f32,
    /// Quadrilateral polygon of the text region, as four `(x, y)` points
    /// in image coordinates, clockwise from top-left.
    pub polygon: [[f32; 2]; 4],
    /// Sort order for merging into reading-order text.
    pub reading_order: usize,
}

impl OcrLine {
    /// Creates a new OCR line.
    #[must_use]
    pub fn new(
        text: impl Into<String>,
        confidence: f32,
        polygon: [[f32; 2]; 4],
        reading_order: usize,
    ) -> Self {
        Self {
            text: text.into(),
            confidence,
            polygon,
            reading_order,
        }
    }
}

/// The complete result of an OCR recognition pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    /// All recognized text lines.
    pub lines: Vec<OcrLine>,
    /// Lines merged into a single string, ordered by `reading_order`.
    pub merged_text: String,
    /// Detected language, if language detection was performed.
    pub detected_language: Option<Language>,
    /// Elapsed time in milliseconds.
    pub elapsed_ms: u64,
}

impl OcrResult {
    /// Creates a new OCR result, computing `merged_text` from lines
    /// sorted by `reading_order`.
    #[must_use]
    pub fn from_lines(
        lines: Vec<OcrLine>,
        detected_language: Option<Language>,
        elapsed_ms: u64,
    ) -> Self {
        let mut sorted = lines.clone();
        sorted.sort_by_key(|l| l.reading_order);
        let merged_text = sorted
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            lines,
            merged_text,
            detected_language,
            elapsed_ms,
        }
    }

    /// Creates an empty OCR result (no lines).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            lines: Vec::new(),
            merged_text: String::new(),
            detected_language: None,
            elapsed_ms: 0,
        }
    }
}

impl Default for OcrResult {
    fn default() -> Self {
        Self::empty()
    }
}

/// Options passed to an [`OcrProvider`](crate::traits::OcrProvider).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrOptions {
    /// Target language for recognition.
    pub language: Language,
    /// Minimum confidence threshold; lines below this are discarded.
    pub min_confidence: f32,
    /// Whether to detect vertical text.
    pub detect_vertical: bool,
}

impl Default for OcrOptions {
    fn default() -> Self {
        Self {
            language: Language::Auto,
            min_confidence: 0.55,
            detect_vertical: true,
        }
    }
}

impl OcrOptions {
    /// Creates new OCR options for the given language with default settings.
    #[must_use]
    pub fn new(language: Language) -> Self {
        Self {
            language,
            ..Default::default()
        }
    }
}

/// A translation request from one language to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationRequest {
    /// Source text to translate.
    pub text: String,
    /// Source language (use [`Language::Auto`] for auto-detection).
    pub source: Language,
    /// Target language.
    pub target: Language,
}

impl TranslationRequest {
    /// Creates a new translation request.
    #[must_use]
    pub fn new(text: impl Into<String>, source: Language, target: Language) -> Self {
        Self {
            text: text.into(),
            source,
            target,
        }
    }
}

/// The result of a translation operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationResult {
    /// The translated text.
    pub translated_text: String,
    /// Identifier of the provider that produced this result.
    pub provider_id: String,
    /// Elapsed time in milliseconds.
    pub elapsed_ms: u64,
}

impl TranslationResult {
    /// Creates a new translation result.
    #[must_use]
    pub fn new(
        translated_text: impl Into<String>,
        provider_id: impl Into<String>,
        elapsed_ms: u64,
    ) -> Self {
        Self {
            translated_text: translated_text.into(),
            provider_id: provider_id.into(),
            elapsed_ms,
        }
    }
}

/// Operating mode of the translation pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineMode {
    /// Single screenshot capture and translation.
    #[serde(rename = "single")]
    SingleCapture,
    /// Continuous live region translation.
    #[serde(rename = "live")]
    LiveRegion,
}

impl PipelineMode {
    /// Returns `true` if this is [`PipelineMode::LiveRegion`].
    #[must_use]
    pub const fn is_live(self) -> bool {
        matches!(self, Self::LiveRegion)
    }

    /// Returns `true` if this is [`PipelineMode::SingleCapture`].
    #[must_use]
    pub const fn is_single(self) -> bool {
        matches!(self, Self::SingleCapture)
    }
}

/// Status of the translation pipeline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum PipelineStatus {
    /// Pipeline is idle.
    #[serde(rename = "idle")]
    #[default]
    Idle,
    /// Capturing a frame.
    #[serde(rename = "capturing")]
    Capturing,
    /// OCR recognition in progress.
    #[serde(rename = "ocr_in_progress")]
    OcrInProgress,
    /// Translation in progress.
    #[serde(rename = "translating")]
    Translating,
    /// Pipeline completed (single capture mode).
    #[serde(rename = "completed")]
    Completed,
    /// Pipeline encountered an error.
    #[serde(rename = "error")]
    Error(String),
}

impl PipelineStatus {
    /// Returns `true` if the pipeline is [`Idle`](Self::Idle).
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Returns `true` if the pipeline is in an [`Error`](Self::Error) state.
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    /// Returns the error message if in an error state, `None` otherwise.
    #[must_use]
    pub fn error_message(&self) -> Option<&str> {
        if let Self::Error(msg) = self {
            Some(msg)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Language ──

    #[test]
    fn language_serde_roundtrip() {
        for &lang in &[
            Language::Auto,
            Language::ChineseSimplified,
            Language::Japanese,
            Language::English,
        ] {
            let json = serde_json::to_string(&lang).unwrap();
            let back: Language = serde_json::from_str(&json).unwrap();
            assert_eq!(lang, back);
        }
    }

    #[test]
    fn language_serde_codes() {
        assert_eq!(serde_json::to_string(&Language::Auto).unwrap(), r#""auto""#);
        assert_eq!(
            serde_json::to_string(&Language::ChineseSimplified).unwrap(),
            r#""zh-CN""#
        );
        assert_eq!(
            serde_json::to_string(&Language::Japanese).unwrap(),
            r#""ja""#
        );
        assert_eq!(
            serde_json::to_string(&Language::English).unwrap(),
            r#""en""#
        );
    }

    #[test]
    fn language_code_and_from_code() {
        for &lang in &[
            Language::Auto,
            Language::ChineseSimplified,
            Language::Japanese,
            Language::English,
        ] {
            let code = lang.code();
            assert_eq!(Language::from_code(code), Some(lang));
        }
        assert_eq!(Language::from_code("unknown"), None);
    }

    #[test]
    fn language_is_auto() {
        assert!(Language::Auto.is_auto());
        assert!(!Language::English.is_auto());
    }

    #[test]
    fn language_display_name() {
        assert_eq!(Language::Auto.display_name(), "Auto");
        assert_eq!(
            Language::ChineseSimplified.display_name(),
            "Chinese (Simplified)"
        );
    }

    #[test]
    fn language_all_concrete() {
        let concrete = Language::all_concrete();
        assert_eq!(concrete.len(), 3);
        assert!(!concrete.contains(&Language::Auto));
    }

    // ── ScreenRegion ──

    #[test]
    fn screen_region_valid() {
        let region = ScreenRegion::new("monitor0", 100, 200, 1920, 1080);
        assert!(region.validate().is_ok());
        assert!(region.is_valid());
    }

    #[test]
    fn screen_region_zero_width() {
        let region = ScreenRegion::new("m", 0, 0, 0, 1080);
        assert!(matches!(
            region.validate(),
            Err(CoreError::InvalidRegion(_))
        ));
        assert!(!region.is_valid());
    }

    #[test]
    fn screen_region_zero_height() {
        let region = ScreenRegion::new("m", 0, 0, 1920, 0);
        assert!(matches!(
            region.validate(),
            Err(CoreError::InvalidRegion(_))
        ));
    }

    #[test]
    fn screen_region_serde_roundtrip() {
        let region = ScreenRegion::new("m0", 10, 20, 100, 200);
        let json = serde_json::to_string(&region).unwrap();
        let back: ScreenRegion = serde_json::from_str(&json).unwrap();
        assert_eq!(region.monitor_id, back.monitor_id);
        assert_eq!(region.width, back.width);
    }

    // ── PixelFormat ──

    #[test]
    fn pixel_format_bytes_and_channels() {
        assert_eq!(PixelFormat::Rgba8.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Bgra8.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Rgba8.channels(), 4);
    }

    // ── CapturedImage ──

    #[test]
    fn captured_image_new_valid() {
        let img = CapturedImage::new(2, 2, PixelFormat::Rgba8, vec![0; 16]);
        assert!(img.is_ok());
    }

    #[test]
    fn captured_image_new_zero_dim() {
        let err = CapturedImage::new(0, 2, PixelFormat::Rgba8, vec![]);
        assert!(matches!(err, Err(CoreError::InvalidRegion(_))));
    }

    #[test]
    fn captured_image_new_bad_data_len() {
        let err = CapturedImage::new(2, 2, PixelFormat::Rgba8, vec![0; 10]);
        assert!(matches!(err, Err(CoreError::InvalidRegion(_))));
    }

    #[test]
    fn captured_image_check_format_match() {
        let img = CapturedImage {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            data: vec![0; 4],
        };
        assert!(img.check_format(PixelFormat::Rgba8).is_ok());
    }

    #[test]
    fn captured_image_check_format_mismatch() {
        let img = CapturedImage {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            data: vec![0; 4],
        };
        let err = img.check_format(PixelFormat::Bgra8);
        assert!(matches!(err, Err(CoreError::FormatMismatch { .. })));
    }

    #[test]
    fn captured_image_validate_ok() {
        let img = CapturedImage {
            width: 2,
            height: 3,
            format: PixelFormat::Bgra8,
            data: vec![0; 24],
        };
        assert!(img.validate().is_ok());
    }

    #[test]
    fn captured_image_validate_bad_len() {
        let img = CapturedImage {
            width: 2,
            height: 2,
            format: PixelFormat::Rgba8,
            data: vec![0; 10],
        };
        assert!(matches!(img.validate(), Err(CoreError::InvalidRegion(_))));
    }

    #[test]
    fn captured_image_data_len() {
        let img = CapturedImage {
            width: 3,
            height: 4,
            format: PixelFormat::Rgba8,
            data: vec![0; 48],
        };
        assert_eq!(img.data_len(), 48);
    }

    // ── OcrResult ──

    #[test]
    fn ocr_result_from_lines_sorts_by_reading_order() {
        let lines = vec![
            OcrLine::new("second", 0.9, [[0., 0.], [0., 0.], [0., 0.], [0., 0.]], 1),
            OcrLine::new("first", 0.9, [[0., 0.], [0., 0.], [0., 0.], [0., 0.]], 0),
            OcrLine::new("third", 0.8, [[0., 0.], [0., 0.], [0., 0.], [0., 0.]], 2),
        ];
        let result = OcrResult::from_lines(lines, Some(Language::English), 100);
        assert_eq!(result.merged_text, "first\nsecond\nthird");
    }

    #[test]
    fn ocr_result_empty() {
        let result = OcrResult::empty();
        assert!(result.lines.is_empty());
        assert!(result.merged_text.is_empty());
        assert_eq!(result.elapsed_ms, 0);
    }

    #[test]
    fn ocr_result_default_is_empty() {
        let result = OcrResult::default();
        assert!(result.lines.is_empty());
    }

    // ── OcrOptions ──

    #[test]
    fn ocr_options_default() {
        let opts = OcrOptions::default();
        assert_eq!(opts.language, Language::Auto);
        assert!((opts.min_confidence - 0.55).abs() < f32::EPSILON);
        assert!(opts.detect_vertical);
    }

    #[test]
    fn ocr_options_new() {
        let opts = OcrOptions::new(Language::Japanese);
        assert_eq!(opts.language, Language::Japanese);
        assert!((opts.min_confidence - 0.55).abs() < f32::EPSILON);
    }

    // ── TranslationRequest / TranslationResult ──

    #[test]
    fn translation_request_new() {
        let req = TranslationRequest::new("hello", Language::English, Language::Japanese);
        assert_eq!(req.text, "hello");
        assert_eq!(req.source, Language::English);
    }

    #[test]
    fn translation_result_new() {
        let res = TranslationResult::new("konnichiwa", "mock", 42);
        assert_eq!(res.translated_text, "konnichiwa");
        assert_eq!(res.provider_id, "mock");
        assert_eq!(res.elapsed_ms, 42);
    }

    // ── PipelineMode ──

    #[test]
    fn pipeline_mode_serde() {
        assert_eq!(
            serde_json::to_string(&PipelineMode::SingleCapture).unwrap(),
            r#""single""#
        );
        assert_eq!(
            serde_json::to_string(&PipelineMode::LiveRegion).unwrap(),
            r#""live""#
        );
    }

    #[test]
    fn pipeline_mode_predicates() {
        assert!(PipelineMode::LiveRegion.is_live());
        assert!(!PipelineMode::LiveRegion.is_single());
        assert!(PipelineMode::SingleCapture.is_single());
    }

    // ── PipelineStatus ──

    #[test]
    fn pipeline_status_serde() {
        assert_eq!(
            serde_json::to_string(&PipelineStatus::Idle).unwrap(),
            r#""idle""#
        );
        assert_eq!(
            serde_json::to_string(&PipelineStatus::Capturing).unwrap(),
            r#""capturing""#
        );
    }

    #[test]
    fn pipeline_status_predicates() {
        assert!(PipelineStatus::Idle.is_idle());
        assert!(!PipelineStatus::Capturing.is_idle());
        assert!(PipelineStatus::Error("boom".into()).is_error());
        assert!(!PipelineStatus::Idle.is_error());
    }

    #[test]
    fn pipeline_status_error_message() {
        assert_eq!(
            PipelineStatus::Error("fail".into()).error_message(),
            Some("fail")
        );
        assert_eq!(PipelineStatus::Idle.error_message(), None);
    }

    #[test]
    fn pipeline_status_default() {
        assert!(PipelineStatus::default().is_idle());
    }
}
