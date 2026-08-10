//! Errors exposed by the `VTrans` application layer.

use serde::Serializer;
use thiserror::Error;
use vtrans_config::ConfigError;
use vtrans_core::CaptureError;
use vtrans_core::{CoreError, OcrError, TranslationError};
use vtrans_models::ModelError;
use vtrans_pipeline::PipelineError;
use vtrans_security::SecurityError;

/// Errors returned by application commands and startup helpers.
#[derive(Debug, Error)]
pub enum AppError {
    /// The requested application resource has not been initialized yet.
    #[error("state not initialized")]
    NotInitialized,

    /// A region selection request is already waiting for frontend confirmation.
    #[error("region selection already in progress")]
    SelectionInProgress,

    /// A core validation or serialization error.
    #[error("core error: {0}")]
    Core(#[source] CoreError),

    /// The pipeline failed.
    #[error("pipeline error: {0}")]
    Pipeline(#[from] PipelineError),

    /// Configuration loading or persistence failed.
    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    /// Secure credential storage failed.
    #[error("security error: {0}")]
    Security(#[from] SecurityError),

    /// Model manifest or integrity verification failed.
    #[error("model error: {0}")]
    Model(#[from] ModelError),

    /// Screen capture initialization or capture failed.
    #[error("capture error: {0}")]
    Capture(#[from] CaptureError),

    /// OCR provider initialization failed.
    #[error("ocr error: {0}")]
    Ocr(#[from] OcrError),

    /// Translation provider initialization or execution failed.
    #[error("translation error: {0}")]
    Translation(#[from] TranslationError),

    /// A region failed core validation.
    #[error("invalid region: {0}")]
    InvalidRegion(String),

    /// An API key failed validation before being stored.
    #[error("invalid api key: {0}")]
    InvalidApiKey(String),

    /// A provider credential could not be validated, stored, or matched to
    /// a credential target.
    #[error("provider credential error: {0}")]
    ProviderCredential(String),

    /// A Tauri operation failed.
    #[error("tauri error: {0}")]
    Tauri(String),

    /// A global shortcut could not be parsed or registered.
    #[error("hotkey registration failed: {0}")]
    HotkeyFailed(String),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<CoreError> for AppError {
    fn from(error: CoreError) -> Self {
        match error {
            CoreError::InvalidRegion(message) => Self::InvalidRegion(message),
            other => Self::Core(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use vtrans_core::Language;

    #[test]
    fn serializes_user_facing_message_without_internal_shape() {
        let error = AppError::Pipeline(PipelineError::Cancelled);
        assert_eq!(
            serde_json::to_string(&error).unwrap(),
            r#""pipeline error: cancelled""#
        );
    }

    #[test]
    fn maps_core_validation_errors() {
        let error = AppError::from(CoreError::UnsupportedLanguage(Language::Japanese));
        assert!(matches!(error, AppError::Core(_)));
        assert!(error.to_string().contains("unsupported language"));
    }

    #[test]
    fn converts_every_upstream_error_variant() {
        let error = AppError::from(ConfigError::NotFound(PathBuf::from("config.json")));
        assert!(matches!(error, AppError::Config(_)));
        assert!(error.to_string().contains("config error"));

        let error = AppError::from(SecurityError::NotFound("translation".into()));
        assert!(matches!(error, AppError::Security(_)));
        assert!(error.to_string().contains("security error"));

        let error = AppError::from(ModelError::FileNotFound(PathBuf::from("det.onnx")));
        assert!(matches!(error, AppError::Model(_)));
        assert!(error.to_string().contains("model error"));

        let error = AppError::from(CaptureError::SessionStopped);
        assert!(matches!(error, AppError::Capture(_)));
        assert!(error.to_string().contains("capture error"));

        let error = AppError::from(OcrError::Cancelled);
        assert!(matches!(error, AppError::Ocr(_)));
        assert!(error.to_string().contains("ocr error"));

        let error = AppError::from(TranslationError::Cancelled);
        assert!(matches!(error, AppError::Translation(_)));
        assert!(error.to_string().contains("translation error"));

        let error = AppError::from(PipelineError::Cancelled);
        assert!(matches!(error, AppError::Pipeline(_)));
        assert!(error.to_string().contains("pipeline error"));
    }

    #[test]
    fn app_specific_messages_are_user_facing() {
        assert!(AppError::NotInitialized
            .to_string()
            .contains("not initialized"));
        assert!(AppError::SelectionInProgress
            .to_string()
            .contains("already"));
        assert!(AppError::InvalidRegion("zero width".into())
            .to_string()
            .contains("invalid region"));
        assert!(AppError::InvalidApiKey("key must not be empty".into())
            .to_string()
            .contains("invalid api key"));
        assert!(
            AppError::ProviderCredential("baidu needs two credentials".into())
                .to_string()
                .contains("provider credential error")
        );
        assert!(AppError::Tauri("boom".into())
            .to_string()
            .contains("tauri error"));
        assert!(AppError::HotkeyFailed("conflict".into())
            .to_string()
            .contains("hotkey registration failed"));
    }
}
