//! Shared application state and provider assembly.

use std::path::Path;
use std::sync::Arc;

use tauri::AppHandle;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use vtrans_capture::WindowsCaptureSource;
use vtrans_config::{AppConfig, ConfigManager};
use vtrans_core::traits::{CaptureSource, OcrProvider, TranslationProvider};
use vtrans_core::{OcrOptions, PipelineMode, PipelineStatus, ScreenRegion, TranslationRequest};
use vtrans_models::ModelManager;
use vtrans_ocr::PaddleOcrProvider;
use vtrans_pipeline::{MultiBoxConfig, MultiBoxPipeline};
use vtrans_pipeline::{Pipeline, PipelineConfig, PipelineDeps};
use vtrans_security::{CredentialManager, CredentialTarget};
use vtrans_translation::{
    AzureTranslatorProvider, BaiduProvider, DeepLProvider, GoogleV2Provider,
    LocalTranslationProvider, OpenAiProvider,
};

use crate::error::AppError;
use crate::window_visibility::SelectionVisibilityState;

/// The `model_id` used for translation provider loading progress events.
///
/// Reused by the provider switch and settings save paths so the frontend
/// shows a single "translation" progress bar during local model loads.
const TRANSLATION_PROGRESS_MODEL_ID: &str = "translation";

/// A lazily-loaded local translation provider cached across switches.
///
/// The local provider performs a heavy one-time load (SHA-256 verification
/// of the full model file, tokenizer parse, and ONNX session commit with
/// full graph optimization). Caching the assembled provider means only the
/// first switch to `"local"` pays that cost; subsequent switches reuse the
/// cached instance. The cache is invalidated when the configured model
/// directory changes.
///
/// This struct is kept separate from [`AppState`] so the cache hit/miss and
/// invalidation logic is unit-testable without a Windows capture/OCR/model
/// environment.
#[derive(Default)]
pub(crate) struct LocalProviderCache {
    /// The cached provider, or `None` when no load has succeeded yet.
    provider: std::sync::RwLock<Option<Arc<dyn TranslationProvider>>>,
    /// The model directory the cached provider was loaded from.
    model_dir: std::sync::RwLock<Option<std::path::PathBuf>>,
}

impl LocalProviderCache {
    /// Creates an empty cache.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Returns the cached provider when it is still valid for
    /// `configured_model_dir`, or `None` when the cache is empty or was
    /// loaded from a different directory.
    ///
    /// When the directory has changed the cache is cleared in place so a
    /// subsequent load can repopulate it. A `None` configured directory
    /// (the default `app_data_dir/models`) is treated as the empty path,
    /// matching how [`AppState::new_with_debug`] seeds the cache.
    pub(crate) fn get(
        &self,
        configured_model_dir: Option<&std::path::Path>,
    ) -> Option<Arc<dyn TranslationProvider>> {
        let configured =
            configured_model_dir.map_or_else(std::path::PathBuf::new, std::path::Path::to_path_buf);
        let cached_dir = self.model_dir.read().unwrap_or_else(poison_inner).clone();
        if cached_dir
            .as_ref()
            .is_some_and(|cached| *cached != configured)
        {
            debug!(
                old = ?cached_dir,
                new = %configured.display(),
                "model directory changed; invalidating local provider cache"
            );
            *self.provider.write().unwrap_or_else(poison_inner) = None;
            *self.model_dir.write().unwrap_or_else(poison_inner) = None;
            return None;
        }
        self.provider.read().unwrap_or_else(poison_inner).clone()
    }

    /// Records a freshly loaded provider and the directory it was loaded
    /// from.
    pub(crate) fn set(
        &self,
        provider: Arc<dyn TranslationProvider>,
        model_dir: Option<std::path::PathBuf>,
    ) {
        *self.provider.write().unwrap_or_else(poison_inner) = Some(provider);
        *self.model_dir.write().unwrap_or_else(poison_inner) = Some(model_dir.unwrap_or_default());
    }

    /// Returns `true` when the cache holds a provider valid for the given
    /// directory, without mutating state.
    #[cfg(test)]
    pub(crate) fn is_hit(&self, configured_model_dir: Option<&std::path::Path>) -> bool {
        let configured =
            configured_model_dir.map_or_else(std::path::PathBuf::new, std::path::Path::to_path_buf);
        let cached_dir = self.model_dir.read().unwrap_or_else(poison_inner).clone();
        cached_dir
            .as_ref()
            .is_some_and(|cached| *cached == configured)
            && self.provider.read().unwrap_or_else(poison_inner).is_some()
    }
}

/// A serializable snapshot returned by the `get_app_status` command.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppStatus {
    /// Session mode the backend last ran in (`"single"` or `"live"`).
    ///
    /// This mirrors the last session started by a command or hotkey: single
    /// captures and their region confirmations report `single`, a live
    /// session (running **or paused**) reports `live`. The frontend uses it
    /// during hydration to decide whether a selected region should restore
    /// the persistent overlay marker.
    pub mode: PipelineMode,
    /// Current pipeline status.
    pub pipeline_status: PipelineStatus,
    /// Stable identifier of the configured OCR provider.
    pub ocr_provider: String,
    /// Runtime implementation id of the configured translation provider
    /// (`"openai"`, `"deepl"`, `"google"`, `"azure"`, `"baidu"`, or
    /// `"local-onnx"`).
    ///
    /// Cloud providers use the same identifier in both domains; only the
    /// local provider differs (`"local-onnx"` at runtime vs `"local"` in
    /// configuration). The frontend maps it back via `normalizeProviderId`.
    pub translation_provider: String,
    /// Last selected region, when one has been selected.
    pub selected_region: Option<ScreenRegion>,
    /// Whether the live task has been started and has not finished.
    pub live_running: bool,
    /// Current model loading progress, if a load is in progress.
    pub model_progress: Option<f32>,
    /// Whether Debug mode (capture-frame preview) is enabled for this run.
    ///
    /// Never persisted; it is parsed from `--debug` / `VTRANS_DEBUG` at
    /// startup and is a plain mirror for the frontend to render the debug
    /// panel.
    pub debug_mode: bool,
}

/// Application-wide state managed by Tauri.
///
/// Providers are stored behind Arc and injected into each pipeline through
/// small trait-object adapters. This avoids reloading ONNX sessions and
/// recreating the capture backend for every command while preserving the
/// ownership contract required by `PipelineDeps`.
pub struct AppState {
    pub(crate) config: std::sync::RwLock<ConfigManager>,
    pub(crate) credentials: Arc<CredentialManager>,
    pub(crate) pipeline: std::sync::RwLock<Option<Arc<Pipeline>>>,
    pub(crate) ocr_provider: std::sync::RwLock<Arc<dyn OcrProvider>>,
    pub(crate) translation_provider: std::sync::RwLock<Arc<dyn TranslationProvider>>,
    pub(crate) capture_source: Arc<WindowsCaptureSource>,
    pub(crate) model_manager: Arc<ModelManager>,
    selected_region: std::sync::RwLock<Option<ScreenRegion>>,
    pub(crate) live_task: Mutex<Option<JoinHandle<()>>>,
    pub(crate) live_lifecycle: Mutex<()>,
    selection_waiter: Mutex<Option<oneshot::Sender<ScreenRegion>>>,
    app_handle: std::sync::RwLock<Option<AppHandle>>,
    model_progress: std::sync::RwLock<Option<f32>>,
    debug_mode: bool,
    current_mode: std::sync::RwLock<PipelineMode>,
    /// Lazily-loaded local translation provider, cached across switches.
    local_provider_cache: LocalProviderCache,

    /// Multi-box pipeline for multi-region real-time translation.
    ///
    /// Lazily created on the first `add_translation_box` or
    /// `start_multi_realtime` call so config changes between calls take
    /// effect without recreating the pipeline.
    pub(crate) multi_pipeline: std::sync::RwLock<Option<Arc<MultiBoxPipeline>>>,

    /// Background task forwarding multi-box results and box-status changes
    /// to the frontend via Tauri events.
    pub(crate) multi_forwarder: Mutex<Option<JoinHandle<()>>>,

    /// IDs of translation boxes currently registered in the multi-box
    /// pipeline. Shared with the forwarder task so dynamically
    /// added/removed boxes are polled for status changes.
    pub(crate) multi_box_ids: Arc<std::sync::RwLock<Vec<u32>>>,

    /// Pre-selection visibility snapshot of the main/result/floater windows.
    ///
    /// Recorded when a region selection starts (first snapshot wins) and
    /// consumed by the restore after a follow-up action completes, or
    /// immediately when the selection is aborted. The lifecycle decisions
    /// live in the pure state machine `window_visibility::SelectionVisibilityState`.
    selection_visibility: SelectionVisibilityState,
}

impl AppState {
    /// Constructs the application state and all production providers.
    ///
    /// `app_data_dir` is used for config.json and, unless overridden by the
    /// persisted configuration, for a models/manifest.json directory.
    ///
    /// # Errors
    ///
    /// Returns an application error when config, models, capture, OCR, or
    /// translation cannot initialize.
    #[tracing::instrument(skip(app_data_dir))]
    pub fn new(app_data_dir: &Path) -> Result<Self, AppError> {
        Self::new_with_debug(app_data_dir, false)
    }

    /// Constructs the application state with Debug mode explicitly enabled
    /// or disabled.
    ///
    /// Debug mode is a per-run flag (command line / environment), never
    /// persisted. See [`new`](Self::new) for the remaining contract.
    ///
    /// # Errors
    ///
    /// Returns an application error when config, models, capture, OCR, or
    /// translation cannot initialize.
    #[tracing::instrument(skip(app_data_dir, debug_mode))]
    pub fn new_with_debug(app_data_dir: &Path, debug_mode: bool) -> Result<Self, AppError> {
        let config_manager = ConfigManager::new(app_data_dir)?;
        let config = config_manager.load()?;
        let credentials = Arc::new(CredentialManager::new()?);
        let model_dir = config
            .model_dir
            .clone()
            .unwrap_or_else(|| app_data_dir.join("models"));
        let model_manager = Arc::new(ModelManager::from_manifest_dir(&model_dir)?);
        let capture_source = Arc::new(WindowsCaptureSource::new()?);
        let ocr_provider =
            Arc::new(PaddleOcrProvider::from_manager(&model_manager)?) as Arc<dyn OcrProvider>;
        let translation_provider =
            build_translation_provider(&config, &credentials, &model_manager)?;
        info!(
            ocr_provider = ocr_provider.id(),
            translation_provider = translation_provider.id(),
            model_dir = %model_dir.display(),
            "application state initialized"
        );
        // Seed the local provider cache when the startup configuration
        // already selects the local provider, so the first runtime switch
        // back to "local" reuses this instance instead of reloading.
        let local_provider_cache = LocalProviderCache::new();
        if config.translation.provider == "local" {
            local_provider_cache.set(Arc::clone(&translation_provider), config.model_dir.clone());
        }
        Ok(Self {
            config: std::sync::RwLock::new(config_manager),
            credentials,
            pipeline: std::sync::RwLock::new(None),
            ocr_provider: std::sync::RwLock::new(ocr_provider),
            translation_provider: std::sync::RwLock::new(translation_provider),
            capture_source,
            model_manager,
            selected_region: std::sync::RwLock::new(None),
            live_task: Mutex::new(None),
            live_lifecycle: Mutex::new(()),
            selection_waiter: Mutex::new(None),
            app_handle: std::sync::RwLock::new(None),
            model_progress: std::sync::RwLock::new(None),
            debug_mode,
            current_mode: std::sync::RwLock::new(PipelineMode::SingleCapture),
            local_provider_cache,
            multi_pipeline: std::sync::RwLock::new(None),
            multi_forwarder: Mutex::new(None),
            multi_box_ids: Arc::new(std::sync::RwLock::new(Vec::new())),
            selection_visibility: SelectionVisibilityState::new(),
        })
    }

    /// Returns whether Debug mode is enabled for this run.
    #[must_use]
    pub(crate) fn debug_mode(&self) -> bool {
        self.debug_mode
    }

    /// Records the session mode of the most recent command or hotkey.
    ///
    /// The mode is a per-run mirror used by [`AppStatus::mode`]; it is never
    /// persisted. A stop keeps the last mode so a paused live session still
    /// reports `live`; only the next region confirmation or single capture
    /// switches it back.
    pub(crate) fn set_current_mode(&self, mode: PipelineMode) {
        *self.current_mode.write().unwrap_or_else(poison_inner) = mode;
    }

    /// Returns the session mode of the most recent command or hotkey.
    #[must_use]
    pub(crate) fn current_mode(&self) -> PipelineMode {
        *self.current_mode.read().unwrap_or_else(poison_inner)
    }

    pub(crate) fn attach_handle(&self, app: AppHandle) {
        *self.app_handle.write().unwrap_or_else(poison_inner) = Some(app);
    }

    pub(crate) fn app_handle(&self) -> Result<AppHandle, AppError> {
        self.app_handle
            .read()
            .unwrap_or_else(poison_inner)
            .clone()
            .ok_or(AppError::NotInitialized)
    }

    pub(crate) async fn begin_region_selection(
        &self,
    ) -> Result<oneshot::Receiver<ScreenRegion>, AppError> {
        let (sender, receiver) = oneshot::channel();
        let mut waiter = self.selection_waiter.lock().await;
        if waiter.is_some() {
            return Err(AppError::SelectionInProgress);
        }
        *waiter = Some(sender);
        Ok(receiver)
    }

    pub(crate) async fn cancel_region_selection(&self) {
        let _ = self.selection_waiter.lock().await.take();
    }

    pub(crate) fn load_config(&self) -> Result<AppConfig, AppError> {
        let manager = self.config.read().unwrap_or_else(poison_inner);
        Ok(manager.load()?)
    }

    pub(crate) fn save_config(&self, config: &AppConfig) -> Result<(), AppError> {
        let manager = self.config.read().unwrap_or_else(poison_inner);
        Ok(manager.save(config)?)
    }

    pub(crate) fn update_config<F>(&self, f: F) -> Result<(), AppError>
    where
        F: FnOnce(&mut AppConfig),
    {
        let manager = self.config.read().unwrap_or_else(poison_inner);
        Ok(manager.update(f)?)
    }

    pub(crate) fn selected_region(&self) -> Option<ScreenRegion> {
        self.selected_region
            .read()
            .unwrap_or_else(poison_inner)
            .clone()
    }

    pub(crate) async fn set_selected_region(&self, region: ScreenRegion) -> Result<(), AppError> {
        region.validate().map_err(AppError::from)?;
        *self.selected_region.write().unwrap_or_else(poison_inner) = Some(region.clone());
        if let Some(sender) = self.selection_waiter.lock().await.take() {
            let _ = sender.send(region);
        }
        Ok(())
    }

    /// Builds a pipeline with an optional frame observer.
    ///
    /// The sink receives every frame that is about to enter OCR. Debug mode
    /// attaches the debug frame forwarder; `None` keeps the exact production
    /// capture path.
    pub(crate) fn build_pipeline(
        &self,
        mode: PipelineMode,
        region: ScreenRegion,
        capture_interval_ms: u32,
        difference_threshold: f32,
        frame_sink: Option<std::sync::Arc<dyn vtrans_pipeline::FrameSink>>,
    ) -> Result<Pipeline, AppError> {
        region.validate().map_err(AppError::from)?;
        let monitor_count = self.capture_source.list_monitors().len();
        debug!(monitor_count, "assembling pipeline capture source");
        let config = self.load_config()?;
        let ocr_options = OcrOptions {
            language: config.ocr.language,
            min_confidence: config.ocr.min_confidence,
            detect_vertical: true,
        };
        let translation_request = TranslationRequest::new(
            "",
            config.translation.source_language,
            config.translation.target_language,
        );
        // Single captures never use the interval or difference threshold;
        // the dedicated constructors keep the defaults for each mode
        // explicit instead of repeating magic values.
        let pipeline_config = if mode.is_live() {
            PipelineConfig::live(
                region,
                capture_interval_ms,
                difference_threshold,
                ocr_options,
                translation_request,
            )
        } else {
            PipelineConfig::single(region, ocr_options, translation_request)
        };
        let capture = Box::new(SharedCaptureSource(Arc::clone(&self.capture_source)))
            as Box<dyn CaptureSource>;
        let ocr = Box::new(SharedOcrProvider(
            self.ocr_provider
                .read()
                .unwrap_or_else(poison_inner)
                .clone(),
        )) as Box<dyn OcrProvider>;
        let translation = Box::new(SharedTranslationProvider(
            self.translation_provider
                .read()
                .unwrap_or_else(poison_inner)
                .clone(),
        )) as Box<dyn TranslationProvider>;
        Ok(Pipeline::with_frame_sink(
            pipeline_config,
            PipelineDeps::new(capture, ocr, translation),
            frame_sink,
        ))
    }

    /// Assembles the translation provider for `config`, reusing the cached
    /// local provider when possible.
    ///
    /// Cloud providers are cheap to assemble (no I/O) and always rebuilt
    /// so credential and endpoint changes take effect. The local provider
    /// is heavy (SHA-256 + ONNX session commit); it is loaded once and
    /// cached, so only the first switch to `"local"` pays the full cost.
    /// When `progress` is `Some`, `model_loading_progress` events are
    /// emitted before the load (`0.0`) and after a hit or successful load
    /// (`1.0`), giving the frontend feedback during the first load and a
    /// near-instant completion when the cache is hit.
    ///
    /// # Errors
    ///
    /// Returns an application error when the configured provider id is
    /// unsupported, the vault cannot be read, or the local provider cannot
    /// be assembled. A load failure leaves the cache untouched so the next
    /// attempt retries from scratch.
    #[tracing::instrument(skip(self, config, progress), fields(provider = %config.translation.provider))]
    pub(crate) async fn prepare_translation_provider(
        &self,
        config: AppConfig,
        progress: Option<&AppHandle>,
    ) -> Result<Arc<dyn TranslationProvider>, AppError> {
        if config.translation.provider == "local" {
            return self
                .prepare_local_translation_provider(&config, progress)
                .await;
        }
        // Cloud providers perform no I/O; rebuild every time so credential
        // and endpoint changes apply immediately.
        let credentials = Arc::clone(&self.credentials);
        tokio::task::spawn_blocking(move || build_cloud_translation_provider(&config, &credentials))
            .await
            .map_err(|error| {
                AppError::Tauri(format!("translation provider setup task failed: {error}"))
            })?
    }

    /// Returns the cached local provider or loads it once, emitting progress
    /// around the load.
    async fn prepare_local_translation_provider(
        &self,
        config: &AppConfig,
        progress: Option<&AppHandle>,
    ) -> Result<Arc<dyn TranslationProvider>, AppError> {
        if let Some(cached) = self.local_provider_cache.get(config.model_dir.as_deref()) {
            debug!("local translation provider cache hit; reusing loaded session");
            self.emit_provider_progress(progress, 1.0);
            return Ok(cached);
        }
        self.emit_provider_progress(progress, 0.0);
        let model_manager = Arc::clone(&self.model_manager);
        let provider = tokio::task::spawn_blocking(move || {
            LocalTranslationProvider::from_manager(&model_manager)
                .map(|provider| Arc::new(provider) as Arc<dyn TranslationProvider>)
                .map_err(AppError::from)
        })
        .await
        .map_err(|error| {
            AppError::Tauri(format!(
                "local translation provider load task failed: {error}"
            ))
        })??;
        self.local_provider_cache
            .set(Arc::clone(&provider), config.model_dir.clone());
        info!("local translation provider loaded and cached");
        self.emit_provider_progress(progress, 1.0);
        Ok(provider)
    }

    /// Emits a translation provider loading progress event and mirrors it
    /// into `AppState` for `get_app_status`.
    fn emit_provider_progress(&self, app: Option<&AppHandle>, progress: f32) {
        self.set_model_progress(Some(progress));
        if let Some(app) = app {
            crate::events::emit_model_loading_progress(
                app,
                TRANSLATION_PROGRESS_MODEL_ID,
                progress,
            );
        }
    }

    /// Switches the active translation provider, caching the local provider
    /// and emitting loading progress when `progress` is provided.
    ///
    /// # Errors
    ///
    /// Returns an application error for an unsupported provider id, a
    /// failed provider load, or a configuration persistence failure.
    pub(crate) async fn set_translation_provider_id(
        &self,
        provider_id: &str,
        progress: Option<&AppHandle>,
    ) -> Result<(), AppError> {
        let mut config = self.load_config()?;
        update_translation_provider_config(&mut config, provider_id)?;
        let provider = self
            .prepare_translation_provider(config.clone(), progress)
            .await?;
        self.save_config(&config)?;
        self.replace_translation_provider(provider);
        Ok(())
    }

    pub(crate) fn replace_translation_provider(&self, provider: Arc<dyn TranslationProvider>) {
        let id = provider.id();
        *self
            .translation_provider
            .write()
            .unwrap_or_else(poison_inner) = provider;
        *self.pipeline.write().unwrap_or_else(poison_inner) = None;
        info!(translation_provider = id, "translation provider replaced");
    }

    pub(crate) fn set_pipeline(&self, pipeline: Pipeline) -> Arc<Pipeline> {
        let pipeline = Arc::new(pipeline);
        *self.pipeline.write().unwrap_or_else(poison_inner) = Some(Arc::clone(&pipeline));
        pipeline
    }

    pub(crate) fn pipeline(&self) -> Option<Arc<Pipeline>> {
        self.pipeline.read().unwrap_or_else(poison_inner).clone()
    }

    pub(crate) fn clear_pipeline(&self) {
        *self.pipeline.write().unwrap_or_else(poison_inner) = None;
    }

    pub(crate) fn status_snapshot(&self, live_running: bool) -> AppStatus {
        let mode = self.current_mode();
        let pipeline_status = self
            .pipeline()
            .map_or(PipelineStatus::Idle, |pipeline| pipeline.status());
        let ocr_provider = self
            .ocr_provider
            .read()
            .unwrap_or_else(poison_inner)
            .id()
            .to_string();
        let translation_provider = self
            .translation_provider
            .read()
            .unwrap_or_else(poison_inner)
            .id()
            .to_string();
        let model_progress = *self.model_progress.read().unwrap_or_else(poison_inner);
        AppStatus {
            mode,
            pipeline_status,
            ocr_provider,
            translation_provider,
            selected_region: self.selected_region(),
            live_running,
            model_progress,
            debug_mode: self.debug_mode,
        }
    }

    pub(crate) fn set_model_progress(&self, progress: Option<f32>) {
        *self.model_progress.write().unwrap_or_else(poison_inner) = progress;
    }

    pub(crate) async fn live_task_is_running(&self) -> bool {
        let mut task = self.live_task.lock().await;
        if task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
        {
            let _ = task.take();
        }
        task.is_some()
    }

    // ── Multi-box pipeline lifecycle ──

    /// Builds a multi-box pipeline from the current configuration.
    ///
    /// The pipeline is assembled with the same shared capture, OCR, and
    /// translation providers as the single-box pipeline so provider
    /// switches apply to both. `max_boxes` is read from the config.
    pub(crate) fn build_multi_pipeline(&self) -> Result<MultiBoxPipeline, AppError> {
        let config = self.load_config()?;
        let ocr_options = OcrOptions {
            language: config.ocr.language,
            min_confidence: config.ocr.min_confidence,
            detect_vertical: true,
        };
        let translation_request = TranslationRequest::new(
            "",
            config.translation.source_language,
            config.translation.target_language,
        );
        let multi_config = MultiBoxConfig::with_max_boxes(
            config.capture.interval_ms,
            config.capture.difference_threshold,
            ocr_options,
            translation_request,
            config.max_boxes,
        );
        let capture = Box::new(SharedCaptureSource(Arc::clone(&self.capture_source)))
            as Box<dyn CaptureSource>;
        let ocr = Box::new(SharedOcrProvider(
            self.ocr_provider
                .read()
                .unwrap_or_else(poison_inner)
                .clone(),
        )) as Box<dyn OcrProvider>;
        let translation = Box::new(SharedTranslationProvider(
            self.translation_provider
                .read()
                .unwrap_or_else(poison_inner)
                .clone(),
        )) as Box<dyn TranslationProvider>;
        Ok(MultiBoxPipeline::new(
            multi_config,
            PipelineDeps::new(capture, ocr, translation),
        ))
    }

    /// Returns the multi-box pipeline, creating it on first access.
    ///
    /// The pipeline is lazily created so config changes between calls
    /// (language, threshold, etc.) take effect without recreating it.
    pub(crate) fn ensure_multi_pipeline(&self) -> Result<Arc<MultiBoxPipeline>, AppError> {
        let mut guard = self.multi_pipeline.write().unwrap_or_else(poison_inner);
        if let Some(pipeline) = guard.as_ref() {
            return Ok(Arc::clone(pipeline));
        }
        let pipeline = Arc::new(self.build_multi_pipeline()?);
        *guard = Some(Arc::clone(&pipeline));
        Ok(pipeline)
    }

    /// Returns the current multi-box pipeline, if one has been created.
    #[must_use]
    pub(crate) fn multi_pipeline(&self) -> Option<Arc<MultiBoxPipeline>> {
        self.multi_pipeline
            .read()
            .unwrap_or_else(poison_inner)
            .clone()
    }

    /// Clears the multi-box pipeline and aborts the forwarder task.
    ///
    /// Called when the multi-box session is stopped so the next start
    /// rebuilds the pipeline from the latest config.
    pub(crate) async fn clear_multi_pipeline(&self) {
        let task = self.multi_forwarder.lock().await.take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        *self.multi_pipeline.write().unwrap_or_else(poison_inner) = None;
        self.multi_box_ids
            .write()
            .unwrap_or_else(poison_inner)
            .clear();
    }

    /// Stores the forwarder task handle, aborting any previous one.
    pub(crate) async fn set_multi_forwarder(&self, task: JoinHandle<()>) {
        let old = self.multi_forwarder.lock().await.replace(task);
        if let Some(old) = old {
            old.abort();
        }
    }

    /// Records a translation box ID for status polling.
    pub(crate) fn add_multi_box_id(&self, box_id: u32) {
        let mut ids = self.multi_box_ids.write().unwrap_or_else(poison_inner);
        if !ids.contains(&box_id) {
            ids.push(box_id);
        }
    }

    /// Removes a translation box ID from status polling.
    pub(crate) fn remove_multi_box_id(&self, box_id: u32) {
        let mut ids = self.multi_box_ids.write().unwrap_or_else(poison_inner);
        ids.retain(|&id| id != box_id);
    }

    /// Returns a snapshot of the current translation box IDs.
    #[must_use]
    pub(crate) fn multi_box_ids_snapshot(&self) -> Vec<u32> {
        self.multi_box_ids
            .read()
            .unwrap_or_else(poison_inner)
            .clone()
    }

    /// Returns a handle to the shared box-ID list for the forwarder task.
    #[must_use]
    pub(crate) fn multi_box_ids_handle(&self) -> Arc<std::sync::RwLock<Vec<u32>>> {
        Arc::clone(&self.multi_box_ids)
    }

    /// Returns the pre-selection window visibility state machine.
    ///
    /// Commands feed selection lifecycle transitions into it and execute the
    /// returned window action; see `window_visibility` for the contract.
    #[must_use]
    pub(crate) fn selection_visibility(&self) -> &SelectionVisibilityState {
        &self.selection_visibility
    }
}

#[derive(Clone)]
struct SharedCaptureSource(Arc<WindowsCaptureSource>);

#[async_trait::async_trait]
impl CaptureSource for SharedCaptureSource {
    async fn capture_once(
        &self,
        region: &ScreenRegion,
    ) -> Result<vtrans_core::CapturedImage, vtrans_core::CaptureError> {
        self.0.capture_once(region).await
    }

    async fn start_session(
        &self,
        region: &ScreenRegion,
    ) -> Result<Box<dyn vtrans_core::CaptureSession>, vtrans_core::CaptureError> {
        self.0.start_session(region).await
    }
}

#[derive(Clone)]
struct SharedOcrProvider(Arc<dyn OcrProvider>);

#[async_trait::async_trait]
impl OcrProvider for SharedOcrProvider {
    fn id(&self) -> &'static str {
        self.0.id()
    }

    async fn recognize(
        &self,
        image: &vtrans_core::CapturedImage,
        region: &ScreenRegion,
        options: &OcrOptions,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<vtrans_core::OcrResult, vtrans_core::OcrError> {
        self.0.recognize(image, region, options, cancel).await
    }

    fn supported_languages(&self) -> &[vtrans_core::Language] {
        self.0.supported_languages()
    }
}

#[derive(Clone)]
struct SharedTranslationProvider(Arc<dyn TranslationProvider>);

#[async_trait::async_trait]
impl TranslationProvider for SharedTranslationProvider {
    fn id(&self) -> &'static str {
        self.0.id()
    }

    async fn translate(
        &self,
        request: &vtrans_core::TranslationRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<vtrans_core::TranslationResult, vtrans_core::TranslationError> {
        self.0.translate(request, cancel).await
    }

    fn supported_pairs(&self) -> &[(vtrans_core::Language, vtrans_core::Language)] {
        self.0.supported_pairs()
    }
}

/// Assembles the translation provider configured by `config`.
///
/// Cloud provider credentials are read from the credential vault through
/// `credentials` (never from configuration or logs): `OpenAI`, `DeepL`,
/// `Google`, and `Azure` use a single API key target; `Baidu` reads
/// `baidu_app_id` and `baidu_secret` as two independent targets. The local
/// provider loads its ONNX session through `model_manager`.
///
/// Provider construction performs no network I/O, so this function is safe
/// to unit-test with an in-memory credential store.
///
/// # Errors
///
/// Returns an application error when the configured provider id is
/// unsupported, the vault cannot be read, or the local provider cannot be
/// assembled from `model_manager`.
fn build_translation_provider(
    config: &AppConfig,
    credentials: &CredentialManager,
    model_manager: &ModelManager,
) -> Result<Arc<dyn TranslationProvider>, AppError> {
    validate_translation_provider_id(&config.translation.provider)?;
    if config.translation.provider == "local" {
        return Ok(Arc::new(LocalTranslationProvider::from_manager(
            model_manager,
        )?));
    }
    build_cloud_translation_provider(config, credentials)
}

/// Assembles one of the cloud translation providers (`openai`, `deepl`,
/// `google`, `azure`, or `baidu`) from the configuration snapshot.
///
/// Credentials are read from the vault (a single key target for the first
/// four, `baidu_app_id` + `baidu_secret` for Baidu). Provider construction
/// performs no network I/O, so this function is unit-testable with an
/// in-memory credential store and no model files.
///
/// # Errors
///
/// Returns an application error when the provider id is not a cloud
/// provider or the vault cannot be read.
fn build_cloud_translation_provider(
    config: &AppConfig,
    credentials: &CredentialManager,
) -> Result<Arc<dyn TranslationProvider>, AppError> {
    let provider = config.translation.provider.as_str();
    let api_endpoint = effective_translation_endpoint(provider, &config.translation.api_endpoint);
    let credentials = load_provider_credentials(credentials, config)?;
    let timeout = std::time::Duration::from_secs(u64::from(config.translation.timeout_seconds));
    let max_retries = config.translation.max_retries;
    let provider: Arc<dyn TranslationProvider> = match provider {
        "openai" => Arc::new(OpenAiProvider::new(
            &api_endpoint,
            &config.translation.api_model,
            &credentials.api_key,
            timeout,
            max_retries,
        )),
        "deepl" => Arc::new(DeepLProvider::new(
            &api_endpoint,
            &credentials.api_key,
            timeout,
            max_retries,
        )),
        "google" => Arc::new(GoogleV2Provider::new(
            &api_endpoint,
            &credentials.api_key,
            timeout,
            max_retries,
        )),
        "azure" => Arc::new(AzureTranslatorProvider::new(
            &api_endpoint,
            config.translation.region.as_deref().unwrap_or(""),
            &credentials.api_key,
            timeout,
            max_retries,
        )),
        "baidu" => Arc::new(BaiduProvider::new(
            &api_endpoint,
            &credentials.app_id,
            &credentials.secret,
            timeout,
            max_retries,
        )),
        _ => unreachable!("provider id was validated above"),
    };
    Ok(provider)
}

/// Validates a translation provider identifier against the stable
/// application-level configuration domain.
///
/// The domain is `"openai"`, `"deepl"`, `"google"`, `"azure"`, `"baidu"`,
/// and `"local"` — the same whitelist enforced by `vtrans-config`
/// validation, so configuration snapshots and runtime provider selection
/// can never disagree. The legacy `"api"` id is intentionally rejected:
/// `OpenAI` is identified as `"openai"` since the cloud-provider refactor.
pub(crate) fn validate_translation_provider_id(provider_id: &str) -> Result<(), AppError> {
    if matches!(
        provider_id,
        "openai" | "deepl" | "google" | "azure" | "baidu" | "local"
    ) {
        Ok(())
    } else {
        warn!(provider = provider_id, "unsupported translation provider");
        Err(vtrans_core::TranslationError::Inference(format!(
            "unsupported translation provider: {provider_id}"
        ))
        .into())
    }
}

/// Applies a provider selection to a configuration snapshot.
///
/// Kept as a pure function so the mutation performed by
/// [`AppState::set_translation_provider_id`] can be unit-tested without a
/// Tauri runtime.
fn update_translation_provider_config(
    config: &mut AppConfig,
    provider_id: &str,
) -> Result<(), AppError> {
    validate_translation_provider_id(provider_id)?;
    config.translation.provider = provider_id.to_string();
    if let Some(endpoint) = provider_default_endpoint(provider_id) {
        config.translation.api_endpoint = endpoint.to_string();
    }
    Ok(())
}

/// Canonical HTTP endpoint for each remote translation provider.
fn provider_default_endpoint(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "openai" => Some("https://api.openai.com/v1/chat/completions"),
        "deepl" => Some("https://api-free.deepl.com/v2/translate"),
        "google" => Some("https://translation.googleapis.com/language/translate/v2"),
        "azure" => Some("https://api.cognitive.microsofttranslator.com/translate"),
        "baidu" => Some("https://fanyi-api.baidu.com/api/trans/vip/translate"),
        _ => None,
    }
}

/// Repairs a stale built-in endpoint left by an earlier provider selection.
///
/// Custom endpoints remain untouched.
fn effective_translation_endpoint(provider_id: &str, configured: &str) -> String {
    let Some(default_endpoint) = provider_default_endpoint(provider_id) else {
        return configured.to_string();
    };
    let configured = configured.trim();
    let stale_builtin_endpoint = ["openai", "deepl", "google", "azure", "baidu"]
        .iter()
        .filter(|candidate| **candidate != provider_id)
        .filter_map(|candidate| provider_default_endpoint(candidate))
        .any(|endpoint| endpoint == configured);

    if configured.is_empty() || stale_builtin_endpoint {
        default_endpoint.to_string()
    } else {
        configured.to_string()
    }
}

/// Credentials for one cloud translation provider, loaded from the vault.
///
/// Only the fields used by the configured provider are populated; the value
/// lives in memory for the lifetime of the assembled provider and never
/// enters configuration, events, or logs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProviderCredentials {
    /// Single API key for OpenAI/DeepL/Google/Azure.
    api_key: String,
    /// Baidu APP ID (non-secret, but stored in the vault for a single
    /// credential lifecycle).
    app_id: String,
    /// Baidu secret key used for request signing.
    secret: String,
}

/// Returns the single credential target for a provider, or `None` for
/// providers that need no key (`"local"`) or two targets (`"baidu"`).
pub(crate) fn provider_credential_target(provider_id: &str) -> Option<CredentialTarget> {
    match provider_id {
        "openai" => Some(CredentialTarget::OpenAI),
        "deepl" => Some(CredentialTarget::DeepL),
        "google" => Some(CredentialTarget::Google),
        "azure" => Some(CredentialTarget::Azure),
        _ => None,
    }
}

/// Loads the credentials required by the configured provider from the vault.
///
/// Missing credentials are tolerated with a warning and become empty
/// strings, matching the legacy behavior where an unconfigured key produced
/// an unauthenticated request that the provider maps to 401/403. Credential
/// values are returned to the caller and never logged.
///
/// # Errors
///
/// Returns `AppError::Security` when the vault cannot be read, or
/// `AppError::ProviderCredential` for an unsupported provider.
fn load_provider_credentials(
    credentials: &CredentialManager,
    config: &AppConfig,
) -> Result<ProviderCredentials, AppError> {
    let provider = config.translation.provider.as_str();
    if let Some(target) = provider_credential_target(provider) {
        let api_key = credentials.load_for_provider(target)?.unwrap_or_else(|| {
            warn!(provider, "translation credential is not configured");
            String::new()
        });
        return Ok(ProviderCredentials {
            api_key,
            ..ProviderCredentials::default()
        });
    }
    match provider {
        "baidu" => {
            let app_id = credentials
                .load_for_provider(CredentialTarget::BaiduAppId)?
                .unwrap_or_else(|| {
                    warn!("baidu app id is not configured");
                    String::new()
                });
            let secret = credentials
                .load_for_provider(CredentialTarget::BaiduSecret)?
                .unwrap_or_else(|| {
                    warn!("baidu secret is not configured");
                    String::new()
                });
            Ok(ProviderCredentials {
                app_id,
                secret,
                ..ProviderCredentials::default()
            })
        }
        "local" => Ok(ProviderCredentials::default()),
        other => Err(AppError::ProviderCredential(format!(
            "provider {other:?} has no credential targets"
        ))),
    }
}

/// Stores a single API key for a cloud provider in the OS credential vault.
///
/// The logical target is derived from `provider_id` (`openai`, `deepl`,
/// `google`, `azure`); Baidu stores the value under `baidu_secret` (its APP
/// ID lives in the separate `baidu_app_id` target, see
/// [`store_provider_credentials`]). The `local` provider and unknown ids are
/// rejected. The key is never logged.
///
/// # Errors
///
/// Returns `AppError::Security` when the underlying store cannot persist the
/// key, or `AppError::ProviderCredential` when the provider does not accept
/// a single API key.
pub(crate) fn store_api_key(
    credentials: &CredentialManager,
    provider_id: &str,
    api_key: &str,
) -> Result<(), AppError> {
    let target = if provider_id == "baidu" {
        CredentialTarget::BaiduSecret
    } else {
        provider_credential_target(provider_id).ok_or_else(|| {
            AppError::ProviderCredential(format!(
                "provider {provider_id:?} does not accept an API key"
            ))
        })?
    };
    credentials
        .store_for_provider(target, api_key)
        .map_err(AppError::from)
}

/// Stores the complete credential set for one cloud provider in the vault.
///
/// OpenAI/DeepL/Google/Azure store `api_key` under their single target;
/// Baidu stores `app_id` and `secret` under the two independent
/// `baidu_app_id` / `baidu_secret` targets, matching how provider assembly
/// reads them. Credential values are never logged.
///
/// # Errors
///
/// Returns `AppError::ProviderCredential` when a required value is missing
/// or the provider is not credential-backed, and `AppError::Security` when
/// the vault cannot persist a value.
pub(crate) fn store_provider_credentials(
    credentials: &CredentialManager,
    provider_id: &str,
    api_key: Option<&str>,
    app_id: Option<&str>,
    secret: Option<&str>,
) -> Result<(), AppError> {
    if provider_id == "baidu" {
        let app_id = app_id
            .ok_or_else(|| AppError::ProviderCredential("baidu requires an app id".to_string()))?;
        let secret = secret.ok_or_else(|| {
            AppError::ProviderCredential("baidu requires a secret key".to_string())
        })?;
        credentials
            .store_for_provider(CredentialTarget::BaiduAppId, app_id)
            .map_err(AppError::from)?;
        return credentials
            .store_for_provider(CredentialTarget::BaiduSecret, secret)
            .map_err(AppError::from);
    }
    let Some(target) = provider_credential_target(provider_id) else {
        return Err(AppError::ProviderCredential(format!(
            "provider {provider_id:?} does not accept credentials"
        )));
    };
    let api_key = api_key.ok_or_else(|| {
        AppError::ProviderCredential(format!("provider {provider_id:?} requires an api key"))
    })?;
    credentials
        .store_for_provider(target, api_key)
        .map_err(AppError::from)
}

fn poison_inner<T>(poisoned: std::sync::PoisonError<T>) -> T {
    debug!("recovering poisoned application state lock");
    poisoned.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtrans_security::credential_store::InMemoryCredentialStore;

    fn memory_credentials() -> CredentialManager {
        CredentialManager::with_store(Arc::new(InMemoryCredentialStore::new()))
    }

    fn cloud_config(provider: &str) -> AppConfig {
        let mut config = AppConfig::default();
        config.translation.provider = provider.to_string();
        config
    }

    #[test]
    fn status_snapshot_contract_is_serializable() {
        let status = AppStatus {
            mode: PipelineMode::SingleCapture,
            pipeline_status: PipelineStatus::Idle,
            ocr_provider: "mock-ocr".to_string(),
            translation_provider: "openai".to_string(),
            selected_region: None,
            live_running: false,
            model_progress: None,
            debug_mode: false,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""mode":"single""#));
        assert!(json.contains("pipeline_status"));
        assert!(json.contains("mock-ocr"));
        assert!(json.contains(r#""translation_provider":"openai""#));
        assert!(json.contains(r#""debug_mode":false"#));
    }

    #[test]
    fn translation_provider_validation_accepts_known_ids() {
        for provider in ["openai", "deepl", "google", "azure", "baidu", "local"] {
            assert!(
                validate_translation_provider_id(provider).is_ok(),
                "provider {provider:?} must validate"
            );
        }
    }

    #[test]
    fn translation_provider_validation_rejects_unknown_ids() {
        for provider in ["api", "local-onnx", "deepseek", ""] {
            let error = validate_translation_provider_id(provider).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("unsupported translation provider"),
                "provider {provider:?}: {error}"
            );
        }
    }

    #[test]
    fn provider_config_update_sets_identifier_and_preserves_other_fields() {
        let mut config = AppConfig::default();
        update_translation_provider_config(&mut config, "local").unwrap();
        assert_eq!(config.translation.provider, "local");
        assert_eq!(
            config.translation.api_endpoint,
            AppConfig::default().translation.api_endpoint
        );
        assert_eq!(config.ocr.language, AppConfig::default().ocr.language);
        assert_eq!(
            config.hotkeys.live_translate,
            AppConfig::default().hotkeys.live_translate
        );
    }

    #[test]
    fn provider_config_update_switches_to_provider_endpoint() {
        let mut config = AppConfig::default();
        update_translation_provider_config(&mut config, "deepl").unwrap();
        assert_eq!(
            config.translation.api_endpoint,
            "https://api-free.deepl.com/v2/translate"
        );

        update_translation_provider_config(&mut config, "baidu").unwrap();
        assert_eq!(
            config.translation.api_endpoint,
            "https://fanyi-api.baidu.com/api/trans/vip/translate"
        );
    }

    #[test]
    fn stale_builtin_endpoint_is_repaired_without_overwriting_custom_endpoint() {
        assert_eq!(
            effective_translation_endpoint("deepl", "https://api.openai.com/v1/chat/completions"),
            "https://api-free.deepl.com/v2/translate"
        );
        assert_eq!(
            effective_translation_endpoint("baidu", "https://custom.example.test/translate"),
            "https://custom.example.test/translate"
        );
    }

    #[test]
    fn provider_config_update_rejects_unknown_id_without_mutation() {
        let mut config = AppConfig::default();
        assert!(
            update_translation_provider_config(&mut config, "api").is_err(),
            "the legacy api id must not be accepted as a configuration identifier"
        );
        assert_eq!(
            config.translation.provider,
            AppConfig::default().translation.provider
        );
    }

    #[test]
    fn provider_credential_target_mapping_covers_single_key_providers() {
        assert_eq!(
            provider_credential_target("openai"),
            Some(CredentialTarget::OpenAI)
        );
        assert_eq!(
            provider_credential_target("deepl"),
            Some(CredentialTarget::DeepL)
        );
        assert_eq!(
            provider_credential_target("google"),
            Some(CredentialTarget::Google)
        );
        assert_eq!(
            provider_credential_target("azure"),
            Some(CredentialTarget::Azure)
        );
        assert_eq!(provider_credential_target("baidu"), None);
        assert_eq!(provider_credential_target("local"), None);
    }

    #[test]
    fn store_api_key_writes_to_the_matching_provider_target() {
        let manager = memory_credentials();
        store_api_key(&manager, "openai", "sk-test-1234").unwrap();
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::OpenAI)
                .unwrap()
                .as_deref(),
            Some("sk-test-1234")
        );
    }

    #[test]
    fn store_api_key_overwrites_a_previous_key() {
        let manager = memory_credentials();
        store_api_key(&manager, "azure", "sk-old").unwrap();
        store_api_key(&manager, "azure", "sk-new").unwrap();
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::Azure)
                .unwrap()
                .as_deref(),
            Some("sk-new")
        );
    }

    #[test]
    fn store_api_key_rejects_providers_without_a_key_target() {
        for provider in ["local", "api", "deepseek"] {
            let manager = memory_credentials();
            let error = store_api_key(&manager, provider, "sk-test").unwrap_err();
            assert!(
                matches!(error, AppError::ProviderCredential(_)),
                "provider {provider:?}: {error}"
            );
        }
    }

    #[test]
    fn store_api_key_writes_baidu_secret_target() {
        let manager = memory_credentials();
        store_api_key(&manager, "baidu", "sk-baidu-secret").unwrap();
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::BaiduSecret)
                .unwrap()
                .as_deref(),
            Some("sk-baidu-secret")
        );
    }

    #[test]
    fn store_provider_credentials_writes_baidu_app_id_and_secret() {
        let manager = memory_credentials();
        store_provider_credentials(
            &manager,
            "baidu",
            None,
            Some("app-2024"),
            Some("secret-1234"),
        )
        .unwrap();
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::BaiduAppId)
                .unwrap()
                .as_deref(),
            Some("app-2024")
        );
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::BaiduSecret)
                .unwrap()
                .as_deref(),
            Some("secret-1234")
        );
    }

    #[test]
    fn store_provider_credentials_rejects_incomplete_baidu_values() {
        let manager = memory_credentials();
        let error =
            store_provider_credentials(&manager, "baidu", None, None, Some("secret")).unwrap_err();
        assert!(matches!(error, AppError::ProviderCredential(_)));
        let error =
            store_provider_credentials(&manager, "baidu", None, Some("app"), None).unwrap_err();
        assert!(matches!(error, AppError::ProviderCredential(_)));
        // Nothing was written by the failed calls.
        assert!(manager
            .load_for_provider(CredentialTarget::BaiduAppId)
            .unwrap()
            .is_none());
    }

    #[test]
    fn store_provider_credentials_writes_single_key_targets() {
        let manager = memory_credentials();
        store_provider_credentials(&manager, "deepl", Some("sk-deepl"), None, None).unwrap();
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::DeepL)
                .unwrap()
                .as_deref(),
            Some("sk-deepl")
        );
    }

    #[test]
    fn store_provider_credentials_rejects_local_and_unknown_providers() {
        for provider in ["local", "api", "deepseek"] {
            let manager = memory_credentials();
            let error = store_provider_credentials(&manager, provider, Some("sk-test"), None, None)
                .unwrap_err();
            assert!(
                matches!(error, AppError::ProviderCredential(_)),
                "provider {provider:?}: {error}"
            );
        }
    }

    #[test]
    fn load_provider_credentials_reads_single_key_targets() {
        for (provider, target) in [
            ("openai", CredentialTarget::OpenAI),
            ("deepl", CredentialTarget::DeepL),
            ("google", CredentialTarget::Google),
            ("azure", CredentialTarget::Azure),
        ] {
            let manager = memory_credentials();
            manager.store_for_provider(target, "sk-configured").unwrap();
            let loaded = load_provider_credentials(&manager, &cloud_config(provider)).unwrap();
            assert_eq!(loaded.api_key, "sk-configured", "provider {provider:?}");
            assert!(loaded.app_id.is_empty());
            assert!(loaded.secret.is_empty());
        }
    }

    #[test]
    fn load_provider_credentials_reads_baidu_targets_separately() {
        let manager = memory_credentials();
        manager
            .store_for_provider(CredentialTarget::BaiduAppId, "app-2024")
            .unwrap();
        manager
            .store_for_provider(CredentialTarget::BaiduSecret, "secret-5678")
            .unwrap();
        let loaded = load_provider_credentials(&manager, &cloud_config("baidu")).unwrap();
        assert_eq!(loaded.app_id, "app-2024");
        assert_eq!(loaded.secret, "secret-5678");
        assert!(loaded.api_key.is_empty());
    }

    #[test]
    fn load_provider_credentials_tolerates_missing_values() {
        let manager = memory_credentials();
        let loaded = load_provider_credentials(&manager, &cloud_config("openai")).unwrap();
        assert!(loaded.api_key.is_empty());
        let loaded = load_provider_credentials(&manager, &cloud_config("baidu")).unwrap();
        assert!(loaded.app_id.is_empty());
        assert!(loaded.secret.is_empty());
    }

    #[test]
    fn build_cloud_translation_provider_assembles_all_five_providers() {
        for provider in ["openai", "deepl", "google", "azure", "baidu"] {
            let manager = memory_credentials();
            match provider {
                "baidu" => {
                    manager
                        .store_for_provider(CredentialTarget::BaiduAppId, "app")
                        .unwrap();
                    manager
                        .store_for_provider(CredentialTarget::BaiduSecret, "secret")
                        .unwrap();
                }
                _ => manager
                    .store_for_provider(
                        provider_credential_target(provider).unwrap(),
                        "sk-configured",
                    )
                    .unwrap(),
            }
            let built =
                build_cloud_translation_provider(&cloud_config(provider), &manager).unwrap();
            assert_eq!(
                built.id(),
                provider,
                "runtime provider id must match the configuration id"
            );
        }
    }

    #[test]
    fn build_cloud_translation_provider_uses_azure_region_from_config() {
        let manager = memory_credentials();
        manager
            .store_for_provider(CredentialTarget::Azure, "sk-azure")
            .unwrap();
        let mut config = cloud_config("azure");
        config.translation.region = Some("eastasia".to_string());
        let built = build_cloud_translation_provider(&config, &manager).unwrap();
        assert_eq!(built.id(), "azure");
    }

    // ── Local provider cache ──

    /// A no-op translation provider used to exercise the cache without a
    /// real ONNX model. Its identity is stable so tests can distinguish
    /// cached instances from freshly built ones.
    struct StubTranslationProvider {
        id: &'static str,
    }

    #[async_trait::async_trait]
    impl TranslationProvider for StubTranslationProvider {
        fn id(&self) -> &'static str {
            self.id
        }

        async fn translate(
            &self,
            _request: &vtrans_core::TranslationRequest,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> Result<vtrans_core::TranslationResult, vtrans_core::TranslationError> {
            Ok(vtrans_core::TranslationResult::new("", self.id, 0))
        }

        fn supported_pairs(&self) -> &[(vtrans_core::Language, vtrans_core::Language)] {
            &[]
        }
    }

    fn stub_provider() -> Arc<dyn TranslationProvider> {
        Arc::new(StubTranslationProvider { id: "stub-local" })
    }

    #[test]
    fn local_provider_cache_miss_when_empty() {
        let cache = LocalProviderCache::new();
        assert!(!cache.is_hit(None));
        assert!(cache.get(None).is_none());
    }

    #[test]
    fn local_provider_cache_hit_after_set_with_matching_directory() {
        let cache = LocalProviderCache::new();
        let provider = stub_provider();
        cache.set(Arc::clone(&provider), None);
        assert!(cache.is_hit(None));
        let cached = cache.get(None).expect("cache hit returns the provider");
        assert_eq!(cached.id(), "stub-local");
    }

    #[test]
    fn local_provider_cache_miss_after_directory_change() {
        let cache = LocalProviderCache::new();
        let provider = stub_provider();
        let original_dir = std::path::PathBuf::from("/app/models");
        cache.set(Arc::clone(&provider), Some(original_dir.clone()));
        assert!(cache.is_hit(Some(&original_dir)));

        // A different configured directory invalidates the cache.
        let new_dir = std::path::PathBuf::from("/app/other-models");
        assert!(!cache.is_hit(Some(&new_dir)));
        assert!(cache.get(Some(&new_dir)).is_none());

        // The invalidation cleared the cache, so even the original dir now
        // misses.
        assert!(!cache.is_hit(Some(&original_dir)));
    }

    #[test]
    fn local_provider_cache_survives_cloud_provider_round_trip() {
        // Switching to a cloud provider must not clear the local cache: a
        // subsequent switch back to "local" should still hit.
        let cache = LocalProviderCache::new();
        let provider = stub_provider();
        cache.set(Arc::clone(&provider), None);
        // (cloud switch touches no cache state)
        assert!(
            cache.is_hit(None),
            "cloud switch must not evict the local cache"
        );
        let cached = cache.get(None).expect("cache hit after cloud round trip");
        assert_eq!(cached.id(), "stub-local");
    }

    #[test]
    fn local_provider_cache_set_overwrites_previous_entry() {
        let cache = LocalProviderCache::new();
        let first = stub_provider();
        cache.set(Arc::clone(&first), None);
        let second = Arc::new(StubTranslationProvider { id: "stub-local-2" })
            as Arc<dyn TranslationProvider>;
        cache.set(Arc::clone(&second), None);
        let cached = cache.get(None).expect("cache hit");
        assert_eq!(cached.id(), "stub-local-2");
    }

    #[test]
    fn translation_progress_model_id_is_stable() {
        // The frontend listens for this id; changing it would silently drop
        // the progress bar during provider switches.
        assert_eq!(TRANSLATION_PROGRESS_MODEL_ID, "translation");
    }
}
