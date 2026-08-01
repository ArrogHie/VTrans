//! Single-capture pipeline mode.
//!
//! Runs one capture -> OCR -> normalize -> translate pass and reports each
//! stage through [`PipelineEvent`](crate::PipelineEvent). The entry point
//! [`run_single_capture`] is a crate-level convenience that wraps a
//! [`Pipeline`](crate::Pipeline) in single mode; [`Pipeline::run`] uses
//! [`run_single_capture_internal`] internally.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn};
use vtrans_core::truncate_for_log;
use vtrans_core::types::{PipelineMode, PipelineStatus};

use crate::{
    image_aligned_region, normalize_result, translate_text, PipelineConfig, PipelineDeps,
    PipelineError, PipelineEvent, PipelineState,
};

/// Runs a single capture -> OCR -> translate pass.
///
/// This convenience entry point builds a [`Pipeline`](crate::Pipeline) from
/// `deps` and `config` and runs it in single mode; `config.mode` is ignored
/// and forced to [`PipelineMode::SingleCapture`]. Stage events are emitted
/// into `event_tx`, ending with [`PipelineEvent::TranslationCompleted`] and
/// [`PipelineEvent::Stopped`].
///
/// # Errors
///
/// Returns a [`PipelineError`] when capture, OCR, or translation fails, when
/// `event_tx` has no receivers, or when the run is cancelled.
#[instrument(skip_all)]
pub async fn run_single_capture(
    deps: PipelineDeps,
    config: PipelineConfig,
    event_tx: mpsc::Sender<PipelineEvent>,
) -> Result<(), PipelineError> {
    let config = PipelineConfig {
        mode: PipelineMode::SingleCapture,
        ..config
    };
    let pipeline = crate::Pipeline::new(config, deps);
    pipeline.run(event_tx).await
}

/// Internal single-mode orchestration, shared with [`Pipeline::run`].
#[instrument(skip_all)]
pub(crate) async fn run_single_capture_internal(
    deps: Arc<PipelineDeps>,
    state: Arc<PipelineState>,
    config: PipelineConfig,
    stop: CancellationToken,
    event_tx: &mpsc::Sender<PipelineEvent>,
) -> Result<(), PipelineError> {
    let region = config.region;
    let options = config.ocr_options;
    let request = config.translation_request;

    // 1. Capture.
    let _ = event_tx.send(PipelineEvent::CaptureStarted).await;
    state.set_status(PipelineStatus::Capturing);
    let image = tokio::select! {
        biased;
        () = stop.cancelled() => return cancelled(event_tx, &state).await,
        result = deps.capture.capture_once(&region) => result.map_err(PipelineError::from)?,
    };
    debug!(width = image.width, height = image.height, "captured frame");

    // 2. OCR.
    let _ = event_tx.send(PipelineEvent::OcrStarted).await;
    state.set_status(PipelineStatus::OcrInProgress);
    let ocr_region = image_aligned_region(&region.monitor_id, &image);
    let result = tokio::select! {
        biased;
        () = stop.cancelled() => return cancelled(event_tx, &state).await,
        result = deps.ocr.recognize(&image, &ocr_region, &options, stop.clone()) => {
            result.map_err(PipelineError::from)?
        }
    };
    debug!(
        elapsed_ms = result.elapsed_ms,
        line_count = result.lines.len(),
        "OCR pass completed"
    );

    let normalized = normalize_result(result, request.source);
    let _ = event_tx
        .send(PipelineEvent::OcrCompleted(normalized.clone()))
        .await;
    info!(
        sample = %truncate_for_log(&normalized.merged_text),
        "OCR completed"
    );

    if normalized.merged_text.trim().is_empty() {
        info!("OCR returned no text; skipping translation");
        state.set_status(PipelineStatus::Completed);
        let _ = event_tx.send(PipelineEvent::Stopped).await;
        return Ok(());
    }

    // 3. Translation.
    let _ = event_tx.send(PipelineEvent::TranslationStarted).await;
    state.set_status(PipelineStatus::Translating);
    let translation = tokio::select! {
        biased;
        () = stop.cancelled() => return cancelled(event_tx, &state).await,
        result = translate_text(
            &deps,
            &normalized.merged_text,
            request.source,
            request.target,
            stop.clone(),
        ) => result.map_err(PipelineError::from)?,
    };
    info!(
        elapsed_ms = translation.elapsed_ms,
        provider = %translation.provider_id,
        "translation completed"
    );
    let _ = event_tx
        .send(PipelineEvent::TranslationCompleted(translation))
        .await;

    state.set_status(PipelineStatus::Completed);
    let _ = event_tx.send(PipelineEvent::Stopped).await;
    Ok(())
}

/// Reports a user-initiated cancellation: emits `Stopped` and returns
/// [`PipelineError::Cancelled`].
async fn cancelled(
    event_tx: &mpsc::Sender<PipelineEvent>,
    state: &PipelineState,
) -> Result<(), PipelineError> {
    warn!("single capture cancelled");
    state.set_status(PipelineStatus::Idle);
    let _ = event_tx.send(PipelineEvent::Stopped).await;
    Err(PipelineError::Cancelled)
}
