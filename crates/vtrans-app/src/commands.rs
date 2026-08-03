//! Tauri command handlers for the `VTrans` frontend.

use std::future::Future;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use vtrans_core::{Language, OcrResult, PipelineMode, ScreenRegion};
use vtrans_pipeline::{PipelineError, PipelineEvent};

use crate::error::AppError;
use crate::events::{emit_model_loading_progress, emit_pipeline_event};
use crate::state::AppState;
use crate::state::AppStatus;

/// Input accepted by `start_live_translation`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTranslationConfig {
    /// Region to capture continuously.
    pub region: ScreenRegion,
    /// Capture interval in milliseconds.
    #[serde(default = "default_capture_interval_ms")]
    pub capture_interval_ms: u32,
    /// Pixel difference threshold that triggers OCR.
    #[serde(default = "default_difference_threshold")]
    pub difference_threshold: f32,
}

fn default_capture_interval_ms() -> u32 {
    500
}

fn default_difference_threshold() -> f32 {
    0.03
}

/// Opens the selector window and waits for the frontend to confirm a region.
///
/// The frontend completes the pending request by calling `update_live_region`.
///
/// # Errors
///
/// Returns an application error when the selector is unavailable, another
/// selection is pending, or the frontend closes the selection request.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn start_region_selection(state: State<'_, AppState>) -> Result<ScreenRegion, AppError> {
    let app = state.app_handle()?;
    select_region(app, state.inner()).await
}

pub(crate) async fn select_region(
    app: AppHandle,
    state: &AppState,
) -> Result<ScreenRegion, AppError> {
    let receiver = state.begin_region_selection().await?;
    let Some(window) = app.get_webview_window("selector") else {
        state.cancel_region_selection().await;
        tracing::warn!("selector window is not configured");
        return Err(AppError::NotInitialized);
    };
    if let Err(error) = window.show().and_then(|()| window.set_focus()) {
        state.cancel_region_selection().await;
        return Err(AppError::Tauri(error.to_string()));
    }
    match timeout(Duration::from_secs(300), receiver).await {
        Ok(Ok(region)) => {
            let _ = window.hide();
            Ok(region)
        }
        Ok(Err(_)) => Err(AppError::NotInitialized),
        Err(_) => {
            state.cancel_region_selection().await;
            let _ = window.hide();
            Err(AppError::Tauri("region selection timed out".to_string()))
        }
    }
}

/// Cancels the pending region selection and hides the selector window.
///
/// # Errors
///
/// Returns an application error when the selector window cannot be accessed.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn cancel_region_selection(state: State<'_, AppState>) -> Result<(), AppError> {
    state.cancel_region_selection().await;
    let app = state.app_handle()?;
    if let Some(window) = app.get_webview_window("selector") {
        window
            .hide()
            .map_err(|error| AppError::Tauri(error.to_string()))?;
    }
    Ok(())
}

/// Runs one capture, OCR, and translation pipeline pass and returns OCR text.
///
/// Stage events (`ocr_started`, `translation_started`, ...) are forwarded to
/// the frontend while the pipeline runs so single captures can show progress;
/// the command still returns the final [`OcrResult`] as its only payload.
///
/// # Errors
///
/// Returns an application error when capture, OCR, translation, or pipeline
/// execution fails.
#[tauri::command]
#[tracing::instrument(skip(state, region))]
pub async fn capture_once(
    region: ScreenRegion,
    state: State<'_, AppState>,
) -> Result<OcrResult, AppError> {
    let app = state.app_handle()?;
    let pipeline = state.build_pipeline(PipelineMode::SingleCapture, region, 0, 0.03)?;
    let (event_tx, event_rx) = mpsc::channel(16);
    let ocr_result = run_capture_pipeline(
        |event| emit_pipeline_event(&app, event),
        pipeline.run(event_tx),
        event_rx,
    )
    .await?;
    ocr_result.ok_or(AppError::NotInitialized)
}

/// Drives a single-capture pipeline run while forwarding stage events.
///
/// Events are consumed concurrently with the run so that `ocr_started` and
/// `translation_started` reach the frontend before the command returns.
/// `Stopped` is intentionally not forwarded: a single capture has no live
/// session, and `live_session_stopped` would be misleading.
///
/// Returns the final `OcrCompleted` payload, if one was produced, or the
/// pipeline error that terminated the run. The `run` future must send all of
/// its events before completing; the final drain is non-blocking so it can
/// never wait on a channel sender that outlives the run.
async fn run_capture_pipeline<E, Fut>(
    mut emit: E,
    run: Fut,
    mut event_rx: mpsc::Receiver<PipelineEvent>,
) -> Result<Option<OcrResult>, PipelineError>
where
    E: FnMut(PipelineEvent),
    Fut: Future<Output = Result<(), PipelineError>>,
{
    tokio::pin!(run);

    let mut ocr_result = None;
    let mut run_result = None;
    loop {
        tokio::select! {
            result = &mut run => {
                run_result = Some(result);
                break;
            }
            event = event_rx.recv() => {
                match event {
                    Some(PipelineEvent::OcrCompleted(result)) => ocr_result = Some(result),
                    Some(PipelineEvent::Stopped) => {}
                    Some(event) => emit(event),
                    None => break,
                }
            }
        }
    }

    // Collect events still buffered when the run future finished. A single
    // capture sends every event before it returns, so a non-blocking drain
    // sees the tail of the buffer and cannot hang.
    while let Ok(event) = event_rx.try_recv() {
        match event {
            PipelineEvent::OcrCompleted(result) => ocr_result = Some(result),
            PipelineEvent::Stopped => {}
            event => emit(event),
        }
    }

    let run_result = match run_result {
        Some(result) => result,
        None => run.await,
    };
    run_result.map(|()| ocr_result)
}

/// Starts a live capture/OCR/translation task and returns immediately.
///
/// # Errors
///
/// Returns an application error when the region or providers are invalid, or
/// when another live task is already running.
#[tauri::command]
#[tracing::instrument(skip(state, config))]
pub async fn start_live_translation(
    config: LiveTranslationConfig,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let app = state.app_handle()?;
    start_live_task(app, state.inner(), config).await
}

/// Shared live task starter used by both commands and global shortcuts.
pub(crate) async fn start_live_task(
    app: AppHandle,
    state: &AppState,
    config: LiveTranslationConfig,
) -> Result<(), AppError> {
    let _lifecycle = state.live_lifecycle.lock().await;
    if state.live_task_is_running().await {
        return Err(PipelineError::AlreadyRunning.into());
    }
    state.set_selected_region(config.region.clone()).await?;
    let pipeline = state.build_pipeline(
        PipelineMode::LiveRegion,
        config.region,
        config.capture_interval_ms,
        config.difference_threshold,
    )?;
    let pipeline = state.set_pipeline(pipeline);
    let (event_tx, event_rx) = mpsc::channel(32);
    let task = tokio::spawn(run_live_task(app, pipeline, event_tx, event_rx));
    *state.live_task.lock().await = Some(task);
    tracing::info!("live translation started");
    Ok(())
}

async fn run_live_task(
    app: AppHandle,
    pipeline: std::sync::Arc<vtrans_pipeline::Pipeline>,
    event_tx: mpsc::Sender<PipelineEvent>,
    mut event_rx: mpsc::Receiver<PipelineEvent>,
) {
    let run = pipeline.run(event_tx);
    tokio::pin!(run);
    loop {
        tokio::select! {
            result = &mut run => {
                if let Err(error) = result {
                    emit_pipeline_event(&app, PipelineEvent::Error(error));
                }
                while let Some(event) = event_rx.recv().await {
                    emit_pipeline_event(&app, event);
                }
                break;
            }
            event = event_rx.recv() => {
                match event {
                    Some(event) => emit_pipeline_event(&app, event),
                    None => break,
                }
            }
        }
    }
}

/// Stops the live pipeline and waits for its task to finish.
///
/// # Errors
///
/// Returns an application error when no live pipeline is active or shutdown
/// cannot complete.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn stop_live_translation(state: State<'_, AppState>) -> Result<(), AppError> {
    stop_live_task(state.inner()).await
}

/// Shared live task stopper used by commands and global shortcuts.
pub(crate) async fn stop_live_task(state: &AppState) -> Result<(), AppError> {
    let _lifecycle = state.live_lifecycle.lock().await;
    let pipeline = state.pipeline().ok_or(PipelineError::NotRunning)?;
    {
        let mut task = state.live_task.lock().await;
        if task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
        {
            let _ = task.take();
        }
        if task.is_none() {
            return Err(PipelineError::NotRunning.into());
        }
    }
    pipeline.stop().await?;
    if let Some(task) = state.live_task.lock().await.take() {
        task.await
            .map_err(|error| AppError::Tauri(format!("live task join failed: {error}")))?;
    }
    state.clear_pipeline();
    tracing::info!("live translation stopped");
    Ok(())
}

/// Updates the active live capture region or completes a pending selection.
///
/// # Errors
///
/// Returns an application error when the region is invalid or the active
/// pipeline rejects the update.
#[tauri::command]
#[tracing::instrument(skip(state, region))]
pub async fn update_live_region(
    region: ScreenRegion,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state.set_selected_region(region.clone()).await?;
    if let Some(pipeline) = state.pipeline() {
        pipeline
            .update_region(region)
            .await
            .map_err(AppError::from)?;
    }
    Ok(())
}

/// Updates the OCR language in the persisted configuration.
///
/// # Errors
///
/// Returns an application error when the configuration cannot be persisted or
/// a live task is currently running.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn set_ocr_language(
    language: Language,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let _lifecycle = state.live_lifecycle.lock().await;
    if state.live_task_is_running().await {
        return Err(PipelineError::AlreadyRunning.into());
    }
    state.update_config(|config| config.ocr.language = language)?;
    state.clear_pipeline();
    tracing::info!(language = language.code(), "OCR language updated");
    Ok(())
}

/// Switches between the API and local translation providers.
///
/// # Errors
///
/// Returns an application error for an unsupported provider, a failed
/// provider/configuration update, or an active live task.
#[tauri::command]
#[tracing::instrument(skip(state), fields(provider = provider_id))]
pub async fn set_translation_provider(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let _lifecycle = state.live_lifecycle.lock().await;
    if state.live_task_is_running().await {
        return Err(PipelineError::AlreadyRunning.into());
    }
    state.set_translation_provider_id(&provider_id)?;
    tracing::info!(provider = provider_id, "translation provider selected");
    Ok(())
}

/// Verifies local model files and returns the integrity report.
///
/// # Errors
///
/// Returns an application error when model integrity verification fails.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn load_local_models(
    state: State<'_, AppState>,
) -> Result<vtrans_models::VerifyReport, AppError> {
    let app = state.app_handle()?;
    state.set_model_progress(Some(0.0));
    emit_model_loading_progress(&app, "manifest", 0.0);
    let model_manager = std::sync::Arc::clone(&state.model_manager);
    let report = tokio::task::spawn_blocking(move || {
        model_manager.verify_integrity().map_err(AppError::from)
    })
    .await
    .map_err(|error| AppError::Tauri(format!("model verification task failed: {error}")))?;
    state.set_model_progress(Some(1.0));
    emit_model_loading_progress(&app, "manifest", 1.0);
    report
}

/// Persists the complete application settings object.
///
/// # Errors
///
/// Returns an application error when validation or atomic persistence fails,
/// or a live task is currently running.
#[tauri::command]
#[tracing::instrument(skip(state, settings))]
pub async fn save_settings(
    settings: vtrans_config::AppConfig,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let _lifecycle = state.live_lifecycle.lock().await;
    if state.live_task_is_running().await {
        return Err(PipelineError::AlreadyRunning.into());
    }
    let provider = state.prepare_translation_provider(&settings)?;
    state.save_config(&settings)?;
    state.replace_translation_provider(provider);
    tracing::info!("application settings saved");
    Ok(())
}

/// Returns a frontend-safe application status snapshot.
///
/// # Errors
///
/// Returns an application error if the managed state is unavailable.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn get_app_status(state: State<'_, AppState>) -> Result<AppStatus, AppError> {
    let live_running = state.live_task_is_running().await;
    Ok(state.status_snapshot(live_running))
}

/// Builds the invoke handler for all application commands.
pub fn invoke_handler<R: tauri::Runtime>(
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        start_region_selection,
        cancel_region_selection,
        capture_once,
        start_live_translation,
        stop_live_translation,
        update_live_region,
        set_ocr_language,
        set_translation_provider,
        load_local_models,
        save_settings,
        get_app_status,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtrans_core::{OcrLine, TranslationResult};

    #[test]
    fn live_config_defaults_are_stable() {
        let value: LiveTranslationConfig = serde_json::from_str(
            r#"{"region":{"monitor_id":"display-1","x":0,"y":0,"width":10,"height":10}}"#,
        )
        .unwrap();
        assert_eq!(value.capture_interval_ms, 500);
        assert!((value.difference_threshold - 0.03).abs() < f32::EPSILON);
    }

    fn ocr_result(text: &str) -> OcrResult {
        OcrResult::from_lines(
            vec![OcrLine::new(
                text,
                0.95,
                [[0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]],
                0,
            )],
            Some(Language::English),
            12,
        )
    }

    #[tokio::test]
    async fn capture_pipeline_forwards_stage_events_and_collects_result() {
        let (tx, rx) = mpsc::channel(16);
        let run = async move {
            tx.send(PipelineEvent::CaptureStarted).await.unwrap();
            tx.send(PipelineEvent::OcrStarted).await.unwrap();
            tx.send(PipelineEvent::OcrCompleted(ocr_result("hello")))
                .await
                .unwrap();
            tx.send(PipelineEvent::TranslationStarted).await.unwrap();
            tx.send(PipelineEvent::TranslationCompleted(TranslationResult::new(
                "hola", "mock", 8,
            )))
            .await
            .unwrap();
            tx.send(PipelineEvent::Stopped).await.unwrap();
            Ok::<(), PipelineError>(())
        };

        let mut forwarded = Vec::new();
        let collected = run_capture_pipeline(|event| forwarded.push(event), run, rx)
            .await
            .unwrap();

        assert_eq!(collected.unwrap().merged_text, "hello");
        assert!(forwarded
            .iter()
            .any(|event| matches!(event, PipelineEvent::CaptureStarted)));
        assert!(forwarded
            .iter()
            .any(|event| matches!(event, PipelineEvent::OcrStarted)));
        assert!(forwarded
            .iter()
            .any(|event| matches!(event, PipelineEvent::TranslationCompleted(_))));
        // A single capture has no live session; `Stopped` must be dropped.
        assert!(!forwarded
            .iter()
            .any(|event| matches!(event, PipelineEvent::Stopped)));
    }

    #[tokio::test]
    async fn capture_pipeline_propagates_run_error() {
        let (tx, rx) = mpsc::channel(16);
        let run = async move {
            tx.send(PipelineEvent::OcrStarted).await.unwrap();
            Err(PipelineError::Cancelled)
        };

        let result = run_capture_pipeline(|_| {}, run, rx).await;
        assert!(matches!(result, Err(PipelineError::Cancelled)));
    }

    #[tokio::test]
    async fn capture_pipeline_returns_none_without_ocr_result() {
        let (tx, rx) = mpsc::channel(16);
        let run = async move {
            tx.send(PipelineEvent::CaptureStarted).await.unwrap();
            tx.send(PipelineEvent::Stopped).await.unwrap();
            Ok::<(), PipelineError>(())
        };

        let collected = run_capture_pipeline(|_| {}, run, rx).await.unwrap();
        assert!(collected.is_none());
    }
}
