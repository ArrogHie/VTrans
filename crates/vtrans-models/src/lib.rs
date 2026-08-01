//! `VTrans` model management module.
//!
//! Manages OCR and translation model manifests, integrity verification
//! (SHA-256), and path resolution. Model files are not committed to Git;
//! they are managed via a `manifest.json` and download scripts.
//!
//! # Example
//!
//! ```no_run
//! # use vtrans_models::{ModelManager, ModelError};
//! let manager = ModelManager::from_manifest_dir(
//!     std::path::Path::new("src-tauri/resources/models"),
//! )?;
//! let report = manager.verify_integrity()?;
//! println!("{}/{} files passed", report.passed, report.checked);
//! # Ok::<(), ModelError>(())
//! ```
//!
//! See `docs/modules/08-models.md` for the full specification.

pub mod manager;
pub mod manifest;
pub mod path;
pub mod verify;

use std::path::PathBuf;

pub use manager::ModelManager;
pub use manifest::{
    InferenceParams, ModelEntry, ModelManifest, OcrModelGroup, PreprocessParams,
    TranslationModelGroup,
};
pub use verify::VerifyReport;

/// Errors that can occur during model manifest loading and integrity verification.
///
/// Includes the variants defined in the module specification plus an
/// `Io` variant for I/O errors that are not covered by `FileNotFound`
/// (e.g. permission errors when reading an existing file for hashing).
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    /// The manifest file was not found at the expected path.
    #[error("manifest not found at {0}")]
    ManifestNotFound(PathBuf),

    /// The manifest JSON could not be parsed or was missing required fields.
    #[error("manifest parse error: {0}")]
    Parse(#[from] serde_json::Error),

    /// A model file referenced by the manifest was not found on disk.
    #[error("model file not found: {0}")]
    FileNotFound(PathBuf),

    /// The SHA-256 hash of a model file did not match the expected value.
    #[error("sha256 mismatch for {id}: expected {expected}, got {actual}")]
    HashMismatch {
        /// The model entry identifier.
        id: String,
        /// The expected SHA-256 hash from the manifest.
        expected: String,
        /// The actual SHA-256 hash computed from the file.
        actual: String,
    },

    /// The manifest schema version is not supported by this crate.
    #[error("unsupported manifest version: {0}")]
    UnsupportedVersion(u32),

    /// An I/O error occurred while reading a file.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_error_manifest_not_found_display() {
        let err = ModelError::ManifestNotFound(PathBuf::from("/models/manifest.json"));
        assert!(err.to_string().contains("manifest not found"));
        assert!(err.to_string().contains("manifest.json"));
    }

    #[test]
    fn model_error_parse_from_serde() {
        let json_err = serde_json::from_str::<i32>("bad").unwrap_err();
        let err = ModelError::from(json_err);
        assert!(matches!(err, ModelError::Parse(_)));
        assert!(err.to_string().contains("manifest parse error"));
    }

    #[test]
    fn model_error_file_not_found_display() {
        let err = ModelError::FileNotFound(PathBuf::from("ocr/det.onnx"));
        assert!(err.to_string().contains("model file not found"));
        assert!(err.to_string().contains("det.onnx"));
    }

    #[test]
    fn model_error_hash_mismatch_display() {
        let err = ModelError::HashMismatch {
            id: "det".to_string(),
            expected: "abc".to_string(),
            actual: "def".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("det"));
        assert!(msg.contains("abc"));
        assert!(msg.contains("def"));
    }

    #[test]
    fn model_error_unsupported_version_display() {
        let err = ModelError::UnsupportedVersion(99);
        assert!(err.to_string().contains("99"));
    }

    #[test]
    fn model_error_io_from_io_error() {
        let io_err = std::io::Error::other("test");
        let err = ModelError::from(io_err);
        assert!(matches!(err, ModelError::Io(_)));
    }
}
