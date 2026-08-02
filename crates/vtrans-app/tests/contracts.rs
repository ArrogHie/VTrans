use vtrans_app::events::{CAPTURE_STATUS_CHANGED, OCR_COMPLETED, PIPELINE_ERROR, REGION_SELECTED};
use vtrans_app::{AppError, AppStatus, LiveTranslationConfig};
use vtrans_core::{Language, PipelineStatus, ScreenRegion};
use vtrans_pipeline::PipelineError;

#[test]
fn public_ipc_contracts_round_trip_through_json() {
    let request = LiveTranslationConfig {
        region: ScreenRegion::new("display-1", 2, 3, 640, 480),
        capture_interval_ms: 750,
        difference_threshold: 0.05,
    };
    let json = serde_json::to_string(&request).unwrap();
    let decoded: LiveTranslationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.region.monitor_id, "display-1");
    assert_eq!(decoded.capture_interval_ms, 750);
    assert!((decoded.difference_threshold - 0.05).abs() < f32::EPSILON);

    let status = AppStatus {
        pipeline_status: PipelineStatus::Idle,
        ocr_provider: "pp-ocr".to_string(),
        translation_provider: "api".to_string(),
        selected_region: Some(decoded.region),
        live_running: false,
        model_progress: None,
    };
    assert!(serde_json::to_string(&status).unwrap().contains("pp-ocr"));
}

#[test]
fn app_errors_are_frontend_safe_strings() {
    let error = AppError::Pipeline(PipelineError::Cancelled);
    assert_eq!(
        serde_json::to_string(&error).unwrap(),
        r#""pipeline error: cancelled""#
    );
}

#[test]
fn event_names_are_stable() {
    assert_eq!(CAPTURE_STATUS_CHANGED, "capture_status_changed");
    assert_eq!(OCR_COMPLETED, "ocr_completed");
    assert_eq!(PIPELINE_ERROR, "pipeline_error");
    assert_eq!(REGION_SELECTED, "region_selected");
    assert_eq!(Language::English.code(), "en");
}
