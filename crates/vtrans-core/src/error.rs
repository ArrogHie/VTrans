use std::time::Duration;

use thiserror::Error;

use crate::types::{Language, ScreenRegion};

/// Core-level errors for type validation and logging.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid screen region: {0}")]
    InvalidRegion(String),

    #[error("unsupported language: {0:?}")]
    UnsupportedLanguage(Language),

    #[error("image format mismatch: expected {expected:?}, got {actual:?}")]
    FormatMismatch {
        expected: crate::types::PixelFormat,
        actual: crate::types::PixelFormat,
    },

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Capture errors.
///
/// Defined in vtrans-core because the `CaptureSource` and `CaptureSession`
/// traits reference this type. Downstream crate `vtrans-capture` imports it
/// from here; it does not redefine its own.
#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("monitor not found: {0}")]
    MonitorNotFound(String),

    #[error("graphics capture init failed: {0}")]
    InitFailed(String),

    #[error("region out of bounds: {region:?}")]
    OutOfBounds { region: ScreenRegion },

    #[error("frame grab failed: {0}")]
    FrameGrabFailed(String),

    #[error("session stopped")]
    SessionStopped,

    #[error("dpi awareness failed: {0}")]
    DpiAwarenessFailed(String),
}

/// OCR errors.
///
/// Defined in vtrans-core because the `OcrProvider` trait references this
/// type. Downstream crate `vtrans-ocr` imports it from here.
#[derive(Debug, Error)]
pub enum OcrError {
    #[error("model load failed: {0}")]
    ModelLoad(String),

    #[error("inference failed: {0}")]
    Inference(String),

    #[error("preprocess failed: {0}")]
    Preprocess(String),

    #[error("postprocess failed: {0}")]
    Postprocess(String),

    #[error("model manifest invalid: {0}")]
    InvalidManifest(String),

    #[error("cancelled")]
    Cancelled,

    #[error("ort runtime error: {0}")]
    OrtRuntime(String),
}

/// Translation errors.
///
/// Defined in vtrans-core because the `TranslationProvider` trait references
/// this type. Downstream crate `vtrans-translation` imports it from here.
#[derive(Debug, Error)]
pub enum TranslationError {
    #[error("unsupported language pair: {source:?} -> {target:?}")]
    UnsupportedPair { source: Language, target: Language },

    #[error("api request failed: {0}")]
    ApiRequest(String),

    #[error("api timeout after {0:?}")]
    Timeout(Duration),

    #[error("api rate limited")]
    RateLimited,

    #[error("api unauthorized: check api key")]
    Unauthorized,

    #[error("model load failed: {0}")]
    ModelLoad(String),

    #[error("inference failed: {0}")]
    Inference(String),

    #[error("cancelled")]
    Cancelled,

    #[error("response parse error: {0}")]
    ParseResponse(String),
}
