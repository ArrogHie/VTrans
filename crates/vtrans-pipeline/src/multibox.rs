//! Multi-box real-time translation pipeline.
//!
//! [`MultiBoxPipeline`] manages multiple [`TranslationBox`] instances, each
//! running as an independent Tokio task with its own capture session, frame
//! differ, OCR worker, and translation worker. Results from all boxes are
//! collected into a single bounded broadcast channel and delivered to
//! subscribers as [`BoxedTranslationResult`].
//!
//! # Architecture
//!
//! Each box spawns three sub-tasks mirroring the single-box live pipeline:
//!
//! 1. **capture loop** -- owns a `CaptureSession`, applies per-box
//!    [`FrameDiffer`] detection, forwards changed frames into a cap-1
//!    channel;
//! 2. **OCR worker** -- consumes frames, runs at most one OCR pass at a
//!    time via [`TaskSlot`], normalizes text, checks per-box dedup via
//!    [`BoxFingerprintCache`], forwards non-duplicate jobs;
//! 3. **translation worker** -- runs at most one translation at a time,
//!    pairs the OCR original text with the result, tags it with `box_id`
//!    and `color`, and broadcasts it. Empty OCR text skips the provider
//!    call, and translation failures still publish a result; both
//!    degraded cases carry an empty `original_text` so the overlay is
//!    cleared rather than left with stale content.
//!
//! All three sub-tasks observe the box's own [`CancellationToken`], so
//! [`MultiBoxPipeline::stop_box`] / [`remove_box`] / [`stop_all`]
//! terminate a box in bounded time. A failure in one box (capture error,
//! etc.) sets its [`BoxStatus`] to [`Error`](BoxStatus::Error) without
//! affecting any other box.
//!
//! # Concurrency
//!
//! The pipeline is `Send + Sync`. The box registry is protected by a
//! `std::sync::RwLock`; locks are held only for quick map operations and
//! dropped before any `.await`. The result channel is a
//! `tokio::sync::broadcast` with capacity `max_boxes * 2`; each subscriber
//! gets a private `mpsc::Receiver` with per-subscriber backpressure via a
//! forwarder task.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn};
use vtrans_core::truncate_for_log;
use vtrans_core::types::{
    CapturedImage, Language, OcrOptions, ScreenRegion, TranslationRequest, TranslationResult,
};
use vtrans_text::BoxFingerprintCache;

use crate::cancel::TaskSlot;
use crate::dedup::FrameDiffer;
use crate::live::{clamp_interval_ms, clamp_threshold};
use crate::{
    image_aligned_region, normalize_result, poison_inner, translate_text, PipelineDeps,
    PipelineError,
};

/// Default maximum number of concurrent translation boxes.
const DEFAULT_MAX_BOXES: u32 = 8;

/// Multiplier applied to `max_boxes` to size the results broadcast channel.
const RESULTS_CHANNEL_CAPACITY_MULTIPLIER: usize = 2;

/// Capacity of the per-box capture-to-OCR and OCR-to-translation channels.
/// Same as the single-box live pipeline: cap-1 keeps memory bounded.
const PER_BOX_CHANNEL_CAPACITY: usize = 1;

// ==========================================================================
// Public types
// ==========================================================================

/// A single translation box identified by `id`, covering `region` and
/// displayed with `color`.
///
/// Serialized for IPC transport to the frontend; `region` reuses the
/// existing `vtrans_core::ScreenRegion` serde representation.
///
/// # Example
///
/// ```
/// use vtrans_core::ScreenRegion;
/// use vtrans_pipeline::TranslationBox;
///
/// let box_ = TranslationBox::new(0, ScreenRegion::new("m0", 0, 0, 800, 600), "#FF6B6B");
/// assert_eq!(box_.id, 0);
/// assert_eq!(box_.color, "#FF6B6B");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationBox {
    /// Unique identifier for this box.
    pub id: u32,
    /// Screen region captured and translated for this box.
    pub region: ScreenRegion,
    /// Hex color string (e.g. `"#FF6B6B"`) used to visually distinguish
    /// this box's results in the overlay.
    pub color: String,
}

impl TranslationBox {
    /// Creates a new translation box.
    #[must_use]
    pub fn new(id: u32, region: ScreenRegion, color: impl Into<String>) -> Self {
        Self {
            id,
            region,
            color: color.into(),
        }
    }
}

/// Configuration for a [`MultiBoxPipeline`].
///
/// Contains pipeline-level settings shared by all boxes. Per-box settings
/// (region, color) live on each [`TranslationBox`].
///
/// # Example
///
/// ```
/// use vtrans_core::{Language, OcrOptions, TranslationRequest};
/// use vtrans_pipeline::MultiBoxConfig;
///
/// let config = MultiBoxConfig::new(
///     250,
///     0.02,
///     OcrOptions::new(Language::Japanese),
///     TranslationRequest::new("", Language::Japanese, Language::ChineseSimplified),
/// );
/// assert_eq!(config.max_boxes, 8);
/// ```
#[derive(Debug, Clone)]
pub struct MultiBoxConfig {
    /// Capture interval in milliseconds (per box). Values below 16 ms are
    /// clamped up to avoid busy-looping.
    pub capture_interval_ms: u32,
    /// Fraction of differing pixels that triggers OCR, in `0.0..=1.0`.
    pub difference_threshold: f32,
    /// OCR options shared by all boxes.
    pub ocr_options: OcrOptions,
    /// Translation request template; the OCR text replaces `text` before
    /// each translation.
    pub translation_request: TranslationRequest,
    /// Maximum number of concurrent boxes allowed. Defaults to 8.
    pub max_boxes: u32,
}

impl MultiBoxConfig {
    /// Creates a configuration with default `max_boxes` (8).
    #[must_use]
    pub fn new(
        capture_interval_ms: u32,
        difference_threshold: f32,
        ocr_options: OcrOptions,
        translation_request: TranslationRequest,
    ) -> Self {
        Self {
            capture_interval_ms,
            difference_threshold,
            ocr_options,
            translation_request,
            max_boxes: DEFAULT_MAX_BOXES,
        }
    }

    /// Creates a configuration with an explicit `max_boxes`.
    #[must_use]
    pub fn with_max_boxes(
        capture_interval_ms: u32,
        difference_threshold: f32,
        ocr_options: OcrOptions,
        translation_request: TranslationRequest,
        max_boxes: u32,
    ) -> Self {
        Self {
            capture_interval_ms,
            difference_threshold,
            ocr_options,
            translation_request,
            max_boxes,
        }
    }

    /// Returns the capacity for the results broadcast channel:
    /// `max_boxes * 2`, clamped to at least 1.
    #[must_use]
    pub(crate) fn results_capacity(&self) -> usize {
        (self.max_boxes as usize)
            .saturating_mul(RESULTS_CHANNEL_CAPACITY_MULTIPLIER)
            .max(1)
    }
}

impl Default for MultiBoxConfig {
    fn default() -> Self {
        Self::new(
            250,
            crate::dedup::DEFAULT_DIFFERENCE_THRESHOLD,
            OcrOptions::default(),
            TranslationRequest::new("", Language::Auto, Language::ChineseSimplified),
        )
    }
}

/// A translation result tagged with the box that produced it, plus the
/// original OCR text that was translated.
///
/// Serialized for IPC transport so the frontend can associate each result
/// with its originating translation box, color, and source text. The
/// `original_text` field carries the cleaned OCR text (the same text sent
/// to the translation provider) and degrades to an empty string when
/// translation fails or OCR produced no text; such degraded results are
/// still published so stale overlay content is cleared.
///
/// # Example
///
/// ```
/// use vtrans_core::TranslationResult;
/// use vtrans_pipeline::BoxedTranslationResult;
///
/// let result = TranslationResult::new("hello", "mock", 42);
/// let boxed = BoxedTranslationResult::new(0, "#FF6B6B", result)
///     .with_original_text("hello");
/// assert_eq!(boxed.box_id, 0);
/// assert_eq!(boxed.original_text, "hello");
/// assert_eq!(boxed.result.translated_text, "hello");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxedTranslationResult {
    /// ID of the box that produced this result.
    pub box_id: u32,
    /// Color of the originating box.
    pub color: String,
    /// The translation result.
    pub result: TranslationResult,
    /// Original OCR text that was translated (cleaned source-language
    /// text). Empty when translation failed or OCR produced no text.
    pub original_text: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
}

impl BoxedTranslationResult {
    /// Creates a new boxed result with the current timestamp and an empty
    /// `original_text`; pair the OCR text with
    /// [`with_original_text`](Self::with_original_text).
    #[must_use]
    pub fn new(box_id: u32, color: impl Into<String>, result: TranslationResult) -> Self {
        Self {
            box_id,
            color: color.into(),
            result,
            original_text: String::new(),
            timestamp: now_millis(),
        }
    }

    /// Attaches the original OCR text paired with this translation.
    ///
    /// The text is the cleaned OCR output that was sent to the translation
    /// provider (see [`normalize_result`]); callers pass an empty string
    /// for degraded results.
    #[must_use]
    pub fn with_original_text(mut self, original_text: impl Into<String>) -> Self {
        self.original_text = original_text.into();
        self
    }
}

/// Runtime status of a single translation box.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BoxStatus {
    /// The box's pipeline task is actively running.
    Running,
    /// The box is registered but its task is stopped.
    Stopped,
    /// The box's task terminated with an error.
    Error(String),
}

// ==========================================================================
// Internal types
// ==========================================================================

/// Registry entry for a single box, holding its info, cancel token, and
/// optional task handle.
struct BoxEntry {
    info: TranslationBox,
    cancel: CancellationToken,
    task: Option<JoinHandle<()>>,
}

/// Shared context handed to each box's sub-tasks.
#[derive(Clone)]
struct BoxWorkerCtx {
    deps: Arc<PipelineDeps>,
    results_tx: broadcast::Sender<BoxedTranslationResult>,
    dedup: Arc<BoxFingerprintCache>,
    status: Arc<RwLock<HashMap<u32, BoxStatus>>>,
    cancel: CancellationToken,
    ocr_options: OcrOptions,
    source: Language,
    target: Language,
    interval: Duration,
    threshold: f32,
    box_id: u32,
    color: String,
    monitor_id: String,
}

/// A cleaned OCR result handed from the OCR stage to the translation stage
/// within a single box.
struct BoxOcrJob {
    text: String,
    source: Language,
    target: Language,
}

// ==========================================================================
// MultiBoxPipeline
// ==========================================================================

/// Orchestrates multiple translation boxes, each running as an independent
/// capture-OCR-translate pipeline.
///
/// The pipeline is `Send + Sync` and designed for concurrent access from
/// the application layer. Boxes can be added, removed, updated, started,
/// and stopped at runtime without affecting other boxes.
///
/// # Example
///
/// ```no_run
/// use vtrans_core::{ScreenRegion, OcrOptions, TranslationRequest, Language};
/// use vtrans_pipeline::{MultiBoxConfig, MultiBoxPipeline, TranslationBox, PipelineDeps};
///
/// # struct MockCapture;
/// # // Mock providers omitted for brevity -- see tests for full examples.
/// #[tokio::main]
/// async fn main() {
/// #   let capture: Box<dyn vtrans_core::traits::CaptureSource> = unimplemented!();
/// #   let ocr: Box<dyn vtrans_core::traits::OcrProvider> = unimplemented!();
/// #   let translation: Box<dyn vtrans_core::traits::TranslationProvider> = unimplemented!();
///     let config = MultiBoxConfig::new(
///         250, 0.02,
///         OcrOptions::new(Language::Japanese),
///         TranslationRequest::new("", Language::Japanese, Language::ChineseSimplified),
///     );
///     let deps = PipelineDeps::new(capture, ocr, translation);
///     let pipeline = MultiBoxPipeline::new(config, deps);
///     pipeline.add_box(TranslationBox::new(0, ScreenRegion::new("m0", 0, 0, 800, 600), "#FF6B6B"))
///         .await
///         .unwrap();
///     pipeline.start_all().await.unwrap();
///     let mut results = pipeline.subscribe_results();
///     // ... consume results ...
///     pipeline.stop_all().await.unwrap();
/// }
/// ```
pub struct MultiBoxPipeline {
    deps: Arc<PipelineDeps>,
    config: MultiBoxConfig,
    boxes: Arc<RwLock<HashMap<u32, BoxEntry>>>,
    status: Arc<RwLock<HashMap<u32, BoxStatus>>>,
    results_tx: broadcast::Sender<BoxedTranslationResult>,
    dedup: Arc<BoxFingerprintCache>,
    running: Arc<AtomicBool>,
}

impl std::fmt::Debug for MultiBoxPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let box_ids: Vec<u32> = self
            .boxes
            .read()
            .unwrap_or_else(poison_inner)
            .keys()
            .copied()
            .collect();
        f.debug_struct("MultiBoxPipeline")
            .field("box_count", &box_ids.len())
            .field("box_ids", &box_ids)
            .field("running", &self.running.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl MultiBoxPipeline {
    /// Creates a multi-box pipeline from configuration and injected
    /// providers.
    #[must_use]
    pub fn new(config: MultiBoxConfig, deps: PipelineDeps) -> Self {
        let capacity = config.results_capacity();
        let (results_tx, _) = broadcast::channel(capacity);
        Self {
            deps: Arc::new(deps),
            config,
            boxes: Arc::new(RwLock::new(HashMap::new())),
            status: Arc::new(RwLock::new(HashMap::new())),
            results_tx,
            dedup: Arc::new(BoxFingerprintCache::new()),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns the maximum number of boxes this pipeline accepts.
    #[must_use]
    pub fn max_boxes(&self) -> u32 {
        self.config.max_boxes
    }

    /// Returns the current number of registered boxes (running or stopped).
    #[must_use]
    pub fn box_count(&self) -> usize {
        self.boxes.read().unwrap_or_else(poison_inner).len()
    }

    /// Returns the status of a specific box.
    #[must_use]
    pub fn box_status(&self, box_id: u32) -> Option<BoxStatus> {
        self.status
            .read()
            .unwrap_or_else(poison_inner)
            .get(&box_id)
            .cloned()
    }

    /// Adds a translation box to the registry. If the pipeline is running,
    /// the box's task starts immediately.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::BoxLimitExceeded`] when the box count would
    /// exceed `max_boxes`, [`PipelineError::DuplicateBoxId`] when a box
    /// with the same ID already exists, or [`PipelineError::InvalidConfig`]
    /// when the region has a zero dimension.
    #[instrument(skip_all, fields(box_id = box_.id))]
    pub async fn add_box(&self, box_: TranslationBox) -> Result<(), PipelineError> {
        box_.region
            .validate()
            .map_err(|e| PipelineError::InvalidConfig(format!("invalid box region: {e}")))?;

        let running = self.running.load(Ordering::SeqCst);

        // Pre-check capacity and duplicate ID with a read lock.
        {
            let boxes = self.boxes.read().unwrap_or_else(poison_inner);
            if boxes.len() >= self.config.max_boxes as usize {
                return Err(PipelineError::BoxLimitExceeded(self.config.max_boxes));
            }
            if boxes.contains_key(&box_.id) {
                return Err(PipelineError::DuplicateBoxId(box_.id));
            }
        }

        // Spawn the task if the pipeline is running. This must happen
        // before acquiring the write lock to avoid a partial-borrow
        // conflict (spawn_box_task borrows &self).
        let cancel = CancellationToken::new();
        let task = if running {
            Some(self.spawn_box_task(box_.clone(), cancel.clone()))
        } else {
            None
        };

        // Insert into the registry.
        {
            let mut boxes = self.boxes.write().unwrap_or_else(poison_inner);
            // A concurrent caller could have inserted the same ID between
            // the pre-check and here. Replace the old entry and cancel its
            // task to avoid orphaned tasks.
            if let Some(old) = boxes.remove(&box_.id) {
                old.cancel.cancel();
            }
            boxes.insert(
                box_.id,
                BoxEntry {
                    info: box_.clone(),
                    cancel,
                    task,
                },
            );
        }

        self.set_status(
            box_.id,
            if running {
                BoxStatus::Running
            } else {
                BoxStatus::Stopped
            },
        );
        info!(box_id = box_.id, color = %box_.color, "translation box added");
        Ok(())
    }

    /// Removes a translation box from the registry, stopping its task first.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::BoxNotFound`] when `box_id` does not exist.
    #[instrument(skip_all, fields(box_id))]
    pub async fn remove_box(&self, box_id: u32) -> Result<(), PipelineError> {
        let entry = {
            let mut boxes = self.boxes.write().unwrap_or_else(poison_inner);
            boxes
                .remove(&box_id)
                .ok_or(PipelineError::BoxNotFound(box_id))?
        };
        entry.cancel.cancel();
        if let Some(handle) = entry.task {
            let _ = handle.await;
        }
        self.dedup.remove_box(box_id);
        self.status
            .write()
            .unwrap_or_else(poison_inner)
            .remove(&box_id);
        info!(box_id, "translation box removed");
        Ok(())
    }

    /// Updates the capture region of a box. If the box is running, its task
    /// is stopped and restarted with the new region.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::BoxNotFound`] when `box_id` does not exist,
    /// or [`PipelineError::InvalidConfig`] when the region has a zero
    /// dimension.
    #[instrument(skip_all, fields(box_id))]
    pub async fn update_box(&self, box_id: u32, region: ScreenRegion) -> Result<(), PipelineError> {
        region
            .validate()
            .map_err(|e| PipelineError::InvalidConfig(format!("invalid box region: {e}")))?;

        // Stop the old task (if running) and update the stored region.
        let old_task = {
            let mut boxes = self.boxes.write().unwrap_or_else(poison_inner);
            let entry = boxes
                .get_mut(&box_id)
                .ok_or(PipelineError::BoxNotFound(box_id))?;
            entry.cancel.cancel();
            entry.info.region = region.clone();
            entry.task.take()
        };
        if let Some(handle) = old_task {
            let _ = handle.await;
        }

        // Restart with the new region if the pipeline is running.
        if self.running.load(Ordering::SeqCst) {
            let box_info = {
                let boxes = self.boxes.read().unwrap_or_else(poison_inner);
                boxes.get(&box_id).map(|e| e.info.clone())
            };
            if let Some(box_info) = box_info {
                self.dedup.clear_box(box_id);
                let cancel = CancellationToken::new();
                let handle = self.spawn_box_task(box_info, cancel.clone());
                {
                    let mut boxes = self.boxes.write().unwrap_or_else(poison_inner);
                    if let Some(entry) = boxes.get_mut(&box_id) {
                        entry.cancel = cancel;
                        entry.task = Some(handle);
                    }
                }
                self.set_status(box_id, BoxStatus::Running);
            }
        }
        info!(box_id, "translation box region updated");
        Ok(())
    }

    /// Starts tasks for all registered boxes that are not yet running.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::AlreadyRunning`] when the pipeline is
    /// already running.
    #[instrument(skip_all)]
    pub async fn start_all(&self) -> Result<(), PipelineError> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(PipelineError::AlreadyRunning);
        }

        // Collect boxes that need starting.
        let to_start: Vec<TranslationBox> = {
            let boxes = self.boxes.read().unwrap_or_else(poison_inner);
            boxes
                .iter()
                .filter(|(_, e)| e.task.is_none())
                .map(|(_, e)| e.info.clone())
                .collect()
        };

        for box_info in to_start {
            let cancel = CancellationToken::new();
            let handle = self.spawn_box_task(box_info.clone(), cancel.clone());
            {
                let mut boxes = self.boxes.write().unwrap_or_else(poison_inner);
                if let Some(entry) = boxes.get_mut(&box_info.id) {
                    entry.cancel = cancel;
                    entry.task = Some(handle);
                }
            }
            self.set_status(box_info.id, BoxStatus::Running);
        }

        info!(box_count = self.box_count(), "multi-box pipeline started");
        Ok(())
    }

    /// Stops all box tasks. Boxes remain registered and can be restarted
    /// with [`start_all`](Self::start_all).
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::NotRunning`] when the pipeline is not
    /// running.
    #[instrument(skip_all)]
    pub async fn stop_all(&self) -> Result<(), PipelineError> {
        if !self.running.swap(false, Ordering::SeqCst) {
            return Err(PipelineError::NotRunning);
        }

        // Cancel all tokens and collect task handles.
        let handles: Vec<(u32, JoinHandle<()>)> = {
            let mut boxes = self.boxes.write().unwrap_or_else(poison_inner);
            boxes
                .iter_mut()
                .map(|(id, entry)| {
                    entry.cancel.cancel();
                    (*id, entry.task.take())
                })
                .filter_map(|(id, task)| task.map(|t| (id, t)))
                .collect()
        };

        for (_, handle) in handles {
            let _ = handle.await;
        }
        self.dedup.clear_all();
        info!("multi-box pipeline stopped");
        Ok(())
    }

    /// Stops a single box task, leaving it registered.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::BoxNotFound`] when `box_id` does not exist,
    /// or [`PipelineError::NotRunning`] when the box has no running task.
    #[instrument(skip_all, fields(box_id))]
    pub async fn stop_box(&self, box_id: u32) -> Result<(), PipelineError> {
        let task_handle = {
            let mut boxes = self.boxes.write().unwrap_or_else(poison_inner);
            let entry = boxes
                .get_mut(&box_id)
                .ok_or(PipelineError::BoxNotFound(box_id))?;
            entry.cancel.cancel();
            entry.task.take()
        };
        let task_handle = task_handle.ok_or(PipelineError::NotRunning)?;
        let _ = task_handle.await;
        info!(box_id, "translation box stopped");
        Ok(())
    }

    /// Subscribes to the multi-box result stream.
    ///
    /// Each call returns a fresh `mpsc::Receiver` backed by a forwarder
    /// task that relays from the internal broadcast channel. The forwarder
    /// applies per-subscriber backpressure: when the mpsc channel is full,
    /// it awaits before receiving the next broadcast message. If the
    /// subscriber is very slow, the broadcast may lag and drop older
    /// results (logged as a warning).
    #[must_use]
    pub fn subscribe_results(&self) -> mpsc::Receiver<BoxedTranslationResult> {
        let capacity = self.config.results_capacity();
        let (tx, rx) = mpsc::channel(capacity);
        let mut bcast_rx = self.results_tx.subscribe();
        tokio::spawn(async move {
            loop {
                match bcast_rx.recv().await {
                    Ok(result) => {
                        if tx.send(result).await.is_err() {
                            break; // subscriber dropped
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(lagged = n, "result subscriber lagged; dropped results");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        rx
    }

    // -- internal helpers --

    /// Sets the status of a box.
    fn set_status(&self, box_id: u32, status: BoxStatus) {
        self.status
            .write()
            .unwrap_or_else(poison_inner)
            .insert(box_id, status);
    }

    /// Spawns the per-box task (capture loop + OCR worker + translation
    /// worker) and returns its `JoinHandle`.
    fn spawn_box_task(
        &self,
        box_info: TranslationBox,
        cancel: CancellationToken,
    ) -> JoinHandle<()> {
        let ctx = BoxWorkerCtx {
            deps: self.deps.clone(),
            results_tx: self.results_tx.clone(),
            dedup: self.dedup.clone(),
            status: self.status.clone(),
            cancel: cancel.clone(),
            ocr_options: self.config.ocr_options.clone(),
            source: self.config.translation_request.source,
            target: self.config.translation_request.target,
            interval: clamp_interval_ms(self.config.capture_interval_ms),
            threshold: clamp_threshold(self.config.difference_threshold),
            box_id: box_info.id,
            color: box_info.color.clone(),
            monitor_id: box_info.region.monitor_id.clone(),
        };
        tokio::spawn(run_box_task(ctx, box_info, cancel))
    }
}

impl Drop for MultiBoxPipeline {
    fn drop(&mut self) {
        let mut boxes = self.boxes.write().unwrap_or_else(poison_inner);
        for entry in boxes.values_mut() {
            entry.cancel.cancel();
        }
        for entry in boxes.values_mut() {
            if let Some(handle) = entry.task.take() {
                handle.abort();
            }
        }
    }
}

// ==========================================================================
// Per-box task implementation
// ==========================================================================

/// Entry point for one box's pipeline. Spawns the OCR and translation
/// workers, runs the capture loop, then waits for workers on exit.
#[instrument(skip_all, fields(box_id = ctx.box_id))]
async fn run_box_task(ctx: BoxWorkerCtx, box_info: TranslationBox, cancel: CancellationToken) {
    let box_id = ctx.box_id;
    let region = box_info.region.clone();
    let color = ctx.color.clone();
    info!(box_id, color = %color, "box task started");

    set_box_status(&ctx.status, box_id, BoxStatus::Running);

    let (frames_tx, frames_rx) = mpsc::channel(PER_BOX_CHANNEL_CAPACITY);
    let (jobs_tx, jobs_rx) = mpsc::channel(PER_BOX_CHANNEL_CAPACITY);

    let ocr_handle = tokio::spawn(box_ocr_worker(ctx.clone(), frames_rx, jobs_tx));
    let trans_handle = tokio::spawn(box_translation_worker(ctx.clone(), jobs_rx));

    let capture_result = box_capture_loop(&ctx, frames_tx, &region).await;

    // Signal workers and wait for them.
    cancel.cancel();
    let _ = ocr_handle.await;
    let _ = trans_handle.await;

    match capture_result {
        Ok(()) => {
            set_box_status(&ctx.status, box_id, BoxStatus::Stopped);
            info!(box_id, "box task stopped");
        }
        Err(error) => {
            warn!(box_id, error = %error, "box task ended with error");
            set_box_status(&ctx.status, box_id, BoxStatus::Error(error.to_string()));
        }
    }
}

/// Captures frames from a session, applies per-box frame-difference
/// detection, and forwards changed frames to the OCR worker.
#[instrument(skip_all, fields(box_id = ctx.box_id))]
async fn box_capture_loop(
    ctx: &BoxWorkerCtx,
    frames_tx: mpsc::Sender<CapturedImage>,
    region: &ScreenRegion,
) -> Result<(), PipelineError> {
    let mut session = ctx.deps.capture.start_session(region).await?;
    let mut differ = FrameDiffer::new(ctx.threshold);
    loop {
        let frame = tokio::select! {
            biased;
            () = ctx.cancel.cancelled() => {
                let _ = session.stop().await;
                return Ok(());
            }
            frame = session.next_frame() => frame,
        };

        match frame {
            Ok(Some(image)) => {
                if let Err(error) = image.validate() {
                    warn!(error = %error, "captured frame failed validation; skipping");
                } else if differ.is_changed(&image) {
                    match frames_tx.try_send(image) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            debug!("frame queue full; dropping frame (backpressure)");
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            warn!("frame queue closed; stopping box");
                            let _ = session.stop().await;
                            return Err(PipelineError::ChannelClosed);
                        }
                    }
                } else {
                    debug!("frame unchanged; skipping OCR");
                }
            }
            Ok(None) => {
                debug!("capture session ended");
                let _ = session.stop().await;
                return Ok(());
            }
            Err(error) => {
                warn!(error = %error, "capture failed");
                let _ = session.stop().await;
                return Err(PipelineError::Capture(error));
            }
        }

        tokio::time::sleep(ctx.interval).await;
    }
}

/// Consumes frames and runs at most one OCR pass at a time for this box.
#[instrument(skip_all, fields(box_id = ctx.box_id))]
async fn box_ocr_worker(
    ctx: BoxWorkerCtx,
    mut frames_rx: mpsc::Receiver<CapturedImage>,
    jobs_tx: mpsc::Sender<BoxOcrJob>,
) {
    let mut slot: TaskSlot<()> = TaskSlot::new();
    loop {
        let frame = tokio::select! {
            biased;
            () = ctx.cancel.cancelled() => break,
            frame = frames_rx.recv() => match frame {
                Some(frame) => frame,
                None => break,
            },
        };
        slot.replace({
            let ctx = ctx.clone();
            let jobs_tx = jobs_tx.clone();
            move |cancel| async move {
                run_box_ocr_job(ctx, jobs_tx, frame, cancel).await;
            }
        })
        .await;
    }
    slot.cancel_and_join().await;
}

/// OCR stage for one frame: recognize, normalize, deduplicate, and forward
/// non-duplicate text to the translation stage.
#[instrument(skip_all, fields(box_id = ctx.box_id))]
async fn run_box_ocr_job(
    ctx: BoxWorkerCtx,
    jobs_tx: mpsc::Sender<BoxOcrJob>,
    frame: CapturedImage,
    cancel: CancellationToken,
) {
    let ocr_region = image_aligned_region(&ctx.monitor_id, &frame);
    let result = match ctx
        .deps
        .ocr
        .recognize(&frame, &ocr_region, &ctx.ocr_options, cancel.clone())
        .await
    {
        Ok(result) => result,
        Err(vtrans_core::OcrError::Cancelled) => {
            debug!("OCR cancelled by a newer frame or by stop");
            return;
        }
        Err(error) => {
            warn!(error = %error, "OCR failed");
            return;
        }
    };
    debug!(
        elapsed_ms = result.elapsed_ms,
        line_count = result.lines.len(),
        "OCR pass completed"
    );

    let normalized = normalize_result(result, ctx.source);
    info!(
        sample = %truncate_for_log(&normalized.merged_text),
        "OCR completed"
    );

    // Per-box fingerprint dedup. Empty text is forwarded (once, before the
    // dedup cache marks it seen) so the translation worker can publish a
    // cleared result with empty `original_text`; repeated unchanged frames
    // are still skipped by the dedup cache.
    if ctx.dedup.is_duplicate(ctx.box_id, &normalized.merged_text) {
        debug!("text unchanged; skipping translation");
        return;
    }

    let job = BoxOcrJob {
        text: normalized.merged_text,
        source: ctx.source,
        target: ctx.target,
    };
    match jobs_tx.try_send(job) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            debug!("translation queue full; dropping stale job");
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            debug!("translation worker is gone; dropping job");
        }
    }
}

/// Consumes OCR jobs and runs at most one translation at a time for this
/// box.
#[instrument(skip_all, fields(box_id = ctx.box_id))]
async fn box_translation_worker(ctx: BoxWorkerCtx, mut jobs_rx: mpsc::Receiver<BoxOcrJob>) {
    let mut slot: TaskSlot<()> = TaskSlot::new();
    loop {
        let job = tokio::select! {
            biased;
            () = ctx.cancel.cancelled() => break,
            job = jobs_rx.recv() => match job {
                Some(job) => job,
                None => break,
            },
        };
        slot.replace({
            let ctx = ctx.clone();
            move |cancel| async move {
                run_box_translation_job(ctx, job, cancel).await;
            }
        })
        .await;
    }
    slot.cancel_and_join().await;
}

/// Translation stage for one OCR job.
///
/// Successful translations are paired with the OCR text carried by the
/// job. Degraded cases still publish a result so the overlay is cleared
/// rather than left with stale content: empty OCR text skips the provider
/// call, and a failed translation publishes an empty translation -- both
/// with an empty `original_text`. Cancellation is not a failure and
/// publishes nothing.
#[instrument(skip_all, fields(box_id = ctx.box_id))]
async fn run_box_translation_job(ctx: BoxWorkerCtx, job: BoxOcrJob, cancel: CancellationToken) {
    if job.text.trim().is_empty() {
        debug!("empty OCR text; publishing cleared result");
        publish_empty_result(&ctx);
        return;
    }

    let result = tokio::select! {
        biased;
        () = ctx.cancel.cancelled() => {
            debug!("translation cancelled by stop");
            return;
        }
        () = cancel.cancelled() => {
            debug!("translation superseded by a newer job");
            return;
        }
        result = translate_text(&ctx.deps, &job.text, job.source, job.target, cancel.clone()) => {
            result
        }
    };

    match result {
        Ok(translation) => {
            info!(
                elapsed_ms = translation.elapsed_ms,
                provider = %translation.provider_id,
                "translation completed"
            );
            let boxed = BoxedTranslationResult::new(ctx.box_id, &ctx.color, translation)
                .with_original_text(job.text);
            let _ = ctx.results_tx.send(boxed);
        }
        Err(vtrans_core::TranslationError::Cancelled) => {
            debug!("translation cancelled by the provider");
        }
        Err(error) => {
            warn!(error = %error, "translation failed");
            // Publish a cleared result so the overlay does not keep showing
            // stale translated text; `original_text` stays empty.
            publish_empty_result(&ctx);
        }
    }
}

/// Publishes a result with empty translated and original text for a box.
///
/// Used for degraded outcomes (empty OCR text, translation failure) so the
/// overlay is cleared instead of retaining stale content.
fn publish_empty_result(ctx: &BoxWorkerCtx) {
    let empty = TranslationResult::new(String::new(), String::new(), 0);
    let boxed = BoxedTranslationResult::new(ctx.box_id, &ctx.color, empty);
    let _ = ctx.results_tx.send(boxed);
}

// ==========================================================================
// Helpers
// ==========================================================================

/// Updates the status map for a box.
fn set_box_status(
    status: &Arc<RwLock<HashMap<u32, BoxStatus>>>,
    box_id: u32,
    new_status: BoxStatus,
) {
    status
        .write()
        .unwrap_or_else(poison_inner)
        .insert(box_id, new_status);
}

/// Returns the current Unix timestamp in milliseconds.
fn now_millis() -> u64 {
    #[allow(clippy::cast_possible_truncation)]
    {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64)
    }
}

// ==========================================================================
// Unit tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use vtrans_core::types::{Language, OcrOptions, ScreenRegion, TranslationRequest};

    #[test]
    fn translation_box_serde_roundtrip() {
        let box_ = TranslationBox::new(5, ScreenRegion::new("m0", 10, 20, 300, 400), "#FF6B6B");
        let json = serde_json::to_string(&box_).unwrap();
        let back: TranslationBox = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, 5);
        assert_eq!(back.color, "#FF6B6B");
        assert_eq!(back.region.monitor_id, "m0");
        assert_eq!(back.region.width, 300);
    }

    #[test]
    fn boxed_translation_result_has_timestamp() {
        let result = vtrans_core::types::TranslationResult::new("hello", "mock", 10);
        let boxed = BoxedTranslationResult::new(0, "#FF0000", result);
        assert_eq!(boxed.box_id, 0);
        assert_eq!(boxed.color, "#FF0000");
        assert_eq!(boxed.original_text, "");
        assert!(boxed.timestamp > 0);
    }

    #[test]
    fn boxed_translation_result_serde_roundtrip() {
        let result = vtrans_core::types::TranslationResult::new("world", "mock", 5);
        let boxed =
            BoxedTranslationResult::new(3, "#00FF00", result).with_original_text("original text");
        let json = serde_json::to_string(&boxed).unwrap();
        assert!(json.contains("\"original_text\":\"original text\""));
        let back: BoxedTranslationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.box_id, 3);
        assert_eq!(back.color, "#00FF00");
        assert_eq!(back.result.translated_text, "world");
        assert_eq!(back.original_text, "original text");
    }

    #[test]
    fn box_status_serde_roundtrip() {
        assert_eq!(
            serde_json::to_string(&BoxStatus::Running).unwrap(),
            r#""Running""#
        );
        let json = serde_json::to_string(&BoxStatus::Error("boom".into())).unwrap();
        let back: BoxStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, BoxStatus::Error("boom".to_string()));
    }

    #[test]
    fn multi_box_config_defaults() {
        let config = MultiBoxConfig::default();
        assert_eq!(config.max_boxes, 8);
        assert_eq!(config.capture_interval_ms, 250);
        assert_eq!(
            config.translation_request.target,
            Language::ChineseSimplified
        );
    }

    #[test]
    fn multi_box_config_results_capacity() {
        let config = MultiBoxConfig::default();
        assert_eq!(config.results_capacity(), 16);
        let config = MultiBoxConfig::with_max_boxes(
            250,
            0.02,
            OcrOptions::new(Language::English),
            TranslationRequest::new("", Language::English, Language::ChineseSimplified),
            4,
        );
        assert_eq!(config.results_capacity(), 8);
    }
}
