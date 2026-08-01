//! Model manifest schema definitions.
//!
//! Defines the structure of `manifest.json`, which describes all OCR and
//! translation model files, their expected SHA-256 hashes, sizes, and
//! inference parameters. The manifest is the single source of truth for
//! model file locations and integrity checks.
//!
//! See `docs/modules/08-models.md` for the full specification.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vtrans_core::Language;

use crate::ModelError;

/// The only manifest schema version currently supported by this crate.
pub const SUPPORTED_MANIFEST_VERSION: u32 = 1;

/// The root model manifest, describing all OCR and translation models.
///
/// This is the top-level structure serialized as `manifest.json` in the
/// models directory.
///
/// # Example
///
/// ```
/// # use vtrans_models::manifest::ModelManifest;
/// let json = r#"{
///   "version": 1,
///   "ocr": {
///     "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
///     "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
///     "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
///     "rec_multi": null,
///     "dicts": {},
///     "preprocess_params": { "image_size": [960, 960], "mean": [0.485, 0.456, 0.406], "std": [0.229, 0.224, 0.225], "det_threshold": 0.3, "unclip_ratio": 2.0 }
///   },
///   "translation": null
/// }"#;
/// let manifest = ModelManifest::from_json_str(json).unwrap();
/// assert_eq!(manifest.version, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelManifest {
    /// Manifest schema version. Must equal [`SUPPORTED_MANIFEST_VERSION`].
    pub version: u32,
    /// OCR model group (detection + recognition + dictionaries).
    pub ocr: OcrModelGroup,
    /// Translation model group, if local translation is configured.
    pub translation: Option<TranslationModelGroup>,
}

/// Group of OCR model files and preprocessing parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcrModelGroup {
    /// Text detection model (e.g. PP-OCRv4 det).
    pub det: ModelEntry,
    /// Japanese text recognition model.
    pub rec_ja: ModelEntry,
    /// English text recognition model.
    pub rec_en: ModelEntry,
    /// Optional multi-language recognition model.
    pub rec_multi: Option<ModelEntry>,
    /// Character dictionary files, keyed by language code (e.g. `"ja"`, `"en"`).
    ///
    /// Values are paths relative to the models directory.
    pub dicts: HashMap<String, PathBuf>,
    /// Image preprocessing parameters for the detection model.
    pub preprocess_params: PreprocessParams,
}

/// Group of translation model files and inference parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationModelGroup {
    /// The translation ONNX model.
    pub model: ModelEntry,
    /// The tokenizer file.
    pub tokenizer: ModelEntry,
    /// Supported `(source, target)` language pairs.
    pub supported_pairs: Vec<(Language, Language)>,
    /// Maximum source sequence length in tokens.
    pub max_length: usize,
    /// Inference parameters for the translation model.
    pub inference_params: InferenceParams,
}

/// A single model file entry with integrity metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Stable identifier (e.g. `"ppocr-det-v4"`).
    pub id: String,
    /// Path relative to the models directory (e.g. `"ocr/det.onnx"`).
    pub path: PathBuf,
    /// Expected SHA-256 hash as a lowercase hex string.
    pub sha256: String,
    /// Expected file size in bytes.
    pub size_bytes: u64,
}

/// Image preprocessing parameters for OCR detection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreprocessParams {
    /// Input image dimensions `(width, height)` for the detection model.
    pub image_size: (u32, u32),
    /// Per-channel mean for normalization (RGB order).
    pub mean: [f32; 3],
    /// Per-channel standard deviation for normalization (RGB order).
    pub std: [f32; 3],
    /// Binarization threshold for the detection probability map.
    pub det_threshold: f32,
    /// Unclip ratio for expanding detected text regions.
    pub unclip_ratio: f32,
}

/// Inference parameters for the translation model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferenceParams {
    /// Maximum batch size for inference.
    pub max_batch_size: usize,
    /// Number of beams for beam search.
    pub num_beams: usize,
}

impl ModelManifest {
    /// Parse a manifest from a JSON string.
    ///
    /// # Errors
    /// Returns [`ModelError::Parse`] if the JSON is malformed or missing
    /// required fields, or [`ModelError::UnsupportedVersion`] if the
    /// manifest version is not [`SUPPORTED_MANIFEST_VERSION`].
    #[tracing::instrument(skip(json))]
    pub fn from_json_str(json: &str) -> Result<Self, ModelError> {
        let manifest: Self = serde_json::from_str(json)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Load and parse a manifest from a file.
    ///
    /// # Errors
    /// Returns [`ModelError::Io`] if the file cannot be read,
    /// [`ModelError::Parse`] if the content is invalid JSON, or
    /// [`ModelError::UnsupportedVersion`] if the version is unsupported.
    #[tracing::instrument]
    pub fn from_path(path: &Path) -> Result<Self, ModelError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_json_str(&content)
    }

    /// Validate the manifest by checking the schema version.
    ///
    /// # Errors
    /// Returns [`ModelError::UnsupportedVersion`] if the version is not
    /// [`SUPPORTED_MANIFEST_VERSION`].
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.version != SUPPORTED_MANIFEST_VERSION {
            return Err(ModelError::UnsupportedVersion(self.version));
        }
        Ok(())
    }

    /// Collect references to all model entries (OCR + translation).
    ///
    /// Includes detection, recognition, and translation model/tokenizer
    /// entries. Optional entries (`rec_multi`, `translation`) are included
    /// only when present.
    #[must_use]
    pub fn all_entries(&self) -> Vec<&ModelEntry> {
        let mut entries = Vec::with_capacity(6);
        entries.push(&self.ocr.det);
        entries.push(&self.ocr.rec_ja);
        entries.push(&self.ocr.rec_en);
        if let Some(ref multi) = self.ocr.rec_multi {
            entries.push(multi);
        }
        if let Some(ref trans) = self.translation {
            entries.push(&trans.model);
            entries.push(&trans.tokenizer);
        }
        entries
    }

    /// Collect references to all dictionary file paths.
    ///
    /// Returns `(language_code, relative_path)` pairs for each dictionary
    /// in the OCR model group.
    #[must_use]
    pub fn dict_paths(&self) -> Vec<(&str, &PathBuf)> {
        self.ocr
            .dicts
            .iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid manifest JSON without translation.
    const VALID_JSON_NO_TRANS: &str = r#"{
        "version": 1,
        "ocr": {
            "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc123", "size_bytes": 100 },
            "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def456", "size_bytes": 200 },
            "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi789", "size_bytes": 300 },
            "rec_multi": null,
            "dicts": { "ja": "ocr/dict_ja.txt", "en": "ocr/dict_en.txt" },
            "preprocess_params": {
                "image_size": [960, 960],
                "mean": [0.485, 0.456, 0.406],
                "std": [0.229, 0.224, 0.225],
                "det_threshold": 0.3,
                "unclip_ratio": 2.0
            }
        },
        "translation": null
    }"#;

    /// Valid manifest JSON with translation.
    const VALID_JSON_WITH_TRANS: &str = r#"{
        "version": 1,
        "ocr": {
            "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
            "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
            "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
            "rec_multi": { "id": "rm", "path": "ocr/rec_multi.onnx", "sha256": "jkl", "size_bytes": 4 },
            "dicts": {},
            "preprocess_params": { "image_size": [960, 960], "mean": [0.485, 0.456, 0.406], "std": [0.229, 0.224, 0.225], "det_threshold": 0.3, "unclip_ratio": 2.0 }
        },
        "translation": {
            "model": { "id": "tm", "path": "translation/model.onnx", "sha256": "mno", "size_bytes": 5 },
            "tokenizer": { "id": "tk", "path": "translation/tokenizer.json", "sha256": "pqr", "size_bytes": 6 },
            "supported_pairs": [["en", "zh-CN"]],
            "max_length": 512,
            "inference_params": { "max_batch_size": 1, "num_beams": 4 }
        }
    }"#;

    #[test]
    fn parse_valid_no_translation() {
        let manifest = ModelManifest::from_json_str(VALID_JSON_NO_TRANS).unwrap();
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.ocr.det.id, "det");
        assert_eq!(manifest.ocr.rec_ja.id, "rj");
        assert_eq!(manifest.ocr.rec_en.id, "re");
        assert!(manifest.ocr.rec_multi.is_none());
        assert!(manifest.translation.is_none());
        assert_eq!(manifest.ocr.dicts.len(), 2);
    }

    #[test]
    fn parse_valid_with_translation() {
        let manifest = ModelManifest::from_json_str(VALID_JSON_WITH_TRANS).unwrap();
        assert!(manifest.ocr.rec_multi.is_some());
        let trans = manifest.translation.as_ref().unwrap();
        assert_eq!(trans.model.id, "tm");
        assert_eq!(trans.tokenizer.id, "tk");
        assert_eq!(trans.supported_pairs.len(), 1);
        assert_eq!(trans.supported_pairs[0].0, Language::English);
        assert_eq!(trans.supported_pairs[0].1, Language::ChineseSimplified);
        assert_eq!(trans.max_length, 512);
        assert_eq!(trans.inference_params.num_beams, 4);
    }

    #[test]
    fn missing_required_field_returns_parse_error() {
        let json = r#"{ "version": 1 }"#;
        let result = ModelManifest::from_json_str(json);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ModelError::Parse(_)));
    }

    #[test]
    fn unsupported_version_returns_error() {
        let json = VALID_JSON_NO_TRANS.replace(r#""version": 1"#, r#""version": 99"#);
        let result = ModelManifest::from_json_str(&json);
        assert!(matches!(
            result.unwrap_err(),
            ModelError::UnsupportedVersion(99)
        ));
    }

    #[test]
    fn serde_roundtrip() {
        let manifest = ModelManifest::from_json_str(VALID_JSON_WITH_TRANS).unwrap();
        let json = serde_json::to_string(&manifest).unwrap();
        let back = ModelManifest::from_json_str(&json).unwrap();
        assert_eq!(manifest, back);
    }

    #[test]
    fn all_entries_no_translation() {
        let manifest = ModelManifest::from_json_str(VALID_JSON_NO_TRANS).unwrap();
        let entries = manifest.all_entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].id, "det");
        assert_eq!(entries[1].id, "rj");
        assert_eq!(entries[2].id, "re");
    }

    #[test]
    fn all_entries_with_translation() {
        let manifest = ModelManifest::from_json_str(VALID_JSON_WITH_TRANS).unwrap();
        let entries = manifest.all_entries();
        assert_eq!(entries.len(), 6);
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"rm"));
        assert!(ids.contains(&"tm"));
        assert!(ids.contains(&"tk"));
    }

    #[test]
    fn dict_paths_collected() {
        let manifest = ModelManifest::from_json_str(VALID_JSON_NO_TRANS).unwrap();
        let dicts = manifest.dict_paths();
        assert_eq!(dicts.len(), 2);
        let keys: Vec<&str> = dicts.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"ja"));
        assert!(keys.contains(&"en"));
    }

    #[test]
    fn validate_ok() {
        let manifest = ModelManifest::from_json_str(VALID_JSON_NO_TRANS).unwrap();
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn preprocess_params_parsed() {
        let manifest = ModelManifest::from_json_str(VALID_JSON_NO_TRANS).unwrap();
        let pp = &manifest.ocr.preprocess_params;
        assert_eq!(pp.image_size, (960, 960));
        assert!((pp.det_threshold - 0.3).abs() < f32::EPSILON);
        assert!((pp.unclip_ratio - 2.0).abs() < f32::EPSILON);
        assert!((pp.mean[0] - 0.485).abs() < 1e-6);
    }
}
