//! Tauri command handlers for the `VTrans` frontend.

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tokio::sync::mpsc;
use vtrans_core::{Language, OcrResult, PipelineMode, ScreenRegion};
use vtrans_pipeline::{PipelineError, PipelineEvent};

use crate::error::AppError;
use crate::events::{emit_model_loading_progress, emit_pipeline_event};
use crate::state::{AppState, AppStatus};

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

/// Opens the selector window and returns the most recently confirmed region.
///
/// Region confirmation is performed by the selector frontend through the
/// `update_live_region` command. Returning `NotInitialized` while the selector is
/// still open avoids inventing coordinates on the Rust side.
#[tauri::command]
#[tracing::instrument(skip(state))]
///
/// # Errors
///
/// Returns `AppError::NotInitialized` until the selector confirms a region, or
/// a Tauri error when the selector window cannot be shown.
pub async fn start_region_selection(state: State<'_, AppState>) -> Result<ScreenRegion, AppError> {
    let app = state.app_handle()?;
    if let Some(window) = app.get_webview_window("selector") {
        window
            .show()
            .map_err(|error| AppError::Tauri(error.to_string()))?;
        window
            .set_focus()
            .map_err(|error| AppError::Tauri(error.to_string()))?;
    } else {
        tracing::warn!("selector window is not configured");
        return Err(AppError::NotInitialized);
    }

    state.selected_region().ok_or_else(|| {
        tracing::debug!("region selector opened; waiting for frontend confirmation");
        AppError::NotInitialized
    })
}

/// Runs one capture, OCR, and translation pipeline pass and returns OCR text.
#[tauri::command]
#[tracing::instrument(skip(state, region))]
///
/// # Errors
///
/// Returns an application error when capture, OCR, translation, or pipeline
/// execution fails.
pub async fn capture_once(
    region: ScreenRegion,
    state: State<'_, AppState>,
) -> Result<OcrResult, AppError> {
    let pipeline = state.build_pipeline(PipelineMode::SingleCapture, region, 0, 0.03)?;
    let (event_tx, mut event_rx) = mpsc::channel(16);
    let run_result = pipeline.run(event_tx).await;
    let mut ocr_result = None;
    while let Ok(event) = event_rx.try_recv() {
        if let PipelineEvent::OcrCompleted(result) = event {
            ocr_result = Some(result);
        }
    }
    match run_result {
        Ok(()) => ocr_result.ok_or(AppError::NotInitialized),
        Err(error) => Err(error.into()),
    }
}

/// Starts a live capture/OCR/translation task and returns immediately.
#[tauri::command]
#[tracing::instrument(skip(state, config))]
///
/// # Errors
///
/// Returns an application error when the region or providers are invalid, or
/// when another live task is already running.
pub async fn start_live_translation(
    config: LiveTranslationConfig,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let app = state.app_handle()?;
    if state.live_task_is_running().await {
        return Err(PipelineError::AlreadyRunning.into());
    }
    state.set_selected_region(config.region.clone())?;
    let pipeline = state.build_pipeline(
        PipelineMode::LiveRegion,
        config.region,
        config.capture_interval_ms,
        config.difference_threshold,
    )?;
    let pipeline = state.set_pipeline(pipeline);
    let (event_tx, mut event_rx) = mpsc::channel(32);
    let app_for_task = app.clone();
    let task = tokio::spawn(async move {
        let run = pipeline.run(event_tx);
        tokio::pin!(run);
        loop {
            tokio::select! {
                result = &mut run => {
                    if let Err(error) = result {
                        emit_pipeline_event(&app_for_task, PipelineEvent::Error(error));
                    }
                    break;
                }
                event = event_rx.recv() => {
                    match event {
                        Some(event) => emit_pipeline_event(&app_for_task, event),
                        None => break,
                    }
                }
            }
        }
    });
    *state.live_task.lock().await = Some(task);
    tracing::info!("live translation started");
    Ok(())
}

/// Stops the live pipeline and waits for its task to finish.
#[tauri::command]
#[tracing::instrument(skip(state))]
///
/// # Errors
///
/// Returns `PipelineError::NotRunning` when no live pipeline is active, or a
/// task/pipeline error when shutdown cannot complete.
pub async fn stop_live_translation(state: State<'_, AppState>) -> Result<(), AppError> {
    let pipeline = state.pipeline().ok_or(AppError::NotInitialized)?;
    pipeline.stop().await?;
    if let Some(task) = state.live_task.lock().await.take() {
        task.await
            .map_err(|error| AppError::Tauri(format!("live task join failed: {error}")))?;
    }
    tracing::info!("live translation stopped");
    Ok(())
}

/// Updates the active live capture region.
#[tauri::command]
#[tracing::instrument(skip(state, region))]
///
/// # Errors
///
/// Returns an application error when the region is invalid or the active
/// pipeline rejects the update.
pub async fn update_live_region(
    region: ScreenRegion,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state.set_selected_region(region.clone())?;
    if let Some(pipeline) = state.pipeline() {
        pipeline
            .update_region(region)
            .await
            .map_err(AppError::from)?;
    }
    Ok(())
}

/// Updates the OCR language in the persisted configuration.
#[tauri::command]
#[tracing::instrument(skip(state))]
///
/// # Errors
///
/// Returns an application error when the configuration cannot be persisted.
pub async fn set_ocr_language(
    language: Language,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state.update_config(|config| config.ocr.language = language)?;
    state.clear_pipeline();
    tracing::info!(language = language.code(), "OCR language updated");
    Ok(())
}

/// Switches between the API and local translation providers.
#[tauri::command]
#[tracing::instrument(skip(state), fields(provider = provider_id))]
///
/// # Errors
///
/// Returns an application error for an unsupported provider or a failed
/// provider/configuration update.
pub async fn set_translation_provider(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state.set_translation_provider_id(&provider_id)?;
    tracing::info!(provider = provider_id, "translation provider selected");
    Ok(())
}

/// Verifies local model files and returns the integrity report.
#[tauri::command]
#[tracing::instrument(skip(state))]
///
/// # Errors
///
/// Returns an application error when model integrity verification fails.
pub async fn load_local_models(
    state: State<'_, AppState>,
) -> Result<vtrans_models::VerifyReport, AppError> {
    let app = state.app_handle()?;
    state.set_model_progress(Some(0.0));
    emit_model_loading_progress(&app, "manifest", 0.0);
    let report = state.verify_models();
    state.set_model_progress(Some(1.0));
    emit_model_loading_progress(&app, "manifest", 1.0);
    report
}

/// Persists the complete application settings object.
#[tauri::command]
#[tracing::instrument(skip(state, settings))]
///
/// # Errors
///
/// Returns an application error when validation or atomic persistence fails.
pub async fn save_settings(
    settings: vtrans_config::AppConfig,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state.save_config(&settings)?;
    state.clear_pipeline();
    tracing::info!("application settings saved");
    Ok(())
}

/// Returns a frontend-safe application status snapshot.
#[tauri::command]
#[tracing::instrument(skip(state))]
///
/// # Errors
///
/// Returns an application error if the managed state is unavailable.
pub async fn get_app_status(state: State<'_, AppState>) -> Result<AppStatus, AppError> {
    let live_running = state.live_task_is_running().await;
    Ok(state.status_snapshot(live_running))
}

/// Builds the invoke handler for all application commands.
pub fn invoke_handler<R: tauri::Runtime>(
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        start_region_selection,
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

    #[test]
    fn live_config_defaults_are_stable() {
        let value: LiveTranslationConfig = serde_json::from_str(
            r#"{"region":{"monitor_id":"display-1","x":0,"y":0,"width":10,"height":10}}"#,
        )
        .unwrap();
        assert_eq!(value.capture_interval_ms, 500);
        assert!((value.difference_threshold - 0.03).abs() < f32::EPSILON);
    }
}
