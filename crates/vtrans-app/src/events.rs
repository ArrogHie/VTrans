//! Conversion of pipeline events into frontend events.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use vtrans_core::truncate_for_log;
use vtrans_pipeline::{BoxStatus, BoxedTranslationResult, PipelineError, PipelineEvent};

/// Stable event names consumed by the frontend.
pub const CAPTURE_STATUS_CHANGED: &str = "capture_status_changed";
pub const OCR_STARTED: &str = "ocr_started";
pub const OCR_COMPLETED: &str = "ocr_completed";
pub const TRANSLATION_STARTED: &str = "translation_started";
pub const TRANSLATION_COMPLETED: &str = "translation_completed";
pub const PIPELINE_ERROR: &str = "pipeline_error";
pub const LIVE_SESSION_STOPPED: &str = "live_session_stopped";
pub const MODEL_LOADING_PROGRESS: &str = "model_loading_progress";
pub const MODEL_DOWNLOAD_PROGRESS: &str = "model_download_progress";
pub const REGION_SELECTED: &str = "region_selected";
pub const OVERLAY_REGION_UPDATED: &str = "overlay_region_updated";
pub const OVERLAY_HIDDEN: &str = "overlay_hidden";
pub const DEBUG_FRAME_UPDATED: &str = "debug_frame_updated";

/// Stable multi-box event names consumed by the frontend.
pub const MULTIBOX_RESULT: &str = "multibox://result";
pub const MULTIBOX_BOX_ADDED: &str = "multibox://box-added";
pub const MULTIBOX_BOX_REMOVED: &str = "multibox://box-removed";
pub const MULTIBOX_BOX_UPDATED: &str = "multibox://box-updated";
pub const MULTIBOX_STATUS: &str = "multibox://status";
pub const MULTIBOX_WARNING: &str = "multibox://warning";
pub const TRANSLATION_SINGLE_RESULT: &str = "translation://single-result";

#[derive(Debug, Serialize)]
struct StatusPayload<'a> {
    status: &'a str,
}

#[derive(Debug, Serialize)]
struct TimestampPayload {
    timestamp: u64,
}

#[derive(Debug, Serialize)]
struct ResultPayload<T> {
    result: T,
}

#[derive(Debug, Serialize)]
struct PipelineErrorPayload {
    message: String,
    recoverable: bool,
}

#[derive(Debug, Serialize)]
struct StoppedPayload<'a> {
    reason: &'a str,
}

#[derive(Debug, Clone, Serialize)]
struct ModelProgressPayload<'a> {
    model_id: &'a str,
    progress: f32,
}

/// Payload of the Debug-only `debug_frame_updated` event.
///
/// `image` is the Base64-encoded JPEG thumbnail (longest edge ≤ 480 px) of
/// the frame that entered OCR. This event exists only while Debug mode is
/// enabled and is display-only: the frontend never persists it.
#[derive(Debug, Clone, Serialize)]
pub struct DebugFramePayload {
    /// Base64-encoded JPEG thumbnail bytes.
    pub image: String,
    /// Screen region the frame was captured from.
    pub region: vtrans_core::ScreenRegion,
    /// Monotonically increasing frame sequence number (wraps on overflow).
    pub frame_index: u64,
    /// Capture timestamp in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
}

/// Maps a pipeline event to its stable event name and JSON payload.
///
/// The payload contains OCR and translation results but never image bytes.
/// The mapping is kept separate from emission so the frontend contract can
/// be asserted in unit tests without a running Tauri application.
fn event_name_and_payload(
    event: PipelineEvent,
) -> (&'static str, Result<serde_json::Value, serde_json::Error>) {
    match event {
        PipelineEvent::CaptureStarted => (
            CAPTURE_STATUS_CHANGED,
            serde_json::to_value(StatusPayload {
                status: "capturing",
            }),
        ),
        PipelineEvent::OcrStarted => (
            OCR_STARTED,
            serde_json::to_value(TimestampPayload {
                timestamp: unix_timestamp_ms(),
            }),
        ),
        PipelineEvent::OcrCompleted(result) => (
            OCR_COMPLETED,
            serde_json::to_value(ResultPayload { result }),
        ),
        PipelineEvent::TranslationStarted => (
            TRANSLATION_STARTED,
            serde_json::to_value(TimestampPayload {
                timestamp: unix_timestamp_ms(),
            }),
        ),
        PipelineEvent::TranslationCompleted(result) => (
            TRANSLATION_COMPLETED,
            serde_json::to_value(ResultPayload { result }),
        ),
        PipelineEvent::Error(error) => (
            PIPELINE_ERROR,
            serde_json::to_value(PipelineErrorPayload {
                message: error.to_string(),
                recoverable: is_recoverable(&error),
            }),
        ),
        PipelineEvent::Stopped => (
            LIVE_SESSION_STOPPED,
            serde_json::to_value(StoppedPayload { reason: "stopped" }),
        ),
    }
}

/// Emits one pipeline event using the stable frontend event contract.
///
/// Any emission failure is logged; pipeline execution must not be rolled back
/// merely because a frontend window was closed.
#[tracing::instrument(skip(app, event))]
pub fn emit_pipeline_event<R: Runtime>(app: &AppHandle<R>, event: PipelineEvent) {
    let (name, payload) = event_name_and_payload(event);

    match payload {
        Ok(payload) => {
            if let Err(error) = app.emit(name, payload) {
                tracing::warn!(event = name, error = %error, "failed to emit frontend event");
            }
        }
        Err(error) => {
            tracing::error!(event = name, error = %error, "failed to serialize frontend event");
        }
    }
}

/// Emits model loading progress using the dedicated event contract.
#[tracing::instrument(skip(app), fields(model_id = model_id))]
pub fn emit_model_loading_progress<R: Runtime>(app: &AppHandle<R>, model_id: &str, progress: f32) {
    let payload = ModelProgressPayload { model_id, progress };
    if let Err(error) = app.emit(MODEL_LOADING_PROGRESS, payload) {
        tracing::warn!(error = %error, "failed to emit model loading progress");
    }
}

/// Payload of the `model_download_progress` event.
///
/// Field names stay `snake_case` to match the frontend contract exactly;
/// `fraction` is `bytes / total` clamped to `[0.0, 1.0]` (0.0 while the
/// total is unknown).
#[derive(Debug, Clone, Serialize)]
pub struct ModelDownloadProgressPayload {
    /// Bytes received so far (including the resumed prefix).
    pub bytes: u64,
    /// Total download size in bytes, `0` while unknown.
    pub total: u64,
    /// Download progress in `[0.0, 1.0]`.
    pub fraction: f32,
}

/// Emits translation model download progress to the frontend.
///
/// Emission failures are logged but never propagated: a closed settings
/// panel must not fail the download.
#[tracing::instrument(skip(app), fields(bytes, total))]
pub fn emit_model_download_progress<R: Runtime>(
    app: &AppHandle<R>,
    bytes: u64,
    total: u64,
    fraction: f32,
) {
    let payload = ModelDownloadProgressPayload {
        bytes,
        total,
        fraction,
    };
    if let Err(error) = app.emit(MODEL_DOWNLOAD_PROGRESS, payload) {
        tracing::warn!(error = %error, "failed to emit model download progress");
    }
}

/// Emits the currently selected region to the persistent overlay window.
///
/// The payload is the region in physical pixels relative to its monitor; the
/// overlay webview converts it to CSS pixels for drawing. No image data is
/// ever included.
#[tracing::instrument(skip(app), fields(monitor_id = %region.monitor_id))]
pub fn emit_overlay_region<R: Runtime>(app: &AppHandle<R>, region: &vtrans_core::ScreenRegion) {
    if let Err(error) = app.emit(OVERLAY_REGION_UPDATED, region) {
        tracing::warn!(
            event = OVERLAY_REGION_UPDATED,
            error = %error,
            "failed to emit overlay region event"
        );
    }
}

/// Tells the persistent overlay window to hide its region marker.
#[tracing::instrument(skip(app))]
pub fn emit_overlay_hidden<R: Runtime>(app: &AppHandle<R>) {
    if let Err(error) = app.emit(OVERLAY_HIDDEN, ()) {
        tracing::warn!(
            event = OVERLAY_HIDDEN,
            error = %error,
            "failed to emit overlay hidden event"
        );
    }
}

/// Emits one debug thumbnail to the frontend debug panel.
///
/// Emission failures are logged but never propagated: a closed debug panel
/// must not affect the capture pipeline.
#[tracing::instrument(skip(app), fields(frame_index = payload.frame_index))]
pub fn emit_debug_frame<R: Runtime>(app: &AppHandle<R>, payload: DebugFramePayload) {
    if let Err(error) = app.emit(DEBUG_FRAME_UPDATED, payload) {
        tracing::warn!(
            event = DEBUG_FRAME_UPDATED,
            error = %error,
            "failed to emit debug frame"
        );
    }
}

/// Payload of `multibox://box-added`.
#[derive(Debug, Clone, Serialize)]
struct BoxAddedPayload {
    box_id: u32,
    color: String,
    region: vtrans_core::ScreenRegion,
}

/// Payload of `multibox://box-removed`.
#[derive(Debug, Clone, Serialize)]
struct BoxRemovedPayload {
    box_id: u32,
}

/// Payload of `multibox://box-updated`.
#[derive(Debug, Clone, Serialize)]
struct BoxUpdatedPayload {
    box_id: u32,
    region: vtrans_core::ScreenRegion,
}

/// Payload of `multibox://status`.
#[derive(Debug, Clone, Serialize)]
struct BoxStatusPayload<'a> {
    box_id: u32,
    status: &'a BoxStatus,
}

/// Payload of `multibox://warning`.
#[derive(Debug, Clone, Serialize)]
struct WarningPayload {
    current_count: u32,
    max_count: u32,
}

/// Payload of `translation://single-result`.
///
/// Carries the original OCR text and its translation so the result window
/// can display both without re-deriving them from separate pipeline events.
/// Text is emitted to the frontend (which needs it) but only appears in
/// logs in truncated form.
#[derive(Debug, Clone, Serialize)]
pub struct SingleResultPayload {
    /// Original OCR-recognized text.
    pub original_text: String,
    /// Translated text from the provider.
    pub translated_text: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
}

/// Emits a multi-box translation result to the frontend.
///
/// The payload is the [`BoxedTranslationResult`] which carries `box_id`,
/// `color`, the translation result, and a timestamp. Only the `box_id` is
/// logged; the translated text is never logged at this layer.
#[tracing::instrument(skip(app, result), fields(box_id = result.box_id))]
pub fn emit_multibox_result<R: Runtime>(app: &AppHandle<R>, result: &BoxedTranslationResult) {
    tracing::debug!(
        event = MULTIBOX_RESULT,
        box_id = result.box_id,
        "emitting multi-box result"
    );
    if let Err(error) = app.emit(MULTIBOX_RESULT, result) {
        tracing::warn!(
            event = MULTIBOX_RESULT,
            error = %error,
            "failed to emit multi-box result"
        );
    }
}

/// Emits a `multibox://box-added` notification.
#[tracing::instrument(skip(app), fields(box_id))]
pub fn emit_multibox_box_added<R: Runtime>(
    app: &AppHandle<R>,
    box_id: u32,
    color: &str,
    region: &vtrans_core::ScreenRegion,
) {
    let payload = BoxAddedPayload {
        box_id,
        color: color.to_string(),
        region: region.clone(),
    };
    tracing::debug!(event = MULTIBOX_BOX_ADDED, box_id, "emitting box-added");
    if let Err(error) = app.emit(MULTIBOX_BOX_ADDED, payload) {
        tracing::warn!(
            event = MULTIBOX_BOX_ADDED,
            error = %error,
            "failed to emit box-added event"
        );
    }
}

/// Emits a `multibox://box-removed` notification.
#[tracing::instrument(skip(app), fields(box_id))]
pub fn emit_multibox_box_removed<R: Runtime>(app: &AppHandle<R>, box_id: u32) {
    let payload = BoxRemovedPayload { box_id };
    tracing::debug!(event = MULTIBOX_BOX_REMOVED, box_id, "emitting box-removed");
    if let Err(error) = app.emit(MULTIBOX_BOX_REMOVED, payload) {
        tracing::warn!(
            event = MULTIBOX_BOX_REMOVED,
            error = %error,
            "failed to emit box-removed event"
        );
    }
}

/// Emits a `multibox://box-updated` notification.
#[tracing::instrument(skip(app), fields(box_id))]
pub fn emit_multibox_box_updated<R: Runtime>(
    app: &AppHandle<R>,
    box_id: u32,
    region: &vtrans_core::ScreenRegion,
) {
    let payload = BoxUpdatedPayload {
        box_id,
        region: region.clone(),
    };
    tracing::debug!(event = MULTIBOX_BOX_UPDATED, box_id, "emitting box-updated");
    if let Err(error) = app.emit(MULTIBOX_BOX_UPDATED, payload) {
        tracing::warn!(
            event = MULTIBOX_BOX_UPDATED,
            error = %error,
            "failed to emit box-updated event"
        );
    }
}

/// Emits a `multibox://status` notification.
#[tracing::instrument(skip(app, status), fields(box_id))]
pub fn emit_multibox_status<R: Runtime>(app: &AppHandle<R>, box_id: u32, status: &BoxStatus) {
    let payload = BoxStatusPayload { box_id, status };
    tracing::debug!(
        event = MULTIBOX_STATUS,
        box_id,
        ?status,
        "emitting box status"
    );
    if let Err(error) = app.emit(MULTIBOX_STATUS, payload) {
        tracing::warn!(
            event = MULTIBOX_STATUS,
            error = %error,
            "failed to emit box status event"
        );
    }
}

/// Emits a `multibox://warning` notification when the box count reaches the
/// warning threshold.
#[tracing::instrument(skip(app))]
pub fn emit_multibox_warning<R: Runtime>(app: &AppHandle<R>, current_count: u32, max_count: u32) {
    let payload = WarningPayload {
        current_count,
        max_count,
    };
    tracing::debug!(
        event = MULTIBOX_WARNING,
        current_count,
        max_count,
        "emitting box count warning"
    );
    if let Err(error) = app.emit(MULTIBOX_WARNING, payload) {
        tracing::warn!(
            event = MULTIBOX_WARNING,
            error = %error,
            "failed to emit box count warning"
        );
    }
}

/// Emits a single-capture translation result to the result window.
///
/// This replaces showing the result on the main page: single-capture
/// results now go to the result window via this event. The original and
/// translated text are emitted to the frontend (which needs them) but
/// only appear in logs in truncated form.
#[tracing::instrument(skip(app, original_text, translated_text))]
pub fn emit_translation_single_result<R: Runtime>(
    app: &AppHandle<R>,
    original_text: &str,
    translated_text: &str,
) {
    tracing::debug!(
        event = TRANSLATION_SINGLE_RESULT,
        original = %truncate_for_log(original_text),
        translated = %truncate_for_log(translated_text),
        "emitting single translation result"
    );
    let payload = SingleResultPayload {
        original_text: original_text.to_string(),
        translated_text: translated_text.to_string(),
        timestamp: unix_timestamp_ms(),
    };
    if let Err(error) = app.emit(TRANSLATION_SINGLE_RESULT, payload) {
        tracing::warn!(
            event = TRANSLATION_SINGLE_RESULT,
            error = %error,
            "failed to emit single translation result"
        );
    }
}

fn is_recoverable(error: &PipelineError) -> bool {
    matches!(error, PipelineError::Ocr(_) | PipelineError::Translation(_))
}

fn unix_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().try_into().unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtrans_core::{Language, OcrResult, TranslationError, TranslationResult};

    #[test]
    fn only_stage_failures_are_recoverable() {
        assert!(is_recoverable(&PipelineError::Ocr(
            vtrans_core::OcrError::Cancelled
        )));
        assert!(is_recoverable(&PipelineError::Translation(
            TranslationError::Cancelled
        )));
        assert!(!is_recoverable(&PipelineError::Cancelled));
        assert!(!is_recoverable(&PipelineError::Capture(
            vtrans_core::CaptureError::MonitorNotFound("x".into())
        )));
        let _ = Language::English;
    }

    #[test]
    fn stage_events_use_stable_names_and_shapes() {
        let (name, payload) = event_name_and_payload(PipelineEvent::CaptureStarted);
        assert_eq!(name, CAPTURE_STATUS_CHANGED);
        assert_eq!(payload.unwrap(), serde_json::json!({"status": "capturing"}));

        let (name, payload) = event_name_and_payload(PipelineEvent::OcrStarted);
        assert_eq!(name, OCR_STARTED);
        assert!(payload.unwrap().get("timestamp").is_some());

        let (name, payload) = event_name_and_payload(PipelineEvent::TranslationStarted);
        assert_eq!(name, TRANSLATION_STARTED);
        assert!(payload.unwrap().get("timestamp").is_some());
    }

    #[test]
    fn completed_events_wrap_standard_results() {
        let (name, payload) =
            event_name_and_payload(PipelineEvent::OcrCompleted(OcrResult::empty()));
        assert_eq!(name, OCR_COMPLETED);
        assert!(payload.unwrap().get("result").is_some());

        let (name, payload) = event_name_and_payload(PipelineEvent::TranslationCompleted(
            TranslationResult::new("hola", "mock", 4),
        ));
        assert_eq!(name, TRANSLATION_COMPLETED);
        let value = payload.unwrap();
        assert_eq!(value["result"]["translated_text"], "hola");
    }

    #[test]
    fn error_and_stop_events_carry_metadata() {
        let (name, payload) =
            event_name_and_payload(PipelineEvent::Error(PipelineError::Cancelled));
        assert_eq!(name, PIPELINE_ERROR);
        let value = payload.unwrap();
        assert!(value.get("message").is_some());
        assert_eq!(value["recoverable"], false);

        let (name, payload) = event_name_and_payload(PipelineEvent::Stopped);
        assert_eq!(name, LIVE_SESSION_STOPPED);
        assert_eq!(payload.unwrap()["reason"], "stopped");
    }

    #[test]
    fn overlay_events_use_stable_names() {
        assert_eq!(OVERLAY_REGION_UPDATED, "overlay_region_updated");
        assert_eq!(OVERLAY_HIDDEN, "overlay_hidden");
    }

    #[test]
    fn model_download_progress_event_name_is_stable() {
        assert_eq!(MODEL_DOWNLOAD_PROGRESS, "model_download_progress");
    }

    #[test]
    fn model_download_progress_payload_serializes_with_snake_case_fields() {
        let payload = ModelDownloadProgressPayload {
            bytes: 52_428_800,
            total: 403_368_390,
            fraction: 0.13,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains(r#""bytes":52428800"#));
        assert!(json.contains(r#""total":403368390"#));
        assert!(json.contains(r#""fraction":0.13"#));
        assert!(!json.contains("bytesReceived"));
        assert!(!json.contains("totalBytes"));
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
    fn single_result_payload_serializes_with_frontend_field_names() {
        let payload = SingleResultPayload {
            original_text: "hello".to_string(),
            translated_text: "hola".to_string(),
            timestamp: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains(r#""original_text":"hello""#));
        assert!(json.contains(r#""translated_text":"hola""#));
        assert!(json.contains(r#""timestamp":1700000000000"#));
    }
}
