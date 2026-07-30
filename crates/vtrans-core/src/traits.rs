use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::error::{CaptureError, OcrError, TranslationError};
use crate::types::{
    CapturedImage, Language, OcrOptions, OcrResult, ScreenRegion, TranslationRequest, TranslationResult,
};

/// OCR provider trait. Implementations load ONNX models and run inference.
[async_trait]
pub trait OcrProvider: Send + Sync {
    fn id(&self) -> &'static str;

    async fn recognize(
        &self,
        image: &CapturedImage,
        region: &ScreenRegion,
        options: &OcrOptions,
        cancel: CancellationToken,
    ) -> Result<OcrResult, OcrError>;

    fn supported_languages(&self) -> &[Language];
}

/// Translation provider trait. API and local ONNX implementations.
[async_trait]
pub trait TranslationProvider: Send + Sync {
    fn id(&self) -> &'static str;

    async fn translate(
        &self,
        request: &TranslationRequest,
        cancel: CancellationToken,
    ) -> Result<TranslationResult, TranslationError>;

    fn supported_pairs(&self) -> &[(Language, Language)];
}

/// Screen capture source trait.
[async_trait]
pub trait CaptureSource: Send + Sync {
    async fn capture_once(&self, region: &ScreenRegion)
        -> Result<CapturedImage, CaptureError>;

    async fn start_session(
        &self,
        region: &ScreenRegion,
    ) -> Result<Box<dyn CaptureSession>, CaptureError>;
}

/// Continuous capture session.
[async_trait]
pub trait CaptureSession: Send {
    async fn next_frame(&mut self) -> Result<Option<CapturedImage>, CaptureError>;
    async fn stop(&mut self) -> Result<(), CaptureError>;
}
