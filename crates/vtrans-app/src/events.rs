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
}
