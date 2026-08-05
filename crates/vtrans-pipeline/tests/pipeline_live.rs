//! Integration tests for live-region pipeline mode.
//!
//! Drives the capture session, frame-difference detection, OCR worker
//! supersession, text-fingerprint deduplication, and translation worker
//! through the public [`Pipeline`] API with scripted mock providers.

mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use vtrans_core::types::{Language, OcrOptions, PipelineStatus, ScreenRegion, TranslationRequest};
use vtrans_pipeline::{Pipeline, PipelineConfig, PipelineDeps, PipelineError, PipelineEvent};

use common::*;

fn live_config(region: ScreenRegion) -> PipelineConfig {
    PipelineConfig::live(
        region,
        16,
        0.0,
        OcrOptions::new(Language::English),
        TranslationRequest::new("", Language::English, Language::ChineseSimplified),
    )
}

fn deps(capture: MockCaptureSource, ocr: MockOcr, translation: MockTranslation) -> PipelineDeps {
    PipelineDeps::new(Box::new(capture), Box::new(ocr), Box::new(translation))
}

/// Scripts capture/OCR/translation and returns the pipeline plus the shared
/// behaviors. Outcomes are consumed in order; an empty script makes calls
/// block until cancelled.
#[allow(clippy::type_complexity)]
fn live_pipeline(
    ocr_outcomes: Vec<Result<vtrans_core::OcrResult, vtrans_core::OcrError>>,
    translation_outcomes: Vec<
        Result<vtrans_core::TranslationResult, vtrans_core::TranslationError>,
    >,
    ocr_delay: Duration,
    translation_delay: Duration,
) -> (
    Arc<Pipeline>,
    Arc<MockCaptureBehavior>,
    Arc<MockOcrBehavior>,
    Arc<MockTranslationBehavior>,
) {
    let region = ScreenRegion::new("m0", 0, 0, 8, 8);
    let capture = Arc::new(MockCaptureBehavior::default());
    let ocr = Arc::new(MockOcrBehavior {
        delay: ocr_delay,
        ..MockOcrBehavior::default()
    });
    for outcome in ocr_outcomes {
        ocr.push(outcome);
    }
    let translation = Arc::new(MockTranslationBehavior {
        delay: translation_delay,
        ..MockTranslationBehavior::default()
    });
    for outcome in translation_outcomes {
        translation.push(outcome);
    }
    let pipeline = Arc::new(Pipeline::new(
        live_config(region),
        deps(
            MockCaptureSource::new(capture.clone()),
            MockOcr::new(ocr.clone()),
            MockTranslation::new(translation.clone()),
        ),
    ));
    (pipeline, capture, ocr, translation)
}

/// Starts a live run and returns its handle plus an event log.
fn start_live(
    pipeline: &Arc<Pipeline>,
) -> (tokio::task::JoinHandle<Result<(), PipelineError>>, EventLog) {
    let (tx, rx) = mpsc::channel(64);
    let log = spawn_event_log(rx);
    let handle = tokio::spawn({
        let pipeline = pipeline.clone();
        async move { pipeline.run(tx).await }
    });
    (handle, log)
}

/// Waits for the capture loop to start a session, then feeds one frame.
async fn feed_frame(behavior: &Arc<MockCaptureBehavior>, frame: vtrans_core::CapturedImage) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let tx = loop {
        let tx = behavior.feeder.lock().unwrap_or_else(poison_inner).clone();
        if let Some(tx) = tx {
            break tx;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for capture session"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    tx.send(frame).await.expect("session receives frames");
}

fn log_count(events: &[PipelineEvent], predicate: impl Fn(&PipelineEvent) -> bool) -> usize {
    events.iter().filter(|event| predicate(event)).count()
}

#[tokio::test]
async fn frame_sink_observes_only_frames_accepted_by_difference_detection() {
    let region = ScreenRegion::new("m0", 0, 0, 8, 8);
    let capture = Arc::new(MockCaptureBehavior::default());
    let ocr = Arc::new(MockOcrBehavior::default());
    ocr.push(Ok(ocr_result("Hello")));
    let translation = Arc::new(MockTranslationBehavior::default());
    translation.push(Ok(translation_result("你好", "mock-translation")));
    let sink = Arc::new(RecordingSink::default());

    let pipeline = Arc::new(Pipeline::with_frame_sink(
        live_config(region),
        deps(
            MockCaptureSource::new(capture.clone()),
            MockOcr::new(ocr.clone()),
            MockTranslation::new(translation.clone()),
        ),
        Some(sink.clone()),
    ));
    let (run_handle, log) = start_live(&pipeline);

    // First frame is always "changed": the sink must see it before OCR.
    feed_frame(&capture, solid_image(8, 8, 1)).await;
    log.wait_until(
        |e| matches!(e, PipelineEvent::TranslationCompleted(_)),
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(sink.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    // An unchanged frame is skipped by the differ and must not reach OCR or
    // the sink.
    feed_frame(&capture, solid_image(8, 8, 1)).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(sink.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    // A changed frame reaches the sink again.
    ocr.push(Ok(ocr_result("World")));
    translation.push(Ok(translation_result("世界", "mock-translation")));
    feed_frame(&capture, varied_image(8, 8, 1, 9)).await;
    // The first `wait_until` already matched the first translation, so wait
    // for the second translation to arrive before asserting the sink count.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while log.count_matching(|e| matches!(e, PipelineEvent::TranslationCompleted(_))) < 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for the second translation"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(sink.calls.load(std::sync::atomic::Ordering::SeqCst), 2);

    pipeline.stop().await.unwrap();
    assert!(run_handle.await.unwrap().is_ok());
}

#[tokio::test]
async fn live_full_chain_emits_events_in_order() {
    let (pipeline, capture, _ocr, translation) = live_pipeline(
        vec![Ok(ocr_result("Hello"))],
        vec![Ok(translation_result("你好", "mock-translation"))],
        Duration::from_millis(1),
        Duration::from_millis(1),
    );
    let (run_handle, log) = start_live(&pipeline);

    feed_frame(&capture, solid_image(8, 8, 1)).await;
    log.wait_until(
        |e| matches!(e, PipelineEvent::TranslationCompleted(_)),
        Duration::from_secs(5),
    )
    .await;

    pipeline.stop().await.unwrap();
    assert!(run_handle.await.unwrap().is_ok());

    let events = log.finish().await;
    assert!(matches!(events[0], PipelineEvent::CaptureStarted));
    assert!(matches!(events.last(), Some(PipelineEvent::Stopped)));
    assert_eq!(
        log_count(&events, |e| matches!(e, PipelineEvent::OcrCompleted(_))),
        1
    );
    assert_eq!(
        log_count(&events, |e| matches!(
            e,
            PipelineEvent::TranslationCompleted(_)
        )),
        1
    );
    let ocr_index = events
        .iter()
        .position(|e| matches!(e, PipelineEvent::OcrCompleted(_)))
        .unwrap();
    let translation_index = events
        .iter()
        .position(|e| matches!(e, PipelineEvent::TranslationStarted))
        .unwrap();
    assert!(ocr_index < translation_index);
    assert_eq!(
        *translation
            .last_request_text
            .lock()
            .unwrap_or_else(poison_inner),
        Some("Hello".to_string())
    );
    assert!(matches!(pipeline.status(), PipelineStatus::Idle));
}

#[tokio::test]
async fn unchanged_frames_are_skipped() {
    let (pipeline, capture, ocr, translation) = live_pipeline(
        vec![Ok(ocr_result("Static"))],
        vec![Ok(translation_result("静态", "mock-translation"))],
        Duration::from_millis(1),
        Duration::from_millis(1),
    );
    let (run_handle, log) = start_live(&pipeline);

    let frame = solid_image(8, 8, 1);
    feed_frame(&capture, frame.clone()).await;
    log.wait_until(
        |e| matches!(e, PipelineEvent::TranslationCompleted(_)),
        Duration::from_secs(5),
    )
    .await;

    // The identical frame must be captured but skipped by frame diffing.
    feed_frame(&capture, frame).await;
    wait_until(|| log.len() >= 2, Duration::from_secs(5)).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    pipeline.stop().await.unwrap();
    assert!(run_handle.await.unwrap().is_ok());

    assert_eq!(ocr.calls.load(Ordering::SeqCst), 1);
    assert_eq!(translation.calls.load(Ordering::SeqCst), 1);
    let events = log.finish().await;
    assert_eq!(
        log_count(&events, |e| matches!(e, PipelineEvent::CaptureStarted)),
        2
    );
    assert_eq!(
        log_count(&events, |e| matches!(e, PipelineEvent::OcrCompleted(_))),
        1
    );
}

#[tokio::test]
async fn unchanged_text_is_not_retranslated() {
    let (pipeline, capture, ocr, translation) = live_pipeline(
        vec![Ok(ocr_result("Same text")), Ok(ocr_result("Same text"))],
        vec![Ok(translation_result("同じテキスト", "mock-translation"))],
        Duration::from_millis(1),
        Duration::from_millis(1),
    );
    let (run_handle, log) = start_live(&pipeline);

    feed_frame(&capture, solid_image(8, 8, 1)).await;
    log.wait_until(
        |e| matches!(e, PipelineEvent::TranslationCompleted(_)),
        Duration::from_secs(5),
    )
    .await;

    // A frame with different pixels but identical text: OCR runs again but
    // the fingerprint check skips translation.
    feed_frame(&capture, varied_image(8, 8, 1, 1)).await;
    wait_until(
        || ocr.calls.load(Ordering::SeqCst) >= 2,
        Duration::from_secs(5),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    pipeline.stop().await.unwrap();
    assert!(run_handle.await.unwrap().is_ok());

    assert_eq!(ocr.calls.load(Ordering::SeqCst), 2);
    assert_eq!(translation.calls.load(Ordering::SeqCst), 1);
    let events = log.finish().await;
    assert_eq!(
        log_count(&events, |e| matches!(e, PipelineEvent::OcrCompleted(_))),
        2
    );
    assert_eq!(
        log_count(&events, |e| matches!(
            e,
            PipelineEvent::TranslationCompleted(_)
        )),
        1
    );
}

#[tokio::test]
async fn newer_frame_cancels_previous_ocr() {
    let (pipeline, capture, ocr, _translation) = live_pipeline(
        vec![Ok(ocr_result("Second")), Ok(ocr_result("Final"))],
        vec![Ok(translation_result("终", "mock-translation"))],
        Duration::from_millis(1),
        Duration::from_millis(1),
    );
    // Prepend a blocking entry so the first OCR call waits for cancellation.
    ocr.outcomes
        .lock()
        .unwrap_or_else(poison_inner)
        .push_front(None);
    let (run_handle, log) = start_live(&pipeline);

    feed_frame(&capture, solid_image(8, 8, 1)).await;
    wait_until(
        || ocr.calls.load(Ordering::SeqCst) >= 1,
        Duration::from_secs(5),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(30)).await;

    // The second frame supersedes the blocking OCR pass.
    feed_frame(&capture, varied_image(8, 8, 1, 1)).await;
    log.wait_until(
        |e| matches!(e, PipelineEvent::TranslationCompleted(_)),
        Duration::from_secs(5),
    )
    .await;

    pipeline.stop().await.unwrap();
    assert!(run_handle.await.unwrap().is_ok());

    assert_eq!(ocr.calls.load(Ordering::SeqCst), 2);
    assert_eq!(ocr.cancelled.load(Ordering::SeqCst), 1);
    let _ = log.finish().await;
}

#[tokio::test]
async fn at_most_one_ocr_and_one_translation_run_concurrently() {
    let (pipeline, capture, ocr, translation) = live_pipeline(
        vec![
            Ok(ocr_result("One")),
            Ok(ocr_result("Two")),
            Ok(ocr_result("Three")),
        ],
        vec![
            Ok(translation_result("一", "mock-translation")),
            Ok(translation_result("二", "mock-translation")),
            Ok(translation_result("三", "mock-translation")),
        ],
        Duration::from_millis(30),
        Duration::from_millis(30),
    );
    let (run_handle, log) = start_live(&pipeline);

    // Feed each frame and wait for *its* OCR and translation to complete
    // (monotonic mock counters, so an already-satisfied event log cannot
    // short-circuit the wait).
    for (index, frame) in [
        solid_image(8, 8, 1),
        varied_image(8, 8, 1, 1),
        varied_image(8, 8, 1, 2),
    ]
    .into_iter()
    .enumerate()
    {
        feed_frame(&capture, frame).await;
        let expected = index + 1;
        wait_until(
            || ocr.calls.load(Ordering::SeqCst) >= expected,
            Duration::from_secs(5),
        )
        .await;
        wait_until(
            || translation.calls.load(Ordering::SeqCst) >= expected,
            Duration::from_secs(5),
        )
        .await;
        // Let the pipeline settle before the next frame.
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    pipeline.stop().await.unwrap();
    assert!(run_handle.await.unwrap().is_ok());

    assert_eq!(ocr.max_concurrent.load(Ordering::SeqCst), 1);
    assert_eq!(translation.max_concurrent.load(Ordering::SeqCst), 1);
    assert_eq!(ocr.calls.load(Ordering::SeqCst), 3);
    assert_eq!(translation.calls.load(Ordering::SeqCst), 3);
    let _ = log.finish().await;
}

#[tokio::test]
async fn stop_terminates_all_workers() {
    let (pipeline, capture, ocr, _translation) = live_pipeline(
        vec![],
        vec![],
        Duration::from_millis(1),
        Duration::from_millis(1),
    );
    // OCR blocks until cancelled.
    ocr.push_block();
    let (run_handle, log) = start_live(&pipeline);

    feed_frame(&capture, solid_image(8, 8, 1)).await;
    wait_until(
        || ocr.calls.load(Ordering::SeqCst) >= 1,
        Duration::from_secs(5),
    )
    .await;

    tokio::time::timeout(Duration::from_secs(5), pipeline.stop())
        .await
        .expect("stop completes quickly")
        .expect("pipeline was running");
    let result = tokio::time::timeout(Duration::from_secs(5), run_handle)
        .await
        .expect("run terminates quickly")
        .unwrap();
    assert!(result.is_ok());

    assert_eq!(ocr.cancelled.load(Ordering::SeqCst), 1);
    assert!(capture.sessions_stopped.load(Ordering::SeqCst) >= 1);
    let events = log.finish().await;
    assert!(events.iter().any(|e| matches!(e, PipelineEvent::Stopped)));
}

#[tokio::test]
async fn region_update_restarts_the_session_without_stopping() {
    let (pipeline, capture, _ocr, translation) = live_pipeline(
        vec![Ok(ocr_result("A")), Ok(ocr_result("B"))],
        vec![
            Ok(translation_result("甲", "mock-translation")),
            Ok(translation_result("乙", "mock-translation")),
        ],
        Duration::from_millis(1),
        Duration::from_millis(1),
    );
    let (run_handle, log) = start_live(&pipeline);

    feed_frame(&capture, solid_image(8, 8, 1)).await;
    log.wait_until(
        |e| matches!(e, PipelineEvent::TranslationCompleted(_)),
        Duration::from_secs(5),
    )
    .await;

    let new_region = ScreenRegion::new("m1", 100, 100, 16, 16);
    pipeline.update_region(new_region).await.unwrap();
    // Give the capture loop a tick to restart the session.
    wait_until(
        || capture.sessions_started.load(Ordering::SeqCst) >= 2,
        Duration::from_secs(5),
    )
    .await;

    feed_frame(&capture, varied_image(8, 8, 1, 1)).await;
    wait_until(
        || translation.calls.load(Ordering::SeqCst) >= 2,
        Duration::from_secs(5),
    )
    .await;
    log.wait_until(
        |e| matches!(e, PipelineEvent::TranslationCompleted(_)),
        Duration::from_secs(5),
    )
    .await;

    pipeline.stop().await.unwrap();
    assert!(run_handle.await.unwrap().is_ok());

    assert_eq!(capture.sessions_started.load(Ordering::SeqCst), 2);
    let monitor_ids: Vec<String> = {
        let regions = capture.session_regions.lock().unwrap_or_else(poison_inner);
        assert_eq!(regions.len(), 2);
        regions.iter().map(|r| r.monitor_id.clone()).collect()
    };
    assert_eq!(monitor_ids, vec!["m0".to_string(), "m1".to_string()]);
    assert_eq!(translation.calls.load(Ordering::SeqCst), 2);
    let _ = log.finish().await;
}

#[tokio::test]
async fn ocr_worker_queue_stays_bounded_under_burst() {
    let (pipeline, capture, ocr, _translation) = live_pipeline(
        vec![
            Ok(ocr_result("1")),
            Ok(ocr_result("2")),
            Ok(ocr_result("3")),
            Ok(ocr_result("4")),
            Ok(ocr_result("5")),
        ],
        vec![Ok(translation_result("x", "mock-translation"))],
        Duration::from_millis(60),
        Duration::from_millis(1),
    );
    let (run_handle, _log) = start_live(&pipeline);

    // Burst five frames back-to-back; the OCR worker must never run more
    // than one pass at a time, and the queue must not grow unboundedly.
    for i in 0..5u8 {
        feed_frame(&capture, varied_image(8, 8, i, 1)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    pipeline.stop().await.unwrap();
    assert!(run_handle.await.unwrap().is_ok());

    assert_eq!(ocr.max_concurrent.load(Ordering::SeqCst), 1);
    assert!(ocr.calls.load(Ordering::SeqCst) <= 5);
    assert!(ocr.calls.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn translation_error_emits_error_event_and_pipeline_continues() {
    let (pipeline, capture, _ocr, translation) = live_pipeline(
        vec![Ok(ocr_result("One")), Ok(ocr_result("Two"))],
        vec![
            Err(vtrans_core::TranslationError::ApiRequest("boom".into())),
            Ok(translation_result("二", "mock-translation")),
        ],
        Duration::from_millis(1),
        Duration::from_millis(1),
    );
    let (run_handle, log) = start_live(&pipeline);

    feed_frame(&capture, solid_image(8, 8, 1)).await;
    log.wait_until(
        |e| matches!(e, PipelineEvent::Error(_)),
        Duration::from_secs(5),
    )
    .await;

    // The pipeline survives the translation error and translates the next
    // frame.
    feed_frame(&capture, varied_image(8, 8, 1, 1)).await;
    log.wait_until(
        |e| matches!(e, PipelineEvent::TranslationCompleted(_)),
        Duration::from_secs(5),
    )
    .await;

    pipeline.stop().await.unwrap();
    assert!(run_handle.await.unwrap().is_ok());

    let events = log.finish().await;
    assert_eq!(
        log_count(&events, |e| matches!(e, PipelineEvent::Error(_))),
        1
    );
    assert_eq!(
        log_count(&events, |e| matches!(
            e,
            PipelineEvent::TranslationCompleted(_)
        )),
        1
    );
    let _ = translation;
}

#[tokio::test]
async fn session_end_stops_the_pipeline_gracefully() {
    let (pipeline, capture, _ocr, _translation) = live_pipeline(
        vec![Ok(ocr_result("Bye"))],
        vec![Ok(translation_result("再见", "mock-translation"))],
        Duration::from_millis(1),
        Duration::from_millis(1),
    );
    let (run_handle, log) = start_live(&pipeline);

    feed_frame(&capture, solid_image(8, 8, 1)).await;
    log.wait_until(
        |e| matches!(e, PipelineEvent::TranslationCompleted(_)),
        Duration::from_secs(5),
    )
    .await;

    // Dropping the feeder closes the session channel; the capture loop sees
    // `Ok(None)` and ends the pipeline.
    *capture.feeder.lock().unwrap_or_else(poison_inner) = None;

    let result = tokio::time::timeout(Duration::from_secs(5), run_handle)
        .await
        .expect("run ends after session end")
        .unwrap();
    assert!(result.is_ok());

    let events = log.finish().await;
    assert!(events.iter().any(|e| matches!(e, PipelineEvent::Stopped)));
    assert!(matches!(pipeline.status(), PipelineStatus::Idle));
}

#[tokio::test]
async fn stop_when_idle_returns_not_running() {
    let (pipeline, _capture, _ocr, _translation) = live_pipeline(
        vec![],
        vec![],
        Duration::from_millis(1),
        Duration::from_millis(1),
    );
    assert!(matches!(
        pipeline.stop().await,
        Err(PipelineError::NotRunning)
    ));
}

#[tokio::test]
async fn concurrent_run_returns_already_running() {
    let (pipeline, capture, ocr, _translation) = live_pipeline(
        vec![],
        vec![],
        Duration::from_millis(1),
        Duration::from_millis(1),
    );
    ocr.push_block();
    let (run_handle, _log) = start_live(&pipeline);

    // Drive one frame into the blocking OCR pass so the run is definitely
    // in flight before attempting a second run.
    feed_frame(&capture, solid_image(8, 8, 1)).await;
    wait_until(
        || ocr.calls.load(Ordering::SeqCst) >= 1,
        Duration::from_secs(5),
    )
    .await;

    let (tx, _rx) = mpsc::channel(16);
    let second = pipeline.run(tx).await;
    assert!(matches!(second, Err(PipelineError::AlreadyRunning)));

    pipeline.stop().await.unwrap();
    assert!(run_handle.await.unwrap().is_ok());
}
