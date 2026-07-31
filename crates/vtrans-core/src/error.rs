//! Error types for `VTrans` core.
//!
//! [`CoreError`] is used for type validation and serialization within this
//! crate. [`CaptureError`], [`OcrError`], and [`TranslationError`] are
//! defined here because the corresponding provider traits in [`traits`]
//! reference them. Downstream implementation crates import these types
//! from here rather than redefining their own.

use std::time::Duration;

use thiserror::Error;

use crate::types::{Language, ScreenRegion};

/// Core-level errors for type validation and serialization.
///
/// Used by [`ScreenRegion::validate`](crate::types::ScreenRegion::validate),
/// [`CapturedImage::check_format`](crate::types::CapturedImage::check_format),
/// and other validation methods in [`types`](crate::types).
#[derive(Debug, Error)]
pub enum CoreError {
    /// The screen region or image has invalid dimensions (e.g. zero width/height).
    #[error("invalid screen region: {0}")]
    InvalidRegion(String),

    /// The requested language is not supported by any provider.
    #[error("unsupported language: {0:?}")]
    UnsupportedLanguage(Language),

    /// The image pixel format does not match the expected format.
    #[error("image format mismatch: expected {expected:?}, got {actual:?}")]
    FormatMismatch {
        /// The expected pixel format.
        expected: crate::types::PixelFormat,
        /// The actual pixel format found in the image.
        actual: crate::types::PixelFormat,
    },

    /// A serialization or deserialization error (JSON).
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Errors that can occur during screen capture.
///
/// Defined in `vtrans-core` because the [`CaptureSource`](crate::traits::CaptureSource)
/// and [`CaptureSession`](crate::traits::CaptureSession) traits reference this type.
/// The downstream crate `vtrans-capture` imports it from here.
#[derive(Debug, Error)]
pub enum CaptureError {
    /// The specified monitor was not found.
    #[error("monitor not found: {0}")]
    MonitorNotFound(String),

    /// Graphics capture initialization failed.
    #[error("graphics capture init failed: {0}")]
    InitFailed(String),

    /// The requested region extends beyond the monitor's bounds.
    #[error("region out of bounds: {region:?}")]
    OutOfBounds {
        /// The region that was out of bounds.
        region: ScreenRegion,
    },

    /// A frame grab operation failed.
    #[error("frame grab failed: {0}")]
    FrameGrabFailed(String),

    /// The capture session has been stopped.
    #[error("session stopped")]
    SessionStopped,

    /// Setting DPI awareness failed.
    #[error("dpi awareness failed: {0}")]
    DpiAwarenessFailed(String),
}

/// Errors that can occur during OCR recognition.
///
/// Defined in `vtrans-core` because the [`OcrProvider`](crate::traits::OcrProvider)
/// trait references this type. The downstream crate `vtrans-ocr` imports it from here.
#[derive(Debug, Error)]
pub enum OcrError {
    /// Model file loading failed (file not found, corrupt, wrong format).
    #[error("model load failed: {0}")]
    ModelLoad(String),

    /// ONNX runtime inference failed.
    #[error("inference failed: {0}")]
    Inference(String),

    /// Image preprocessing failed (resize, normalize, etc.).
    #[error("preprocess failed: {0}")]
    Preprocess(String),

    /// Result postprocessing failed (decode, NMS, etc.).
    #[error("postprocess failed: {0}")]
    Postprocess(String),

    /// The model manifest is invalid or missing required fields.
    #[error("model manifest invalid: {0}")]
    InvalidManifest(String),

    /// The operation was cancelled via `CancellationToken`.
    #[error("cancelled")]
    Cancelled,

    /// The ONNX runtime returned an error.
    #[error("ort runtime error: {0}")]
    OrtRuntime(String),
}

/// Errors that can occur during translation.
///
/// Defined in `vtrans-core` because the [`TranslationProvider`](crate::traits::TranslationProvider)
/// trait references this type. The downstream crate `vtrans-translation` imports it from here.
///
/// Note: the `source` field is named `src` to avoid conflict with
/// `thiserror`'s automatic `source()` method.
#[derive(Debug, Error)]
pub enum TranslationError {
    /// The requested language pair is not supported by this provider.
    #[error("unsupported language pair: {src:?} -> {target:?}")]
    UnsupportedPair {
        /// Source language.
        src: Language,
        /// Target language.
        target: Language,
    },

    /// The API request failed (network error, bad response, etc.).
    #[error("api request failed: {0}")]
    ApiRequest(String),

    /// The API request timed out.
    #[error("api timeout after {0:?}")]
    Timeout(Duration),

    /// The API rate limit was exceeded.
    #[error("api rate limited")]
    RateLimited,

    /// The API key is invalid or missing.
    #[error("api unauthorized: check api key")]
    Unauthorized,

    /// Local model file loading failed.
    #[error("model load failed: {0}")]
    ModelLoad(String),

    /// Local model inference failed.
    #[error("inference failed: {0}")]
    Inference(String),

    /// The operation was cancelled via `CancellationToken`.
    #[error("cancelled")]
    Cancelled,

    /// The API response could not be parsed.
    #[error("response parse error: {0}")]
    ParseResponse(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PixelFormat;

    #[test]
    fn core_error_invalid_region_display() {
        let err = CoreError::InvalidRegion("zero width".into());
        assert!(err.to_string().contains("invalid screen region"));
        assert!(err.to_string().contains("zero width"));
    }

    #[test]
    fn core_error_format_mismatch_display() {
        let err = CoreError::FormatMismatch {
            expected: PixelFormat::Rgba8,
            actual: PixelFormat::Bgra8,
        };
        let msg = err.to_string();
        assert!(msg.contains("Rgba8"));
        assert!(msg.contains("Bgra8"));
    }

    #[test]
    fn core_error_unsupported_language_display() {
        let err = CoreError::UnsupportedLanguage(Language::Japanese);
        assert!(err.to_string().contains("Japanese"));
    }

    #[test]
    fn core_error_serialization_from_serde() {
        let json_err = serde_json::from_str::<i32>("not a number").unwrap_err();
        let core_err = CoreError::from(json_err);
        assert!(matches!(core_err, CoreError::Serialization(_)));
    }

    #[test]
    fn capture_error_monitor_not_found() {
        let err = CaptureError::MonitorNotFound("Display1".into());
        assert!(err.to_string().contains("Display1"));
    }

    #[test]
    fn capture_error_session_stopped() {
        let err = CaptureError::SessionStopped;
        assert_eq!(err.to_string(), "session stopped");
    }

    #[test]
    fn ocr_error_cancelled() {
        let err = OcrError::Cancelled;
        assert_eq!(err.to_string(), "cancelled");
    }

    #[test]
    fn ocr_error_model_load() {
        let err = OcrError::ModelLoad("file not found".into());
        assert!(err.to_string().contains("model load failed"));
    }

    #[test]
    fn translation_error_unsupported_pair() {
        let err = TranslationError::UnsupportedPair {
            src: Language::English,
            target: Language::Japanese,
        };
        let msg = err.to_string();
        assert!(msg.contains("English"));
        assert!(msg.contains("Japanese"));
    }

    #[test]
    fn translation_error_unauthorized() {
        let err = TranslationError::Unauthorized;
        assert!(err.to_string().contains("api key"));
    }

    #[test]
    fn translation_error_timeout() {
        let err = TranslationError::Timeout(Duration::from_secs(30));
        assert!(err.to_string().contains("30"));
    }
}
