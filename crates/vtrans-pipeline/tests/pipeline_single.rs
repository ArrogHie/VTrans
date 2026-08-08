//! Integration tests for single-capture pipeline mode.
//!
//! Exercises the capture -> OCR -> normalize -> translate chain through the
//! public [`Pipeline`] API and the [`run_single_capture`] convenience
//! function, using scripted mock providers.

mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use vtrans_core::types::{Language, OcrOptions, PipelineStatus, ScreenRegion, TranslationRequest};
use vtrans_core::{CaptureError, CoreError, OcrError};
use vtrans_pipeline::{
    run_single_capture, Pipeline, PipelineConfig, PipelineDeps, PipelineError, PipelineEvent,
};

use common::*;

fn single_config(region: ScreenRegion) -> PipelineConfig {
    PipelineConfig::single(
        region,
        OcrOptions::new(Language::English),
        TranslationRequest::new("", Language::English, Language::ChineseSimplified),
    )
}

/// Single-mode configuration whose translation source is `Auto`; the
/// pipeline must resolve it after OCR.
fn auto_source_config(region: ScreenRegion) -> PipelineConfig {
    PipelineConfig::single(
        region,
        OcrOptions::new(Language::Auto),
        TranslationRequest::new("", Language::Auto, Language::ChineseSimplified),
    )
}

fn deps(capture: MockCaptureSource, ocr: MockOcr, translation: MockTranslation) -> PipelineDeps {
    PipelineDeps::new(Box::new(capture), Box::new(ocr), Box::new(translation))
}

/// Builds a pipeline whose capture, OCR, and translation are scripted to
/// succeed quickly. Returns the pipeline plus the shared behaviors.
fn happy_path_pipeline() -> (
    Arc<Pipeline>,
    Arc<MockCaptureBehavior>,
    Arc<MockOcrBehavior>,
    Arc<MockTranslationBehavior>,
) {
    let region = ScreenRegion::new("m0", 10, 20, 8, 8);
    let capture = Arc::new(MockCaptureBehavior::default());
    capture
        .capture_once_outcome
        .lock()
        .unwrap_or_else(poison_inner)
        .replace(Ok(solid_image(8, 8, 1)));
    let ocr = Arc::new(MockOcrBehavior {
        delay: Duration::from_millis(1),
        ..MockOcrBehavior::default()
    });
    ocr.push(Ok(ocr_result("Hello world")));
    let translation = Arc::new(MockTranslationBehavior {
        delay: Duration::from_millis(1),
        ..MockTranslationBehavior::default()
    });
    translation.push(Ok(translation_result("你好世界", "mock-translation")));

    let pipeline = Arc::new(Pipeline::new(
        single_config(region),
        deps(
            MockCaptureSource::new(capture.clone()),
            MockOcr::new(ocr.clone()),
            MockTranslation::new(translation.clone()),
        ),
    ));
    (pipeline, capture, ocr, translation)
}

#[tokio::test]
async fn frame_sink_observes_the_captured_frame_before_ocr() {
    let region = ScreenRegion::new("m0", 10, 20, 8, 8);
    let capture = Arc::new(MockCaptureBehavior::default());
    capture
        .capture_once_outcome
        .lock()
        .unwrap_or_else(poison_inner)
        .replace(Ok(solid_image(8, 8, 1)));
    let ocr = Arc::new(MockOcrBehavior::default());
    ocr.push(Ok(ocr_result("Hello world")));
    let translation = Arc::new(MockTranslationBehavior::default());
    translation.push(Ok(translation_result("你好世界", "mock-translation")));
    let sink = Arc::new(RecordingSink::default());

    let pipeline = Arc::new(Pipeline::with_frame_sink(
        single_config(region),
        deps(
            MockCaptureSource::new(capture.clone()),
            MockOcr::new(ocr.clone()),
            MockTranslation::new(translation.clone()),
        ),
        Some(sink.clone()),
    ));
    let (tx, rx) = mpsc::channel(32);

    let result = pipeline.run(tx).await;
    assert!(result.is_ok(), "run failed: {result:?}");
    let events = collect_events(rx).await;
    assert!(matches!(events[1], PipelineEvent::OcrStarted));

    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
    let observed = sink
        .last
        .lock()
        .unwrap_or_else(poison_inner)
        .clone()
        .unwrap();
    assert_eq!((observed.width, observed.height), (8, 8));
}

#[tokio::test]
async fn single_capture_emits_full_event_chain() {
    let (pipeline, capture, ocr, translation) = happy_path_pipeline();
    let (tx, rx) = mpsc::channel(32);

    let result = pipeline.run(tx).await;
    assert!(result.is_ok(), "run failed: {result:?}");

    let events = collect_events(rx).await;
    assert_eq!(events.len(), 6, "unexpected events: {events:?}");
    assert!(matches!(events[0], PipelineEvent::CaptureStarted));
    assert!(matches!(events[1], PipelineEvent::OcrStarted));
    assert!(matches!(events[2], PipelineEvent::OcrCompleted(_)));
    assert!(matches!(events[3], PipelineEvent::TranslationStarted));
    assert!(matches!(events[4], PipelineEvent::TranslationCompleted(_)));
    assert!(matches!(events[5], PipelineEvent::Stopped));

    // The provider saw the configured region, and the normalized text was
    // passed to the translation provider.
    assert_eq!(
        ocr.last_region
            .lock()
            .unwrap_or_else(poison_inner)
            .as_ref()
            .map(|r| (r.monitor_id.as_str(), r.x, r.y, r.width, r.height)),
        // OCR receives an image-aligned region (offset 0) even though
        // the configured screen region has a screen offset.
        Some(("m0", 0, 0, 8, 8))
    );
    assert_eq!(
        *translation
            .last_request_text
            .lock()
            .unwrap_or_else(poison_inner),
        Some("Hello world".to_string())
    );
    assert_eq!(capture.sessions_started.load(Ordering::SeqCst), 0);
    assert!(matches!(pipeline.status(), PipelineStatus::Completed));
}

#[tokio::test]
async fn single_capture_empty_ocr_skips_translation() {
    let (pipeline, _capture, ocr, translation) = happy_path_pipeline();
    *ocr.outcomes.lock().unwrap_or_else(poison_inner) =
        std::collections::VecDeque::from([Some(Ok(vtrans_core::OcrResult::empty()))]);
    let (tx, rx) = mpsc::channel(32);

    let result = pipeline.run(tx).await;
    assert!(result.is_ok());

    let events = collect_events(rx).await;
    assert_eq!(events.len(), 4);
    assert!(matches!(events[2], PipelineEvent::OcrCompleted(_)));
    assert!(matches!(events[3], PipelineEvent::Stopped));
    assert_eq!(translation.calls.load(Ordering::SeqCst), 0);
    assert!(matches!(pipeline.status(), PipelineStatus::Completed));
}

#[tokio::test]
async fn single_capture_capture_error_is_reported() {
    let (pipeline, capture, _ocr, _translation) = happy_path_pipeline();
    capture
        .capture_once_outcome
        .lock()
        .unwrap_or_else(poison_inner)
        .replace(Err(CaptureError::MonitorNotFound("Display2".into())));
    let (tx, rx) = mpsc::channel(32);

    let result = pipeline.run(tx).await;
    assert!(matches!(result, Err(PipelineError::Capture(_))));

    let events = collect_events(rx).await;
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], PipelineEvent::CaptureStarted));
    assert!(pipeline.status().is_error());
}

#[tokio::test]
async fn single_capture_ocr_error_is_reported() {
    let (pipeline, _capture, ocr, _translation) = happy_path_pipeline();
    *ocr.outcomes.lock().unwrap_or_else(poison_inner) =
        std::collections::VecDeque::from([Some(Err(OcrError::Inference("boom".into())))]);
    let (tx, rx) = mpsc::channel(32);

    let result = pipeline.run(tx).await;
    assert!(matches!(result, Err(PipelineError::Ocr(_))));

    let events = collect_events(rx).await;
    assert_eq!(events.len(), 2);
    assert!(matches!(events[1], PipelineEvent::OcrStarted));
    assert!(pipeline.status().is_error());
}

#[tokio::test]
async fn single_capture_translation_error_is_reported() {
    let (pipeline, _capture, _ocr, translation) = happy_path_pipeline();
    *translation.outcomes.lock().unwrap_or_else(poison_inner) =
        std::collections::VecDeque::from([Some(Err(vtrans_core::TranslationError::ApiRequest(
            "boom".into(),
        )))]);
    let (tx, rx) = mpsc::channel(32);

    let result = pipeline.run(tx).await;
    assert!(matches!(result, Err(PipelineError::Translation(_))));

    let events = collect_events(rx).await;
    assert_eq!(events.len(), 4);
    assert!(pipeline.status().is_error());
}

#[tokio::test]
async fn long_text_is_chunked_before_translation() {
    let (pipeline, _capture, ocr, translation) = happy_path_pipeline();
    // A single OCR line longer than the English chunk budget (1024 chars)
    // must be split into chunks before each is translated.
    let long_text = "x".repeat(2500);
    let polygon = [[0.0, 0.0], [100.0, 0.0], [100.0, 20.0], [0.0, 20.0]];
    *ocr.outcomes.lock().unwrap_or_else(poison_inner) =
        std::collections::VecDeque::from([Some(Ok(vtrans_core::OcrResult::from_lines(
            vec![vtrans_core::OcrLine::new(long_text, 0.9, polygon, 0)],
            Some(Language::English),
            5,
        )))]);
    *translation.outcomes.lock().unwrap_or_else(poison_inner) = std::collections::VecDeque::from([
        Some(Ok(translation_result("译文一", "mock-translation"))),
        Some(Ok(translation_result("译文二", "mock-translation"))),
        Some(Ok(translation_result("译文三", "mock-translation"))),
    ]);

    let (tx, rx) = mpsc::channel(32);
    let result = pipeline.run(tx).await;
    assert!(result.is_ok());

    assert_eq!(translation.calls.load(Ordering::SeqCst), 3);
    let request_texts = translation
        .request_texts
        .lock()
        .unwrap_or_else(poison_inner)
        .clone();
    assert_eq!(request_texts.len(), 3);
    assert_eq!(request_texts[0].len(), 1024);
    assert_eq!(request_texts[1].len(), 1024);
    assert_eq!(request_texts[2].len(), 452);
    assert!(request_texts
        .iter()
        .all(|chunk| chunk.chars().all(|c| c == 'x')));

    let events = collect_events(rx).await;
    let completed = events.iter().find_map(|event| match event {
        PipelineEvent::TranslationCompleted(result) => Some(result.clone()),
        _ => None,
    });
    assert_eq!(
        completed.map(|result| result.translated_text),
        Some("译文一\n译文二\n译文三".to_string())
    );
}

#[tokio::test]
async fn auto_source_uses_ocr_detection_for_translation() {
    let region = ScreenRegion::new("m0", 10, 20, 8, 8);
    let capture = Arc::new(MockCaptureBehavior::default());
    capture
        .capture_once_outcome
        .lock()
        .unwrap_or_else(poison_inner)
        .replace(Ok(solid_image(8, 8, 1)));
    let ocr = Arc::new(MockOcrBehavior::default());
    ocr.push(Ok(ocr_result_with_detection(
        "こんにちは",
        Language::Japanese,
    )));
    let translation = Arc::new(MockTranslationBehavior::default());
    translation.push(Ok(translation_result("你好", "mock-translation")));

    let pipeline = Arc::new(Pipeline::new(
        auto_source_config(region),
        deps(
            MockCaptureSource::new(capture),
            MockOcr::new(ocr),
            MockTranslation::new(translation.clone()),
        ),
    ));
    let (tx, rx) = mpsc::channel(32);
    pipeline.run(tx).await.unwrap();
    let _events = collect_events(rx).await;

    // OCR detected Japanese while the configured source is Auto: the
    // translation request must use Japanese.
    assert_eq!(
        *translation
            .last_request_source
            .lock()
            .unwrap_or_else(poison_inner),
        Some(Language::Japanese)
    );
}

#[tokio::test]
async fn auto_source_falls_back_to_heuristic_without_detection() {
    let region = ScreenRegion::new("m0", 10, 20, 8, 8);
    let capture = Arc::new(MockCaptureBehavior::default());
    capture
        .capture_once_outcome
        .lock()
        .unwrap_or_else(poison_inner)
        .replace(Ok(solid_image(8, 8, 1)));
    let ocr = Arc::new(MockOcrBehavior::default());
    // No OCR language detection: the pipeline must fall back to the Unicode
    // heuristic on the recognized text (hiragana -> Japanese).
    ocr.push(Ok(ocr_result_no_detection("こんにちは世界")));
    let translation = Arc::new(MockTranslationBehavior::default());
    translation.push(Ok(translation_result("你好，世界", "mock-translation")));

    let pipeline = Arc::new(Pipeline::new(
        auto_source_config(region),
        deps(
            MockCaptureSource::new(capture),
            MockOcr::new(ocr),
            MockTranslation::new(translation.clone()),
        ),
    ));
    let (tx, rx) = mpsc::channel(32);
    pipeline.run(tx).await.unwrap();
    let _events = collect_events(rx).await;

    assert_eq!(
        *translation
            .last_request_source
            .lock()
            .unwrap_or_else(poison_inner),
        Some(Language::Japanese)
    );
}

#[tokio::test]
async fn auto_source_heuristic_detects_english() {
    let region = ScreenRegion::new("m0", 10, 20, 8, 8);
    let capture = Arc::new(MockCaptureBehavior::default());
    capture
        .capture_once_outcome
        .lock()
        .unwrap_or_else(poison_inner)
        .replace(Ok(solid_image(8, 8, 1)));
    let ocr = Arc::new(MockOcrBehavior::default());
    ocr.push(Ok(ocr_result_no_detection("Hello world")));
    let translation = Arc::new(MockTranslationBehavior::default());
    translation.push(Ok(translation_result("你好，世界", "mock-translation")));

    let pipeline = Arc::new(Pipeline::new(
        auto_source_config(region),
        deps(
            MockCaptureSource::new(capture),
            MockOcr::new(ocr),
            MockTranslation::new(translation.clone()),
        ),
    ));
    let (tx, rx) = mpsc::channel(32);
    pipeline.run(tx).await.unwrap();
    let _events = collect_events(rx).await;

    assert_eq!(
        *translation
            .last_request_source
            .lock()
            .unwrap_or_else(poison_inner),
        Some(Language::English)
    );
}

#[tokio::test]
async fn auto_source_stays_auto_when_undecidable() {
    let region = ScreenRegion::new("m0", 10, 20, 8, 8);
    let capture = Arc::new(MockCaptureBehavior::default());
    capture
        .capture_once_outcome
        .lock()
        .unwrap_or_else(poison_inner)
        .replace(Ok(solid_image(8, 8, 1)));
    let ocr = Arc::new(MockOcrBehavior::default());
    // Digits-only text: no kana, and Latin letters do not dominate, so the
    // source stays Auto and the provider decides.
    ocr.push(Ok(ocr_result_no_detection("12345")));
    let translation = Arc::new(MockTranslationBehavior::default());
    translation.push(Ok(translation_result("12345", "mock-translation")));

    let pipeline = Arc::new(Pipeline::new(
        auto_source_config(region),
        deps(
            MockCaptureSource::new(capture),
            MockOcr::new(ocr),
            MockTranslation::new(translation.clone()),
        ),
    ));
    let (tx, rx) = mpsc::channel(32);
    pipeline.run(tx).await.unwrap();
    let _events = collect_events(rx).await;

    assert_eq!(
        *translation
            .last_request_source
            .lock()
            .unwrap_or_else(poison_inner),
        Some(Language::Auto)
    );
}

#[tokio::test]
async fn run_with_closed_event_channel_returns_channel_closed() {
    let (pipeline, _capture, _ocr, _translation) = happy_path_pipeline();
    let (tx, rx) = mpsc::channel(4);
    drop(rx);

    let result = pipeline.run(tx).await;
    assert!(matches!(result, Err(PipelineError::ChannelClosed)));
}

#[tokio::test]
async fn stop_when_idle_returns_not_running() {
    let (pipeline, _capture, _ocr, _translation) = happy_path_pipeline();
    assert!(matches!(
        pipeline.stop().await,
        Err(PipelineError::NotRunning)
    ));
}

#[tokio::test]
async fn run_single_capture_convenience_entry_point() {
    let region = ScreenRegion::new("m0", 0, 0, 8, 8);
    let capture = Arc::new(MockCaptureBehavior::default());
    capture
        .capture_once_outcome
        .lock()
        .unwrap_or_else(poison_inner)
        .replace(Ok(solid_image(8, 8, 1)));
    let ocr = Arc::new(MockOcrBehavior::default());
    ocr.push(Ok(ocr_result("Convenience")));
    let translation = Arc::new(MockTranslationBehavior::default());
    translation.push(Ok(translation_result("便利", "mock-translation")));

    let (tx, rx) = mpsc::channel(16);
    let result = run_single_capture(
        deps(
            MockCaptureSource::new(capture),
            MockOcr::new(ocr),
            MockTranslation::new(translation),
        ),
        single_config(region),
        tx,
    )
    .await;
    assert!(result.is_ok());

    let events = collect_events(rx).await;
    assert_eq!(events.len(), 6);
    assert!(matches!(events[4], PipelineEvent::TranslationCompleted(_)));
}

#[tokio::test]
async fn update_region_is_applied_to_subsequent_run() {
    let (pipeline, _capture, ocr, _translation) = happy_path_pipeline();
    let new_region = ScreenRegion::new("m1", 5, 5, 16, 16);
    pipeline.update_region(new_region.clone()).await.unwrap();

    let (tx, rx) = mpsc::channel(16);
    pipeline.run(tx).await.unwrap();
    let _events = collect_events(rx).await;

    assert_eq!(
        ocr.last_region
            .lock()
            .unwrap_or_else(poison_inner)
            .as_ref()
            .map(|r| (r.monitor_id.as_str(), r.x, r.y, r.width, r.height)),
        // The monitor id carries over from the updated region.
        Some(("m1", 0, 0, 8, 8))
    );
}

#[tokio::test]
async fn update_region_rejects_invalid_region() {
    let (pipeline, _capture, _ocr, _translation) = happy_path_pipeline();
    let invalid = ScreenRegion::new("m0", 0, 0, 0, 100);
    assert!(matches!(
        pipeline.update_region(invalid).await,
        Err(CoreError::InvalidRegion(_))
    ));
}

#[tokio::test]
async fn stop_cancels_a_single_capture_run() {
    let (pipeline, _capture, ocr, _translation) = happy_path_pipeline();
    // OCR blocks until cancelled.
    let (tx, rx) = mpsc::channel(16);
    let run_handle = tokio::spawn({
        let pipeline = pipeline.clone();
        async move { pipeline.run(tx).await }
    });
    wait_until(
        || ocr.calls.load(Ordering::SeqCst) >= 1,
        Duration::from_secs(5),
    )
    .await;

    pipeline.stop().await.unwrap();
    let result = run_handle.await.unwrap();
    assert!(matches!(result, Err(PipelineError::Cancelled)));

    let events = collect_events(rx).await;
    assert!(events.iter().any(|e| matches!(e, PipelineEvent::Stopped)));
    assert!(pipeline.status().is_idle());
}

#[tokio::test]
async fn concurrent_run_returns_already_running() {
    let region = ScreenRegion::new("m0", 0, 0, 8, 8);
    let capture = Arc::new(MockCaptureBehavior::default());
    capture
        .capture_once_outcome
        .lock()
        .unwrap_or_else(poison_inner)
        .replace(Ok(solid_image(8, 8, 1)));
    let ocr = Arc::new(MockOcrBehavior::default());
    let pipeline = Arc::new(Pipeline::new(
        single_config(region),
        deps(
            MockCaptureSource::new(capture.clone()),
            MockOcr::new(ocr.clone()),
            MockTranslation::new(Arc::new(MockTranslationBehavior::default())),
        ),
    ));

    let (tx, rx) = mpsc::channel(16);
    let run_handle = tokio::spawn({
        let pipeline = pipeline.clone();
        async move { pipeline.run(tx).await }
    });
    wait_until(
        || ocr.calls.load(Ordering::SeqCst) >= 1,
        Duration::from_secs(5),
    )
    .await;

    let (tx2, rx2) = mpsc::channel(16);
    let second = pipeline.run(tx2).await;
    assert!(matches!(second, Err(PipelineError::AlreadyRunning)));

    pipeline.stop().await.unwrap();
    assert!(run_handle.await.unwrap().is_err());
    drop(rx);
    drop(rx2);
}
