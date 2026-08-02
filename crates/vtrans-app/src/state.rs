//! Shared application state and provider assembly.

use std::path::Path;
use std::sync::Arc;

use tauri::AppHandle;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use vtrans_capture::WindowsCaptureSource;
use vtrans_config::{AppConfig, ConfigManager};
use vtrans_core::traits::{OcrProvider, TranslationProvider};
use vtrans_core::{OcrOptions, PipelineMode, PipelineStatus, ScreenRegion, TranslationRequest};
use vtrans_models::{ModelManager, VerifyReport};
use vtrans_ocr::PaddleOcrProvider;
use vtrans_pipeline::{Pipeline, PipelineConfig, PipelineDeps};
use vtrans_security::CredentialManager;
use vtrans_translation::{ApiTranslationProvider, LocalTranslationProvider};

use crate::error::AppError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppStatus {
    pub pipeline_status: PipelineStatus,
    pub ocr_provider: String,
    pub translation_provider: String,
    pub selected_region: Option<ScreenRegion>,
    pub live_running: bool,
    pub model_progress: Option<f32>,
}

pub struct AppState {
    pub(crate) config: std::sync::RwLock<ConfigManager>,
    pub(crate) credentials: CredentialManager,
    pub(crate) pipeline: std::sync::RwLock<Option<Arc<Pipeline>>>,
    pub(crate) ocr_provider: std::sync::RwLock<Box<dyn OcrProvider>>,
    pub(crate) translation_provider: std::sync::RwLock<Box<dyn TranslationProvider>>,
    pub(crate) capture_source: WindowsCaptureSource,
    pub(crate) model_manager: ModelManager,
    selected_region: std::sync::RwLock<Option<ScreenRegion>>,
    pub(crate) live_task: Mutex<Option<JoinHandle<()>>>,
    app_handle: std::sync::RwLock<Option<AppHandle>>,
    model_progress: std::sync::RwLock<Option<f32>>,
}

impl AppState {
    ///
    /// # Errors
    ///
    /// Returns an application error when config, models, capture, OCR, or translation cannot initialize.
    #[tracing::instrument(skip(app_data_dir))]
    pub fn new(app_data_dir: &Path) -> Result<Self, AppError> {
        let config_manager = ConfigManager::new(app_data_dir)?;
        let config = config_manager.load()?;
        let credentials = CredentialManager::new()?;
        let model_dir = config
            .model_dir
            .clone()
            .unwrap_or_else(|| app_data_dir.join("models"));
        let model_manager = ModelManager::from_manifest_dir(&model_dir)?;
        let capture_source = WindowsCaptureSource::new()?;
        let ocr_provider =
            Box::new(PaddleOcrProvider::from_manager(&model_manager)?) as Box<dyn OcrProvider>;
        let translation_provider =
            build_translation_provider(&config, &credentials, &model_manager)?;
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

    pub(crate) fn set_selected_region(&self, region: ScreenRegion) -> Result<(), AppError> {
        region.validate().map_err(AppError::from)?;
        *self.selected_region.write().unwrap_or_else(poison_inner) = Some(region);
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
        let pipeline_config = PipelineConfig::new(
            mode,
            region,
            capture_interval_ms,
            difference_threshold,
            ocr_options,
            translation_request,
        );
        let capture = Box::new(WindowsCaptureSource::new()?) as Box<dyn vtrans_core::CaptureSource>;
        let ocr =
            Box::new(PaddleOcrProvider::from_manager(&self.model_manager)?) as Box<dyn OcrProvider>;
        let translation =
            build_translation_provider(&config, &self.credentials, &self.model_manager)?;
        Ok(Pipeline::new(
            pipeline_config,
            PipelineDeps::new(capture, ocr, translation),
        ))
    }

    pub(crate) fn set_translation_provider_id(&self, provider_id: &str) -> Result<(), AppError> {
        if provider_id != "api" && provider_id != "local" {
            return Err(vtrans_core::TranslationError::Inference(format!(
                "unsupported translation provider: {provider_id}"
            ))
            .into());
        }
        let mut config = self.load_config()?;
        config.translation.provider = provider_id.to_string();
        let provider = build_translation_provider(&config, &self.credentials, &self.model_manager)?;
        self.save_config(&config)?;
        self.replace_translation_provider(provider);
        Ok(())
    }

    pub(crate) fn replace_translation_provider(&self, provider: Box<dyn TranslationProvider>) {
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

    pub(crate) fn verify_models(&self) -> Result<VerifyReport, AppError> {
        self.model_manager
            .verify_integrity()
            .map_err(AppError::from)
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

fn build_translation_provider(
    config: &AppConfig,
    credentials: &CredentialManager,
    model_manager: &ModelManager,
) -> Result<Box<dyn TranslationProvider>, AppError> {
    match config.translation.provider.as_str() {
        "api" => {
            let key = credentials.load("translation")?.unwrap_or_else(|| {
                warn!("translation API credential is not configured");
                String::new()
            });
            Ok(Box::new(ApiTranslationProvider::new(
                &config.translation.api_endpoint,
                &config.translation.api_model,
                &key,
                std::time::Duration::from_secs(u64::from(config.translation.timeout_seconds)),
                config.translation.max_retries,
            )))
        }
        "local" => Ok(Box::new(LocalTranslationProvider::from_manager(
            model_manager,
        )?)),
        provider => {
            warn!(provider, "unknown translation provider in config");
            Err(vtrans_core::TranslationError::Inference(format!(
                "unsupported translation provider: {provider}"
            ))
            .into())
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
}
