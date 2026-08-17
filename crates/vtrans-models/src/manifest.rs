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
///     "preprocess_params": {
///       "image_size": [960, 960],
///       "mean": [0.485, 0.456, 0.406],
///       "std": [0.229, 0.224, 0.225],
///       "det_threshold": 0.2,
///       "unclip_ratio": 1.4,
///       "box_threshold": 0.45,
///       "max_candidates": 3000,
///       "min_box_size": 3.0,
///       "rec_input_height": 48,
///       "rec_input_width": 320,
///       "rec_append_space": true,
///       "rec_blank_index": 0
///     }
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
    /// Text detection model (e.g. PP-OCRv6 Small det).
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
///
/// # Schema evolution
///
/// The manifest schema version stays at 1. Fields added after the original
/// release are optional and default via serde, so an old manifest without
/// them still deserializes (see the per-field docs for the defaults).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Stable identifier (e.g. `"ppocr-det-v6"`).
    pub id: String,
    /// Path relative to the models directory (e.g. `"ocr/det.onnx"`).
    pub path: PathBuf,
    /// Expected SHA-256 hash as a lowercase hex string.
    pub sha256: String,
    /// Expected file size in bytes.
    pub size_bytes: u64,
    /// Whether the entry is optional: missing optional files do not count
    /// as integrity failures; they are reported as skipped and can be
    /// installed later (e.g. downloaded by the app).
    ///
    /// Absent in the JSON: defaults to `false`.
    #[serde(default)]
    pub optional: bool,
    /// Download URL for the file, consumed by the app's download flow.
    /// This crate never performs downloads; it only carries the metadata.
    ///
    /// Absent in the JSON: defaults to `None`.
    #[serde(default)]
    pub download_url: Option<String>,
    /// Expected download size in bytes, used by the app to show progress.
    /// For bundled optional files this typically equals [`size_bytes`](Self::size_bytes).
    ///
    /// Absent in the JSON: defaults to `None`.
    #[serde(default)]
    pub download_size_bytes: Option<u64>,
}

/// Image preprocessing parameters for OCR detection.
///
/// # Schema evolution
///
/// The manifest schema version stays at 1. Fields added after the original
/// v4-era release are optional and default via serde. A manifest without
/// them (e.g. a v4 manifest) still deserializes, and the accessor methods
/// on this struct fall back to the PP-OCRv6 defaults documented in
/// `docs/PP-OCRv6_small_ONNX_Rust_TS_接入指南.md` §10.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreprocessParams {
    /// Input image dimensions `(width, height)` for the detection model.
    pub image_size: (u32, u32),
    /// Per-channel mean for normalization. Channel order is determined by
    /// the model pipeline (PP-OCRv6 uses BGR; the Python baseline is the
    /// authority — see the integration guide §6.3).
    pub mean: [f32; 3],
    /// Per-channel standard deviation for normalization. Channel order is
    /// determined by the model pipeline (see `mean`).
    pub std: [f32; 3],
    /// Binarization threshold for the detection probability map.
    pub det_threshold: f32,
    /// Unclip ratio for expanding detected text regions.
    pub unclip_ratio: f32,
    /// Confidence threshold for filtering detected text boxes (default 0.45).
    ///
    /// Absent in the JSON: defaults to [`DEFAULT_BOX_THRESHOLD`].
    #[serde(default = "default_box_threshold")]
    pub box_threshold: f32,
    /// Maximum number of candidate boxes considered by DB postprocessing
    /// (default 3000).
    ///
    /// Absent in the JSON: defaults to [`DEFAULT_MAX_CANDIDATES`].
    #[serde(default = "default_max_candidates")]
    pub max_candidates: usize,
    /// Minimum box side length in pixels for detection boxes (default 3.0).
    ///
    /// Absent in the JSON: defaults to [`DEFAULT_MIN_BOX_SIZE`].
    #[serde(default = "default_min_box_size")]
    pub min_box_size: f32,
    /// Recognition input height in pixels (default 48).
    ///
    /// Absent in the JSON: defaults to [`DEFAULT_REC_INPUT_HEIGHT`].
    #[serde(default = "default_rec_input_height")]
    pub rec_input_height: u32,
    /// Recognition input width in pixels (default 320).
    ///
    /// Absent in the JSON: defaults to [`DEFAULT_REC_INPUT_WIDTH`].
    #[serde(default = "default_rec_input_width")]
    pub rec_input_width: u32,
    /// Whether the CTC character table appends a space after the dictionary
    /// (default `true`).
    ///
    /// Absent in the JSON: defaults to [`DEFAULT_REC_APPEND_SPACE`].
    #[serde(default = "default_rec_append_space")]
    pub rec_append_space: bool,
    /// Index of the CTC blank token in the character table (default 0).
    ///
    /// Absent in the JSON: defaults to [`DEFAULT_REC_BLANK_INDEX`].
    #[serde(default = "default_rec_blank_index")]
    pub rec_blank_index: usize,
}

/// Default `box_threshold` (PP-OCRv6 Small det, guide §6.1 / §10.1).
pub const DEFAULT_BOX_THRESHOLD: f32 = 0.45;
/// Default `max_candidates` (PP-OCRv6 Small det, guide §6.1 / §10.1).
pub const DEFAULT_MAX_CANDIDATES: usize = 3000;
/// Default `min_box_size` (guide §6.5 / §10.1).
pub const DEFAULT_MIN_BOX_SIZE: f32 = 3.0;
/// Default recognition input height (PP-OCRv6 Small rec, guide §8 / §10.1).
pub const DEFAULT_REC_INPUT_HEIGHT: u32 = 48;
/// Default recognition input width (PP-OCRv6 Small rec, guide §8 / §10.1).
pub const DEFAULT_REC_INPUT_WIDTH: u32 = 320;
/// Default `append_space` (PP-OCRv6 rec uses a space character, guide §8.1/§9.2).
pub const DEFAULT_REC_APPEND_SPACE: bool = true;
/// Default CTC blank index (guide §9.2).
pub const DEFAULT_REC_BLANK_INDEX: usize = 0;

const fn default_box_threshold() -> f32 {
    DEFAULT_BOX_THRESHOLD
}

const fn default_max_candidates() -> usize {
    DEFAULT_MAX_CANDIDATES
}

const fn default_min_box_size() -> f32 {
    DEFAULT_MIN_BOX_SIZE
}

const fn default_rec_input_height() -> u32 {
    DEFAULT_REC_INPUT_HEIGHT
}

const fn default_rec_input_width() -> u32 {
    DEFAULT_REC_INPUT_WIDTH
}

const fn default_rec_append_space() -> bool {
    DEFAULT_REC_APPEND_SPACE
}

const fn default_rec_blank_index() -> usize {
    DEFAULT_REC_BLANK_INDEX
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
                "det_threshold": 0.2,
                "unclip_ratio": 1.4,
                "box_threshold": 0.45,
                "max_candidates": 3000,
                "min_box_size": 3.0,
                "rec_input_height": 48,
                "rec_input_width": 320,
                "rec_append_space": true,
                "rec_blank_index": 0
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
            "preprocess_params": { "image_size": [960, 960], "mean": [0.485, 0.456, 0.406], "std": [0.229, 0.224, 0.225], "det_threshold": 0.2, "unclip_ratio": 1.4 }
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
        assert!((pp.det_threshold - 0.2).abs() < f32::EPSILON);
        assert!((pp.unclip_ratio - 1.4).abs() < f32::EPSILON);
        assert!((pp.mean[0] - 0.485).abs() < 1e-6);
        assert!((pp.box_threshold - DEFAULT_BOX_THRESHOLD).abs() < f32::EPSILON);
        assert_eq!(pp.max_candidates, DEFAULT_MAX_CANDIDATES);
        assert!((pp.min_box_size - DEFAULT_MIN_BOX_SIZE).abs() < f32::EPSILON);
        assert_eq!(pp.rec_input_height, DEFAULT_REC_INPUT_HEIGHT);
        assert_eq!(pp.rec_input_width, DEFAULT_REC_INPUT_WIDTH);
        assert_eq!(pp.rec_append_space, DEFAULT_REC_APPEND_SPACE);
        assert_eq!(pp.rec_blank_index, DEFAULT_REC_BLANK_INDEX);
    }

    #[test]
    fn preprocess_params_v4_manifest_uses_ppocrv6_defaults() {
        // A legacy v4-era manifest has no det/rec extension fields. It must
        // still deserialize, and every new field must take the PP-OCRv6
        // default value (schema is backward compatible).
        let json = r#"{
            "version": 1,
            "ocr": {
                "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
                "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
                "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
                "rec_multi": null,
                "dicts": {},
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
        let manifest = ModelManifest::from_json_str(json).unwrap();
        let pp = &manifest.ocr.preprocess_params;
        assert!((pp.det_threshold - 0.3).abs() < f32::EPSILON);
        assert!((pp.unclip_ratio - 2.0).abs() < f32::EPSILON);
        assert!((pp.box_threshold - DEFAULT_BOX_THRESHOLD).abs() < f32::EPSILON);
        assert_eq!(pp.max_candidates, DEFAULT_MAX_CANDIDATES);
        assert!((pp.min_box_size - DEFAULT_MIN_BOX_SIZE).abs() < f32::EPSILON);
        assert_eq!(pp.rec_input_height, DEFAULT_REC_INPUT_HEIGHT);
        assert_eq!(pp.rec_input_width, DEFAULT_REC_INPUT_WIDTH);
        assert!(pp.rec_append_space);
        assert_eq!(pp.rec_blank_index, DEFAULT_REC_BLANK_INDEX);
    }

    #[test]
    fn preprocess_params_absent_uses_defaults() {
        // An entirely absent preprocess_params block is not allowed (the
        // field is required), but individual new fields may be omitted.
        let json = r#"{
            "version": 1,
            "ocr": {
                "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
                "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
                "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
                "rec_multi": null,
                "dicts": {},
                "preprocess_params": {
                    "image_size": [960, 960],
                    "mean": [0.485, 0.456, 0.406],
                    "std": [0.229, 0.224, 0.225],
                    "det_threshold": 0.2,
                    "unclip_ratio": 1.4,
                    "box_threshold": 0.6,
                    "rec_input_height": 64
                }
            },
            "translation": null
        }"#;
        let manifest = ModelManifest::from_json_str(json).unwrap();
        let pp = &manifest.ocr.preprocess_params;
        assert!((pp.box_threshold - 0.6).abs() < f32::EPSILON);
        assert_eq!(pp.max_candidates, DEFAULT_MAX_CANDIDATES);
        assert!((pp.min_box_size - DEFAULT_MIN_BOX_SIZE).abs() < f32::EPSILON);
        assert_eq!(pp.rec_input_height, 64);
        assert_eq!(pp.rec_input_width, DEFAULT_REC_INPUT_WIDTH);
        assert!(pp.rec_append_space);
        assert_eq!(pp.rec_blank_index, DEFAULT_REC_BLANK_INDEX);
    }

    #[test]
    fn preprocess_params_custom_values_roundtrip() {
        let json = r#"{
            "version": 1,
            "ocr": {
                "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
                "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
                "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
                "rec_multi": null,
                "dicts": {},
                "preprocess_params": {
                    "image_size": [960, 960],
                    "mean": [0.485, 0.456, 0.406],
                    "std": [0.229, 0.224, 0.225],
                    "det_threshold": 0.18,
                    "unclip_ratio": 1.6,
                    "box_threshold": 0.5,
                    "max_candidates": 2000,
                    "min_box_size": 5.0,
                    "rec_input_height": 48,
                    "rec_input_width": 256,
                    "rec_append_space": false,
                    "rec_blank_index": 1
                }
            },
            "translation": null
        }"#;
        let manifest = ModelManifest::from_json_str(json).unwrap();
        let pp = &manifest.ocr.preprocess_params;
        assert!((pp.det_threshold - 0.18).abs() < f32::EPSILON);
        assert!((pp.unclip_ratio - 1.6).abs() < f32::EPSILON);
        assert!((pp.box_threshold - 0.5).abs() < f32::EPSILON);
        assert_eq!(pp.max_candidates, 2000);
        assert!((pp.min_box_size - 5.0).abs() < f32::EPSILON);
        assert_eq!(pp.rec_input_height, 48);
        assert_eq!(pp.rec_input_width, 256);
        assert!(!pp.rec_append_space);
        assert_eq!(pp.rec_blank_index, 1);

        // Roundtrip must preserve the custom values.
        let serialized = serde_json::to_string(&manifest).unwrap();
        let back = ModelManifest::from_json_str(&serialized).unwrap();
        assert_eq!(back.ocr.preprocess_params, *pp);
    }

    #[test]
    fn model_entry_optional_fields_defaults() {
        // A manifest without the new optional-entry fields must still
        // deserialize, with `optional` false and no download metadata.
        let manifest = ModelManifest::from_json_str(VALID_JSON_WITH_TRANS).unwrap();
        let model = &manifest.translation.as_ref().unwrap().model;
        assert!(!model.optional);
        assert_eq!(model.download_url, None);
        assert_eq!(model.download_size_bytes, None);
    }

    #[test]
    fn model_entry_optional_fields_parsed() {
        let json = r#"{
            "version": 1,
            "ocr": {
                "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
                "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
                "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
                "rec_multi": null,
                "dicts": {},
                "preprocess_params": {
                    "image_size": [960, 960],
                    "mean": [0.485, 0.456, 0.406],
                    "std": [0.229, 0.224, 0.225],
                    "det_threshold": 0.2,
                    "unclip_ratio": 1.4
                }
            },
            "translation": {
                "model": {
                    "id": "tm",
                    "path": "translation/model.onnx",
                    "sha256": "mno",
                    "size_bytes": 5,
                    "optional": true,
                    "download_url": "https://example.com/translation-model.onnx",
                    "download_size_bytes": 403368390
                },
                "tokenizer": { "id": "tk", "path": "translation/tokenizer.json", "sha256": "pqr", "size_bytes": 6 },
                "supported_pairs": [["en", "zh-CN"]],
                "max_length": 512,
                "inference_params": { "max_batch_size": 1, "num_beams": 4 }
            }
        }"#;
        let manifest = ModelManifest::from_json_str(json).unwrap();
        let model = &manifest.translation.as_ref().unwrap().model;
        assert!(model.optional);
        assert_eq!(
            model.download_url.as_deref(),
            Some("https://example.com/translation-model.onnx")
        );
        assert_eq!(model.download_size_bytes, Some(403_368_390));
        // The tokenizer has no new fields: all defaults.
        let tokenizer = &manifest.translation.as_ref().unwrap().tokenizer;
        assert!(!tokenizer.optional);
        assert_eq!(tokenizer.download_url, None);
        assert_eq!(tokenizer.download_size_bytes, None);
    }

    #[test]
    fn model_entry_optional_fields_roundtrip() {
        let json = r#"{
            "version": 1,
            "ocr": {
                "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
                "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
                "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
                "rec_multi": null,
                "dicts": {},
                "preprocess_params": {
                    "image_size": [960, 960],
                    "mean": [0.485, 0.456, 0.406],
                    "std": [0.229, 0.224, 0.225],
                    "det_threshold": 0.2,
                    "unclip_ratio": 1.4
                }
            },
            "translation": {
                "model": {
                    "id": "tm",
                    "path": "translation/model.onnx",
                    "sha256": "mno",
                    "size_bytes": 5,
                    "optional": true,
                    "download_url": "https://example.com/translation-model.onnx",
                    "download_size_bytes": 5
                },
                "tokenizer": { "id": "tk", "path": "translation/tokenizer.json", "sha256": "pqr", "size_bytes": 6 },
                "supported_pairs": [["en", "zh-CN"]],
                "max_length": 512,
                "inference_params": { "max_batch_size": 1, "num_beams": 4 }
            }
        }"#;
        let manifest = ModelManifest::from_json_str(json).unwrap();
        let serialized = serde_json::to_string(&manifest).unwrap();
        let back = ModelManifest::from_json_str(&serialized).unwrap();
        assert_eq!(manifest, back);
        assert_eq!(
            back.translation.as_ref().unwrap().model,
            manifest.translation.as_ref().unwrap().model
        );
    }

    #[test]
    fn model_entry_explicit_optional_false_parsed() {
        // `optional: false` and `download_url: null` must parse the same as
        // absent fields.
        let json = r#"{
            "version": 1,
            "ocr": {
                "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1, "optional": false, "download_url": null, "download_size_bytes": null },
                "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
                "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
                "rec_multi": null,
                "dicts": {},
                "preprocess_params": {
                    "image_size": [960, 960],
                    "mean": [0.485, 0.456, 0.406],
                    "std": [0.229, 0.224, 0.225],
                    "det_threshold": 0.2,
                    "unclip_ratio": 1.4
                }
            },
            "translation": null
        }"#;
        let manifest = ModelManifest::from_json_str(json).unwrap();
        let det = &manifest.ocr.det;
        assert!(!det.optional);
        assert_eq!(det.download_url, None);
        assert_eq!(det.download_size_bytes, None);
    }
}
