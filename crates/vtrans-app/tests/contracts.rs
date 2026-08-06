use vtrans_app::events::{
    DebugFramePayload, CAPTURE_STATUS_CHANGED, DEBUG_FRAME_UPDATED, OCR_COMPLETED, OVERLAY_HIDDEN,
    OVERLAY_REGION_UPDATED, PIPELINE_ERROR, REGION_SELECTED,
};
use vtrans_app::{AppError, AppStatus, LiveTranslationConfig};
use vtrans_config::AppConfig;
use vtrans_core::{Language, PipelineMode, PipelineStatus, ScreenRegion};
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
//   update_live_region        -> { region, mode }
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
        mode: PipelineMode::SingleCapture,
        pipeline_status: PipelineStatus::Idle,
        ocr_provider: "pp-ocr".to_string(),
        translation_provider: "api".to_string(),
        selected_region: Some(decoded.region),
        live_running: false,
        model_progress: None,
        debug_mode: false,
    };
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("pp-ocr"));
    assert!(json.contains(r#""mode":"single""#));
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
    assert_eq!(DEBUG_FRAME_UPDATED, "debug_frame_updated");
    assert_eq!(Language::English.code(), "en");
}

#[test]
fn debug_frame_payload_serializes_with_frontend_field_names() {
    let payload = DebugFramePayload {
        image: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"jpeg-bytes"),
        region: ScreenRegion::new("display-1", 10, 20, 800, 400),
        frame_index: 42,
        timestamp_ms: 1_785_911_487_496,
    };
    let json = serde_json::to_string(&payload).unwrap();
    assert!(json.contains(r#""image":"anBlZy1ieXRlcw==""#));
    assert!(json.contains(r#""region""#));
    assert!(json.contains(r#""frame_index":42"#));
    assert!(json.contains(r#""timestamp_ms":1785911487496"#));
    assert!(json.contains(r#""monitor_id":"display-1""#));
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
