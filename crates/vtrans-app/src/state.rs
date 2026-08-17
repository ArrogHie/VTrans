//! Shared application state and provider assembly.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::AppHandle;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use vtrans_capture::WindowsCaptureSource;
use vtrans_config::{AppConfig, ConfigManager};
use vtrans_core::traits::{CaptureSource, OcrProvider, TranslationProvider};
use vtrans_core::{
    CapturedImage, Language, OcrOptions, OcrResult, PipelineMode, PipelineStatus, ScreenRegion,
    TranslationRequest, TranslationResult,
};
use vtrans_models::ModelManager;
use vtrans_ocr::PaddleOcrProvider;
use vtrans_pipeline::{MultiBoxConfig, MultiBoxPipeline};
use vtrans_pipeline::{Pipeline, PipelineConfig, PipelineDeps};
use vtrans_security::{
    migrate_windows_to_dpapi, CredentialManager, CredentialTarget, DpapiFileStore,
    InMemoryCredentialStore,
};
use vtrans_translation::{
    AzureTranslatorProvider, BaiduProvider, DeepLProvider, GoogleV2Provider,
    LocalTranslationProvider, OpenAiProvider,
};

use crate::error::AppError;
use crate::model_setup::{ensure_data_models, model_status_report, ModelStatusReport};
use crate::window_visibility::SelectionVisibilityState;

/// The `model_id` used for translation provider loading progress events.
///
/// Reused by the provider switch and settings save paths so the frontend
/// shows a single "translation" progress bar during local model loads.
const TRANSLATION_PROGRESS_MODEL_ID: &str = "translation";

/// File name of the DPAPI-encrypted credential container inside the data root.
pub(crate) const CREDENTIAL_FILE_NAME: &str = "credentials.bin";

/// Runtime id of the placeholder OCR provider installed when the OCR models
/// are missing or fail to load (see [`UnavailableOcrProvider`]).
pub(crate) const UNAVAILABLE_OCR_PROVIDER_ID: &str = "unavailable-ocr";
/// Runtime id of the placeholder translation provider installed when the
/// configured provider (usually `local`) cannot be assembled.
pub(crate) const UNAVAILABLE_TRANSLATION_PROVIDER_ID: &str = "unavailable-translation";

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

    /// Drops the cached provider, forcing the next switch to `"local"` to
    /// reload from disk (used after the model file was deleted or replaced).
    pub(crate) fn invalidate(&self) {
        *self.provider.write().unwrap_or_else(poison_inner) = None;
        *self.model_dir.write().unwrap_or_else(poison_inner) = None;
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
    /// session (running **or paused**) reports `live`, and a multi-box
    /// real-time session also reports `live` while it runs (falling back to
    /// `single` when it stops, unless a single-box live session is still
    /// active). The frontend uses it during hydration to decide whether a
    /// selected region should restore the persistent overlay marker.
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
    /// Model manager over `{data}/models`; `None` when the manifest itself
    /// could not be loaded (the app still starts, with degraded model state).
    pub(crate) model_manager: Option<Arc<ModelManager>>,
    /// Directory holding the runtime model files (`{data}/models` by default,
    /// overridable through the advanced `config.model_dir` setting).
    data_models_dir: PathBuf,
    /// Read-only bundled model source (`resource_dir()/resources/models`),
    /// `None` when the packaged resources are unavailable.
    bundled_models_dir: Option<PathBuf>,
    /// Latest model availability snapshot (updated at startup and after
    /// download/delete/retry; `get_model_status` recomputes it fresh).
    model_status: std::sync::RwLock<ModelStatusReport>,
    /// Cancellation token of the in-flight translation model download, if
    /// any. `Some` also means "a download owns the slot".
    model_download: std::sync::RwLock<Option<CancellationToken>>,
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
    /// `app_data_dir` is the portable data root (`{exe}/data`, resolved by
    /// `setup.rs`) used for config.json, credentials.bin, logs, and — unless
    /// overridden by the persisted configuration — `models/`.
    /// `bundled_models_dir` is the read-only packaged model source
    /// (`resource_dir()/resources/models`), used by the self-healing
    /// provisioning pass.
    ///
    /// # Errors
    ///
    /// Returns an application error when config or capture cannot initialize.
    /// Model problems never fail startup: provisioning/loading failures are
    /// logged, the status snapshot records them, and placeholder providers
    /// answer with clear errors until the models are repaired.
    #[tracing::instrument(skip(app_data_dir))]
    pub fn new(app_data_dir: &Path) -> Result<Self, AppError> {
        Self::new_with_debug(app_data_dir, None, false)
    }

    /// Constructs the application state with Debug mode explicitly enabled
    /// or disabled and an explicit bundled model source.
    ///
    /// Debug mode is a per-run flag (command line / environment), never
    /// persisted. See [`new`](Self::new) for the remaining contract and the
    /// startup tolerance rules.
    ///
    /// # Errors
    ///
    /// Returns an application error when config or capture cannot initialize.
    #[tracing::instrument(skip(app_data_dir, bundled_models_dir, debug_mode))]
    pub fn new_with_debug(
        app_data_dir: &Path,
        bundled_models_dir: Option<&Path>,
        debug_mode: bool,
    ) -> Result<Self, AppError> {
        let config_manager = ConfigManager::new(app_data_dir)?;
        let config = config_manager.load()?;
        // R5: credentials live in a DPAPI-protected file inside the data
        // root. First boot migrates the legacy Windows Credential Manager
        // entries; every failure is tolerated (clear errors at use time).
        let credential_path = app_data_dir.join(CREDENTIAL_FILE_NAME);
        let run_migration = should_migrate_credentials(&credential_path);
        let credentials = build_credential_manager(&credential_path, run_migration);
        let data_models_dir = config
            .model_dir
            .clone()
            .unwrap_or_else(|| app_data_dir.join("models"));
        // R2/R6: provision {data}/models from the bundled source before
        // anything loads from it. Failures only degrade the model status.
        let setup = ensure_data_models(&data_models_dir, bundled_models_dir);
        for error in &setup.errors {
            warn!(error = %error, "model provisioning problem");
        }
        let model_manager = match ModelManager::from_manifest_dir(&data_models_dir) {
            Ok(manager) => Some(Arc::new(manager)),
            Err(error) => {
                warn!(
                    error = %error,
                    "model manifest unavailable; the app starts with degraded model state"
                );
                None
            }
        };
        let capture_source = Arc::new(WindowsCaptureSource::new()?);
        let ocr_provider: Arc<dyn OcrProvider> = match model_manager.as_deref() {
            Some(manager) => match PaddleOcrProvider::from_manager(manager) {
                Ok(provider) => Arc::new(provider),
                Err(error) => {
                    warn!(
                        error = %error,
                        "OCR provider unavailable; translation commands report a clear error until the models are repaired"
                    );
                    Arc::new(UnavailableOcrProvider)
                }
            },
            None => Arc::new(UnavailableOcrProvider),
        };
        let translation_provider = match build_translation_provider(
            &config,
            &credentials,
            model_manager.as_deref(),
        ) {
            Ok(provider) => provider,
            Err(error) => {
                warn!(
                    error = %error,
                    "translation provider unavailable; it is rebuilt once the models or credentials are ready"
                );
                Arc::new(UnavailableTranslationProvider)
            }
        };
        info!(
            ocr_provider = ocr_provider.id(),
            translation_provider = translation_provider.id(),
            model_dir = %data_models_dir.display(),
            ocr_ready = setup.report.ocr_ready,
            translation_ready = setup.report.translation_ready,
            "application state initialized"
        );
        // Seed the local provider cache when the startup configuration
        // already selects the local provider, so the first runtime switch
        // back to "local" reuses this instance instead of reloading.
        let local_provider_cache = LocalProviderCache::new();
        if config.translation.provider == "local"
            && translation_provider.id() != UNAVAILABLE_TRANSLATION_PROVIDER_ID
        {
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
            data_models_dir,
            bundled_models_dir: bundled_models_dir.map(Path::to_path_buf),
            model_status: std::sync::RwLock::new(setup.report),
            model_download: std::sync::RwLock::new(None),
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
    /// persisted. A single-box stop keeps the last mode so a paused live
    /// session still reports `live`; only the next region confirmation,
    /// single capture, or multi-box stop (without a concurrent single-box
    /// live session) switches it back.
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
        let Some(model_manager) = self.model_manager() else {
            self.set_model_progress(None);
            return Err(AppError::ModelNotReady(
                "模型清单不可用，无法加载本地翻译引擎".to_string(),
            ));
        };
        let provider = tokio::task::spawn_blocking(move || {
            LocalTranslationProvider::from_manager(&model_manager)
                .map(|provider| Arc::new(provider) as Arc<dyn TranslationProvider>)
                .map_err(AppError::from)
        })
        .await
        .map_err(|error| {
            self.set_model_progress(None);
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

    // ── Model provisioning and download state ──

    /// Returns the directory holding the runtime model files.
    #[must_use]
    pub(crate) fn data_models_dir(&self) -> &Path {
        &self.data_models_dir
    }

    /// Returns the read-only bundled model source, if available.
    #[must_use]
    pub(crate) fn bundled_models_dir(&self) -> Option<&Path> {
        self.bundled_models_dir.as_deref()
    }

    /// Returns a clone of the loaded [`ModelManager`], or `None` when the
    /// manifest could not be loaded (degraded model state).
    #[must_use]
    pub(crate) fn model_manager(&self) -> Option<Arc<ModelManager>> {
        self.model_manager.clone()
    }

    /// Stores the latest model availability snapshot.
    pub(crate) fn set_model_status(&self, report: ModelStatusReport) {
        *self.model_status.write().unwrap_or_else(poison_inner) = report;
    }

    /// Returns the latest model availability snapshot.
    #[must_use]
    pub(crate) fn model_status_snapshot(&self) -> ModelStatusReport {
        self.model_status
            .read()
            .unwrap_or_else(poison_inner)
            .clone()
    }

    /// Whether the current OCR provider is functional (not the placeholder).
    #[must_use]
    pub(crate) fn ocr_ready(&self) -> bool {
        self.ocr_provider.read().unwrap_or_else(poison_inner).id() != UNAVAILABLE_OCR_PROVIDER_ID
    }

    /// Gate used by the translation entry commands: fails with a clear
    /// "OCR 模型未就位" error while the OCR provider is the placeholder.
    pub(crate) fn ocr_ready_gate(&self) -> Result<(), AppError> {
        check_ocr_ready(self.ocr_provider.read().unwrap_or_else(poison_inner).id())
    }

    /// Tries to claim the download slot for `token`.
    ///
    /// Returns `false` when another download is already in progress, so a
    /// concurrent `download_translation_model` call fails fast instead of
    /// starting a duplicate transfer.
    pub(crate) fn try_start_model_download(&self, token: CancellationToken) -> bool {
        let mut slot = self.model_download.write().unwrap_or_else(poison_inner);
        if slot.is_some() {
            return false;
        }
        *slot = Some(token);
        true
    }

    /// Releases the download slot after the download task finished.
    pub(crate) fn finish_model_download(&self) {
        *self.model_download.write().unwrap_or_else(poison_inner) = None;
    }

    /// Whether a translation model download is currently in progress.
    #[must_use]
    pub(crate) fn model_download_active(&self) -> bool {
        self.model_download
            .read()
            .unwrap_or_else(poison_inner)
            .as_ref()
            .is_some_and(|token| !token.is_cancelled())
    }

    /// Requests cancellation of the in-flight download, if any.
    pub(crate) fn cancel_model_download(&self) {
        if let Some(token) = self
            .model_download
            .read()
            .unwrap_or_else(poison_inner)
            .clone()
        {
            token.cancel();
        }
    }

    /// Cancels the in-flight download and waits (bounded) until the task
    /// releases the slot, so file deletion can safely follow.
    pub(crate) async fn cancel_and_wait_model_download(&self, wait: std::time::Duration) {
        self.cancel_model_download();
        let deadline = tokio::time::Instant::now() + wait;
        while self
            .model_download
            .read()
            .unwrap_or_else(poison_inner)
            .is_some()
        {
            if tokio::time::Instant::now() >= deadline {
                warn!("timed out waiting for the cancelled model download to finish");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// Rebuilds the OCR provider when it is currently the placeholder and
    /// the OCR model files verify (used by `retry_model_setup`).
    pub(crate) async fn refresh_ocr_provider_if_ready(&self) {
        if self.ocr_ready() {
            return;
        }
        let Some(model_manager) = self.model_manager() else {
            warn!("cannot rebuild the OCR provider: model manifest unavailable");
            return;
        };
        let result =
            tokio::task::spawn_blocking(move || PaddleOcrProvider::from_manager(&model_manager))
                .await;
        let provider = match result {
            Ok(Ok(provider)) => provider,
            Ok(Err(error)) => {
                warn!(error = %error, "OCR provider rebuild failed; models may still be incomplete");
                return;
            }
            Err(error) => {
                warn!(error = %error, "OCR provider rebuild task failed");
                return;
            }
        };
        let id = provider.id();
        *self.ocr_provider.write().unwrap_or_else(poison_inner) =
            Arc::new(provider) as Arc<dyn OcrProvider>;
        info!(ocr_provider = id, "OCR provider rebuilt after model repair");
    }

    /// Recomputes and stores the model status snapshot on the blocking pool
    /// (async callers; the sync startup path uses
    /// [`refresh_model_status_after_change`](Self::refresh_model_status_after_change)
    /// directly).
    pub(crate) async fn refresh_model_status_async(&self) -> Result<(), AppError> {
        let Some(model_manager) = self.model_manager() else {
            self.set_model_status(ModelStatusReport::default());
            return Ok(());
        };
        let report = tokio::task::spawn_blocking(move || model_status_report(&model_manager))
            .await
            .map_err(|error| AppError::Tauri(format!("model status task failed: {error}")))?;
        self.set_model_status(report);
        Ok(())
    }

    /// Rebuilds the translation provider after a model change (download,
    /// delete, repair) when the configured provider is `"local"`.
    ///
    /// On failure the provider is replaced with a placeholder that answers
    /// with a clear "model not ready" error, matching the "not installed"
    /// state the frontend derives from `get_model_status`.
    pub(crate) async fn rebuild_translation_provider_after_model_change(
        &self,
        app: &AppHandle,
    ) -> Result<(), AppError> {
        let config = self.load_config()?;
        if config.translation.provider != "local" {
            return Ok(());
        }
        self.local_provider_cache.invalidate();
        match self.prepare_translation_provider(config, Some(app)).await {
            Ok(provider) => {
                self.replace_translation_provider(provider);
                Ok(())
            }
            Err(error) => {
                self.set_model_progress(None);
                warn!(
                    error = %error,
                    "local translation provider unavailable after a model change"
                );
                self.replace_translation_provider(Arc::new(UnavailableTranslationProvider));
                Err(error)
            }
        }
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

/// Session mode recorded after a multi-box real-time session stops.
///
/// Multi-box sessions follow the live semantics of [`AppStatus::mode`]
/// while they run. Once a multi-box session stops, the authoritative mode
/// falls back to `SingleCapture` — unless a single-box live task is still
/// running or paused, in which case the recorded mode must stay
/// `LiveRegion` so the concurrent single-box session is never overwritten.
///
/// Kept as a pure function so the fallback decision is unit-testable
/// without Windows capture state or a Tokio runtime.
#[must_use]
pub(crate) fn mode_after_multi_stop(single_live_running: bool) -> PipelineMode {
    if single_live_running {
        PipelineMode::LiveRegion
    } else {
        PipelineMode::SingleCapture
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
    model_manager: Option<&ModelManager>,
) -> Result<Arc<dyn TranslationProvider>, AppError> {
    validate_translation_provider_id(&config.translation.provider)?;
    if config.translation.provider == "local" {
        let Some(model_manager) = model_manager else {
            return Err(AppError::ModelNotReady(
                "模型清单不可用，无法加载本地翻译引擎".to_string(),
            ));
        };
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

/// Placeholder OCR provider installed when the OCR models are missing or
/// fail to load. It answers every recognition with a clear "repair the
/// models" error instead of failing silently.
struct UnavailableOcrProvider;

#[async_trait::async_trait]
impl OcrProvider for UnavailableOcrProvider {
    fn id(&self) -> &'static str {
        UNAVAILABLE_OCR_PROVIDER_ID
    }

    async fn recognize(
        &self,
        _image: &CapturedImage,
        _region: &ScreenRegion,
        _options: &OcrOptions,
        _cancel: CancellationToken,
    ) -> Result<OcrResult, vtrans_core::OcrError> {
        Err(vtrans_core::OcrError::Inference(
            "OCR 模型未就位，请重试模型修复".to_string(),
        ))
    }

    fn supported_languages(&self) -> &[Language] {
        &[]
    }
}

/// Placeholder translation provider installed when the configured provider
/// (usually `local`) cannot be assembled. It answers every translation with
/// a clear "model not ready" error.
struct UnavailableTranslationProvider;

#[async_trait::async_trait]
impl TranslationProvider for UnavailableTranslationProvider {
    fn id(&self) -> &'static str {
        UNAVAILABLE_TRANSLATION_PROVIDER_ID
    }

    async fn translate(
        &self,
        _request: &TranslationRequest,
        _cancel: CancellationToken,
    ) -> Result<TranslationResult, vtrans_core::TranslationError> {
        Err(vtrans_core::TranslationError::Inference(
            "翻译模型未就位，请先下载模型或重试模型修复".to_string(),
        ))
    }

    fn supported_pairs(&self) -> &[(Language, Language)] {
        &[]
    }
}

/// Rejects translation entry commands while the OCR provider is the
/// unavailable placeholder.
///
/// Kept as a pure function over the provider id so the gate is unit-testable
/// without a full [`AppState`]; the commands wire it with the current
/// provider id.
pub(crate) fn check_ocr_ready(ocr_provider_id: &str) -> Result<(), AppError> {
    if ocr_provider_id == UNAVAILABLE_OCR_PROVIDER_ID {
        return Err(AppError::ModelNotReady(
            "OCR 模型未就位，请重试模型修复".to_string(),
        ));
    }
    Ok(())
}

/// Whether the legacy Windows Credential Manager migration should run: the
/// credential container must not exist yet (first boot in this data root).
#[must_use]
pub(crate) fn should_migrate_credentials(credential_path: &Path) -> bool {
    !credential_path.exists()
}

/// Builds the credential manager backed by the DPAPI file store.
///
/// First boot (`run_migration`) migrates the legacy Windows Credential
/// Manager entries into the container. Every failure degrades gracefully:
/// DPAPI store unavailable → legacy Windows Credential Manager; that
/// unavailable too → in-memory store (credentials do not persist). The app
/// always starts; translation reports a clear error when credentials are
/// actually needed but unusable.
#[tracing::instrument(skip(credential_path), fields(path = %credential_path.display()))]
fn build_credential_manager(credential_path: &Path, run_migration: bool) -> Arc<CredentialManager> {
    match DpapiFileStore::new(credential_path) {
        Ok(store) => {
            let store = Arc::new(store);
            if run_migration {
                match migrate_windows_to_dpapi(&store) {
                    Ok(migrated) => info!(
                        migrated,
                        "legacy windows credentials migrated to the dpapi file store"
                    ),
                    Err(error) => warn!(
                        error = %error,
                        "credential migration failed; continuing with the local credential store"
                    ),
                }
            }
            Arc::new(CredentialManager::with_store(store))
        }
        Err(error) => {
            warn!(
                error = %error,
                "dpapi file credential store unavailable; falling back to the windows credential manager"
            );
            match CredentialManager::new() {
                Ok(manager) => Arc::new(manager),
                Err(error) => {
                    warn!(
                        error = %error,
                        "windows credential manager unavailable; using an in-memory credential store (credentials will not persist)"
                    );
                    Arc::new(CredentialManager::with_store(Arc::new(
                        InMemoryCredentialStore::new(),
                    )))
                }
            }
        }
    }
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
    fn mode_after_multi_stop_falls_back_to_single_without_single_live() {
        // Bug-004: a stopped multi-box session falls back to `single` when
        // no single-box live task is running or paused.
        assert_eq!(mode_after_multi_stop(false), PipelineMode::SingleCapture);
    }

    #[test]
    fn mode_after_multi_stop_preserves_live_with_running_single_live() {
        // Bug-004: a concurrent single-box live session must never be
        // overwritten by a multi-box stop.
        assert_eq!(mode_after_multi_stop(true), PipelineMode::LiveRegion);
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

    // ── Model readiness gate ──

    #[test]
    fn ocr_readiness_gate_rejects_only_the_unavailable_placeholder() {
        let error = check_ocr_ready(UNAVAILABLE_OCR_PROVIDER_ID).unwrap_err();
        assert!(matches!(error, AppError::ModelNotReady(_)));
        assert!(error.to_string().contains("OCR 模型未就位"));
        // Functional OCR providers (any other id) pass the gate.
        for provider in ["pp-ocr", "mock-ocr", ""] {
            assert!(
                check_ocr_ready(provider).is_ok(),
                "provider {provider:?} must pass the gate"
            );
        }
    }

    #[tokio::test]
    async fn unavailable_ocr_provider_reports_a_clear_error() {
        let provider = UnavailableOcrProvider;
        assert_eq!(provider.id(), UNAVAILABLE_OCR_PROVIDER_ID);
        let error = provider
            .recognize(
                &CapturedImage::new(1, 1, vtrans_core::PixelFormat::Bgra8, vec![0u8; 4]).unwrap(),
                &ScreenRegion::new("m0", 0, 0, 10, 10),
                &OcrOptions::default(),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("OCR 模型未就位"));
    }

    #[tokio::test]
    async fn unavailable_translation_provider_reports_a_clear_error() {
        let provider = UnavailableTranslationProvider;
        assert_eq!(provider.id(), UNAVAILABLE_TRANSLATION_PROVIDER_ID);
        let error = provider
            .translate(
                &TranslationRequest::new("", Language::Auto, Language::ChineseSimplified),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("翻译模型未就位"));
    }

    #[test]
    fn credential_migration_runs_only_when_the_container_is_absent() {
        let dir =
            std::env::temp_dir().join(format!("vtrans-app-migration-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CREDENTIAL_FILE_NAME);
        assert!(should_migrate_credentials(&path));
        std::fs::write(&path, b"existing-container").unwrap();
        assert!(!should_migrate_credentials(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn credential_manager_falls_back_when_the_dpapi_store_is_unusable() {
        // A path whose parent directory does not exist makes
        // `DpapiFileStore::new` fail; the builder must fall back to the
        // legacy Windows Credential Manager instead of failing startup.
        // (Migration is disabled, so this test never touches the real vault.)
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let missing_parent = std::env::temp_dir().join(format!(
            "vtrans-app-no-such-parent-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let path = missing_parent.join("credentials.bin");
        let manager = build_credential_manager(&path, false);
        assert!(
            manager.load_for_provider(CredentialTarget::OpenAI).is_ok(),
            "the fallback manager must answer credential reads"
        );
    }

    #[cfg(windows)]
    #[test]
    fn credential_manager_uses_the_dpapi_store_for_a_valid_path() {
        // A real DPAPI-backed container is created for a valid temp path.
        // Migration is disabled so the real Windows Credential Manager is
        // never enumerated or mutated by this test.
        let dir =
            std::env::temp_dir().join(format!("vtrans-app-dpapi-build-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CREDENTIAL_FILE_NAME);
        let manager = build_credential_manager(&path, false);
        assert!(path.exists(), "the container file must be created");
        assert!(manager
            .load_for_provider(CredentialTarget::OpenAI)
            .unwrap()
            .is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_provider_cache_invalidate_forces_a_reload() {
        let cache = LocalProviderCache::new();
        let provider = stub_provider();
        cache.set(Arc::clone(&provider), None);
        assert!(cache.is_hit(None));
        cache.invalidate();
        assert!(!cache.is_hit(None));
        assert!(cache.get(None).is_none());
    }
}
