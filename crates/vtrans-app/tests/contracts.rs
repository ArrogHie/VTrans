use vtrans_app::events::{
    DebugFramePayload, CAPTURE_STATUS_CHANGED, DEBUG_FRAME_UPDATED, MULTIBOX_BOX_ADDED,
    MULTIBOX_BOX_REMOVED, MULTIBOX_BOX_UPDATED, MULTIBOX_RESULT, MULTIBOX_STATUS, MULTIBOX_WARNING,
    OCR_COMPLETED, OVERLAY_HIDDEN, OVERLAY_REGION_UPDATED, PIPELINE_ERROR, REGION_SELECTED,
    TRANSLATION_SINGLE_RESULT,
};
use vtrans_app::{AppError, AppStatus, LiveTranslationConfig, TranslationBoxInfo};
use vtrans_config::AppConfig;
use vtrans_core::{Language, PipelineMode, PipelineStatus, ScreenRegion, TranslationResult};
use vtrans_pipeline::{BoxStatus, BoxedTranslationResult, PipelineError, TranslationBox};

// IPC argument-name contract:
//
// Tauri 2 maps Rust snake_case command parameters to camelCase on the
// frontend by default, and `vtrans-app` keeps that default (no `rename_all`
// attributes). The frontend therefore sends:
//
//   set_api_key               -> { apiKey }
//   set_provider_credentials  -> { providerId, apiKey?, appId?, secret? }
//   set_translation_provider  -> { providerId }
//   set_source_language       -> { language }
//   set_target_language       -> { language }
//   set_ocr_language          -> { language }
//   start_live_translation    -> { config }
//   update_live_region        -> { region, mode }
//   update_result_window_appearance  -> { opacity, fontSizePx }
//   update_floating_ball_appearance  -> { opacity, sizePx }
//   add_translation_box              -> { region }
//   remove_translation_box           -> { boxId }
//   update_translation_box           -> { boxId, region }
//   list_translation_boxes           -> {}
//   start_multi_realtime             -> {}
//   stop_multi_realtime              -> {}
//   stop_box                         -> { boxId }
//   open_result_window               -> {}
//   download_translation_model       -> {}
//   cancel_translation_model_download -> {}
//   delete_translation_model         -> {}
//   get_model_status                 -> {}
//   retry_model_setup                -> {}
//
// The five model-management commands take no arguments (Tauri 2 default
// camelCase naming is irrelevant for them); their results are
// `ModelStatusReport` (see below) or plain `()`.
//
// Do not add `rename_all = "snake_case"` to a command without updating
// `src/services/tauri.ts` and `src/test/ipc.test.ts` on the frontend branch
// in the same change.

/// Mirrors the wire shape of `update_result_window_appearance` arguments.
///
/// Tauri 2 maps the Rust `font_size_px` parameter to camelCase by default,
/// so the frontend must send `{ opacity, fontSizePx }`.
#[derive(serde::Deserialize)]
struct ResultWindowAppearanceArgs {
    opacity: f64,
    #[serde(rename = "fontSizePx")]
    font_size_px: u32,
}

/// Mirrors the wire shape of `update_floating_ball_appearance` arguments.
///
/// Tauri 2 maps the Rust `size_px` parameter to camelCase by default, so
/// the frontend must send `{ opacity, sizePx }`.
#[derive(serde::Deserialize)]
struct FloatingBallAppearanceArgs {
    opacity: f64,
    #[serde(rename = "sizePx")]
    size_px: u32,
}

#[test]
fn update_result_window_appearance_args_use_camel_case() {
    let args: ResultWindowAppearanceArgs =
        serde_json::from_str(r#"{"opacity":0.8,"fontSizePx":18}"#).unwrap();
    assert!((args.opacity - 0.8).abs() < f64::EPSILON);
    assert_eq!(args.font_size_px, 18);
}

#[test]
fn update_floating_ball_appearance_args_use_camel_case() {
    let args: FloatingBallAppearanceArgs =
        serde_json::from_str(r#"{"opacity":0.9,"sizePx":56}"#).unwrap();
    assert!((args.opacity - 0.9).abs() < f64::EPSILON);
    assert_eq!(args.size_px, 56);
}

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
        translation_provider: "openai".to_string(),
        selected_region: Some(decoded.region),
        live_running: false,
        model_progress: None,
        debug_mode: false,
    };
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("pp-ocr"));
    assert!(json.contains(r#""mode":"single""#));
    assert!(json.contains(r#""translation_provider":"openai""#));
}

/// Mirrors the wire shape of `set_provider_credentials` arguments.
///
/// Tauri 2 maps the Rust `provider_id` / `api_key` / `app_id` parameters to
/// camelCase by default, so the frontend must send `{ providerId, apiKey,
/// appId, secret }` with only the fields required by the provider.
#[derive(serde::Deserialize)]
struct ProviderCredentialsArgs {
    #[serde(rename = "providerId")]
    provider_id: String,
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
    #[serde(rename = "appId")]
    app_id: Option<String>,
    secret: Option<String>,
}

#[test]
fn set_provider_credentials_args_use_camel_case_with_optional_values() {
    let args: ProviderCredentialsArgs =
        serde_json::from_str(r#"{"providerId":"baidu","appId":"app-2024","secret":"sk-secret"}"#)
            .unwrap();
    assert_eq!(args.provider_id, "baidu");
    assert_eq!(args.api_key, None);
    assert_eq!(args.app_id.as_deref(), Some("app-2024"));
    assert_eq!(args.secret.as_deref(), Some("sk-secret"));

    let args: ProviderCredentialsArgs =
        serde_json::from_str(r#"{"providerId":"openai","apiKey":"sk-1234"}"#).unwrap();
    assert_eq!(args.provider_id, "openai");
    assert_eq!(args.api_key.as_deref(), Some("sk-1234"));
    assert_eq!(args.app_id, None);
    assert_eq!(args.secret, None);
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

#[test]
fn multibox_event_names_are_stable() {
    assert_eq!(MULTIBOX_RESULT, "multibox://result");
    assert_eq!(MULTIBOX_BOX_ADDED, "multibox://box-added");
    assert_eq!(MULTIBOX_BOX_REMOVED, "multibox://box-removed");
    assert_eq!(MULTIBOX_BOX_UPDATED, "multibox://box-updated");
    assert_eq!(MULTIBOX_STATUS, "multibox://status");
    assert_eq!(MULTIBOX_WARNING, "multibox://warning");
    assert_eq!(TRANSLATION_SINGLE_RESULT, "translation://single-result");
}

#[test]
fn translation_box_info_serde_matches_frontend_contract() {
    let info = TranslationBoxInfo {
        box_id: 5,
        region: ScreenRegion::new("display-1", 10, 20, 300, 400),
        color: "#FF6B6B".to_string(),
    };
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains(r#""box_id":5"#));
    assert!(json.contains("\"color\":\"#FF6B6B\""));
    assert!(json.contains(r#""region""#));
    let back: TranslationBoxInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back.box_id, 5);
    assert_eq!(back.color, "#FF6B6B");
    assert_eq!(back.region.width, 300);
}

#[test]
fn box_status_serde_matches_frontend_contract() {
    assert_eq!(
        serde_json::to_string(&BoxStatus::Running).unwrap(),
        r#""Running""#
    );
    assert_eq!(
        serde_json::to_string(&BoxStatus::Stopped).unwrap(),
        r#""Stopped""#
    );
    let json = serde_json::to_string(&BoxStatus::Error("capture failed".to_string())).unwrap();
    assert!(json.contains(r#""Error""#));
    assert!(json.contains("capture failed"));
}

#[test]
fn boxed_translation_result_serde_matches_frontend_contract() {
    let result = TranslationResult::new("translated text", "mock", 42);
    let boxed = BoxedTranslationResult::new(0, "#FF6B6B", result);
    let json = serde_json::to_string(&boxed).unwrap();
    assert!(json.contains(r#""box_id":0"#));
    assert!(json.contains("\"color\":\"#FF6B6B\""));
    assert!(json.contains(r#""translated_text":"translated text""#));
    assert!(json.contains(r#""timestamp""#));
}

#[test]
fn translation_box_serde_uses_id_not_box_id() {
    // The pipeline's TranslationBox uses `id`, but the app layer's
    // TranslationBoxInfo uses `box_id` for the frontend contract.
    let box_ = TranslationBox::new(3, ScreenRegion::new("m0", 0, 0, 100, 100), "#4ECDC4");
    let json = serde_json::to_string(&box_).unwrap();
    assert!(json.contains(r#""id":3"#));
    assert!(!json.contains(r#""box_id""#));

    let info = TranslationBoxInfo::from_pipeline_box(&box_);
    let info_json = serde_json::to_string(&info).unwrap();
    assert!(info_json.contains(r#""box_id":3"#));
    assert!(!info_json.contains(r#""id":3"#));
}

// ── 发行部署：模型下载/状态 IPC 契约 ──

#[test]
fn model_download_progress_event_name_is_stable() {
    assert_eq!(
        vtrans_app::events::MODEL_DOWNLOAD_PROGRESS,
        "model_download_progress"
    );
}

#[test]
fn model_download_progress_payload_matches_the_frontend_contract() {
    use vtrans_app::events::ModelDownloadProgressPayload;
    let payload = ModelDownloadProgressPayload {
        bytes: 104_857_600,
        total: 403_368_390,
        fraction: 0.26,
    };
    let json = serde_json::to_string(&payload).unwrap();
    // Field names stay snake_case, exactly as TASK-10 specifies.
    assert!(json.contains(r#""bytes":104857600"#));
    assert!(json.contains(r#""total":403368390"#));
    assert!(json.contains(r#""fraction":0.26"#));
}

#[test]
fn model_status_report_matches_the_frontend_contract() {
    use vtrans_app::{ModelEntryStatus, ModelState, ModelStatusReport};
    let report = ModelStatusReport {
        entries: vec![
            ModelEntryStatus {
                id: "ppocr-det-v6".to_string(),
                state: ModelState::Ready,
                optional: false,
            },
            ModelEntryStatus {
                id: "opus-mt-en-zh-int8".to_string(),
                state: ModelState::Missing,
                optional: true,
            },
        ],
        ocr_ready: true,
        translation_ready: false,
    };
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains(r#""entries""#));
    assert!(json.contains(r#""id":"ppocr-det-v6""#));
    assert!(json.contains(r#""state":"ready""#));
    assert!(json.contains(r#""state":"missing""#));
    assert!(json.contains(r#""optional":true"#));
    assert!(json.contains(r#""ocr_ready":true"#));
    assert!(json.contains(r#""translation_ready":false"#));
}

#[test]
fn model_state_serializes_to_lowercase_identifiers() {
    use vtrans_app::ModelState;
    assert_eq!(
        serde_json::to_string(&ModelState::Ready).unwrap(),
        r#""ready""#
    );
    assert_eq!(
        serde_json::to_string(&ModelState::Missing).unwrap(),
        r#""missing""#
    );
    assert_eq!(
        serde_json::to_string(&ModelState::Invalid).unwrap(),
        r#""invalid""#
    );
}

#[test]
fn model_download_errors_are_frontend_safe_strings() {
    let error = AppError::ModelDownload("翻译模型下载已在进行中".to_string());
    assert_eq!(
        serde_json::to_string(&error).unwrap(),
        r#""model download error: 翻译模型下载已在进行中""#
    );
    let error = AppError::ModelNotReady("OCR 模型未就位，请重试模型修复".to_string());
    assert_eq!(
        serde_json::to_string(&error).unwrap(),
        r#""model not ready: OCR 模型未就位，请重试模型修复""#
    );
}
