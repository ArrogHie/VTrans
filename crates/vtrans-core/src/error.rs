use thiserror::Error;

/// Core-level errors for type validation and logging.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid screen region: {0}")]
    InvalidRegion(String),

    #[error("unsupported language: {0:?}")]
    UnsupportedLanguage(crate::types::Language),

    #[error("image format mismatch")]
    FormatMismatch,

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Capture errors. Extended by vtrans-capture.
#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("monitor not found: {0}")]
    MonitorNotFound(String),

    #[error("region out of bounds")]
    OutOfBounds,

    #[error("frame grab failed: {0}")]
    FrameGrabFailed(String),

    #[error("session stopped")]
    SessionStopped,

    #[error("{0}")]
    Other(String),
}

/// OCR errors. Extended by vtrans-ocr.
#[derive(Debug, Error)]
pub enum OcrError {
    #[error("model load failed: {0}")]
    ModelLoad(String),

    #[error("inference failed: {0}")]
    Inference(String),

    #[error("cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

/// Translation errors. Extended by vtrans-translation.
#[derive(Debug, Error)]
pub enum TranslationError {
    #[error("unsupported language pair")]
    UnsupportedPair,

    #[error("api request failed: {0}")]
    ApiRequest(String),

    #[error("timeout")]
    Timeout,

    #[error("unauthorized: check api key")]
    Unauthorized,

    #[error("rate limited")]
    RateLimited,

    #[error("cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

/// Pipeline errors. Extended by vtrans-pipeline.
#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("capture error: {0}")]
    Capture(#[from] CaptureError),

    #[error("ocr error: {0}")]
    Ocr(#[from] OcrError),

    #[error("translation error: {0}")]
    Translation(#[from] TranslationError),

    #[error("channel closed")]
    ChannelClosed,

    #[error("cancelled")]
    Cancelled,
}
