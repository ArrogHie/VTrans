//! Live-region pipeline mode.
//!
//! Continuously captures a fixed screen region and drives three concurrent
//! stages through bounded channels:
//!
//! 1. the **capture loop** captures frames from a
//!    [`vtrans_core::traits::CaptureSession`], skips frames whose pixels
//!    have not changed ([`FrameDiffer`]), and forwards changed frames into a
//!    capacity-1 channel;
//! 2. the **OCR worker** consumes frames, runs at most one OCR pass at a
//!    time (a newer frame cancels the previous pass), normalizes the text,
//!    and forwards non-duplicate text ([`TextDedup`]) into a second
//!    capacity-1 channel;
//! 3. the **translation worker** runs at most one translation at a time and
//!    emits the result.
//!
//! Every stage observes the shared stop token, so [`crate::Pipeline::stop`]
//! terminates the whole pipeline in bounded time. All channels are
//! capacity 1, so queue memory never grows unboundedly: when a channel is
//! full, the newest item is dropped and a debug log records the backpressure
//! event.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn};
use vtrans_core::truncate_for_log;
use vtrans_core::types::{CapturedImage, Language, OcrOptions, PipelineStatus, ScreenRegion};
use vtrans_core::OcrError;

use crate::cancel::TaskSlot;
use crate::dedup::{FrameDiffer, TextDedup};
use crate::{
    image_aligned_region, normalize_result, poison_inner, resolve_effective_source, translate_text,
    FrameSink, PipelineDeps, PipelineError, PipelineEvent, PipelineState,
};

/// Capture intervals below this (in milliseconds) would busy-spin the loop;
/// the configured interval is clamped up to this value.
const MIN_CAPTURE_INTERVAL_MS: u32 = 16;

/// Capacity of the capture -> OCR and OCR -> translation channels. A
/// capacity-1 channel holds at most one pending item, so memory stays
/// bounded under all input rates.
const CHANNEL_CAPACITY: usize = 1;

/// Shared handles handed to every live-mode stage.
#[derive(Clone)]
struct WorkerCtx {
    deps: Arc<PipelineDeps>,
    state: Arc<PipelineState>,
    event_tx: mpsc::Sender<PipelineEvent>,
    stop: CancellationToken,
}

/// A cleaned OCR result handed from the OCR stage to the translation stage.
struct OcrJob {
    text: String,
    source: Language,
    target: Language,
}

/// OCR parameters shared by every job of one worker.
#[derive(Clone)]
struct OcrWorkerParams {
    options: OcrOptions,
    source: Language,
    target: Language,
}

/// Runs the live pipeline until the stop token is cancelled.
#[instrument(skip_all)]
pub(crate) async fn run_live(
    deps: Arc<PipelineDeps>,
    state: Arc<PipelineState>,
    stop: CancellationToken,
    event_tx: mpsc::Sender<PipelineEvent>,
    frame_sink: Option<Arc<dyn FrameSink>>,
) -> Result<(), PipelineError> {
    let config = state.config();
    let region = config.region;
    let interval_ms = clamp_interval_ms(config.capture_interval_ms);
    let threshold = clamp_threshold(config.difference_threshold);

    let (frames_tx, frames_rx) = mpsc::channel(CHANNEL_CAPACITY);
    let (jobs_tx, jobs_rx) = mpsc::channel(CHANNEL_CAPACITY);
    let ctx = WorkerCtx {
        deps,
        state: state.clone(),
        event_tx: event_tx.clone(),
        stop: stop.clone(),
    };
    let ocr_params = OcrWorkerParams {
        options: config.ocr_options,
        source: config.translation_request.source,
        target: config.translation_request.target,
    };

    let ocr_worker = tokio::spawn(ocr_worker(ctx.clone(), frames_rx, jobs_tx, ocr_params));
    let translation_worker = tokio::spawn(translation_worker(ctx.clone(), jobs_rx));

    let capture_result = capture_loop(
        &ctx,
        &frames_tx,
        interval_ms,
        threshold,
        &region,
        frame_sink,
    )
    .await;

    // The capture loop has ended (stop, session end, or failure): signal the
    // workers and wait for them to terminate before returning.
    stop.cancel();
    let _ = ocr_worker.await;
    let _ = translation_worker.await;

    match capture_result {
        Ok(()) => {
            info!("live pipeline stopped");
            state.set_status(PipelineStatus::Idle);
            let _ = event_tx.send(PipelineEvent::Stopped).await;
            Ok(())
        }
        Err(error) => {
            state.set_status(PipelineStatus::Idle);
            Err(error)
        }
    }
}

/// Captures frames from a session, applies frame-difference detection, and
/// forwards changed frames to the OCR stage.
#[instrument(skip_all)]
async fn capture_loop(
    ctx: &WorkerCtx,
    frames_tx: &mpsc::Sender<CapturedImage>,
    interval: Duration,
    threshold: f32,
    region: &ScreenRegion,
    frame_sink: Option<Arc<dyn FrameSink>>,
) -> Result<(), PipelineError> {
    let mut session_region = region.clone();
    let mut session = ctx.deps.capture.start_session(&session_region).await?;
    let mut differ = FrameDiffer::new(threshold);
    loop {
        // Re-session when the region changed since the session started. The
        // pipeline itself is not interrupted; the OCR and translation
        // workers keep running across the restart.
        let current_region = ctx.state.current_region();
        if region_changed(&current_region, &session_region) {
            info!("capture region updated; restarting capture session");
            let _ = session.stop().await;
            session = ctx.deps.capture.start_session(&current_region).await?;
            session_region = current_region;
            differ.reset();
        }

        let frame = tokio::select! {
            biased;
            () = ctx.stop.cancelled() => {
                let _ = session.stop().await;
                return Ok(());
            }
            // Restart the session promptly when the region is updated, even
            // if no new frame has arrived yet.
            () = ctx.state.region_changed.notified() => continue,
            frame = session.next_frame() => frame,
        };

        match frame {
            Ok(Some(image)) => {
                if let Err(error) = image.validate() {
                    warn!(error = %error, "captured frame failed validation; skipping");
                } else {
                    let _ = ctx.event_tx.send(PipelineEvent::CaptureStarted).await;
                    ctx.state.set_status(PipelineStatus::Capturing);
                    if differ.is_changed(&image) {
                        if let Some(sink) = &frame_sink {
                            sink.on_frame(&image);
                        }
                        match frames_tx.try_send(image) {
                            Ok(()) => {}
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                debug!("frame queue full; dropping frame (backpressure)");
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                warn!("frame queue closed; stopping live pipeline");
                                let _ = session.stop().await;
                                return Err(PipelineError::ChannelClosed);
                            }
                        }
                    } else {
                        debug!("frame unchanged; skipping OCR");
                    }
                }
            }
            Ok(None) => {
                warn!("capture session ended");
                let _ = session.stop().await;
                return Ok(());
            }
            Err(error) => {
                warn!(error = %error, "capture failed");
                let _ = session.stop().await;
                return Err(PipelineError::Capture(error));
            }
        }

        tokio::time::sleep(interval).await;
    }
}

/// Consumes frames and runs at most one OCR pass at a time.
///
/// A newer frame supersedes the in-flight pass: [`TaskSlot::replace`]
/// cancels the previous OCR task before starting the next one.
#[instrument(skip_all)]
async fn ocr_worker(
    ctx: WorkerCtx,
    mut frames_rx: mpsc::Receiver<CapturedImage>,
    jobs_tx: mpsc::Sender<OcrJob>,
    params: OcrWorkerParams,
) {
    let dedup = Arc::new(Mutex::new(TextDedup::new()));
    let mut slot: TaskSlot<()> = TaskSlot::new();
    loop {
        let frame = tokio::select! {
            biased;
            () = ctx.stop.cancelled() => break,
            frame = frames_rx.recv() => match frame {
                Some(frame) => frame,
                None => break,
            },
        };
        slot.replace({
            let ctx = ctx.clone();
            let jobs_tx = jobs_tx.clone();
            let dedup = dedup.clone();
            let params = params.clone();
            move |cancel| async move {
                run_ocr_job(ctx, jobs_tx, dedup, frame, params, cancel).await;
            }
        })
        .await;
    }
    slot.cancel_and_join().await;
}

/// OCR stage for one frame: recognize, normalize, deduplicate, and forward
/// non-duplicate text to the translation stage.
#[instrument(skip_all)]
async fn run_ocr_job(
    ctx: WorkerCtx,
    jobs_tx: mpsc::Sender<OcrJob>,
    dedup: Arc<Mutex<TextDedup>>,
    frame: CapturedImage,
    params: OcrWorkerParams,
    cancel: CancellationToken,
) {
    let _ = ctx.event_tx.send(PipelineEvent::OcrStarted).await;
    ctx.state.set_status(PipelineStatus::OcrInProgress);

    let region = ctx.state.current_region();
    let ocr_region = image_aligned_region(&region.monitor_id, &frame);
    let result = match ctx
        .deps
        .ocr
        .recognize(&frame, &ocr_region, &params.options, cancel.clone())
        .await
    {
        Ok(result) => result,
        Err(OcrError::Cancelled) => {
            debug!("OCR cancelled by a newer frame or by stop");
            return;
        }
        Err(error) => {
            warn!(error = %error, "OCR failed");
            let _ = ctx
                .event_tx
                .send(PipelineEvent::Error(PipelineError::Ocr(error)))
                .await;
            return;
        }
    };
    debug!(
        elapsed_ms = result.elapsed_ms,
        line_count = result.lines.len(),
        "OCR pass completed"
    );

    // Resolve the effective translation source per frame (configured
    // language -> OCR detection -> Unicode heuristic -> Auto) so the
    // translated request and the Japanese punctuation normalization agree.
    let source =
        resolve_effective_source(result.detected_language, params.source, &result.merged_text);
    let normalized = normalize_result(result, source);
    let _ = ctx
        .event_tx
        .send(PipelineEvent::OcrCompleted(normalized.clone()))
        .await;
    info!(
        sample = %truncate_for_log(&normalized.merged_text),
        "OCR completed"
    );

    // Skip translation when the text is unchanged (fingerprint dedup) or
    // empty. The fingerprint is recorded for every frame so that content
    // that disappears and reappears is translated again.
    let duplicate = dedup
        .lock()
        .unwrap_or_else(poison_inner)
        .record(&normalized.merged_text);
    if duplicate {
        debug!("text unchanged; skipping translation");
        return;
    }
    if normalized.merged_text.trim().is_empty() {
        debug!("empty text; skipping translation");
        return;
    }

    let job = OcrJob {
        text: normalized.merged_text,
        source,
        target: params.target,
    };
    match jobs_tx.try_send(job) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            debug!("translation queue full; dropping stale job");
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            warn!("translation worker is gone; dropping job");
        }
    }
}

/// Consumes OCR jobs and runs at most one translation at a time.
#[instrument(skip_all)]
async fn translation_worker(ctx: WorkerCtx, mut jobs_rx: mpsc::Receiver<OcrJob>) {
    let mut slot: TaskSlot<()> = TaskSlot::new();
    loop {
        let job = tokio::select! {
            biased;
            () = ctx.stop.cancelled() => break,
            job = jobs_rx.recv() => match job {
                Some(job) => job,
                None => break,
            },
        };
        slot.replace({
            let ctx = ctx.clone();
            move |cancel| async move {
                run_translation_job(ctx, job, cancel).await;
            }
        })
        .await;
    }
    slot.cancel_and_join().await;
}

/// Translation stage for one OCR job.
#[instrument(skip_all)]
async fn run_translation_job(ctx: WorkerCtx, job: OcrJob, cancel: CancellationToken) {
    let _ = ctx.event_tx.send(PipelineEvent::TranslationStarted).await;
    ctx.state.set_status(PipelineStatus::Translating);

    let result = tokio::select! {
        biased;
        () = ctx.stop.cancelled() => {
            debug!("translation cancelled by stop");
            return;
        }
        () = cancel.cancelled() => {
            debug!("translation superseded by a newer job");
            return;
        }
        result = translate_text(&ctx.deps, &job.text, job.source, job.target, cancel.clone()) => {
            result
        }
    };

    match result {
        Ok(translation) => {
            info!(
                elapsed_ms = translation.elapsed_ms,
                provider = %translation.provider_id,
                "translation completed"
            );
            let _ = ctx
                .event_tx
                .send(PipelineEvent::TranslationCompleted(translation))
                .await;
        }
        Err(vtrans_core::TranslationError::Cancelled) => {
            debug!("translation cancelled by the provider");
        }
        Err(error) => {
            warn!(error = %error, "translation failed");
            let _ = ctx
                .event_tx
                .send(PipelineEvent::Error(PipelineError::Translation(error)))
                .await;
        }
    }
}

/// Clamps the capture interval to avoid busy-looping, logging when the
/// configured value was below the minimum.
fn clamp_interval_ms(interval_ms: u32) -> Duration {
    let clamped = interval_ms.max(MIN_CAPTURE_INTERVAL_MS);
    if clamped != interval_ms {
        warn!(
            requested_ms = interval_ms,
            min_ms = MIN_CAPTURE_INTERVAL_MS,
            "capture interval below minimum; clamping"
        );
    }
    Duration::from_millis(u64::from(clamped))
}

/// Clamps the difference threshold into `0.0..=1.0`, logging when the
/// configured value was out of range.
fn clamp_threshold(threshold: f32) -> f32 {
    if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
        warn!(
            requested = threshold,
            "difference threshold out of 0..=1 range; clamping"
        );
    }
    // `f32::clamp` returns NaN for NaN input; fall back to 0.0 instead so a
    // NaN threshold cannot silently disable frame-difference detection.
    if threshold.is_nan() {
        0.0
    } else {
        threshold.clamp(0.0, 1.0)
    }
}

/// Returns `true` when two regions differ in any field.
fn region_changed(a: &ScreenRegion, b: &ScreenRegion) -> bool {
    a.monitor_id != b.monitor_id
        || a.x != b.x
        || a.y != b.y
        || a.width != b.width
        || a.height != b.height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_interval_raises_below_minimum() {
        assert_eq!(clamp_interval_ms(0), Duration::from_millis(16));
        assert_eq!(clamp_interval_ms(10), Duration::from_millis(16));
        assert_eq!(clamp_interval_ms(250), Duration::from_millis(250));
    }

    #[test]
    fn clamp_threshold_bounds_range() {
        assert!(clamp_threshold(-0.5).abs() < f32::EPSILON);
        assert!((clamp_threshold(1.5) - 1.0).abs() < f32::EPSILON);
        assert!((clamp_threshold(0.02) - 0.02).abs() < f32::EPSILON);
        assert!(clamp_threshold(f32::NAN).abs() < f32::EPSILON);
    }

    #[test]
    fn region_changed_detects_any_field_difference() {
        let a = ScreenRegion::new("m0", 0, 0, 100, 100);
        let same = ScreenRegion::new("m0", 0, 0, 100, 100);
        assert!(!region_changed(&a, &same));
        assert!(region_changed(&a, &ScreenRegion::new("m1", 0, 0, 100, 100)));
        assert!(region_changed(&a, &ScreenRegion::new("m0", 1, 0, 100, 100)));
        assert!(region_changed(&a, &ScreenRegion::new("m0", 0, 0, 200, 100)));
    }

    #[tokio::test]
    async fn stage_channel_capacity_1_never_holds_more_than_one_frame() {
        // The live stages communicate through `mpsc` channels of capacity 1:
        // a full channel rejects the newest item instead of growing. This
        // keeps queue memory bounded under all input rates.
        let (tx, mut rx) = mpsc::channel::<u32>(CHANNEL_CAPACITY);
        assert!(tx.try_send(1).is_ok());
        assert!(matches!(
            tx.try_send(2),
            Err(mpsc::error::TrySendError::Full(2))
        ));
        assert_eq!(rx.recv().await, Some(1));
        assert!(tx.try_send(3).is_ok());
        assert_eq!(rx.recv().await, Some(3));
    }
}
