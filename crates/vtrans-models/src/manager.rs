//! Model manager: loads manifests, resolves paths, and verifies integrity.
//!
//! [`ModelManager`] is the primary entry point for this crate. It reads a
//! `manifest.json` from a directory, validates the schema, and provides
//! methods for resolving model file paths and verifying SHA-256 integrity.

use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::manifest::{ModelEntry, ModelManifest};
use crate::path::resolve_model_path;
use crate::verify::{verify_entry, VerifyReport};
use crate::ModelError;

/// Manages model manifests, path resolution, and integrity verification.
///
/// Created from a directory containing `manifest.json`. The directory is
/// used as the base for resolving all relative model paths in the manifest.
///
/// # Example
///
/// ```no_run
/// # use vtrans_models::manager::ModelManager;
/// let manager = ModelManager::from_manifest_dir(
///     std::path::Path::new("src-tauri/resources/models"),
/// )
/// .unwrap();
/// println!("manifest version: {}", manager.manifest().version);
/// ```
#[derive(Debug)]
pub struct ModelManager {
    /// The parsed manifest.
    manifest: ModelManifest,
    /// Directory containing `manifest.json`; base for relative paths.
    manifest_dir: PathBuf,
    /// Current model loading progress in `[0.0, 1.0]`, or `None` if idle.
    load_progress: Option<f32>,
}

impl ModelManager {
    /// Create a [`ModelManager`] from a directory containing `manifest.json`.
    ///
    /// # Errors
    /// Returns [`ModelError::ManifestNotFound`] if `manifest.json` is absent,
    /// [`ModelError::Parse`] if the JSON is invalid, or
    /// [`ModelError::UnsupportedVersion`] if the schema version is unsupported.
    #[tracing::instrument]
    pub fn from_manifest_dir(dir: &Path) -> Result<Self, ModelError> {
        let manifest_path = dir.join("manifest.json");
        if !manifest_path.exists() {
            warn!(path = %manifest_path.display(), "manifest not found");
            return Err(ModelError::ManifestNotFound(manifest_path));
        }
        let manifest = ModelManifest::from_path(&manifest_path)?;
        info!(
            version = manifest.version,
            dir = %dir.display(),
            "manifest loaded"
        );
        Ok(Self {
            manifest,
            manifest_dir: dir.to_path_buf(),
            load_progress: None,
        })
    }

    /// Returns a reference to the loaded manifest.
    #[must_use]
    pub fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    /// Returns the directory the manifest was loaded from.
    #[must_use]
    pub fn manifest_dir(&self) -> &Path {
        &self.manifest_dir
    }

    /// Verify the integrity of all model files referenced by the manifest.
    ///
    /// Checks that every model entry file exists and its SHA-256 hash matches
    /// the expected value. Dictionary files are checked for existence only
    /// (they have no hash in the manifest). Results are aggregated into a
    /// [`VerifyReport`].
    ///
    /// All failures (missing files, hash mismatches, I/O errors) are
    /// recorded as human-readable strings in the report's `failed` list.
    /// The method always returns `Ok(report)`; it never returns `Err`.
    ///
    /// # Errors
    ///
    /// This method currently always returns `Ok`. The `Result` return type
    /// is retained for forward compatibility if future versions need to
    /// propagate fatal errors. All file-level failures are recorded in
    /// [`VerifyReport::failed`].
    #[tracing::instrument(skip(self))]
    pub fn verify_integrity(&self) -> Result<VerifyReport, ModelError> {
        let mut report = VerifyReport::new();

        for entry in self.manifest.all_entries() {
            report.checked += 1;
            match verify_entry(&self.manifest_dir, entry) {
                Ok(()) => {
                    report.passed += 1;
                }
                Err(e) => {
                    report.failed.push(e.to_string());
                }
            }
        }

        for (name, dict_path) in self.manifest.dict_paths() {
            report.checked += 1;
            let full_path = self.manifest_dir.join(dict_path);
            if full_path.exists() {
                report.passed += 1;
            } else {
                warn!(
                    dict = name,
                    path = %full_path.display(),
                    "dict file not found"
                );
                report
                    .failed
                    .push(format!("dict file not found: {}", full_path.display()));
            }
        }

        info!(
            checked = report.checked,
            passed = report.passed,
            failed = report.failed.len(),
            "integrity verification complete"
        );

        Ok(report)
    }

    /// Resolve a model entry's relative path to an absolute path.
    ///
    /// The returned path is `manifest_dir / entry.path`. This method does
    /// not check whether the file exists; use [`verify_integrity`](Self::verify_integrity)
    /// for that.
    #[must_use]
    pub fn model_path(&self, entry: &ModelEntry) -> PathBuf {
        resolve_model_path(&self.manifest_dir, &entry.path)
    }

    /// Returns the current model loading progress, if any.
    ///
    /// `None` means no loading is in progress. When set, the value is in
    /// `[0.0, 1.0]` where `1.0` means complete.
    #[must_use]
    pub fn load_progress(&self) -> Option<f32> {
        self.load_progress
    }

    /// Set the current model loading progress.
    ///
    /// Pass `None` to indicate loading is complete or idle. Values should be
    /// in `[0.0, 1.0]`.
    pub fn set_load_progress(&mut self, progress: Option<f32>) {
        if let Some(p) = progress {
            debug_assert!(
                (0.0..=1.0).contains(&p),
                "load_progress should be in [0.0, 1.0], got {p}"
            );
        }
        self.load_progress = progress;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    /// Write `content` to `dir/rel_path` and return its SHA-256 hex string.
    fn write_file(dir: &Path, rel_path: &str, content: &[u8]) -> String {
        let full = dir.join(rel_path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, content).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(content);
        format!("{:x}", hasher.finalize())
    }

    /// Build a manifest JSON string with the given entry hashes.
    fn manifest_json(det_sha: &str, rec_ja_sha: &str, rec_en_sha: &str) -> String {
        format!(
            r#"{{
  "version": 1,
  "ocr": {{
    "det": {{ "id": "det", "path": "ocr/det.onnx", "sha256": "{det_sha}", "size_bytes": 10 }},
    "rec_ja": {{ "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "{rec_ja_sha}", "size_bytes": 20 }},
    "rec_en": {{ "id": "re", "path": "ocr/rec_en.onnx", "sha256": "{rec_en_sha}", "size_bytes": 30 }},
    "rec_multi": null,
    "dicts": {{ "ja": "ocr/dict_ja.txt", "en": "ocr/dict_en.txt" }},
    "preprocess_params": {{
      "image_size": [960, 960],
      "mean": [0.485, 0.456, 0.406],
      "std": [0.229, 0.224, 0.225],
      "det_threshold": 0.3,
      "unclip_ratio": 2.0
    }}
  }},
  "translation": null
}}"#
        )
    }

    /// Create a models directory with files and manifest, return the dir path.
    fn setup_models_dir() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let det_sha = write_file(dir.path(), "ocr/det.onnx", b"det model");
        let rec_ja_sha = write_file(dir.path(), "ocr/rec_ja.onnx", b"ja model");
        let rec_en_sha = write_file(dir.path(), "ocr/rec_en.onnx", b"en model");
        write_file(dir.path(), "ocr/dict_ja.txt", b"ja dict");
        write_file(dir.path(), "ocr/dict_en.txt", b"en dict");
        let json = manifest_json(&det_sha, &rec_ja_sha, &rec_en_sha);
        std::fs::write(dir.path().join("manifest.json"), json).unwrap();
        dir
    }

    #[test]
    fn from_manifest_dir_valid() {
        let dir = setup_models_dir();
        let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
        assert_eq!(manager.manifest().version, 1);
        assert_eq!(manager.manifest_dir(), dir.path());
    }

    #[test]
    fn from_manifest_dir_not_found() {
        let dir = tempdir().unwrap();
        let result = ModelManager::from_manifest_dir(dir.path());
        assert!(matches!(result, Err(ModelError::ManifestNotFound(_))));
    }

    #[test]
    fn from_manifest_dir_bad_json() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("manifest.json"), "not json").unwrap();
        let result = ModelManager::from_manifest_dir(dir.path());
        assert!(matches!(result, Err(ModelError::Parse(_))));
    }

    #[test]
    fn from_manifest_dir_unsupported_version() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("manifest.json"),
            r#"{
  "version": 99,
  "ocr": {
    "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
    "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
    "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
    "rec_multi": null,
    "dicts": {},
    "preprocess_params": { "image_size": [960, 960], "mean": [0.485, 0.456, 0.406], "std": [0.229, 0.224, 0.225], "det_threshold": 0.3, "unclip_ratio": 2.0 }
  },
  "translation": null
}"#,
        )
        .unwrap();
        let result = ModelManager::from_manifest_dir(dir.path());
        assert!(matches!(result, Err(ModelError::UnsupportedVersion(99))));
    }

    #[test]
    fn verify_integrity_all_pass() {
        let dir = setup_models_dir();
        let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
        let report = manager.verify_integrity().unwrap();
        assert_eq!(report.checked, 5); // 3 model entries + 2 dicts
        assert_eq!(report.passed, 5);
        assert!(report.failed.is_empty());
        assert!(report.is_ok());
    }

    #[test]
    fn verify_integrity_missing_model_file() {
        let dir = setup_models_dir();
        // Delete a model file.
        std::fs::remove_file(dir.path().join("ocr/det.onnx")).unwrap();
        let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
        let report = manager.verify_integrity().unwrap();
        assert_eq!(report.checked, 5);
        assert_eq!(report.passed, 4);
        assert_eq!(report.failed.len(), 1);
        assert!(!report.is_ok());
    }

    #[test]
    fn verify_integrity_hash_mismatch() {
        let dir = setup_models_dir();
        // Corrupt a model file.
        std::fs::write(dir.path().join("ocr/det.onnx"), b"corrupted").unwrap();
        let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
        let report = manager.verify_integrity().unwrap();
        assert_eq!(report.checked, 5);
        assert_eq!(report.passed, 4);
        assert_eq!(report.failed.len(), 1);
        assert!(report.failed[0].contains("sha256 mismatch"));
    }

    #[test]
    fn verify_integrity_missing_dict() {
        let dir = setup_models_dir();
        std::fs::remove_file(dir.path().join("ocr/dict_ja.txt")).unwrap();
        let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
        let report = manager.verify_integrity().unwrap();
        assert_eq!(report.checked, 5);
        assert_eq!(report.passed, 4);
        assert_eq!(report.failed.len(), 1);
        assert!(report.failed[0].contains("dict file not found"));
    }

    #[test]
    fn model_path_resolves() {
        let dir = setup_models_dir();
        let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
        let entry = &manager.manifest().ocr.det;
        let path = manager.model_path(entry);
        assert_eq!(path, dir.path().join("ocr/det.onnx"));
    }

    #[test]
    fn load_progress_default_none() {
        let dir = setup_models_dir();
        let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
        assert!(manager.load_progress().is_none());
    }

    #[test]
    fn load_progress_set_and_get() {
        let dir = setup_models_dir();
        let mut manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
        manager.set_load_progress(Some(0.5));
        assert_eq!(manager.load_progress(), Some(0.5));
        manager.set_load_progress(None);
        assert!(manager.load_progress().is_none());
    }
}
