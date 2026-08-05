//! Conversion of pipeline events into frontend events.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use vtrans_pipeline::{PipelineError, PipelineEvent};

/// Stable event names consumed by the frontend.
pub const CAPTURE_STATUS_CHANGED: &str = "capture_status_changed";
pub const OCR_STARTED: &str = "ocr_started";
pub const OCR_COMPLETED: &str = "ocr_completed";
pub const TRANSLATION_STARTED: &str = "translation_started";
pub const TRANSLATION_COMPLETED: &str = "translation_completed";
pub const PIPELINE_ERROR: &str = "pipeline_error";
pub const LIVE_SESSION_STOPPED: &str = "live_session_stopped";
pub const MODEL_LOADING_PROGRESS: &str = "model_loading_progress";
pub const REGION_SELECTED: &str = "region_selected";
pub const OVERLAY_REGION_UPDATED: &str = "overlay_region_updated";
pub const OVERLAY_HIDDEN: &str = "overlay_hidden";
pub const DEBUG_FRAME_UPDATED: &str = "debug_frame_updated";

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
}
