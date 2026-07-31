//! Provider trait definitions for OCR, translation, and screen capture.
//!
//! These traits define the interfaces that downstream crates implement.
//! All trait methods are `async` and accept a [`CancellationToken`] for
//! cooperative cancellation. Implementations should use
//! `#[tracing::instrument(skip(self, cancel))]` for structured logging.

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::error::{CaptureError, OcrError, TranslationError};
use crate::types::{
    CapturedImage, Language, OcrOptions, OcrResult, ScreenRegion, TranslationRequest,
    TranslationResult,
};

/// OCR provider trait.
///
/// Implementations load ONNX models and run inference to recognize text
/// from captured screen images.
///
/// # Cancellation
///
/// The `cancel` parameter allows the caller to abort a long-running
/// recognition pass. Implementations should check the token periodically
/// and return [`OcrError::Cancelled`] when cancelled.
///
/// # Example
///
/// ```no_run
/// use vtrans_core::traits::OcrProvider;
/// use vtrans_core::types::*;
/// use vtrans_core::error::OcrError;
/// use async_trait::async_trait;
/// use tokio_util::sync::CancellationToken;
///
/// struct MyOcr;
///
/// #[async_trait]
/// impl OcrProvider for MyOcr {
///     fn id(&self) -> &'static str { "my-ocr" }
///
///     async fn recognize(
///         &self,
///         _image: &CapturedImage,
///         _region: &ScreenRegion,
///         _options: &OcrOptions,
///         cancel: CancellationToken,
///     ) -> Result<OcrResult, OcrError> {
///         cancel.cancelled().await;
///         Err(OcrError::Cancelled)
///     }
///
///     fn supported_languages(&self) -> &[Language] {
///         &[Language::English, Language::ChineseSimplified]
///     }
/// }
/// ```
#[async_trait]
pub trait OcrProvider: Send + Sync {
    /// Returns a stable identifier for this provider (e.g. `"pp-ocr"`).
    fn id(&self) -> &'static str;

    /// Recognize text in the given image within the specified region.
    ///
    /// # Arguments
    /// * `image` - The captured screen frame to process.
    /// * `region` - The sub-region of the image to focus on.
    /// * `options` - Recognition options (language, confidence, etc.).
    /// * `cancel` - Cancellation token; when triggered, returns [`OcrError::Cancelled`].
    async fn recognize(
        &self,
        image: &CapturedImage,
        region: &ScreenRegion,
        options: &OcrOptions,
        cancel: CancellationToken,
    ) -> Result<OcrResult, OcrError>;

    /// Returns the languages this provider can recognize.
    fn supported_languages(&self) -> &[Language];
}

/// Translation provider trait.
///
/// Implementations include cloud API providers (`DeepL`, Google, etc.) and
/// local ONNX-based providers. All implementations must respect the
/// cancellation token.
#[async_trait]
pub trait TranslationProvider: Send + Sync {
    /// Returns a stable identifier for this provider (e.g. `"deepl"`).
    fn id(&self) -> &'static str;

    /// Translate text from `request.source` to `request.target`.
    ///
    /// # Arguments
    /// * `request` - The translation request containing text and language pair.
    /// * `cancel` - Cancellation token; when triggered, returns [`TranslationError::Cancelled`].
    async fn translate(
        &self,
        request: &TranslationRequest,
        cancel: CancellationToken,
    ) -> Result<TranslationResult, TranslationError>;

    /// Returns the `(source, target)` language pairs this provider supports.
    fn supported_pairs(&self) -> &[(Language, Language)];
}

/// Screen capture source trait.
///
/// Provides both one-shot capture and continuous session capture.
/// Implementations use the Windows Graphics Capture API.
#[async_trait]
pub trait CaptureSource: Send + Sync {
    /// Capture a single frame from the specified screen region.
    ///
    /// # Arguments
    /// * `region` - The screen region to capture.
    ///
    /// # Errors
    /// Returns [`CaptureError`] if the monitor is not found, the region is
    /// out of bounds, or the graphics capture fails.
    async fn capture_once(&self, region: &ScreenRegion) -> Result<CapturedImage, CaptureError>;

    /// Start a continuous capture session for the specified region.
    ///
    /// Returns a [`CaptureSession`] that yields frames via [`CaptureSession::next_frame`].
    ///
    /// # Arguments
    /// * `region` - The screen region to capture continuously.
    async fn start_session(
        &self,
        region: &ScreenRegion,
    ) -> Result<Box<dyn CaptureSession>, CaptureError>;
}

/// Continuous capture session.
///
/// Yielded frames are `Option<CapturedImage>`: `None` signals the session
/// has ended (e.g. monitor disconnected). Call [`stop`](Self::stop) to
/// release capture resources.
#[async_trait]
pub trait CaptureSession: Send {
    /// Wait for the next captured frame.
    ///
    /// Returns `Ok(None)` when the session has ended gracefully.
    /// Returns `Err(CaptureError::SessionStopped)` after [`stop`](Self::stop) is called.
    async fn next_frame(&mut self) -> Result<Option<CapturedImage>, CaptureError>;

    /// Stop the capture session and release resources.
    async fn stop(&mut self) -> Result<(), CaptureError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A mock OCR provider that sleeps until cancelled or a timeout.
    struct MockOcrProvider;

    #[async_trait]
    impl OcrProvider for MockOcrProvider {
        fn id(&self) -> &'static str {
            "mock-ocr"
        }

        async fn recognize(
            &self,
            _image: &CapturedImage,
            _region: &ScreenRegion,
            _options: &OcrOptions,
            cancel: CancellationToken,
        ) -> Result<OcrResult, OcrError> {
            tokio::select! {
                () = cancel.cancelled() => Err(OcrError::Cancelled),
                () = tokio::time::sleep(Duration::from_secs(600)) => Ok(OcrResult::empty()),
            }
        }

        fn supported_languages(&self) -> &[Language] {
            &[
                Language::English,
                Language::ChineseSimplified,
                Language::Japanese,
            ]
        }
    }

    #[tokio::test]
    async fn cancel_token_returns_cancelled() {
        let provider = MockOcrProvider;
        let image = CapturedImage {
            width: 1,
            height: 1,
            format: crate::types::PixelFormat::Rgba8,
            data: vec![0; 4],
        };
        let region = ScreenRegion::new("test", 0, 0, 1, 1);
        let options = OcrOptions::default();
        let cancel = CancellationToken::new();

        let cancel_clone = cancel.clone();
        let handle = tokio::spawn(async move {
            provider
                .recognize(&image, &region, &options, cancel_clone)
                .await
        });

        cancel.cancel();
        let result = handle.await.unwrap();
        assert!(matches!(result, Err(OcrError::Cancelled)));
    }

    #[tokio::test]
    async fn cancel_token_pre_cancelled() {
        let provider = MockOcrProvider;
        let image = CapturedImage {
            width: 1,
            height: 1,
            format: crate::types::PixelFormat::Rgba8,
            data: vec![0; 4],
        };
        let region = ScreenRegion::new("test", 0, 0, 1, 1);
        let options = OcrOptions::default();
        let cancel = CancellationToken::new();
        cancel.cancel();

        let result = provider.recognize(&image, &region, &options, cancel).await;
        assert!(matches!(result, Err(OcrError::Cancelled)));
    }

    #[test]
    fn mock_provider_id() {
        let provider = MockOcrProvider;
        assert_eq!(provider.id(), "mock-ocr");
        assert!(provider.supported_languages().contains(&Language::English));
    }
}
