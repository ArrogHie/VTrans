use vtrans_app::events::{
    CAPTURE_STATUS_CHANGED, OCR_COMPLETED, OVERLAY_HIDDEN, OVERLAY_REGION_UPDATED, PIPELINE_ERROR,
    REGION_SELECTED,
};
use vtrans_app::{AppError, AppStatus, LiveTranslationConfig};
use vtrans_config::AppConfig;
use vtrans_core::{Language, PipelineStatus, ScreenRegion};
use vtrans_pipeline::PipelineError;

// IPC argument-name contract:
//
// Tauri 2 maps Rust snake_case command parameters to camelCase on the
// frontend by default, and `vtrans-app` keeps that default (no `rename_all`
// attributes). The frontend therefore sends:
//
//   set_api_key               -> { apiKey }
//   set_translation_provider  -> { providerId }
//   set_source_language       -> { language }
//   set_target_language       -> { language }
//   set_ocr_language          -> { language }
//   start_live_translation    -> { config }
//
// Do not add `rename_all = "snake_case"` to a command without updating
// `src/services/tauri.ts` and `src/test/ipc.test.ts` on the frontend branch
// in the same change.

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
    assert_eq!(OVERLAY_REGION_UPDATED, "overlay_region_updated");
    assert_eq!(OVERLAY_HIDDEN, "overlay_hidden");
    assert_eq!(Language::English.code(), "en");
}

#[test]
fn app_config_serializes_with_frontend_field_names() {
    let config = AppConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    // Field names consumed by the frontend settings panel and type definitions.
    assert!(json.contains(r#""capture""#));
    assert!(json.contains(r#""interval_ms""#));
    assert!(json.contains(r#""difference_threshold""#));
    assert!(json.contains(r#""hotkeys""#));
    assert!(json.contains(r#""live_translate""#));
    assert!(json.contains(r#""log_level""#));
    // Language codes match the frontend `LanguageCode` union.
    assert!(json.contains(r#""language":"auto""#));
    // The API key never travels inside AppConfig.
    assert!(!json.contains("api_key"));
    assert!(!json.contains("apiKey"));
}

#[test]
fn api_key_validation_errors_are_frontend_safe_strings() {
    let error = AppError::InvalidApiKey("key must not be empty".to_string());
    assert_eq!(
        serde_json::to_string(&error).unwrap(),
        r#""invalid api key: key must not be empty""#
    );
}
