//! `VTrans` pipeline orchestration.
//!
//! The pipeline chains screen capture, OCR, text normalization, and
//! translation into two operating modes:
//!
//! - **single capture** ([`PipelineMode::SingleCapture`]): one
//!   capture -> OCR -> translate pass, driven by [`Pipeline::run`] or the
//!   convenience function [`run_single_capture`];
//! - **live region** ([`PipelineMode::LiveRegion`]): a capture session
//!   streams frames into a bounded channel; unchanged frames are skipped by
//!   pixel-difference detection ([`FrameDiffer`]), changed frames are OCR'd
//!   by a worker that cancels its previous pass when a newer frame arrives,
//!   and text whose fingerprint is unchanged ([`TextDedup`]) is not
//!   re-translated.
//!
//! The pipeline only ever depends on the provider traits from `vtrans-core`
//! ([`CaptureSource`], [`OcrProvider`], [`TranslationProvider`]); concrete
//! implementations are injected through [`PipelineDeps`].
//!
//! See `docs/modules/09-pipeline.md` for the module specification.

pub mod cancel;
pub mod dedup;
pub mod live;
pub mod single;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use thiserror::Error;
use tokio::sync::{mpsc, Notify};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument};
use vtrans_core::traits::{CaptureSource, OcrProvider, TranslationProvider};
use vtrans_core::types::{
    CapturedImage, Language, OcrOptions, OcrResult, PipelineMode, PipelineStatus, ScreenRegion,
    TranslationRequest, TranslationResult,
};
use vtrans_core::{truncate_for_log, CaptureError, CoreError, OcrError, TranslationError};
use vtrans_text::japanese;
use vtrans_text::TextNormalizer;

pub use cancel::TaskSlot;
pub use dedup::{FrameDiffer, TextDedup, DEFAULT_DIFFERENCE_THRESHOLD};
pub use single::run_single_capture;

/// Errors reported by the translation pipeline.
///
/// Capture, OCR, and translation errors are imported from `vtrans-core` and
/// wrapped via `#[from]`, so provider errors convert into [`PipelineError`]
/// automatically. The remaining variants describe pipeline lifecycle
/// problems: channels that were closed, a session that is already running or
/// not running, and user-initiated cancellation.
#[derive(Debug, Error)]
pub enum PipelineError {
    /// A screen capture operation failed.
    #[error("capture error: {0}")]
    Capture(#[from] CaptureError),

    /// An OCR recognition pass failed.
    #[error("ocr error: {0}")]
    Ocr(#[from] OcrError),

    /// A translation failed.
    #[error("translation error: {0}")]
    Translation(#[from] TranslationError),

    /// The event channel or an internal stage channel was closed.
    #[error("channel closed")]
    ChannelClosed,

    /// [`Pipeline::run`] was called while a run was already in progress.
    #[error("session already running")]
    AlreadyRunning,

    /// [`Pipeline::stop`] was called while no run was in progress.
    #[error("session not running")]
    NotRunning,

    /// The run was cancelled via [`Pipeline::stop`].
    #[error("cancelled")]
    Cancelled,
}

/// Configuration for a pipeline run.
///
/// # Example
///
/// ```
/// use vtrans_core::{OcrOptions, PipelineMode, ScreenRegion, TranslationRequest, Language};
/// use vtrans_pipeline::PipelineConfig;
///
/// let region = ScreenRegion::new("monitor0", 0, 0, 800, 600);
/// let config = PipelineConfig::live(
///     region,
///     250,                                  // capture every 250 ms
///     0.02,                                 // 2% pixel diff triggers OCR
///     OcrOptions::new(Language::Japanese),
///     TranslationRequest::new("", Language::Auto, Language::ChineseSimplified),
/// );
/// assert!(config.mode.is_live());
/// ```
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Operating mode of the pipeline.
    pub mode: PipelineMode,
    /// Screen region to capture.
    pub region: ScreenRegion,
    /// Capture interval in milliseconds (live mode). Values below
    /// `MIN_CAPTURE_INTERVAL_MS` are clamped up to avoid busy-looping.
    pub capture_interval_ms: u32,
    /// Fraction of differing pixels that triggers OCR (live mode), in the
    /// range `0.0..=1.0`. Out-of-range values are clamped.
    pub difference_threshold: f32,
    /// Options passed to the OCR provider.
    pub ocr_options: OcrOptions,
    /// Translation request template; the OCR text replaces `text` before
    /// each translation.
    pub translation_request: TranslationRequest,
}

impl PipelineConfig {
    /// Creates a pipeline configuration from its parts.
    #[must_use]
    pub fn new(
        mode: PipelineMode,
        region: ScreenRegion,
        capture_interval_ms: u32,
        difference_threshold: f32,
        ocr_options: OcrOptions,
        translation_request: TranslationRequest,
    ) -> Self {
        Self {
            mode,
            region,
            capture_interval_ms,
            difference_threshold,
            ocr_options,
            translation_request,
        }
    }

    /// Creates a single-capture configuration.
    ///
    /// The capture interval and difference threshold are unused in single
    /// mode and take their default values.
    #[must_use]
    pub fn single(
        region: ScreenRegion,
        ocr_options: OcrOptions,
        translation_request: TranslationRequest,
    ) -> Self {
        Self {
            mode: PipelineMode::SingleCapture,
            region,
            capture_interval_ms: 0,
            difference_threshold: DEFAULT_DIFFERENCE_THRESHOLD,
            ocr_options,
            translation_request,
        }
    }

    /// Creates a live-region configuration.
    #[must_use]
    pub fn live(
        region: ScreenRegion,
        capture_interval_ms: u32,
        difference_threshold: f32,
        ocr_options: OcrOptions,
        translation_request: TranslationRequest,
    ) -> Self {
        Self {
            mode: PipelineMode::LiveRegion,
            region,
            capture_interval_ms,
            difference_threshold,
            ocr_options,
            translation_request,
        }
    }
}

/// Concrete provider implementations injected into a [`Pipeline`].
///
/// The pipeline never constructs providers itself; the application layer
/// assembles the concrete capture, OCR, and translation implementations and
/// hands them over through this struct.
pub struct PipelineDeps {
    /// Screen capture source.
    pub capture: Box<dyn CaptureSource>,
    /// OCR provider.
    pub ocr: Box<dyn OcrProvider>,
    /// Translation provider.
    pub translation: Box<dyn TranslationProvider>,
}

impl PipelineDeps {
    /// Creates the dependency set from concrete providers.
    #[must_use]
    pub fn new(
        capture: Box<dyn CaptureSource>,
        ocr: Box<dyn OcrProvider>,
        translation: Box<dyn TranslationProvider>,
    ) -> Self {
        Self {
            capture,
            ocr,
            translation,
        }
    }
}

/// An event emitted by the pipeline at each stage.
///
/// Events are delivered through the `tokio::sync::mpsc` channel passed to
/// [`Pipeline::run`]. OCR and translation payloads are the standard
/// `vtrans-core` types; image data never crosses this boundary.
#[derive(Debug)]
pub enum PipelineEvent {
    /// A frame was captured.
    CaptureStarted,
    /// An OCR pass started.
    OcrStarted,
    /// An OCR pass completed, with the normalized `merged_text`.
    OcrCompleted(OcrResult),
    /// A translation started.
    TranslationStarted,
    /// A translation completed.
    TranslationCompleted(TranslationResult),
    /// A non-terminal error occurred (live mode: an OCR or translation
    /// failure that does not stop the session).
    Error(PipelineError),
    /// The pipeline finished or was stopped.
    Stopped,
}

/// Recovers the inner value of a poisoned synchronization primitive.
///
/// Locks are held for a few instructions at a time; if a task panics while
/// holding one, treating the lock as recoverable keeps the pipeline alive
/// instead of unwinding the caller.
pub(crate) fn poison_inner<T>(poisoned: std::sync::PoisonError<T>) -> T {
    poisoned.into_inner()
}

/// Shared state of a [`Pipeline`].
#[derive(Debug)]
pub(crate) struct PipelineState {
    config: RwLock<PipelineConfig>,
    status: RwLock<PipelineStatus>,
    running: AtomicBool,
    stop: RwLock<Option<CancellationToken>>,
    done: Notify,
    /// Woken whenever the capture region is updated so the live capture
    /// loop can restart its session without waiting for the next frame.
    region_changed: Notify,
}

impl PipelineState {
    /// Returns a snapshot of the current configuration.
    pub(crate) fn config(&self) -> PipelineConfig {
        self.config.read().unwrap_or_else(poison_inner).clone()
    }

    /// Returns the currently configured capture region.
    pub(crate) fn current_region(&self) -> ScreenRegion {
        self.config().region
    }

    /// Overwrites the pipeline status.
    pub(crate) fn set_status(&self, status: PipelineStatus) {
        *self.status.write().unwrap_or_else(poison_inner) = status;
    }

    /// Returns a snapshot of the current status.
    pub(crate) fn current_status(&self) -> PipelineStatus {
        self.status.read().unwrap_or_else(poison_inner).clone()
    }
}

/// Orchestrates capture, OCR, and translation.
///
/// A pipeline holds a configuration plus injected provider implementations.
/// Call [`run`](Self::run) to start a run and [`stop`](Self::stop) to
/// terminate a live run early. Only one run may be active at a time; the
/// same pipeline can be re-run after a run finishes.
///
/// # Example
///
/// ```no_run
/// use tokio::sync::mpsc;
/// use vtrans_core::{OcrOptions, ScreenRegion, TranslationRequest, Language};
/// use vtrans_pipeline::{Pipeline, PipelineConfig, PipelineDeps, PipelineEvent};
///
/// # struct MockCapture;
/// # struct MockOcr;
/// # struct MockTranslation;
/// # #[async_trait::async_trait]
/// # impl vtrans_core::traits::CaptureSource for MockCapture {
/// #     async fn capture_once(&self, _r: &ScreenRegion) -> Result<vtrans_core::CapturedImage, vtrans_core::CaptureError> { unimplemented!() }
/// #     async fn start_session(&self, _r: &ScreenRegion) -> Result<Box<dyn vtrans_core::traits::CaptureSession>, vtrans_core::CaptureError> { unimplemented!() }
/// # }
/// # #[async_trait::async_trait]
/// # impl vtrans_core::traits::OcrProvider for MockOcr {
/// #     fn id(&self) -> &'static str { "mock" }
/// #     async fn recognize(&self, _i: &vtrans_core::CapturedImage, _r: &ScreenRegion, _o: &OcrOptions, _c: tokio_util::sync::CancellationToken) -> Result<vtrans_core::OcrResult, vtrans_core::OcrError> { unimplemented!() }
/// #     fn supported_languages(&self) -> &[Language] { &[] }
/// # }
/// # #[async_trait::async_trait]
/// # impl vtrans_core::traits::TranslationProvider for MockTranslation {
/// #     fn id(&self) -> &'static str { "mock" }
/// #     async fn translate(&self, _r: &TranslationRequest, _c: tokio_util::sync::CancellationToken) -> Result<vtrans_core::TranslationResult, vtrans_core::TranslationError> { unimplemented!() }
/// #     fn supported_pairs(&self) -> &[(Language, Language)] { &[] }
/// # }
///
/// #[tokio::main]
/// async fn main() {
///     let config = PipelineConfig::single(
///         ScreenRegion::new("monitor0", 0, 0, 800, 600),
///         OcrOptions::default(),
///         TranslationRequest::new("", Language::Auto, Language::ChineseSimplified),
///     );
///     let deps = PipelineDeps::new(Box::new(MockCapture), Box::new(MockOcr), Box::new(MockTranslation));
///     let pipeline = Pipeline::new(config, deps);
///     let (tx, _rx) = mpsc::channel(16);
///     let result = pipeline.run(tx).await;
///     assert!(result.is_ok());
/// }
/// ```
pub struct Pipeline {
    deps: Arc<PipelineDeps>,
    state: Arc<PipelineState>,
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `PipelineDeps` holds `Box<dyn Trait>` providers, which do not
        // implement `Debug`; log only the shared state.
        f.debug_struct("Pipeline")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Pipeline {
    /// Creates a pipeline from a configuration and its provider
    /// dependencies.
    #[must_use]
    pub fn new(config: PipelineConfig, deps: PipelineDeps) -> Self {
        Self {
            deps: Arc::new(deps),
            state: Arc::new(PipelineState {
                config: RwLock::new(config),
                status: RwLock::new(PipelineStatus::Idle),
                running: AtomicBool::new(false),
                stop: RwLock::new(None),
                done: Notify::new(),
                region_changed: Notify::new(),
            }),
        }
    }

    /// Runs the pipeline, emitting stage events into `event_tx`.
    ///
    /// In single mode this returns once the capture -> OCR -> translate
    /// chain finishes (or fails). In live mode this blocks until
    /// [`stop`](Self::stop) is called or the capture session ends.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::AlreadyRunning`] when a run is already in
    /// progress, [`PipelineError::ChannelClosed`] when `event_tx` has no
    /// receivers, or a capture/OCR/translation error that terminated the
    /// run.
    #[instrument(skip_all)]
    pub async fn run(&self, event_tx: mpsc::Sender<PipelineEvent>) -> Result<(), PipelineError> {
        if event_tx.is_closed() {
            return Err(PipelineError::ChannelClosed);
        }

        // Mark the pipeline as running and create a fresh stop token. A
        // second concurrent `run` is rejected by the running flag.
        let stop = CancellationToken::new();
        {
            if self.state.running.swap(true, Ordering::SeqCst) {
                return Err(PipelineError::AlreadyRunning);
            }
            self.state.set_status(PipelineStatus::Idle);
            *self.state.stop.write().unwrap_or_else(poison_inner) = Some(stop.clone());
        }

        let config = self.state.config();
        info!(
            mode = ?config.mode,
            region = ?config.region,
            "pipeline run started"
        );
        let result = match config.mode {
            PipelineMode::SingleCapture => {
                single::run_single_capture_internal(
                    self.deps.clone(),
                    self.state.clone(),
                    config,
                    stop,
                    &event_tx,
                )
                .await
            }
            PipelineMode::LiveRegion => {
                live::run_live(
                    self.deps.clone(),
                    self.state.clone(),
                    stop,
                    event_tx.clone(),
                )
                .await
            }
        };

        // Lifecycle cleanup: clear the running flag and stop token, wake
        // any `stop()` waiters, and settle the final status.
        {
            self.state.running.store(false, Ordering::SeqCst);
            *self.state.stop.write().unwrap_or_else(poison_inner) = None;
            match &result {
                Ok(()) => {
                    if matches!(
                        self.state.current_status(),
                        PipelineStatus::Capturing
                            | PipelineStatus::OcrInProgress
                            | PipelineStatus::Translating
                    ) {
                        self.state.set_status(PipelineStatus::Idle);
                    }
                }
                Err(PipelineError::Cancelled) => self.state.set_status(PipelineStatus::Idle),
                Err(error) => {
                    self.state
                        .set_status(PipelineStatus::Error(error.to_string()));
                }
            }
            self.state.done.notify_waiters();
        }
        result
    }

    /// Stops a running pipeline and waits for it to terminate.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::NotRunning`] when no run is in progress.
    #[instrument(skip_all)]
    pub async fn stop(&self) -> Result<(), PipelineError> {
        let token = self.state.stop.read().unwrap_or_else(poison_inner).clone();
        let Some(token) = token else {
            return Err(PipelineError::NotRunning);
        };
        info!("stopping pipeline");
        token.cancel();
        while self.state.running.load(Ordering::SeqCst) {
            self.state.done.notified().await;
        }
        info!("pipeline stopped");
        Ok(())
    }

    /// Returns the current pipeline status.
    #[must_use]
    pub fn status(&self) -> PipelineStatus {
        self.state.current_status()
    }

    /// Updates the capture region.
    ///
    /// The new region is validated first. In live mode an active capture
    /// session is restarted with the new region at the next capture tick;
    /// the pipeline itself is not interrupted.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidRegion`] when the region has a zero
    /// dimension.
    #[instrument(skip_all)]
    #[allow(clippy::unused_async)] // The spec fixes an async surface; the body is synchronous.
    pub async fn update_region(&self, region: ScreenRegion) -> Result<(), CoreError> {
        region.validate()?;
        let mut config = self.state.config.write().unwrap_or_else(poison_inner);
        let previous = config.region.clone();
        config.region = region.clone();
        drop(config);
        self.state.region_changed.notify_one();
        debug!(
            old = ?previous,
            new = ?region,
            "updated pipeline capture region"
        );
        Ok(())
    }
}

/// Builds a region aligned with a captured image's own coordinate space.
///
/// `vtrans-capture` crops frames to the requested screen region, so the
/// image's top-left corner is always `(0, 0)`. Passing the original region
/// with its screen offset to OCR would trigger a second crop (see the
/// `vtrans-ocr` integration notes); callers should pass a region derived
/// from the image dimensions instead.
pub(crate) fn image_aligned_region(monitor_id: &str, image: &CapturedImage) -> ScreenRegion {
    ScreenRegion::new(monitor_id.to_string(), 0, 0, image.width, image.height)
}

/// Translates `text` through the configured provider, chunking long text
/// first.
///
/// `vtrans-text` splits the text into paragraphs of at most
/// [`DEFAULT_MAX_PARAGRAPH_LEN`](vtrans_text::DEFAULT_MAX_PARAGRAPH_LEN)
/// characters, and each chunk is translated sequentially with the same
/// cancellation token. The chunk translations are joined with `\n` into a
/// single [`TranslationResult`]; `elapsed_ms` is the sum over all chunks.
///
/// # Errors
///
/// Returns the first [`TranslationError`] from any chunk.
#[instrument(skip(deps, text), fields(sample = %truncate_for_log(text)))]
pub(crate) async fn translate_text(
    deps: &PipelineDeps,
    text: &str,
    source: Language,
    target: Language,
    cancel: CancellationToken,
) -> Result<TranslationResult, TranslationError> {
    let chunks = TextNormalizer::split_paragraphs_default(text);
    let mut translated = Vec::with_capacity(chunks.len());
    let mut provider_id = String::new();
    let mut total_elapsed_ms = 0;
    for chunk in chunks {
        let request = TranslationRequest::new(chunk, source, target);
        let result = deps.translation.translate(&request, cancel.clone()).await?;
        provider_id = result.provider_id;
        total_elapsed_ms += result.elapsed_ms;
        translated.push(result.translated_text);
    }
    Ok(TranslationResult::new(
        translated.join("\n"),
        provider_id,
        total_elapsed_ms,
    ))
}

/// Merges and cleans OCR lines for translation.
///
/// Applies the language-neutral cleaner, then Japanese punctuation
/// normalization when the source is Japanese (or detected as Japanese while
/// the source is `Auto`). The cleaned text replaces `merged_text` in the
/// returned result.
pub(crate) fn normalize_result(result: OcrResult, source: Language) -> OcrResult {
    let merged = TextNormalizer::merge_lines(&result.lines);
    let cleaned = TextNormalizer::clean(&merged);
    let japanese_source = source == Language::Japanese
        || (source.is_auto() && result.detected_language == Some(Language::Japanese));
    let cleaned = if japanese_source {
        japanese::normalize_punctuation(&cleaned)
    } else {
        cleaned
    };
    OcrResult {
        merged_text: cleaned,
        ..result
    }
}
