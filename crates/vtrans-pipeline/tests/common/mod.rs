#![allow(dead_code)] // each integration-test binary compiles this module separately
//! Shared mock providers and test helpers for `vtrans-pipeline` integration
//! tests.
//!
//! The mocks are scripted through shared `Arc` state so tests can observe
//! call counts, cancellation, concurrency, and recorded inputs without
//! blocking the pipeline.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use vtrans_core::traits::{CaptureSession, CaptureSource, OcrProvider, TranslationProvider};
use vtrans_core::types::{
    CapturedImage, Language, OcrOptions, OcrResult, PixelFormat, ScreenRegion, TranslationRequest,
    TranslationResult,
};
use vtrans_core::{CaptureError, OcrError, TranslationError};

use vtrans_pipeline::PipelineEvent;

/// Recovers the inner value of a poisoned lock in test code.
pub fn poison_inner<T>(poisoned: std::sync::PoisonError<T>) -> T {
    poisoned.into_inner()
}

// ─────────────────────────────────────────────────────────────────────────
// Images
// ─────────────────────────────────────────────────────────────────────────

/// Builds a solid-color RGBA image.
#[must_use]
pub fn solid_image(width: u32, height: u32, byte: u8) -> CapturedImage {
    let len = usize::try_from(width * height * 4).expect("test image size");
    CapturedImage::new(width, height, PixelFormat::Rgba8, vec![byte; len])
        .expect("valid test image")
}

/// Builds a two-frame image whose pixels differ from `solid_image` at the
/// given offset.
#[must_use]
pub fn varied_image(width: u32, height: u32, base: u8, offset: u8) -> CapturedImage {
    let len = usize::try_from(width * height * 4).expect("test image size");
    let mut data = vec![base; len];
    data[0] = base.wrapping_add(offset);
    CapturedImage::new(width, height, PixelFormat::Rgba8, data).expect("valid test image")
}

/// Builds a single-line OCR result with the given text.
#[must_use]
pub fn ocr_result(text: &str) -> OcrResult {
    let polygon = [[0.0, 0.0], [100.0, 0.0], [100.0, 20.0], [0.0, 20.0]];
    OcrResult::from_lines(
        vec![vtrans_core::OcrLine::new(text, 0.9, polygon, 0)],
        Some(Language::English),
        5,
    )
}

/// Builds a translation result for the given provider id.
#[must_use]
pub fn translation_result(text: &str, provider_id: &str) -> TranslationResult {
    TranslationResult::new(text, provider_id, 7)
}

// ─────────────────────────────────────────────────────────────────────────
// Mock OCR provider
// ─────────────────────────────────────────────────────────────────────────

/// Scripted behavior of [`MockOcr`].
#[derive(Default)]
pub struct MockOcrBehavior {
    /// One outcome per `recognize` call. A `None` entry blocks until the
    /// call is cancelled; an empty queue blocks as well.
    pub outcomes: Mutex<VecDeque<Option<Result<OcrResult, OcrError>>>>,
    /// Delay applied to scripted outcomes.
    pub delay: Duration,
    /// Total `recognize` calls.
    pub calls: AtomicUsize,
    /// Calls that returned [`OcrError::Cancelled`].
    pub cancelled: AtomicUsize,
    /// Live concurrency (peaks tracked in `max_concurrent`).
    pub concurrent: AtomicUsize,
    /// Highest observed concurrent `recognize` calls.
    pub max_concurrent: AtomicUsize,
    /// Most recent region passed to `recognize`.
    pub last_region: Mutex<Option<ScreenRegion>>,
    /// Most recent options passed to `recognize`.
    pub last_options: Mutex<Option<OcrOptions>>,
}

impl MockOcrBehavior {
    /// Pushes a fixed outcome onto the script queue.
    pub fn push(&self, outcome: Result<OcrResult, OcrError>) {
        self.outcomes
            .lock()
            .unwrap_or_else(poison_inner)
            .push_back(Some(outcome));
    }

    /// Pushes a script entry that blocks until the call is cancelled.
    pub fn push_block(&self) {
        self.outcomes
            .lock()
            .unwrap_or_else(poison_inner)
            .push_back(None);
    }
}

/// An [`OcrProvider`] driven by a scripted [`MockOcrBehavior`].
pub struct MockOcr {
    behavior: Arc<MockOcrBehavior>,
}

impl MockOcr {
    #[must_use]
    pub fn new(behavior: Arc<MockOcrBehavior>) -> Self {
        Self { behavior }
    }

    /// Access to the shared behavior for assertions.
    #[must_use]
    pub fn behavior(&self) -> &Arc<MockOcrBehavior> {
        &self.behavior
    }
}

#[async_trait]
impl OcrProvider for MockOcr {
    fn id(&self) -> &'static str {
        "mock-ocr"
    }

    async fn recognize(
        &self,
        _image: &CapturedImage,
        region: &ScreenRegion,
        options: &OcrOptions,
        cancel: CancellationToken,
    ) -> Result<OcrResult, OcrError> {
        let behavior = &self.behavior;
        behavior.calls.fetch_add(1, Ordering::SeqCst);
        *behavior.last_region.lock().unwrap_or_else(poison_inner) = Some(region.clone());
        *behavior.last_options.lock().unwrap_or_else(poison_inner) = Some(options.clone());
        let current = behavior.concurrent.fetch_add(1, Ordering::SeqCst) + 1;
        behavior.max_concurrent.fetch_max(current, Ordering::SeqCst);

        let outcome = behavior
            .outcomes
            .lock()
            .unwrap_or_else(poison_inner)
            .pop_front()
            .flatten();
        let delay = behavior.delay;
        // A `None` script entry (or an empty script) blocks until the call
        // is cancelled; a scripted outcome is produced after `delay`.
        let result = if let Some(outcome) = outcome {
            tokio::select! {
                () = cancel.cancelled() => Err(OcrError::Cancelled),
                () = tokio::time::sleep(delay) => outcome,
            }
        } else {
            cancel.cancelled().await;
            Err(OcrError::Cancelled)
        };
        behavior.concurrent.fetch_sub(1, Ordering::SeqCst);
        if matches!(result, Err(OcrError::Cancelled)) {
            behavior.cancelled.fetch_add(1, Ordering::SeqCst);
        }
        result
    }

    fn supported_languages(&self) -> &[Language] {
        &[
            Language::English,
            Language::ChineseSimplified,
            Language::Japanese,
        ]
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Mock translation provider
// ─────────────────────────────────────────────────────────────────────────

/// Scripted behavior of [`MockTranslation`].
#[derive(Default)]
pub struct MockTranslationBehavior {
    /// One outcome per `translate` call. A `None` entry blocks until the
    /// call is cancelled; an empty queue blocks as well.
    pub outcomes: Mutex<VecDeque<Option<Result<TranslationResult, TranslationError>>>>,
    pub delay: Duration,
    pub calls: AtomicUsize,
    pub cancelled: AtomicUsize,
    pub concurrent: AtomicUsize,
    pub max_concurrent: AtomicUsize,
    /// Most recent request text passed to `translate`.
    pub last_request_text: Mutex<Option<String>>,
    /// All request texts passed to `translate`, in order.
    pub request_texts: Mutex<Vec<String>>,
}

impl MockTranslationBehavior {
    /// Pushes a fixed outcome onto the script queue.
    pub fn push(&self, outcome: Result<TranslationResult, TranslationError>) {
        self.outcomes
            .lock()
            .unwrap_or_else(poison_inner)
            .push_back(Some(outcome));
    }

    /// Pushes a script entry that blocks until the call is cancelled.
    pub fn push_block(&self) {
        self.outcomes
            .lock()
            .unwrap_or_else(poison_inner)
            .push_back(None);
    }
}

/// A [`TranslationProvider`] driven by a scripted [`MockTranslationBehavior`].
pub struct MockTranslation {
    behavior: Arc<MockTranslationBehavior>,
}

impl MockTranslation {
    #[must_use]
    pub fn new(behavior: Arc<MockTranslationBehavior>) -> Self {
        Self { behavior }
    }

    /// Access to the shared behavior for assertions.
    #[must_use]
    pub fn behavior(&self) -> &Arc<MockTranslationBehavior> {
        &self.behavior
    }
}

#[async_trait]
impl TranslationProvider for MockTranslation {
    fn id(&self) -> &'static str {
        "mock-translation"
    }

    async fn translate(
        &self,
        request: &TranslationRequest,
        cancel: CancellationToken,
    ) -> Result<TranslationResult, TranslationError> {
        let behavior = &self.behavior;
        behavior.calls.fetch_add(1, Ordering::SeqCst);
        behavior
            .last_request_text
            .lock()
            .unwrap_or_else(poison_inner)
            .clone_from(&Some(request.text.clone()));
        behavior
            .request_texts
            .lock()
            .unwrap_or_else(poison_inner)
            .push(request.text.clone());
        let current = behavior.concurrent.fetch_add(1, Ordering::SeqCst) + 1;
        behavior.max_concurrent.fetch_max(current, Ordering::SeqCst);

        let outcome = behavior
            .outcomes
            .lock()
            .unwrap_or_else(poison_inner)
            .pop_front()
            .flatten();
        let delay = behavior.delay;
        let result = if let Some(outcome) = outcome {
            tokio::select! {
                () = cancel.cancelled() => Err(TranslationError::Cancelled),
                () = tokio::time::sleep(delay) => outcome,
            }
        } else {
            cancel.cancelled().await;
            Err(TranslationError::Cancelled)
        };
        behavior.concurrent.fetch_sub(1, Ordering::SeqCst);
        if matches!(result, Err(TranslationError::Cancelled)) {
            behavior.cancelled.fetch_add(1, Ordering::SeqCst);
        }
        result
    }

    fn supported_pairs(&self) -> &[(Language, Language)] {
        &[
            (Language::English, Language::Japanese),
            (Language::Japanese, Language::ChineseSimplified),
        ]
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Mock capture source
// ─────────────────────────────────────────────────────────────────────────

/// Scripted behavior of [`MockCaptureSource`].
#[derive(Default)]
pub struct MockCaptureBehavior {
    /// Outcome of the next `capture_once` call.
    pub capture_once_outcome: Mutex<Option<Result<CapturedImage, CaptureError>>>,
    /// Regions passed to `start_session`, in order.
    pub session_regions: Mutex<Vec<ScreenRegion>>,
    pub sessions_started: AtomicUsize,
    pub sessions_stopped: Arc<AtomicUsize>,
    /// Sender used to feed frames into the active session.
    pub feeder: Mutex<Option<mpsc::Sender<CapturedImage>>>,
}

/// A [`CaptureSource`] whose sessions are fed by the test through
/// [`MockCaptureBehavior::feeder`].
pub struct MockCaptureSource {
    behavior: Arc<MockCaptureBehavior>,
}

impl MockCaptureSource {
    #[must_use]
    pub fn new(behavior: Arc<MockCaptureBehavior>) -> Self {
        Self { behavior }
    }

    /// Access to the shared behavior for assertions.
    #[must_use]
    pub fn behavior(&self) -> &Arc<MockCaptureBehavior> {
        &self.behavior
    }
}

/// A capture session backed by a channel the test feeds frames into.
struct MockCaptureSession {
    frames_rx: mpsc::Receiver<CapturedImage>,
    stopped: Arc<AtomicBool>,
    stopped_counter: Arc<AtomicUsize>,
}

#[async_trait]
impl CaptureSession for MockCaptureSession {
    async fn next_frame(&mut self) -> Result<Option<CapturedImage>, CaptureError> {
        match self.frames_rx.recv().await {
            Some(frame) => Ok(Some(frame)),
            None => Ok(None),
        }
    }

    async fn stop(&mut self) -> Result<(), CaptureError> {
        self.stopped.store(true, Ordering::SeqCst);
        self.stopped_counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait]
impl CaptureSource for MockCaptureSource {
    async fn capture_once(&self, _region: &ScreenRegion) -> Result<CapturedImage, CaptureError> {
        let outcome = self
            .behavior
            .capture_once_outcome
            .lock()
            .unwrap_or_else(poison_inner)
            .take();
        outcome.unwrap_or_else(|| {
            Err(CaptureError::InitFailed(
                "no scripted capture outcome".into(),
            ))
        })
    }

    async fn start_session(
        &self,
        region: &ScreenRegion,
    ) -> Result<Box<dyn CaptureSession>, CaptureError> {
        let behavior = &self.behavior;
        behavior.sessions_started.fetch_add(1, Ordering::SeqCst);
        behavior
            .session_regions
            .lock()
            .unwrap_or_else(poison_inner)
            .push(region.clone());
        let (tx, rx) = mpsc::channel(8);
        *behavior.feeder.lock().unwrap_or_else(poison_inner) = Some(tx);
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_counter = behavior.sessions_stopped.clone();
        Ok(Box::new(MockCaptureSession {
            frames_rx: rx,
            stopped,
            stopped_counter,
        }))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Event log
// ─────────────────────────────────────────────────────────────────────────

/// Collects [`PipelineEvent`]s from the pipeline's event channel into a
/// shared log so tests can poll mid-run and drain at the end.
pub struct EventLog {
    events: Arc<Mutex<Vec<PipelineEvent>>>,
    collector: Option<tokio::task::JoinHandle<()>>,
}

/// Spawns a task that forwards every event into an [`EventLog`].
#[must_use]
pub fn spawn_event_log(rx: mpsc::Receiver<PipelineEvent>) -> EventLog {
    let events = Arc::new(Mutex::new(Vec::new()));
    let collector = tokio::spawn({
        let events = events.clone();
        async move {
            let mut rx = rx;
            while let Some(event) = rx.recv().await {
                events.lock().unwrap_or_else(poison_inner).push(event);
            }
        }
    });
    EventLog {
        events,
        collector: Some(collector),
    }
}

impl EventLog {
    /// Total number of collected events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.lock().unwrap_or_else(poison_inner).len()
    }

    /// Returns `true` if the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of events matching `predicate`.
    #[must_use]
    pub fn count_matching(&self, predicate: impl Fn(&PipelineEvent) -> bool) -> usize {
        self.events
            .lock()
            .unwrap_or_else(poison_inner)
            .iter()
            .filter(|event| predicate(event))
            .count()
    }

    /// Waits (up to `timeout`) for an event matching `predicate`.
    pub async fn wait_until(&self, predicate: impl Fn(&PipelineEvent) -> bool, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while self.count_matching(&predicate) == 0 {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for pipeline event"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Waits for the event channel to close and returns every collected
    /// event in order.
    pub async fn finish(mut self) -> Vec<PipelineEvent> {
        if let Some(collector) = self.collector.take() {
            let _ = collector.await;
        }
        self.events
            .lock()
            .unwrap_or_else(poison_inner)
            .drain(..)
            .collect()
    }
}

/// Convenience: waits for the event channel to close and drains it without
/// mid-run polling.
pub async fn collect_events(rx: mpsc::Receiver<PipelineEvent>) -> Vec<PipelineEvent> {
    spawn_event_log(rx).finish().await
}

/// Polls `condition` until it returns `true` or `timeout` elapses.
pub async fn wait_until(condition: impl Fn() -> bool, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for condition");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
