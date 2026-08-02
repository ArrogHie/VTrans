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
        Self::InvalidRegion(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(matches!(error, AppError::InvalidRegion(_)));
        assert!(error.to_string().contains("unsupported language"));
    }
}
