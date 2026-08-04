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
use vtrans_pipeline::{Pipeline, PipelineConfig, PipelineDeps};
use vtrans_security::CredentialManager;
use vtrans_translation::{ApiTranslationProvider, LocalTranslationProvider};

use crate::error::AppError;

/// A serializable snapshot returned by the `get_app_status` command.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppStatus {
    /// Current pipeline status.
    pub pipeline_status: PipelineStatus,
    /// Stable identifier of the configured OCR provider.
    pub ocr_provider: String,
    /// Runtime implementation id of the configured translation provider
    /// (`"api"` or `"local-onnx"`).
    ///
    /// This differs from the configuration identifier domain (`"api"` /
    /// `"local"`) accepted by `set_translation_provider_id`; the frontend
    /// maps it back via `normalizeProviderId`.
    pub translation_provider: String,
    /// Last selected region, when one has been selected.
    pub selected_region: Option<ScreenRegion>,
    /// Whether the live task has been started and has not finished.
    pub live_running: bool,
    /// Current model loading progress, if a load is in progress.
    pub model_progress: Option<f32>,
}

/// Application-wide state managed by Tauri.
///
/// Providers are stored behind Arc and injected into each pipeline through
/// small trait-object adapters. This avoids reloading ONNX sessions and
/// recreating the capture backend for every command while preserving the
/// ownership contract required by `PipelineDeps`.
pub struct AppState {
    pub(crate) config: std::sync::RwLock<ConfigManager>,
    pub(crate) credentials: CredentialManager,
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
        let config_manager = ConfigManager::new(app_data_dir)?;
        let config = config_manager.load()?;
        let credentials = CredentialManager::new()?;
        let model_dir = config
            .model_dir
            .clone()
            .unwrap_or_else(|| app_data_dir.join("models"));
        let model_manager = Arc::new(ModelManager::from_manifest_dir(&model_dir)?);
        let capture_source = Arc::new(WindowsCaptureSource::new()?);
        let ocr_provider =
            Arc::new(PaddleOcrProvider::from_manager(&model_manager)?) as Arc<dyn OcrProvider>;
        let api_key = load_api_key(&credentials, &config)?;
        let translation_provider = build_translation_provider(&config, &api_key, &model_manager)?;
        info!(
            ocr_provider = ocr_provider.id(),
            translation_provider = translation_provider.id(),
            model_dir = %model_dir.display(),
            "application state initialized"
        );
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
        })
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

    pub(crate) fn build_pipeline(
        &self,
        mode: PipelineMode,
        region: ScreenRegion,
        capture_interval_ms: u32,
        difference_threshold: f32,
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
        Ok(Pipeline::new(
            pipeline_config,
            PipelineDeps::new(capture, ocr, translation),
        ))
    }

    pub(crate) async fn prepare_translation_provider(
        &self,
        config: AppConfig,
    ) -> Result<Arc<dyn TranslationProvider>, AppError> {
        let api_key = load_api_key(&self.credentials, &config)?;
        let model_manager = Arc::clone(&self.model_manager);
        // Loading a local provider verifies SHA-256 hashes, parses the
        // tokenizer, and creates an ONNX session; run it on the blocking
        // pool so the Tokio workers never stall while switching providers.
        tokio::task::spawn_blocking(move || {
            build_translation_provider(&config, &api_key, &model_manager)
        })
        .await
        .map_err(|error| {
            AppError::Tauri(format!("translation provider setup task failed: {error}"))
        })?
    }

    pub(crate) async fn set_translation_provider_id(
        &self,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let mut config = self.load_config()?;
        update_translation_provider_config(&mut config, provider_id)?;
        let provider = self.prepare_translation_provider(config.clone()).await?;
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
            pipeline_status,
            ocr_provider,
            translation_provider,
            selected_region: self.selected_region(),
            live_running,
            model_progress,
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

fn build_translation_provider(
    config: &AppConfig,
    api_key: &str,
    model_manager: &ModelManager,
) -> Result<Arc<dyn TranslationProvider>, AppError> {
    validate_translation_provider_id(&config.translation.provider)?;
    if config.translation.provider == "api" {
        Ok(Arc::new(ApiTranslationProvider::new(
            &config.translation.api_endpoint,
            &config.translation.api_model,
            api_key,
            std::time::Duration::from_secs(u64::from(config.translation.timeout_seconds)),
            config.translation.max_retries,
        )))
    } else {
        Ok(Arc::new(LocalTranslationProvider::from_manager(
            model_manager,
        )?))
    }
}

/// Validates a translation provider identifier against the stable
/// application-level domain (`"api"` or `"local"`).
///
/// The same domain is accepted by the `set_translation_provider` command and
/// enforced by `vtrans-config` validation, so configuration snapshots and
/// runtime provider selection can never disagree.
fn validate_translation_provider_id(provider_id: &str) -> Result<(), AppError> {
    if matches!(provider_id, "api" | "local") {
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
    Ok(())
}

/// Loads the API credential from secure storage for the API provider.
///
/// The vault is only touched when the configured provider is `"api"`; other
/// providers return an empty key. The key is returned to the caller and never
/// logged or persisted.
fn load_api_key(credentials: &CredentialManager, config: &AppConfig) -> Result<String, AppError> {
    if config.translation.provider != "api" {
        return Ok(String::new());
    }
    Ok(credentials.load("translation")?.unwrap_or_else(|| {
        warn!("translation API credential is not configured");
        String::new()
    }))
}

fn poison_inner<T>(poisoned: std::sync::PoisonError<T>) -> T {
    debug!("recovering poisoned application state lock");
    poisoned.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_snapshot_contract_is_serializable() {
        let status = AppStatus {
            pipeline_status: PipelineStatus::Idle,
            ocr_provider: "mock-ocr".to_string(),
            translation_provider: "mock-translation".to_string(),
            selected_region: None,
            live_running: false,
            model_progress: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("pipeline_status"));
        assert!(json.contains("mock-ocr"));
    }

    #[test]
    fn translation_provider_validation_accepts_known_ids() {
        assert!(validate_translation_provider_id("api").is_ok());
        assert!(validate_translation_provider_id("local").is_ok());
    }

    #[test]
    fn translation_provider_validation_rejects_unknown_ids() {
        let error = validate_translation_provider_id("local-onnx").unwrap_err();
        assert!(error
            .to_string()
            .contains("unsupported translation provider"));
    }

    #[test]
    fn provider_config_update_sets_identifier_and_preserves_other_fields() {
        let mut config = AppConfig::default();
        update_translation_provider_config(&mut config, "local").unwrap();
        assert_eq!(config.translation.provider, "local");
        assert_eq!(config.ocr.language, AppConfig::default().ocr.language);
        assert_eq!(
            config.hotkeys.live_translate,
            AppConfig::default().hotkeys.live_translate
        );
    }

    #[test]
    fn provider_config_update_rejects_unknown_id_without_mutation() {
        let mut config = AppConfig::default();
        assert!(
            update_translation_provider_config(&mut config, "local-onnx").is_err(),
            "runtime provider ids must not be accepted as configuration identifiers"
        );
        assert_eq!(
            config.translation.provider,
            AppConfig::default().translation.provider
        );
    }
}
