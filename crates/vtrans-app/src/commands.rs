//! Tauri command handlers for the `VTrans` frontend.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use vtrans_config::AppConfig;
use vtrans_config::TranslationBoxConfig;
use vtrans_core::{Language, OcrResult, PipelineMode, ScreenRegion, TranslationResult};
use vtrans_pipeline::{BoxStatus, MultiBoxPipeline, PipelineError, PipelineEvent, TranslationBox};

use crate::debug_frame::{spawn_debug_frame_forwarder, RegionSource};
use crate::error::AppError;
use crate::events::{
    emit_model_loading_progress, emit_multibox_box_added, emit_multibox_box_removed,
    emit_multibox_box_updated, emit_multibox_result, emit_multibox_status, emit_multibox_warning,
    emit_pipeline_event, emit_translation_single_result,
};
use crate::overlay::{
    apply_overlay, hide_region_overlay, overlay_intent, overlay_intent_for_stop, OverlayEvent,
    OverlayIntent, StopKind, OVERLAY_WINDOW_LABEL,
};
use crate::state::AppStatus;
use crate::state::{
    store_api_key, store_provider_credentials, validate_translation_provider_id, AppState,
};

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

/// Frontend-facing translation box info returned by multi-box commands.
///
/// Mirrors `vtrans_pipeline::TranslationBox` but uses `box_id` to match
/// the IPC contract with the frontend TypeScript types.
///
/// # Example
///
/// ```
/// use vtrans_app::TranslationBoxInfo;
/// use vtrans_core::ScreenRegion;
///
/// let info = TranslationBoxInfo {
///     box_id: 0,
///     region: ScreenRegion::new("m0", 10, 20, 300, 400),
///     color: "#FF6B6B".to_string(),
/// };
/// assert_eq!(info.box_id, 0);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationBoxInfo {
    /// Unique identifier for this translation box.
    pub box_id: u32,
    /// Screen region captured and translated for this box.
    pub region: ScreenRegion,
    /// Display color as a hex string (e.g. `"#FF6B6B"`).
    pub color: String,
}

impl TranslationBoxInfo {
    /// Creates box info from a pipeline [`TranslationBox`].
    #[must_use]
    pub fn from_pipeline_box(box_: &TranslationBox) -> Self {
        Self {
            box_id: box_.id,
            region: box_.region.clone(),
            color: box_.color.clone(),
        }
    }

    /// Creates box info from a config entry.
    #[must_use]
    pub fn from_config(config: &TranslationBoxConfig) -> Self {
        Self {
            box_id: config.id,
            region: config.region.clone(),
            color: config.color.clone(),
        }
    }
}

/// Adds a translation box to the configuration and returns the new entry.
///
/// The id and color are assigned from the config's `next_box_id` and
/// `next_box_color` helpers so they stay unique and follow the palette.
fn add_box_config(config: &mut AppConfig, region: ScreenRegion) -> TranslationBoxConfig {
    let id = config.next_box_id();
    let color = config.next_box_color().to_string();
    let box_config = TranslationBoxConfig::new(id, region, color);
    config.translation_boxes.push(box_config.clone());
    box_config
}

/// Removes a translation box from the configuration by ID.
fn remove_box_config(config: &mut AppConfig, box_id: u32) {
    config.translation_boxes.retain(|b| b.id != box_id);
}

/// Updates the region of a translation box in the configuration.
///
/// Silently does nothing when the box is not in the config (the box may
/// have been added to the pipeline without a config entry).
fn update_box_config_region(config: &mut AppConfig, box_id: u32, region: ScreenRegion) {
    if let Some(entry) = config.translation_boxes.iter_mut().find(|b| b.id == box_id) {
        entry.region = region;
    }
}

/// Maximum accepted length of an API key, guarding against accidental
/// pastes of huge payloads into the OS credential vault.
const MAX_API_KEY_LEN: usize = 4096;

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
    // A new selection invalidates the previously confirmed region marker.
    hide_region_overlay(&app);
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
    hide_region_overlay(&app);
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
    // A single capture never shows the persistent marker: hide it on entry
    // (defensive against a stale selector or hotkey path) and record the
    // single mode for hydration. The final hide below also covers failures.
    apply_overlay(&app, OverlayIntent::Hide, None);
    state.set_current_mode(PipelineMode::SingleCapture);
    let frame_sink = state
        .debug_mode()
        .then(|| spawn_debug_frame_forwarder(app.clone(), RegionSource::Fixed(region.clone())));

    let translation_result = std::sync::Arc::new(std::sync::Mutex::new(None::<TranslationResult>));

    // The interval and threshold are ignored for single captures; the
    // pipeline builder uses the single-mode defaults for them.
    let result = async {
        let tr = std::sync::Arc::clone(&translation_result);
        let app_for_closure = app.clone();
        let pipeline =
            state.build_pipeline(PipelineMode::SingleCapture, region, 0, 0.03, frame_sink)?;
        let (event_tx, event_rx) = mpsc::channel(16);
        let ocr_result = run_capture_pipeline(
            move |event| {
                if let PipelineEvent::TranslationCompleted(translation) = &event {
                    *tr.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(translation.clone());
                }
                emit_pipeline_event(&app_for_closure, event);
            },
            pipeline.run(event_tx),
            event_rx,
        )
        .await?;
        ocr_result.ok_or(AppError::NotInitialized)
    }
    .await;

    // Emit single-result when both OCR text and translation are available.
    let translation = translation_result
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Ok(ref ocr) = result {
        if let Some(ref translation) = translation {
            if !ocr.merged_text.is_empty() {
                emit_translation_single_result(
                    &app,
                    &ocr.merged_text,
                    &translation.translated_text,
                );
            }
        }
    }

    // The marker must never outlive a single capture, success or failure.
    apply_overlay(
        &app,
        overlay_intent(OverlayEvent::SingleCaptureCompleted),
        None,
    );
    result
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
    // The live session captures this region; make sure the persistent marker
    // is visible even when the session was started from a hotkey with every
    // webview hidden. The frontend re-positions the same values afterwards,
    // so the calls converge on an identical window placement.
    state.set_current_mode(PipelineMode::LiveRegion);
    apply_overlay(
        &app,
        overlay_intent(OverlayEvent::LiveStarted),
        Some(&config.region),
    );
    let frame_sink = state.debug_mode().then(|| {
        spawn_debug_frame_forwarder(
            app.clone(),
            RegionSource::FollowSelected(config.region.clone()),
        )
    });
    let pipeline = state.build_pipeline(
        PipelineMode::LiveRegion,
        config.region,
        config.capture_interval_ms,
        config.difference_threshold,
        frame_sink,
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
    let app = state.app_handle()?;
    stop_live_task(&app, state.inner(), StopKind::Pause).await
}

/// Shared live task stopper used by commands and global shortcuts.
///
/// The overlay decision is applied through [`overlay_intent_for_stop`]: a
/// pause (UI pause command) keeps the marker, a real stop (stop hotkey)
/// hides it. The UI stop button hides the marker itself before calling the
/// pause command, so both stop paths converge on a hidden marker.
pub(crate) async fn stop_live_task(
    app: &AppHandle,
    state: &AppState,
    stop: StopKind,
) -> Result<(), AppError> {
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
    apply_overlay(app, overlay_intent_for_stop(stop), None);
    tracing::info!("live translation stopped");
    Ok(())
}

/// Updates the active capture region or completes a pending selection.
///
/// `mode` tells the backend whether the confirmation belongs to a single
/// capture or a live session: live confirms show the persistent marker,
/// single confirms keep it hidden (the single capture hides it again when
/// it finishes, see [`capture_once`]). The frontend selector passes the
/// current session mode under the Tauri camelCase `mode` argument.
///
/// # Errors
///
/// Returns an application error when the region is invalid or the active
/// pipeline rejects the update.
#[tauri::command]
#[tracing::instrument(skip(state, region), fields(mode = ?mode))]
pub async fn update_live_region(
    region: ScreenRegion,
    mode: PipelineMode,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state.set_selected_region(region.clone()).await?;
    state.set_current_mode(mode);
    if let Some(pipeline) = state.pipeline() {
        pipeline
            .update_region(region.clone())
            .await
            .map_err(AppError::from)?;
    }
    let app = state.app_handle()?;
    apply_overlay(
        &app,
        overlay_intent(OverlayEvent::RegionConfirmed(mode)),
        Some(&region),
    );
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
    state.update_config(|config| apply_ocr_language(config, language))?;
    state.clear_pipeline();
    tracing::info!(language = language.code(), "OCR language updated");
    Ok(())
}

/// Updates the translation source language in the persisted configuration.
///
/// `Language::Auto` enables automatic source-language detection.
///
/// # Errors
///
/// Returns an application error when the configuration cannot be persisted or
/// a live task is currently running.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn set_source_language(
    language: Language,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let _lifecycle = state.live_lifecycle.lock().await;
    if state.live_task_is_running().await {
        return Err(PipelineError::AlreadyRunning.into());
    }
    state.update_config(|config| apply_source_language(config, language))?;
    state.clear_pipeline();
    tracing::info!(
        language = language.code(),
        "translation source language updated"
    );
    Ok(())
}

/// Updates the translation target language in the persisted configuration.
///
/// `Language::Auto` is rejected by configuration validation because the
/// target language must be a concrete language (`zh-CN`, `ja`, or `en`).
///
/// # Errors
///
/// Returns an application error when the configuration cannot be persisted or
/// a live task is currently running.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn set_target_language(
    language: Language,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let _lifecycle = state.live_lifecycle.lock().await;
    if state.live_task_is_running().await {
        return Err(PipelineError::AlreadyRunning.into());
    }
    state.update_config(|config| apply_target_language(config, language))?;
    state.clear_pipeline();
    tracing::info!(
        language = language.code(),
        "translation target language updated"
    );
    Ok(())
}

/// Applies an OCR-language change to a configuration snapshot.
///
/// `ocr.language` and `translation.source_language` are linked settings
/// (see `vtrans_config::validate_language_linkage`): both are set to the
/// same value so the subsequent `ConfigManager::save` validation always
/// succeeds. Kept as a pure function so the exact mutation performed by
/// [`set_ocr_language`] is unit-testable without a Tauri runtime.
fn apply_ocr_language(config: &mut AppConfig, language: Language) {
    config.ocr.language = language;
    config.translation.source_language = language;
}

/// Applies a source-language change to a configuration snapshot.
///
/// `translation.source_language` and `ocr.language` are linked settings
/// (see `vtrans_config::validate_language_linkage`): both are set to the
/// same value so the subsequent `ConfigManager::save` validation always
/// succeeds. Kept as a pure function so the exact mutation performed by
/// [`set_source_language`] is unit-testable without a Tauri runtime.
fn apply_source_language(config: &mut AppConfig, language: Language) {
    config.translation.source_language = language;
    config.ocr.language = language;
}

/// Applies a target-language change to a configuration snapshot.
///
/// Kept as a pure function so the exact mutation performed by
/// [`set_target_language`] can be unit-tested without a Tauri runtime.
fn apply_target_language(config: &mut AppConfig, language: Language) {
    config.translation.target_language = language;
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
    let app = state.app_handle()?;
    state
        .set_translation_provider_id(&provider_id, Some(&app))
        .await?;
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
    settings: AppConfig,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let _lifecycle = state.live_lifecycle.lock().await;
    if state.live_task_is_running().await {
        return Err(PipelineError::AlreadyRunning.into());
    }
    let app = state.app_handle()?;
    let provider = state
        .prepare_translation_provider(settings.clone(), Some(&app))
        .await?;
    state.save_config(&settings)?;
    state.replace_translation_provider(provider);
    tracing::info!("application settings saved");
    Ok(())
}

/// Updates the persisted result-window appearance (opacity and font size).
///
/// The change is applied to the two `result_window` fields and persisted
/// through `save_config`, which validates the values and writes the file
/// atomically. Out-of-range values surface as `ConfigError::Validation`
/// mapped to `AppError::Config`.
///
/// Unlike `save_settings`, this command never acquires the live lifecycle
/// lock, never checks whether a live task is running, and never rebuilds a
/// translation provider: appearance changes are independent of capture,
/// OCR, and translation state, so they apply even while a live session is
/// active. The window itself is styled by the frontend from the persisted
/// fields; this command only persists them.
///
/// The frontend passes the arguments as `{ opacity, fontSizePx }` (Tauri 2
/// maps the `font_size_px` parameter to camelCase by default).
///
/// # Errors
///
/// Returns an application error when the configuration cannot be loaded or
/// persisted.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn update_result_window_appearance(
    opacity: f64,
    font_size_px: u32,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let mut config = state.load_config()?;
    apply_result_window_appearance(&mut config, opacity, font_size_px);
    state.save_config(&config)?;
    tracing::info!(opacity, font_size_px, "result window appearance updated");
    Ok(())
}

/// Updates the persisted floating-ball appearance (opacity and size).
///
/// The change is applied to the two `floating_ball` fields and persisted
/// through `save_config`, which validates the values and writes the file
/// atomically. Out-of-range values surface as `ConfigError::Validation`
/// mapped to `AppError::Config`.
///
/// Like [`update_result_window_appearance`], this command never acquires
/// the live lifecycle lock, never checks whether a live task is running,
/// and never rebuilds a translation provider, so appearance changes apply
/// while a live session is active.
///
/// The frontend passes the arguments as `{ opacity, sizePx }` (Tauri 2
/// maps the `size_px` parameter to camelCase by default).
///
/// # Errors
///
/// Returns an application error when the configuration cannot be loaded or
/// persisted.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn update_floating_ball_appearance(
    opacity: f64,
    size_px: u32,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let mut config = state.load_config()?;
    apply_floating_ball_appearance(&mut config, opacity, size_px);
    state.save_config(&config)?;
    tracing::info!(opacity, size_px, "floating ball appearance updated");
    Ok(())
}

/// Applies a result-window appearance change to a configuration snapshot.
///
/// Kept as a pure function so the exact mutation performed by
/// [`update_result_window_appearance`] can be unit-tested without a Tauri
/// runtime. Out-of-range values are not rejected here; they surface as
/// `ConfigError::Validation` when the snapshot is persisted by
/// `vtrans-config`.
fn apply_result_window_appearance(config: &mut AppConfig, opacity: f64, font_size_px: u32) {
    config.result_window.opacity = opacity;
    config.result_window.font_size_px = font_size_px;
}

/// Applies a floating-ball appearance change to a configuration snapshot.
///
/// Kept as a pure function so the exact mutation performed by
/// [`update_floating_ball_appearance`] can be unit-tested without a Tauri
/// runtime. Out-of-range values are not rejected here; they surface as
/// `ConfigError::Validation` when the snapshot is persisted by
/// `vtrans-config`.
fn apply_floating_ball_appearance(config: &mut AppConfig, opacity: f64, size_px: u32) {
    config.floating_ball.opacity = opacity;
    config.floating_ball.size_px = size_px;
}

/// Stores an API key for the **currently configured** translation provider
/// in the OS credential vault.
///
/// The key is written to the provider-specific credential target (`openai`,
/// `deepl`, `google`, `azure`, or `baidu_secret` for Baidu) and never
/// enters `config.json`, the frontend store, or any log. When the provider
/// is credential-backed, the running provider is rebuilt with the new key
/// immediately so the change applies without a restart. The `local`
/// provider does not accept credentials and is rejected.
///
/// For Baidu, this command stores only the secret key; use
/// [`set_provider_credentials`] to set both the APP ID and the secret.
///
/// The frontend passes the key as `{ apiKey }` (Tauri 2 maps the Rust
/// `api_key` parameter to camelCase by default).
///
/// # Errors
///
/// Returns an application error when the key is empty after trimming, exceeds
/// [`MAX_API_KEY_LEN`] characters, the credential vault write fails, a live
/// task is running, the configured provider does not accept credentials, or
/// the provider cannot be rebuilt.
#[tauri::command]
#[tracing::instrument(skip(state, api_key))]
pub async fn set_api_key(api_key: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let _lifecycle = state.live_lifecycle.lock().await;
    if state.live_task_is_running().await {
        return Err(PipelineError::AlreadyRunning.into());
    }
    let api_key = validate_api_key(&api_key)?;
    let config = state.load_config()?;
    let provider_id = config.translation.provider.clone();
    let masked_key = vtrans_core::mask_sensitive(&api_key);
    let credentials = Arc::clone(&state.credentials);
    let store_provider_id = provider_id.clone();
    tokio::task::spawn_blocking(move || store_api_key(&credentials, &store_provider_id, &api_key))
        .await
        .map_err(|error| AppError::Tauri(format!("credential store task failed: {error}")))??;

    let app = state.app_handle()?;
    let provider = state
        .prepare_translation_provider(config, Some(&app))
        .await?;
    state.replace_translation_provider(provider);
    tracing::info!(
        provider = provider_id,
        masked_key = %masked_key,
        "translation credential updated"
    );
    Ok(())
}

/// Stores the complete credential set for a cloud translation provider in
/// the OS credential vault.
///
/// OpenAI/DeepL/Google/Azure accept `api_key`; Baidu requires both `app_id`
/// and `secret` (stored under the independent `baidu_app_id` /
/// `baidu_secret` targets). The `local` provider does not accept
/// credentials. When the stored provider matches the currently configured
/// provider, the running provider is rebuilt immediately so the change
/// applies without a restart.
///
/// The frontend passes the arguments as `{ providerId, apiKey, appId,
/// secret }` (Tauri 2 maps Rust `snake_case` parameters to camelCase by
/// default).
///
/// # Errors
///
/// Returns an application error when the provider id is unsupported, a
/// required credential value is missing or invalid, a live task is running,
/// the vault write fails, or the provider cannot be rebuilt.
#[tauri::command]
#[tracing::instrument(skip(state, api_key, app_id, secret), fields(provider_id))]
pub async fn set_provider_credentials(
    provider_id: String,
    api_key: Option<String>,
    app_id: Option<String>,
    secret: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let _lifecycle = state.live_lifecycle.lock().await;
    if state.live_task_is_running().await {
        return Err(PipelineError::AlreadyRunning.into());
    }
    validate_translation_provider_id(&provider_id)?;
    if provider_id == "local" {
        return Err(AppError::ProviderCredential(format!(
            "provider {provider_id:?} does not accept credentials"
        )));
    }
    let api_key = api_key
        .map(|value| validate_credential_value(&value, "api key"))
        .transpose()?;
    let app_id = app_id
        .map(|value| validate_credential_value(&value, "app id"))
        .transpose()?;
    let secret = secret
        .map(|value| validate_credential_value(&value, "secret"))
        .transpose()?;
    let credentials = Arc::clone(&state.credentials);
    let store_provider_id = provider_id.clone();
    tokio::task::spawn_blocking(move || {
        store_provider_credentials(
            &credentials,
            &store_provider_id,
            api_key.as_deref(),
            app_id.as_deref(),
            secret.as_deref(),
        )
    })
    .await
    .map_err(|error| AppError::Tauri(format!("credential store task failed: {error}")))??;

    let config = state.load_config()?;
    if config.translation.provider == provider_id {
        let app = state.app_handle()?;
        let provider = state
            .prepare_translation_provider(config, Some(&app))
            .await?;
        state.replace_translation_provider(provider);
    }
    tracing::info!(provider = provider_id, "provider credentials updated");
    Ok(())
}

/// Validates and normalizes an API key before storage.
///
/// The key is trimmed, must not be empty, and must not exceed
/// [`MAX_API_KEY_LEN`] characters. Kept as a pure function so the exact
/// validation performed by [`set_api_key`] is unit-testable without a Tauri
/// runtime.
///
/// # Errors
///
/// Returns `AppError::InvalidApiKey` when the key fails validation.
fn validate_api_key(api_key: &str) -> Result<String, AppError> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidApiKey("key must not be empty".to_string()));
    }
    if trimmed.chars().count() > MAX_API_KEY_LEN {
        return Err(AppError::InvalidApiKey(format!(
            "key exceeds {MAX_API_KEY_LEN} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Validates and normalizes a provider credential value (APP ID or secret)
/// before storage.
///
/// Kept as a pure function so the validation performed by
/// [`set_provider_credentials`] is unit-testable without a Tauri runtime.
///
/// # Errors
///
/// Returns `AppError::ProviderCredential` when the value is empty after
/// trimming or exceeds [`MAX_API_KEY_LEN`] characters.
fn validate_credential_value(value: &str, label: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::ProviderCredential(format!(
            "{label} must not be empty"
        )));
    }
    if trimmed.chars().count() > MAX_API_KEY_LEN {
        return Err(AppError::ProviderCredential(format!(
            "{label} exceeds {MAX_API_KEY_LEN} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Returns the complete persisted application configuration.
///
/// The frontend hydrates its settings panel from this snapshot before
/// calling `save_settings`, so a full save never overwrites backend fields
/// (OCR language, log level, model directory, ...) with frontend defaults.
///
/// # Errors
///
/// Returns an application error when the configuration cannot be loaded.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn get_app_config(state: State<'_, AppState>) -> Result<AppConfig, AppError> {
    state.load_config()
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

// ── Multi-box translation commands ──

/// Adds a translation box to the multi-box pipeline and persists it.
///
/// The box is assigned the next available id and color from the
/// configuration palette. If the multi-box pipeline has not been created
/// yet, it is lazily initialized from the current config. When the box
/// count reaches the warning threshold, a `multibox://warning` event is
/// emitted (non-blocking).
///
/// # Errors
///
/// Returns an application error when the region is invalid, the pipeline
/// rejects the box (limit exceeded, duplicate id), or the config cannot be
/// persisted.
#[tauri::command]
#[tracing::instrument(skip(state, region))]
pub async fn add_translation_box(
    region: ScreenRegion,
    state: State<'_, AppState>,
) -> Result<TranslationBoxInfo, AppError> {
    let app = state.app_handle()?;
    region.validate().map_err(AppError::from)?;

    // Add to config (computes id and color from the current snapshot).
    let mut config = state.load_config()?;
    let box_config = add_box_config(&mut config, region.clone());
    state.save_config(&config)?;

    // Add to the multi-box pipeline (creates it lazily).
    let pipeline = state.ensure_multi_pipeline()?;
    let translation_box = TranslationBox::new(
        box_config.id,
        box_config.region.clone(),
        box_config.color.clone(),
    );
    pipeline.add_box(translation_box).await?;

    state.add_multi_box_id(box_config.id);
    emit_multibox_box_added(&app, box_config.id, &box_config.color, &box_config.region);

    let count = u32::try_from(config.translation_boxes.len()).unwrap_or(u32::MAX);
    if config.warning_threshold > 0 && count >= config.warning_threshold {
        emit_multibox_warning(&app, count, config.max_boxes);
    }

    tracing::info!(box_id = box_config.id, "translation box added");
    Ok(TranslationBoxInfo {
        box_id: box_config.id,
        region: box_config.region,
        color: box_config.color,
    })
}

/// Removes a translation box from the pipeline and config.
///
/// # Errors
///
/// Returns an application error when the config cannot be persisted or a
/// pipeline error other than `BoxNotFound` occurs.
#[tauri::command]
#[tracing::instrument(skip(state), fields(box_id))]
pub async fn remove_translation_box(
    box_id: u32,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let app = state.app_handle()?;

    if let Some(pipeline) = state.multi_pipeline() {
        if let Err(error) = pipeline.remove_box(box_id).await {
            if !matches!(error, PipelineError::BoxNotFound(_)) {
                return Err(AppError::from(error));
            }
        }
    }

    state.update_config(|cfg| {
        remove_box_config(cfg, box_id);
    })?;

    state.remove_multi_box_id(box_id);
    emit_multibox_box_removed(&app, box_id);

    tracing::info!(box_id, "translation box removed");
    Ok(())
}

/// Updates the capture region of a translation box.
///
/// If the pipeline is running, the box's task is restarted with the new
/// region. The config entry is updated and a `multibox://box-updated`
/// event is emitted.
///
/// # Errors
///
/// Returns an application error when the region is invalid or a pipeline
/// error other than `BoxNotFound` occurs.
#[tauri::command]
#[tracing::instrument(skip(state, region), fields(box_id))]
pub async fn update_translation_box(
    box_id: u32,
    region: ScreenRegion,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let app = state.app_handle()?;
    region.validate().map_err(AppError::from)?;

    if let Some(pipeline) = state.multi_pipeline() {
        if let Err(error) = pipeline.update_box(box_id, region.clone()).await {
            if !matches!(error, PipelineError::BoxNotFound(_)) {
                return Err(AppError::from(error));
            }
        }
    }

    state.update_config(|cfg| {
        update_box_config_region(cfg, box_id, region.clone());
    })?;

    emit_multibox_box_updated(&app, box_id, &region);

    tracing::info!(box_id, "translation box region updated");
    Ok(())
}

/// Lists all configured translation boxes.
///
/// The list is read from the persisted config so it survives restarts
/// even when the pipeline has not been started.
///
/// # Errors
///
/// Returns an application error when the config cannot be loaded.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn list_translation_boxes(
    state: State<'_, AppState>,
) -> Result<Vec<TranslationBoxInfo>, AppError> {
    let config = state.load_config()?;
    Ok(config
        .translation_boxes
        .iter()
        .map(TranslationBoxInfo::from_config)
        .collect())
}

/// Starts real-time translation for all configured boxes.
///
/// Rebuilds the multi-box pipeline from the current config, spawns a
/// forwarder task that relays results and status changes to the frontend,
/// and starts all box tasks. If a previous session was running, it is
/// stopped first.
///
/// # Errors
///
/// Returns an application error when the pipeline cannot be created or
/// `start_all` fails (e.g. already running).
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn start_multi_realtime(state: State<'_, AppState>) -> Result<(), AppError> {
    let app = state.app_handle()?;

    state.clear_multi_pipeline().await;

    let pipeline = state.ensure_multi_pipeline()?;
    let config = state.load_config()?;
    for box_config in &config.translation_boxes {
        let translation_box = TranslationBox::new(
            box_config.id,
            box_config.region.clone(),
            box_config.color.clone(),
        );
        if let Err(error) = pipeline.add_box(translation_box).await {
            tracing::warn!(
                box_id = box_config.id,
                error = %error,
                "failed to add box during multi-box start"
            );
        }
        state.add_multi_box_id(box_config.id);
    }

    let box_ids = state.multi_box_ids_handle();
    let forwarder_pipeline = Arc::clone(&pipeline);
    let forwarder_app = app.clone();
    let task = tokio::spawn(run_multi_forwarder(
        forwarder_app,
        forwarder_pipeline,
        box_ids,
    ));
    state.set_multi_forwarder(task).await;

    pipeline.start_all().await?;

    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        let _ = window.show();
    }

    tracing::info!(
        box_count = pipeline.box_count(),
        "multi-box real-time started"
    );
    Ok(())
}

/// Stops all multi-box translation tasks.
///
/// The pipeline and forwarder are cleared, the overlay is hidden, and a
/// `Stopped` status is emitted for every box.
///
/// # Errors
///
/// Returns an application error when no multi-box session is running or
/// `stop_all` fails.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn stop_multi_realtime(state: State<'_, AppState>) -> Result<(), AppError> {
    let app = state.app_handle()?;
    let pipeline = state.multi_pipeline().ok_or(PipelineError::NotRunning)?;
    let box_ids = state.multi_box_ids_snapshot();

    pipeline.stop_all().await?;
    state.clear_multi_pipeline().await;

    hide_region_overlay(&app);

    for box_id in box_ids {
        emit_multibox_status(&app, box_id, &BoxStatus::Stopped);
    }

    tracing::info!("multi-box real-time stopped");
    Ok(())
}

/// Stops a single translation box, leaving it registered.
///
/// # Errors
///
/// Returns an application error when no multi-box session is running or
/// the box does not exist / has no running task.
#[tauri::command]
#[tracing::instrument(skip(state), fields(box_id))]
pub async fn stop_box(box_id: u32, state: State<'_, AppState>) -> Result<(), AppError> {
    let app = state.app_handle()?;
    let pipeline = state.multi_pipeline().ok_or(PipelineError::NotRunning)?;

    pipeline.stop_box(box_id).await?;
    emit_multibox_status(&app, box_id, &BoxStatus::Stopped);

    tracing::info!(box_id, "translation box stopped");
    Ok(())
}

/// Opens the result (translation popup) window, or focuses it if visible.
///
/// The window is pre-declared in `tauri.conf.json` and hidden on close,
/// so this command never creates a new window — it only shows and
/// focuses the existing one.
///
/// # Errors
///
/// Returns an application error when the result window is not configured
/// or cannot be shown / focused.
#[tauri::command]
#[tracing::instrument(skip(state))]
pub async fn open_result_window(state: State<'_, AppState>) -> Result<(), AppError> {
    let app = state.app_handle()?;
    let Some(window) = app.get_webview_window("result") else {
        return Err(AppError::Tauri(
            "result window is not configured".to_string(),
        ));
    };
    window
        .show()
        .and_then(|()| window.set_focus())
        .map_err(|error| AppError::Tauri(error.to_string()))?;
    tracing::info!("result window opened");
    Ok(())
}

/// Background task forwarding multi-box results and status changes.
///
/// Subscribes to the pipeline's result stream and emits each result via
/// `multibox://result`. A periodic poll (500 ms) checks box statuses and
/// emits `multibox://status` when a status changes (e.g. a box erroring).
/// The task exits when the result stream closes (pipeline dropped).
async fn run_multi_forwarder(
    app: AppHandle,
    pipeline: Arc<MultiBoxPipeline>,
    box_ids: Arc<std::sync::RwLock<Vec<u32>>>,
) {
    let mut rx = pipeline.subscribe_results();
    let mut last_status: HashMap<u32, BoxStatus> = HashMap::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    ticker.tick().await; // consume the first immediate tick

    loop {
        tokio::select! {
            biased;
            result = rx.recv() => {
                if let Some(result) = result {
                    emit_multibox_result(&app, &result);
                } else {
                    tracing::debug!("multi-box result stream ended; forwarder exiting");
                    break;
                }
            },
            _ = ticker.tick() => {
                let ids = box_ids
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                for box_id in ids {
                    if let Some(status) = pipeline.box_status(box_id) {
                        if last_status.get(&box_id) != Some(&status) {
                            last_status.insert(box_id, status.clone());
                            emit_multibox_status(&app, box_id, &status);
                        }
                    }
                }
            }
        }
    }
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
        set_source_language,
        set_target_language,
        set_translation_provider,
        load_local_models,
        save_settings,
        update_result_window_appearance,
        update_floating_ball_appearance,
        set_api_key,
        set_provider_credentials,
        get_app_config,
        get_app_status,
        add_translation_box,
        remove_translation_box,
        update_translation_box,
        list_translation_boxes,
        start_multi_realtime,
        stop_multi_realtime,
        stop_box,
        open_result_window,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use vtrans_config::{ConfigError, ConfigManager};
    use vtrans_core::{OcrLine, TranslationResult};

    /// Isolated config directory for persistence tests, removed on drop.
    ///
    /// std-only (no extra dev-dependency): the directory name combines the
    /// process id with a monotonic counter so parallel tests never collide.
    struct TestConfigDir {
        path: std::path::PathBuf,
    }

    impl TestConfigDir {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let unique = format!(
                "vtrans-app-appearance-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestConfigDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn live_config_defaults_are_stable() {
        let value: LiveTranslationConfig = serde_json::from_str(
            r#"{"region":{"monitor_id":"display-1","x":0,"y":0,"width":10,"height":10}}"#,
        )
        .unwrap();
        assert_eq!(value.capture_interval_ms, 500);
        assert!((value.difference_threshold - 0.03).abs() < f32::EPSILON);
    }

    #[test]
    fn source_language_update_syncs_linked_fields_and_preserves_others() {
        let mut config = AppConfig::default();
        apply_source_language(&mut config, Language::Japanese);
        assert_eq!(config.translation.source_language, Language::Japanese);
        assert_eq!(config.ocr.language, Language::Japanese);
        assert_eq!(
            config.translation.target_language,
            AppConfig::default().translation.target_language
        );
    }

    #[test]
    fn ocr_language_update_syncs_linked_fields_and_preserves_others() {
        let mut config = AppConfig::default();
        apply_ocr_language(&mut config, Language::English);
        assert_eq!(config.ocr.language, Language::English);
        assert_eq!(config.translation.source_language, Language::English);
        assert_eq!(
            config.translation.target_language,
            AppConfig::default().translation.target_language
        );
    }

    #[test]
    fn linked_language_updates_cover_every_language_variant() {
        for &language in &[
            Language::Auto,
            Language::English,
            Language::Japanese,
            Language::ChineseSimplified,
        ] {
            let mut via_ocr = AppConfig::default();
            apply_ocr_language(&mut via_ocr, language);
            assert_eq!(via_ocr.ocr.language, language);
            assert_eq!(via_ocr.translation.source_language, language);

            let mut via_source = AppConfig::default();
            apply_source_language(&mut via_source, language);
            assert_eq!(via_source.ocr.language, language);
            assert_eq!(via_source.translation.source_language, language);
        }
    }

    #[test]
    fn linked_language_updates_pass_config_validation() {
        for &language in &[
            Language::Auto,
            Language::English,
            Language::Japanese,
            Language::ChineseSimplified,
        ] {
            let mut via_ocr = AppConfig::default();
            apply_ocr_language(&mut via_ocr, language);
            assert!(via_ocr.validate().is_ok(), "ocr path: {language:?}");

            let mut via_source = AppConfig::default();
            apply_source_language(&mut via_source, language);
            assert!(via_source.validate().is_ok(), "source path: {language:?}");
        }
    }

    #[test]
    fn target_language_update_mutates_only_target_field() {
        let mut config = AppConfig::default();
        apply_target_language(&mut config, Language::English);
        assert_eq!(config.translation.target_language, Language::English);
        assert_eq!(
            config.translation.source_language,
            AppConfig::default().translation.source_language
        );
    }

    #[test]
    fn target_language_auto_is_rejected_by_config_validation() {
        let mut config = AppConfig::default();
        apply_target_language(&mut config, Language::Auto);
        assert!(config.validate().is_err());
    }

    #[test]
    fn result_window_appearance_mutates_only_its_two_fields() {
        let mut config = AppConfig::default();
        apply_result_window_appearance(&mut config, 0.8, 18);
        assert!((config.result_window.opacity - 0.8).abs() < f64::EPSILON);
        assert_eq!(config.result_window.font_size_px, 18);
        assert_eq!(
            config.result_window.always_on_top,
            AppConfig::default().result_window.always_on_top
        );
        assert_eq!(config.floating_ball, AppConfig::default().floating_ball);
        assert_eq!(config.capture, AppConfig::default().capture);
        assert_eq!(config.translation, AppConfig::default().translation);
    }

    #[test]
    fn floating_ball_appearance_mutates_only_its_two_fields() {
        let mut config = AppConfig::default();
        apply_floating_ball_appearance(&mut config, 0.75, 56);
        assert!((config.floating_ball.opacity - 0.75).abs() < f64::EPSILON);
        assert_eq!(config.floating_ball.size_px, 56);
        assert_eq!(
            config.floating_ball.enabled,
            AppConfig::default().floating_ball.enabled
        );
        assert_eq!(config.result_window, AppConfig::default().result_window);
        assert_eq!(config.hotkeys, AppConfig::default().hotkeys);
    }

    #[test]
    fn result_window_appearance_out_of_range_is_rejected_by_validation() {
        for (opacity, font_size_px) in [(0.2, 14), (1.1, 14), (0.8, 11), (0.8, 25)] {
            let mut config = AppConfig::default();
            apply_result_window_appearance(&mut config, opacity, font_size_px);
            let error = AppError::from(config.validate().unwrap_err());
            assert!(
                matches!(error, AppError::Config(ConfigError::Validation(_))),
                "expected ConfigError::Validation for ({opacity}, {font_size_px})"
            );
        }
    }

    #[test]
    fn floating_ball_appearance_out_of_range_is_rejected_by_validation() {
        for (opacity, size_px) in [(0.2, 48), (1.1, 48), (1.0, 31), (1.0, 73)] {
            let mut config = AppConfig::default();
            apply_floating_ball_appearance(&mut config, opacity, size_px);
            let error = AppError::from(config.validate().unwrap_err());
            assert!(
                matches!(error, AppError::Config(ConfigError::Validation(_))),
                "expected ConfigError::Validation for ({opacity}, {size_px})"
            );
        }
    }

    #[test]
    fn appearance_updates_persist_and_survive_reload() {
        let dir = TestConfigDir::new();
        let manager = ConfigManager::new(dir.path()).unwrap();
        manager.save(&AppConfig::default()).unwrap();

        let mut config = manager.load().unwrap();
        apply_result_window_appearance(&mut config, 0.85, 16);
        apply_floating_ball_appearance(&mut config, 0.9, 60);
        manager.save(&config).unwrap();

        let persisted = manager.load().unwrap();
        assert!((persisted.result_window.opacity - 0.85).abs() < f64::EPSILON);
        assert_eq!(persisted.result_window.font_size_px, 16);
        assert!((persisted.floating_ball.opacity - 0.9).abs() < f64::EPSILON);
        assert_eq!(persisted.floating_ball.size_px, 60);
        assert_eq!(persisted.translation, AppConfig::default().translation);
    }

    #[test]
    fn appearance_update_out_of_range_is_rejected_without_writing() {
        let dir = TestConfigDir::new();
        let manager = ConfigManager::new(dir.path()).unwrap();
        manager.save(&AppConfig::default()).unwrap();

        let mut config = manager.load().unwrap();
        apply_result_window_appearance(&mut config, 0.2, 14);
        let error = manager.save(&config).unwrap_err();
        assert!(matches!(error, ConfigError::Validation(_)));

        // The on-disk config is untouched.
        assert_eq!(manager.load().unwrap(), AppConfig::default());
    }

    #[test]
    fn appearance_boundary_values_are_accepted() {
        let dir = TestConfigDir::new();
        let manager = ConfigManager::new(dir.path()).unwrap();
        manager.save(&AppConfig::default()).unwrap();

        // Inclusive range boundaries: opacity 0.3/1.0, font 12/24, size 32/72.
        let mut config = manager.load().unwrap();
        apply_result_window_appearance(&mut config, 0.3, 12);
        apply_floating_ball_appearance(&mut config, 1.0, 72);
        manager.save(&config).unwrap();

        let persisted = manager.load().unwrap();
        assert!((persisted.result_window.opacity - 0.3).abs() < f64::EPSILON);
        assert_eq!(persisted.result_window.font_size_px, 12);
        assert!((persisted.floating_ball.opacity - 1.0).abs() < f64::EPSILON);
        assert_eq!(persisted.floating_ball.size_px, 72);
    }

    #[test]
    fn appearance_persistence_is_independent_of_live_and_provider_state() {
        // Regression for bug 2 (backend side): appearance updates used to be
        // routed through the `save_settings` gate, which acquires the live
        // lifecycle lock, rejects saves while a live task is running, and
        // rebuilds the translation provider. The new commands persist through
        // a `ConfigManager`-only path — no live lifecycle, no live-task
        // handle, and no provider state is consulted — so they apply while a
        // live session is active. This test pins that contract by exercising
        // the exact mutation + save cycle the commands run with only a
        // `ConfigManager` in scope.
        let dir = TestConfigDir::new();
        let manager = ConfigManager::new(dir.path()).unwrap();
        manager.save(&AppConfig::default()).unwrap();

        let mut config = manager.load().unwrap();
        apply_result_window_appearance(&mut config, 0.8, 18);
        apply_floating_ball_appearance(&mut config, 0.7, 48);
        manager.save(&config).unwrap();

        // Persisting appearance never touches translation/OCR/capture state.
        let persisted = manager.load().unwrap();
        assert_eq!(persisted.translation, AppConfig::default().translation);
        assert_eq!(persisted.ocr, AppConfig::default().ocr);
        assert_eq!(persisted.capture, AppConfig::default().capture);
    }

    #[test]
    fn api_key_validation_trims_and_accepts_normal_keys() {
        assert_eq!(
            validate_api_key("  sk-test-1234  ").unwrap(),
            "sk-test-1234"
        );
    }

    #[test]
    fn api_key_validation_rejects_empty_and_whitespace() {
        for key in ["", "   ", "\t\n"] {
            let error = validate_api_key(key).unwrap_err();
            assert!(matches!(error, AppError::InvalidApiKey(_)));
        }
    }

    #[test]
    fn api_key_validation_rejects_oversized_keys() {
        let error = validate_api_key(&"a".repeat(MAX_API_KEY_LEN + 1)).unwrap_err();
        assert!(matches!(error, AppError::InvalidApiKey(_)));
    }

    #[test]
    fn credential_value_validation_trims_and_accepts_normal_values() {
        assert_eq!(
            validate_credential_value("  app-2024  ", "app id").unwrap(),
            "app-2024"
        );
    }

    #[test]
    fn credential_value_validation_rejects_empty_and_oversized_values() {
        let error = validate_credential_value("   ", "app id").unwrap_err();
        assert!(matches!(error, AppError::ProviderCredential(_)));
        let error =
            validate_credential_value(&"s".repeat(MAX_API_KEY_LEN + 1), "secret").unwrap_err();
        assert!(matches!(error, AppError::ProviderCredential(_)));
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

    // ── Multi-box config helpers ──

    #[test]
    fn add_box_config_assigns_next_id_and_color() {
        let mut config = AppConfig::default();
        let region = ScreenRegion::new("m0", 10, 20, 300, 400);
        let box_config = add_box_config(&mut config, region.clone());
        assert_eq!(box_config.id, 0);
        assert_eq!(box_config.color, "#FF6B6B");
        assert_eq!(config.translation_boxes.len(), 1);
        assert_eq!(config.translation_boxes[0].id, 0);
    }

    #[test]
    fn add_box_config_increments_id_and_uses_next_color() {
        let mut config = AppConfig::default();
        let region = ScreenRegion::new("m0", 10, 20, 300, 400);
        add_box_config(&mut config, region.clone());
        let box_config = add_box_config(&mut config, region);
        assert_eq!(box_config.id, 1);
        assert_eq!(box_config.color, "#4ECDC4");
        assert_eq!(config.translation_boxes.len(), 2);
    }

    #[test]
    fn remove_box_config_removes_by_id_and_preserves_others() {
        let mut config = AppConfig::default();
        let region = ScreenRegion::new("m0", 0, 0, 100, 100);
        add_box_config(&mut config, region.clone());
        add_box_config(&mut config, region.clone());
        add_box_config(&mut config, region);
        assert_eq!(config.translation_boxes.len(), 3);

        remove_box_config(&mut config, 1);
        assert_eq!(config.translation_boxes.len(), 2);
        assert_eq!(config.translation_boxes[0].id, 0);
        assert_eq!(config.translation_boxes[1].id, 2);
    }

    #[test]
    fn update_box_config_region_updates_existing_box() {
        let mut config = AppConfig::default();
        add_box_config(&mut config, ScreenRegion::new("m0", 0, 0, 100, 100));
        let new_region = ScreenRegion::new("m0", 10, 20, 300, 400);
        update_box_config_region(&mut config, 0, new_region.clone());
        assert_eq!(config.translation_boxes[0].region.x, 10);
        assert_eq!(config.translation_boxes[0].region.width, 300);
    }

    #[test]
    fn update_box_config_region_silently_skips_missing_id() {
        let mut config = AppConfig::default();
        add_box_config(&mut config, ScreenRegion::new("m0", 0, 0, 100, 100));
        let original = config.translation_boxes[0].clone();
        update_box_config_region(&mut config, 99, ScreenRegion::new("m0", 5, 5, 50, 50));
        assert_eq!(config.translation_boxes[0].region.x, original.region.x);
        assert_eq!(
            config.translation_boxes[0].region.width,
            original.region.width
        );
    }

    #[test]
    fn translation_box_info_from_config_preserves_all_fields() {
        let config = TranslationBoxConfig::new(5, ScreenRegion::new("m0", 1, 2, 3, 4), "#FF6B6B");
        let info = TranslationBoxInfo::from_config(&config);
        assert_eq!(info.box_id, 5);
        assert_eq!(info.color, "#FF6B6B");
        assert_eq!(info.region.width, 3);
        assert_eq!(info.region.monitor_id, "m0");
    }

    #[test]
    fn translation_box_info_from_pipeline_box_maps_id_to_box_id() {
        let box_ = TranslationBox::new(7, ScreenRegion::new("m0", 1, 2, 3, 4), "#4ECDC4");
        let info = TranslationBoxInfo::from_pipeline_box(&box_);
        assert_eq!(info.box_id, 7);
        assert_eq!(info.color, "#4ECDC4");
        assert_eq!(info.region.height, 4);
    }

    #[test]
    fn translation_box_info_serde_uses_box_id_field_name() {
        let info = TranslationBoxInfo {
            box_id: 3,
            region: ScreenRegion::new("m0", 10, 20, 300, 400),
            color: "#FF6B6B".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains(r#""box_id":3"#));
        assert!(json.contains("\"color\":\"#FF6B6B\""));
        let back: TranslationBoxInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.box_id, 3);
        assert_eq!(back.color, "#FF6B6B");
    }
}
