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
pub mod language;
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
pub(crate) use language::resolve_effective_source;
pub use language::{heuristic_detect_language, resolve_translation_source};
pub use single::run_single_capture;

/// Observes captured frames before they enter OCR.
///
/// The pipeline calls [`on_frame`](Self::on_frame) for every frame that
/// reaches the OCR stage (in live mode, after frame-difference detection
/// accepted the frame; in single mode, after capture). The callback is
/// synchronous and must not block the pipeline: implementations forward the
/// frame into a bounded queue and return immediately. A pipeline without a
/// sink (`None`) performs no extra work, so Debug-only frame observation is
/// zero-cost when disabled.
pub trait FrameSink: Send + Sync {
    /// Observes one frame that is about to enter OCR.
    fn on_frame(&self, frame: &CapturedImage);
}

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
    frame_sink: Option<Arc<dyn FrameSink>>,
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
        Self::with_frame_sink(config, deps, None)
    }

    /// Creates a pipeline with an optional frame observer.
    ///
    /// The sink receives every frame that is about to enter OCR. Passing
    /// `None` (the default, see [`new`](Self::new)) keeps the capture path
    /// identical to a pipeline without frame observation.
    #[must_use]
    pub fn with_frame_sink(
        config: PipelineConfig,
        deps: PipelineDeps,
        frame_sink: Option<Arc<dyn FrameSink>>,
    ) -> Self {
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
            frame_sink,
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
        let frame_sink = self.frame_sink.clone();
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
                    frame_sink,
                )
                .await
            }
            PipelineMode::LiveRegion => {
                live::run_live(
                    self.deps.clone(),
                    self.state.clone(),
                    stop,
                    event_tx.clone(),
                    frame_sink,
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

/// Absolute ceiling for one translation chunk, in characters.
///
/// This is the final hard-split fallback: sources without a dedicated
/// budget (Chinese, or an unresolved `Auto`) keep the historical
/// 2000-character limit, and every chunk produced by the punctuation-aware
/// splitter is at most this long.
pub const MAX_TRANSLATION_CHUNK_CHARS: usize = 2000;

/// Character budget for a translation chunk of Japanese text.
///
/// Aligned with the native engines' `max_input_tokens = 256` using a
/// conservative characters-per-token estimate (translation integration
/// guide §9.3). Unit tests lock the value.
pub const JA_CHUNK_CHARS: usize = 512;

/// Character budget for a translation chunk of English text.
///
/// English tokenizes denser than Japanese (roughly 4 characters per token
/// for the `SentencePiece` vocabularies used by the native engines), so the
/// budget is larger while still staying within `max_input_tokens = 256`.
/// Unit tests lock the value.
pub const EN_CHUNK_CHARS: usize = 1024;

/// Returns the per-chunk character budget for `source`.
fn chunk_budget(source: Language) -> usize {
    match source {
        Language::Japanese => JA_CHUNK_CHARS,
        Language::English => EN_CHUNK_CHARS,
        // Chinese and unresolved `Auto` sources keep the historical
        // ceiling; the native provider does not serve Chinese sources, and
        // API providers have their own length limits.
        Language::ChineseSimplified | Language::Auto => MAX_TRANSLATION_CHUNK_CHARS,
    }
}

/// Splits `text` into translation chunks for `source`.
///
/// A text that fits within the source-specific budget
/// ([`chunk_budget`]) is translated in a single call with newlines
/// preserved - the common case for screen translation. Longer texts are
/// separated at newlines first (each paragraph becomes one or more chunks,
/// matching the paragraph semantics of `vtrans-text`), and an over-long
/// paragraph is split at sentence boundaries (`。！？.!?`), then at
/// commas/semicolons (`，、,;`), then at whitespace, and finally at a hard
/// character boundary (never inside a Unicode scalar). Chunks are trimmed;
/// [`translate_text`] joins the translations back with `\n`, so paragraph
/// structure survives chunking.
fn chunk_translation_text(text: &str, source: Language) -> Vec<String> {
    let budget = chunk_budget(source);
    if text.is_empty() || text.chars().count() <= budget {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        if paragraph.chars().count() <= budget {
            chunks.push(paragraph.to_string());
        } else {
            chunks.extend(split_long_paragraph(paragraph, budget));
        }
    }
    chunks
}

/// Splits one over-long paragraph into chunks of at most `budget` chars.
fn split_long_paragraph(paragraph: &str, budget: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut remaining = paragraph;
    while remaining.chars().count() > budget {
        let (chunk, rest) = take_chunk(remaining, budget);
        // `trim_end` guards the window-boundary cut: the window may end
        // with whitespace when the input has consecutive spaces.
        chunks.push(chunk.trim_end().to_string());
        remaining = rest.trim_start();
    }
    if !remaining.is_empty() {
        chunks.push(remaining.to_string());
    }
    chunks
}

/// Returns `true` when `ch` ends a sentence for chunking purposes.
///
/// The union of the Japanese (`。！？`) and English (`.!?`) sentence-ending
/// sets (translation integration guide §9.3). Fullwidth forms are matched
/// as well because `vtrans-text` cleaning may leave either representation.
fn is_sentence_ender(ch: char) -> bool {
    matches!(ch, '。' | '！' | '？' | '.' | '!' | '?')
}

/// Returns `true` when `ch` is a clause-level chunk boundary.
fn is_comma_boundary(ch: char) -> bool {
    matches!(ch, '，' | '、' | ',' | ';')
}

/// Cuts the next chunk off `paragraph`.
///
/// Returns `(chunk, rest)` where `chunk` has at most `budget` characters.
/// Sentence-ending punctuation is preferred, then commas/semicolons, then
/// whitespace; each boundary must consume at least half of the window so no
/// tiny fragments are produced. When no soft boundary exists, the chunk is
/// cut at the window boundary (a Unicode-scalar boundary by construction).
fn take_chunk(paragraph: &str, budget: usize) -> (&str, &str) {
    debug_assert!(paragraph.chars().count() > budget);

    let mut indices = paragraph.char_indices();
    // Byte offset just past the first `budget` characters (the window).
    let window_end = indices
        .nth(budget - 1)
        .map_or(paragraph.len(), |(idx, ch)| idx + ch.len_utf8());

    let window = &paragraph[..window_end];
    let floor = budget / 2;
    if let Some((byte_idx, ch)) = last_boundary(window, is_sentence_ender, floor) {
        // Include the sentence-ending punctuation in the chunk.
        let end = byte_idx + ch.len_utf8();
        return (&paragraph[..end], &paragraph[end..]);
    }
    if let Some((byte_idx, ch)) = last_boundary(window, is_comma_boundary, floor) {
        let end = byte_idx + ch.len_utf8();
        return (&paragraph[..end], &paragraph[end..]);
    }
    if let Some((byte_idx, _)) = last_boundary(window, char::is_whitespace, floor) {
        // Cut before the whitespace; `split_long_paragraph` trims the rest.
        return (&paragraph[..byte_idx], &paragraph[byte_idx..]);
    }
    (&paragraph[..window_end], &paragraph[window_end..])
}

/// Finds the last character in `window` matching `predicate` whose chunk
/// prefix would consume at least half of the window (`floor + 1` characters
/// or more), and returns its byte index together with the character itself
/// so the caller can decide whether to include it.
///
/// Returns `None` when no such boundary exists.
fn last_boundary(window: &str, predicate: fn(char) -> bool, floor: usize) -> Option<(usize, char)> {
    let mut result = None;
    for (char_idx, (byte_idx, ch)) in window.char_indices().enumerate() {
        if char_idx + 1 >= floor && predicate(ch) {
            result = Some((byte_idx, ch));
        }
    }
    result
}

/// Translates `text` through the configured provider.
///
/// The whole text is sent as a single request whenever it fits within the
/// source-specific budget (see [`chunk_translation_text`]) - the common
/// case for screen translation. This keeps the number of provider calls as
/// low as possible, which is the dominant cost for local engines. Longer
/// texts are split at newlines and then at sentence / comma / whitespace
/// boundaries; each chunk is translated sequentially with the same
/// cancellation token. Chunk translations are joined with `\n` into a
/// single [`TranslationResult`]; `elapsed_ms` is the sum over all chunks.
///
/// Note: providers truncate input at their own `max_length` (see the model
/// manifest); the pipeline limit is a defensive upper bound for the
/// provider-agnostic text size.
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
    let chunks = chunk_translation_text(text, source);
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
/// normalization when the effective translation source is Japanese.
///
/// Callers must pass the source resolved by
/// [`language::resolve_effective_source`]; the `Auto` + detected-Japanese
/// combination is still honored defensively for standalone use.
/// The cleaned text replaces `merged_text` in the returned result.
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── chunk budgets ──

    #[test]
    fn chunk_budgets_are_locked_by_tests() {
        assert_eq!(MAX_TRANSLATION_CHUNK_CHARS, 2000);
        assert_eq!(JA_CHUNK_CHARS, 512);
        assert_eq!(EN_CHUNK_CHARS, 1024);
        assert_eq!(chunk_budget(Language::Japanese), JA_CHUNK_CHARS);
        assert_eq!(chunk_budget(Language::English), EN_CHUNK_CHARS);
        assert_eq!(
            chunk_budget(Language::ChineseSimplified),
            MAX_TRANSLATION_CHUNK_CHARS
        );
        assert_eq!(chunk_budget(Language::Auto), MAX_TRANSLATION_CHUNK_CHARS);
    }

    // ── chunk_translation_text ──

    #[test]
    fn chunk_short_text_in_single_call() {
        assert_eq!(
            chunk_translation_text("hello world", Language::English),
            vec!["hello world"]
        );
        let exactly_en = "a".repeat(EN_CHUNK_CHARS);
        assert_eq!(
            chunk_translation_text(&exactly_en, Language::English),
            vec![exactly_en]
        );
        let exactly_ja = "あ".repeat(JA_CHUNK_CHARS);
        assert_eq!(
            chunk_translation_text(&exactly_ja, Language::Japanese),
            vec![exactly_ja]
        );
    }

    #[test]
    fn chunk_short_text_preserves_newlines() {
        let text = "line one\nline two";
        assert_eq!(
            chunk_translation_text(text, Language::English),
            vec![text.to_string()]
        );
    }

    #[test]
    fn chunk_long_text_hard_splits_at_default_budget() {
        let text = "x".repeat(2500);
        let chunks = chunk_translation_text(&text, Language::ChineseSimplified);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), MAX_TRANSLATION_CHUNK_CHARS);
        assert_eq!(chunks[1].len(), 500);
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunk_never_splits_unicode_scalars() {
        let text = "日".repeat(2500);
        let chunks = chunk_translation_text(&text, Language::ChineseSimplified);
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|chunk| chunk.chars().all(|c| c == '日')));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunk_empty_text_is_single_call() {
        assert_eq!(chunk_translation_text("", Language::English), vec![""]);
    }

    #[test]
    fn chunk_japanese_uses_512_char_budget() {
        let text = "こんにちは".repeat(200); // 1000 characters > 512
        let chunks = chunk_translation_text(&text, Language::Japanese);
        assert!(chunks.len() >= 2);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.chars().count() <= JA_CHUNK_CHARS));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunk_splits_long_text_at_newlines() {
        let first = "あ".repeat(300);
        let second = "い".repeat(300);
        let text = format!("{first}\n{second}");
        let chunks = chunk_translation_text(&text, Language::Japanese);
        assert_eq!(chunks, vec![first, second]);
    }

    #[test]
    fn chunk_drops_blank_lines_only_when_over_budget() {
        let first = "あ".repeat(300);
        let second = "い".repeat(300);
        let text = format!("{first}\n\n{second}");
        let chunks = chunk_translation_text(&text, Language::Japanese);
        assert_eq!(chunks, vec![first, second]);
    }

    // ── split_long_paragraph / take_chunk ──

    #[test]
    fn split_prefers_sentence_boundaries() {
        let chunks = split_long_paragraph("AAAA. BBBB. CCCC.", 8);
        assert_eq!(chunks, vec!["AAAA.", "BBBB.", "CCCC."]);
    }

    #[test]
    fn split_prefers_sentence_over_comma_within_window() {
        // The last sentence ender (index 15) wins over the comma (index 10)
        // inside the 20-character window.
        let chunks = split_long_paragraph("AAAA. BBBB, CCCC. DDDD, EEEE.", 20);
        assert_eq!(chunks, vec!["AAAA. BBBB, CCCC.", "DDDD, EEEE."]);
    }

    #[test]
    fn split_falls_back_to_comma_boundaries() {
        let chunks = split_long_paragraph("aaaa,bbbb cccc", 8);
        assert_eq!(chunks, vec!["aaaa,", "bbbb", "cccc"]);
    }

    #[test]
    fn split_falls_back_to_whitespace() {
        let chunks = split_long_paragraph("aaaa bbbb cccc", 6);
        assert_eq!(chunks, vec!["aaaa", "bbbb", "cccc"]);
    }

    #[test]
    fn split_falls_back_to_hard_cut() {
        let chunks = split_long_paragraph("abcdefgh", 3);
        assert_eq!(chunks, vec!["abc", "def", "gh"]);
    }

    #[test]
    fn split_respects_japanese_sentence_enders() {
        let chunks = split_long_paragraph("こんにちは。また明日。さようなら。", 10);
        assert_eq!(chunks, vec!["こんにちは。", "また明日。", "さようなら。"]);
    }

    #[test]
    fn split_never_breaks_unicode_scalars() {
        let text = "😀".repeat(600);
        let chunks = split_long_paragraph(&text, 512);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.chars().count() <= 512 && chunk.chars().all(|c| c == '😀')));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn take_chunk_prefers_sentence_ender_over_comma() {
        // Window "AAAA, BBBB." with budget 12: the sentence ender at index
        // 11 is preferred over the comma at index 4.
        let (chunk, rest) = take_chunk("AAAA, BBBB. CCCC", 12);
        assert_eq!(chunk, "AAAA, BBBB.");
        assert_eq!(rest, " CCCC");
    }

    #[test]
    fn normalize_result_uses_resolved_source_for_japanese_punctuation() {
        let polygon = [[0.0, 0.0], [100.0, 0.0], [100.0, 20.0], [0.0, 20.0]];
        let result = OcrResult::from_lines(
            vec![vtrans_core::OcrLine::new(
                "ＨＰ １００，攻撃力アップ．",
                0.9,
                polygon,
                0,
            )],
            Some(Language::Japanese),
            5,
        );
        let normalized = normalize_result(result, Language::Japanese);
        assert_eq!(normalized.merged_text, "HP 100、攻撃力アップ。");
    }

    #[test]
    fn normalize_result_keeps_punctuation_for_non_japanese_source() {
        let polygon = [[0.0, 0.0], [100.0, 0.0], [100.0, 20.0], [0.0, 20.0]];
        let result = OcrResult::from_lines(
            vec![vtrans_core::OcrLine::new(
                "ＨＰ １００，攻撃力アップ．",
                0.9,
                polygon,
                0,
            )],
            Some(Language::Japanese),
            5,
        );
        let normalized = normalize_result(result, Language::English);
        assert_eq!(normalized.merged_text, "HP 100，攻撃力アップ．");
    }
}
