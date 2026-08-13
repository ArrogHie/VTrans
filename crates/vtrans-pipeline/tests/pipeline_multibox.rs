//! Integration tests for multi-box real-time translation.
//!
//! These tests exercise [`MultiBoxPipeline`] with stateless mock providers
//! designed for concurrent multi-box scenarios. Unlike the single-box
//! mocks in `common/mod.rs` (which script outcomes through shared queues),
//! these mocks are stateless: they always return a fixed result after a
//! short delay, so multiple boxes can call them concurrently without
//! interference.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use vtrans_core::traits::{CaptureSession, CaptureSource, OcrProvider, TranslationProvider};
use vtrans_core::types::{
    CapturedImage, Language, OcrOptions, OcrResult, ScreenRegion, TranslationRequest,
    TranslationResult,
};
use vtrans_core::{CaptureError, OcrError, TranslationError};
use vtrans_pipeline::{
    BoxStatus, BoxedTranslationResult, MultiBoxConfig, MultiBoxPipeline, PipelineDeps,
    PipelineError, TranslationBox,
};

use common::{solid_image, varied_image};

// ==========================================================================
// Stateless mock providers (concurrent-safe)
// ==========================================================================

/// A capture source that generates frames from a pre-configured list.
/// Each `start_session` creates an independent session with its own frame
/// cursor, so multiple boxes can capture concurrently.
#[derive(Clone)]
struct GeneratingCaptureSource {
    frames: Arc<Vec<CapturedImage>>,
    delay: Duration,
    sessions_started: Arc<AtomicUsize>,
    sessions_stopped: Arc<AtomicUsize>,
    /// Regions passed to `start_session`, in order.
    session_regions: Arc<std::sync::Mutex<Vec<ScreenRegion>>>,
}

impl GeneratingCaptureSource {
    fn new(frames: Vec<CapturedImage>, delay: Duration) -> Self {
        Self {
            frames: Arc::new(frames),
            delay,
            sessions_started: Arc::new(AtomicUsize::new(0)),
            sessions_stopped: Arc::new(AtomicUsize::new(0)),
            session_regions: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn sessions_started(&self) -> usize {
        self.sessions_started.load(Ordering::SeqCst)
    }

    fn sessions_stopped(&self) -> usize {
        self.sessions_stopped.load(Ordering::SeqCst)
    }
}

struct GeneratingCaptureSession {
    frames: Arc<Vec<CapturedImage>>,
    index: usize,
    delay: Duration,
    stopped_counter: Arc<AtomicUsize>,
}

#[async_trait]
impl CaptureSession for GeneratingCaptureSession {
    async fn next_frame(&mut self) -> Result<Option<CapturedImage>, CaptureError> {
        tokio::time::sleep(self.delay).await;
        let frame = self.frames[self.index % self.frames.len()].clone();
        self.index += 1;
        Ok(Some(frame))
    }

    async fn stop(&mut self) -> Result<(), CaptureError> {
        self.stopped_counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait]
impl CaptureSource for GeneratingCaptureSource {
    async fn capture_once(&self, _region: &ScreenRegion) -> Result<CapturedImage, CaptureError> {
        Ok(self.frames[0].clone())
    }

    async fn start_session(
        &self,
        region: &ScreenRegion,
    ) -> Result<Box<dyn CaptureSession>, CaptureError> {
        self.sessions_started.fetch_add(1, Ordering::SeqCst);
        self.session_regions
            .lock()
            .expect("session_regions mutex poisoned")
            .push(region.clone());
        Ok(Box::new(GeneratingCaptureSession {
            frames: self.frames.clone(),
            index: 0,
            delay: self.delay,
            stopped_counter: self.sessions_stopped.clone(),
        }))
    }
}

/// A stateless OCR provider that always returns the same text after a
/// short delay. Safe for concurrent calls from multiple boxes.
struct EchoOcrProvider {
    text: String,
    delay: Duration,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl OcrProvider for EchoOcrProvider {
    fn id(&self) -> &'static str {
        "echo-ocr"
    }

    async fn recognize(
        &self,
        _image: &CapturedImage,
        _region: &ScreenRegion,
        _options: &OcrOptions,
        cancel: CancellationToken,
    ) -> Result<OcrResult, OcrError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::select! {
            () = cancel.cancelled() => Err(OcrError::Cancelled),
            () = tokio::time::sleep(self.delay) => Ok(echo_ocr_result(&self.text)),
        }
    }

    fn supported_languages(&self) -> &[Language] {
        &[Language::English, Language::Japanese]
    }
}

/// A stateless translation provider that prefixes the input text.
struct EchoTranslationProvider {
    prefix: String,
    delay: Duration,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl TranslationProvider for EchoTranslationProvider {
    fn id(&self) -> &'static str {
        "echo-translation"
    }

    async fn translate(
        &self,
        request: &TranslationRequest,
        cancel: CancellationToken,
    ) -> Result<TranslationResult, TranslationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let text = format!("{}{}", self.prefix, request.text);
        tokio::select! {
            () = cancel.cancelled() => Err(TranslationError::Cancelled),
            () = tokio::time::sleep(self.delay) => Ok(TranslationResult::new(text, "echo-translation", 1)),
        }
    }

    fn supported_pairs(&self) -> &[(Language, Language)] {
        &[
            (Language::English, Language::ChineseSimplified),
            (Language::Japanese, Language::ChineseSimplified),
        ]
    }
}

// ==========================================================================
// Helpers
// ==========================================================================

fn echo_ocr_result(text: &str) -> OcrResult {
    let polygon = [[0.0, 0.0], [100.0, 0.0], [100.0, 20.0], [0.0, 20.0]];
    OcrResult::from_lines(
        vec![vtrans_core::OcrLine::new(text, 0.9, polygon, 0)],
        Some(Language::English),
        1,
    )
}

fn multibox_config(max_boxes: u32) -> MultiBoxConfig {
    MultiBoxConfig::with_max_boxes(
        10,
        0.0,
        OcrOptions::new(Language::English),
        TranslationRequest::new("", Language::English, Language::ChineseSimplified),
        max_boxes,
    )
}

fn make_deps<C: CaptureSource + 'static>(
    capture: C,
    ocr: EchoOcrProvider,
    translation: EchoTranslationProvider,
) -> PipelineDeps {
    PipelineDeps::new(Box::new(capture), Box::new(ocr), Box::new(translation))
}

/// A 3-frame sequence: solid, varied, varied-again (all different so the
/// frame differ triggers OCR each time).
fn three_frames() -> Vec<CapturedImage> {
    vec![
        solid_image(8, 8, 1),
        varied_image(8, 8, 1, 9),
        varied_image(8, 8, 1, 18),
    ]
}

/// Collects results from the receiver with a timeout.
async fn collect_results(
    rx: &mut tokio::sync::mpsc::Receiver<BoxedTranslationResult>,
    timeout: Duration,
) -> Vec<BoxedTranslationResult> {
    let mut results = Vec::new();
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(result)) => results.push(result),
            Ok(None) | Err(_) => break,
        }
    }
    results
}

// ==========================================================================
// Tests
// ==========================================================================

#[tokio::test]
async fn two_boxes_run_independently() {
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "hello".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(
        multibox_config(8),
        make_deps(capture.clone(), ocr, translation),
    );

    pipeline
        .add_box(TranslationBox::new(
            0,
            ScreenRegion::new("m0", 0, 0, 8, 8),
            "#FF0000",
        ))
        .await
        .unwrap();
    pipeline
        .add_box(TranslationBox::new(
            1,
            ScreenRegion::new("m1", 0, 0, 8, 8),
            "#00FF00",
        ))
        .await
        .unwrap();

    let mut rx = pipeline.subscribe_results();
    pipeline.start_all().await.unwrap();

    let results = collect_results(&mut rx, Duration::from_secs(5)).await;
    pipeline.stop_all().await.unwrap();

    assert!(
        results.len() >= 2,
        "expected at least 2 results, got {}",
        results.len()
    );
    let box_ids: std::collections::HashSet<u32> = results.iter().map(|r| r.box_id).collect();
    assert!(box_ids.contains(&0), "box 0 should have produced results");
    assert!(box_ids.contains(&1), "box 1 should have produced results");
    assert_eq!(capture.sessions_started(), 2);
}

#[tokio::test]
async fn add_box_while_running_starts_task() {
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "added".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(
        multibox_config(8),
        make_deps(capture.clone(), ocr, translation),
    );

    let mut rx = pipeline.subscribe_results();
    pipeline.start_all().await.unwrap();

    // Add a box after the pipeline is already running.
    pipeline
        .add_box(TranslationBox::new(
            7,
            ScreenRegion::new("m0", 0, 0, 8, 8),
            "#FF6B6B",
        ))
        .await
        .unwrap();
    assert_eq!(pipeline.box_count(), 1);
    assert_eq!(
        pipeline.box_status(7),
        Some(BoxStatus::Running),
        "box added while running should be Running"
    );

    let results = collect_results(&mut rx, Duration::from_secs(5)).await;
    pipeline.stop_all().await.unwrap();

    assert!(
        !results.is_empty(),
        "the runtime-added box should produce results"
    );
    assert!(results.iter().all(|r| r.box_id == 7));
}

#[tokio::test]
async fn remove_box_stops_task_and_cleans_up() {
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "remove".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(
        multibox_config(8),
        make_deps(capture.clone(), ocr, translation),
    );

    pipeline
        .add_box(TranslationBox::new(
            0,
            ScreenRegion::new("m0", 0, 0, 8, 8),
            "#FF0000",
        ))
        .await
        .unwrap();
    pipeline
        .add_box(TranslationBox::new(
            1,
            ScreenRegion::new("m1", 0, 0, 8, 8),
            "#00FF00",
        ))
        .await
        .unwrap();

    let mut rx = pipeline.subscribe_results();
    pipeline.start_all().await.unwrap();
    // Let boxes produce a few results.
    let _ = collect_results(&mut rx, Duration::from_millis(200)).await;

    pipeline.remove_box(1).await.unwrap();
    assert_eq!(pipeline.box_count(), 1);
    assert!(
        pipeline.box_status(1).is_none(),
        "removed box should have no status"
    );
    assert_eq!(
        pipeline.box_status(0),
        Some(BoxStatus::Running),
        "remaining box should still be running"
    );

    pipeline.stop_all().await.unwrap();
    assert!(capture.sessions_stopped() >= 2);
}

#[tokio::test]
async fn update_box_restarts_with_new_region() {
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "update".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(
        multibox_config(8),
        make_deps(capture.clone(), ocr, translation),
    );

    pipeline
        .add_box(TranslationBox::new(
            0,
            ScreenRegion::new("m0", 0, 0, 8, 8),
            "#FF0000",
        ))
        .await
        .unwrap();

    let mut rx = pipeline.subscribe_results();
    pipeline.start_all().await.unwrap();

    // Wait for at least one session.
    common::wait_until(|| capture.sessions_started() >= 1, Duration::from_secs(5)).await;

    // Update the region -- this restarts the box task.
    pipeline
        .update_box(0, ScreenRegion::new("m1", 100, 200, 16, 16))
        .await
        .unwrap();

    // The task should restart with a new session.
    common::wait_until(|| capture.sessions_started() >= 2, Duration::from_secs(5)).await;

    let _ = collect_results(&mut rx, Duration::from_millis(200)).await;
    pipeline.stop_all().await.unwrap();

    let regions = capture.session_regions.lock().unwrap().clone();
    assert!(regions.len() >= 2);
    assert_eq!(regions[0].monitor_id, "m0");
    assert_eq!(regions[1].monitor_id, "m1");
}

#[tokio::test]
async fn stop_box_terminates_single_task() {
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "stop".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(
        multibox_config(8),
        make_deps(capture.clone(), ocr, translation),
    );

    pipeline
        .add_box(TranslationBox::new(
            0,
            ScreenRegion::new("m0", 0, 0, 8, 8),
            "#FF0000",
        ))
        .await
        .unwrap();
    pipeline
        .add_box(TranslationBox::new(
            1,
            ScreenRegion::new("m1", 0, 0, 8, 8),
            "#00FF00",
        ))
        .await
        .unwrap();

    let mut rx = pipeline.subscribe_results();
    pipeline.start_all().await.unwrap();
    let _ = collect_results(&mut rx, Duration::from_millis(100)).await;

    pipeline.stop_box(0).await.unwrap();
    assert_eq!(
        pipeline.box_status(0),
        Some(BoxStatus::Stopped),
        "stopped box should be Stopped"
    );
    assert_eq!(
        pipeline.box_status(1),
        Some(BoxStatus::Running),
        "other box should still be Running"
    );
    assert_eq!(pipeline.box_count(), 2, "stopped box remains registered");

    pipeline.stop_all().await.unwrap();
}

#[tokio::test]
async fn error_in_one_box_does_not_affect_others() {
    // Box 0 will fail to start its session; box 1 should still work.
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "ok".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };

    // Use a capture source that fails for box 0's region (monitor "fail")
    // but succeeds for box 1's region.
    let failing_capture = FailingCaptureSource {
        inner: capture.clone(),
        fail_monitor_id: "fail".to_string(),
    };

    let pipeline = MultiBoxPipeline::new(
        multibox_config(8),
        make_deps(failing_capture, ocr, translation),
    );

    pipeline
        .add_box(TranslationBox::new(
            0,
            ScreenRegion::new("fail", 0, 0, 8, 8),
            "#FF0000",
        ))
        .await
        .unwrap();
    pipeline
        .add_box(TranslationBox::new(
            1,
            ScreenRegion::new("m1", 0, 0, 8, 8),
            "#00FF00",
        ))
        .await
        .unwrap();

    let mut rx = pipeline.subscribe_results();
    pipeline.start_all().await.unwrap();

    let results = collect_results(&mut rx, Duration::from_secs(5)).await;
    pipeline.stop_all().await.unwrap();

    // Box 1 should have produced results despite box 0 failing.
    assert!(!results.is_empty(), "box 1 should produce results");
    assert!(
        results.iter().all(|r| r.box_id == 1),
        "only box 1 should have results, not box 0"
    );
    // Box 0 should be in Error status.
    let status0 = pipeline.box_status(0).expect("box 0 should have a status");
    match status0 {
        BoxStatus::Error(msg) => {
            assert!(
                msg.contains("mock fail_start"),
                "error message should mention the mock failure: {msg}"
            );
        }
        other => panic!("box 0 should be in Error status, got {other:?}"),
    }
}

/// A capture source that fails `start_session` for a specific monitor ID.
struct FailingCaptureSource {
    inner: GeneratingCaptureSource,
    fail_monitor_id: String,
}

#[async_trait]
impl CaptureSource for FailingCaptureSource {
    async fn capture_once(&self, _region: &ScreenRegion) -> Result<CapturedImage, CaptureError> {
        self.inner.capture_once(_region).await
    }

    async fn start_session(
        &self,
        region: &ScreenRegion,
    ) -> Result<Box<dyn CaptureSession>, CaptureError> {
        if region.monitor_id == self.fail_monitor_id {
            return Err(CaptureError::InitFailed("mock fail_start".into()));
        }
        self.inner.start_session(region).await
    }
}

#[tokio::test]
async fn dedup_isolation_between_boxes() {
    // Both boxes receive the same OCR text ("same"). Each box should
    // translate the first occurrence and skip the second (per-box dedup).
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "same".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation_calls = translation.calls.clone();

    let pipeline = MultiBoxPipeline::new(multibox_config(8), make_deps(capture, ocr, translation));

    pipeline
        .add_box(TranslationBox::new(
            0,
            ScreenRegion::new("m0", 0, 0, 8, 8),
            "#FF0000",
        ))
        .await
        .unwrap();
    pipeline
        .add_box(TranslationBox::new(
            1,
            ScreenRegion::new("m1", 0, 0, 8, 8),
            "#00FF00",
        ))
        .await
        .unwrap();

    let mut rx = pipeline.subscribe_results();
    pipeline.start_all().await.unwrap();

    // Each box has 3 frames with different pixels, so OCR runs 3 times
    // per box (6 total). But the text is always "same", so each box
    // translates only once (the first occurrence). Total: 2 translations.
    let results = collect_results(&mut rx, Duration::from_secs(5)).await;
    pipeline.stop_all().await.unwrap();

    assert_eq!(
        results.len(),
        2,
        "each box should translate once; got {} results",
        results.len()
    );
    let box_ids: std::collections::HashSet<u32> = results.iter().map(|r| r.box_id).collect();
    assert_eq!(box_ids.len(), 2, "both boxes should be represented");
    assert_eq!(
        translation_calls.load(Ordering::SeqCst),
        2,
        "translation should be called exactly twice (once per box)"
    );
}

#[tokio::test]
async fn results_are_delivered_through_bounded_channel() {
    // With a small max_boxes, the channel capacity is small. Results
    // should still be delivered (the forwarder applies backpressure).
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "delivery".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(
        // max_boxes=2 => channel capacity = 4
        multibox_config(2),
        make_deps(capture, ocr, translation),
    );

    pipeline
        .add_box(TranslationBox::new(
            0,
            ScreenRegion::new("m0", 0, 0, 8, 8),
            "#FF0000",
        ))
        .await
        .unwrap();
    pipeline
        .add_box(TranslationBox::new(
            1,
            ScreenRegion::new("m1", 0, 0, 8, 8),
            "#00FF00",
        ))
        .await
        .unwrap();

    let mut rx = pipeline.subscribe_results();
    pipeline.start_all().await.unwrap();

    let results = collect_results(&mut rx, Duration::from_secs(5)).await;
    pipeline.stop_all().await.unwrap();

    // Each box translates once (dedup), so at least 2 results.
    assert!(
        results.len() >= 2,
        "results should not be lost; got {}",
        results.len()
    );
}

#[tokio::test]
async fn eight_boxes_run_concurrently_without_panic_or_deadlock() {
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "eight".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(
        multibox_config(8),
        make_deps(capture.clone(), ocr, translation),
    );

    // Add 8 boxes.
    for i in 0..8u32 {
        pipeline
            .add_box(TranslationBox::new(
                i,
                ScreenRegion::new(format!("m{i}"), 0, 0, 8, 8),
                "#FF6B6B",
            ))
            .await
            .unwrap();
    }
    assert_eq!(pipeline.box_count(), 8);

    let mut rx = pipeline.subscribe_results();
    pipeline.start_all().await.unwrap();

    let results = collect_results(&mut rx, Duration::from_secs(10)).await;
    pipeline.stop_all().await.unwrap();

    // Each box should produce at least one result (dedup limits to 1
    // per box since OCR always returns "eight").
    assert!(
        results.len() >= 8,
        "expected at least 8 results (one per box); got {}",
        results.len()
    );
    let box_ids: std::collections::HashSet<u32> = results.iter().map(|r| r.box_id).collect();
    for i in 0..8u32 {
        assert!(
            box_ids.contains(&i),
            "box {i} should have produced a result"
        );
    }
    assert_eq!(capture.sessions_started(), 8);
}

#[tokio::test]
async fn add_duplicate_box_id_returns_error() {
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "dup".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(multibox_config(8), make_deps(capture, ocr, translation));

    pipeline
        .add_box(TranslationBox::new(
            0,
            ScreenRegion::new("m0", 0, 0, 8, 8),
            "#FF0000",
        ))
        .await
        .unwrap();

    let result = pipeline
        .add_box(TranslationBox::new(
            0,
            ScreenRegion::new("m1", 0, 0, 8, 8),
            "#00FF00",
        ))
        .await;
    assert!(matches!(result, Err(PipelineError::DuplicateBoxId(0))));
    assert_eq!(pipeline.box_count(), 1);
}

#[tokio::test]
async fn add_box_exceeding_limit_returns_error() {
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "limit".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(multibox_config(2), make_deps(capture, ocr, translation));

    pipeline
        .add_box(TranslationBox::new(
            0,
            ScreenRegion::new("m0", 0, 0, 8, 8),
            "#FF0000",
        ))
        .await
        .unwrap();
    pipeline
        .add_box(TranslationBox::new(
            1,
            ScreenRegion::new("m1", 0, 0, 8, 8),
            "#00FF00",
        ))
        .await
        .unwrap();

    let result = pipeline
        .add_box(TranslationBox::new(
            2,
            ScreenRegion::new("m2", 0, 0, 8, 8),
            "#0000FF",
        ))
        .await;
    assert!(matches!(result, Err(PipelineError::BoxLimitExceeded(2))));
}

#[tokio::test]
async fn remove_nonexistent_box_returns_error() {
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "none".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(multibox_config(8), make_deps(capture, ocr, translation));

    let result = pipeline.remove_box(99).await;
    assert!(matches!(result, Err(PipelineError::BoxNotFound(99))));
}

#[tokio::test]
async fn stop_all_when_not_running_returns_error() {
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "idle".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(multibox_config(8), make_deps(capture, ocr, translation));

    let result = pipeline.stop_all().await;
    assert!(matches!(result, Err(PipelineError::NotRunning)));
}

#[tokio::test]
async fn start_all_when_already_running_returns_error() {
    let capture = GeneratingCaptureSource::new(three_frames(), Duration::from_millis(1));
    let ocr = EchoOcrProvider {
        text: "running".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let translation = EchoTranslationProvider {
        prefix: "zh:".into(),
        delay: Duration::from_millis(1),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let pipeline = MultiBoxPipeline::new(multibox_config(8), make_deps(capture, ocr, translation));

    pipeline
        .add_box(TranslationBox::new(
            0,
            ScreenRegion::new("m0", 0, 0, 8, 8),
            "#FF0000",
        ))
        .await
        .unwrap();
    pipeline.start_all().await.unwrap();

    let result = pipeline.start_all().await;
    assert!(matches!(result, Err(PipelineError::AlreadyRunning)));

    pipeline.stop_all().await.unwrap();
}
