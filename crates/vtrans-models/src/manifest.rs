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

use crate::ModelError;

/// The only manifest schema version currently supported by this crate.
///
/// Version 2 restructures the translation group into two engines
/// (Bergamot en→zh + `CTranslate2` ja→zh, see [`TranslationModels`]). The
/// OCR group structure is unchanged from version 1 and remains compatible.
/// Version 1 manifests are rejected by [`ModelManifest::validate`].
pub const SUPPORTED_MANIFEST_VERSION: u32 = 2;

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
///   "version": 2,
///   "ocr": {
///     "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
///     "rec_ja": { "id": "rj", "path": "ocr/rec.onnx", "sha256": "def", "size_bytes": 2 },
///     "rec_en": { "id": "re", "path": "ocr/rec.onnx", "sha256": "ghi", "size_bytes": 3 },
///     "rec_multi": { "id": "rm", "path": "ocr/rec.onnx", "sha256": "jkl", "size_bytes": 4 },
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
///   "translation": {
///     "target": "zh-Hans",
///     "engines": {
///       "en_zh": {
///         "engine": "bergamot",
///         "model": { "id": "enzh-model", "path": "translation/en-zh/model.enzh.intgemm.alphas.bin", "sha256": "aaa", "size_bytes": 1 },
///         "src_vocab": { "id": "enzh-src-vocab", "path": "translation/en-zh/srcvocab.enzh.spm", "sha256": "bbb", "size_bytes": 2 },
///         "trg_vocab": { "id": "enzh-trg-vocab", "path": "translation/en-zh/trgvocab.enzh.spm", "sha256": "ccc", "size_bytes": 3 },
///         "lexical_shortlist": { "id": "enzh-lex", "path": "translation/en-zh/lex.50.50.enzh.s2t.bin", "sha256": "ddd", "size_bytes": 4 },
///         "beam_size": 1,
///         "gemm_precision": "int8shiftAlphaAll"
///       },
///       "ja_zh": {
///         "engine": "ctranslate2",
///         "model": { "id": "jazh-model", "path": "translation/ja-zh/model.bin", "sha256": "eee", "size_bytes": 5 },
///         "config": { "id": "jazh-config", "path": "translation/ja-zh/config.json", "sha256": "fff", "size_bytes": 6 },
///         "source_vocabulary": { "id": "jazh-src-vocab", "path": "translation/ja-zh/source_vocabulary.json", "sha256": "ggg", "size_bytes": 7 },
///         "target_vocabulary": { "id": "jazh-trg-vocab", "path": "translation/ja-zh/target_vocabulary.json", "sha256": "hhh", "size_bytes": 8 },
///         "source_spm": { "id": "jazh-src-spm", "path": "translation/ja-zh/source.spm", "sha256": "iii", "size_bytes": 9 },
///         "target_spm": { "id": "jazh-trg-spm", "path": "translation/ja-zh/target.spm", "sha256": "jjj", "size_bytes": 10 },
///         "beam_size_fast": 1,
///         "beam_size_balanced": 4,
///         "max_input_tokens": 256
///       }
///     },
///     "budget_mb": { "hard_mb": 200, "target_mb": 175, "en_zh_mb": 65, "ja_zh_mb": 110 }
///   }
/// }"#;
/// let manifest = ModelManifest::from_json_str(json).unwrap();
/// assert_eq!(manifest.version, 2);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelManifest {
    /// Manifest schema version. Must equal [`SUPPORTED_MANIFEST_VERSION`].
    pub version: u32,
    /// OCR model group (detection + recognition + dictionaries).
    pub ocr: OcrModelGroup,
    /// Translation model groups (dual-engine), if local translation is configured.
    pub translation: Option<TranslationModels>,
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

/// Translation models for the two supported offline language pairs.
///
/// `en_zh` is a Bergamot (Marian) model family and `ja_zh` is a
/// `CTranslate2` INT8 model family; each family bundles its own model,
/// vocabularies and `SentencePiece` tokenizers. `budget_mb` carries the
/// size budget enforced by `scripts/translation/audit_model_sizes.py`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationModels {
    /// Target language code (always `"zh-Hans"` for this app).
    pub target: String,
    /// The two engine groups (en→zh and ja→zh).
    pub engines: TranslationEngines,
    /// Size budget in megabytes (hard / target / per-pair).
    pub budget_mb: TranslationBudget,
    /// Free-form provenance metadata, e.g. `model_revision`,
    /// `converted_with`, `registry_generated` (filled by the download and
    /// conversion scripts).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

/// The two translation engine groups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationEngines {
    /// English → Chinese (Bergamot / Marian).
    pub en_zh: BergamotModelGroup,
    /// Japanese → Chinese (`CTranslate2` INT8).
    pub ja_zh: CTranslate2ModelGroup,
}

/// Bergamot (Marian) English → Chinese model family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BergamotModelGroup {
    /// Engine identifier, always `"bergamot"`.
    pub engine: String,
    /// The quantized Marian model binary (`model.enzh.intgemm.alphas.bin`).
    pub model: ModelEntry,
    /// Source `SentencePiece` vocabulary (`srcvocab.enzh.spm`).
    pub src_vocab: ModelEntry,
    /// Target `SentencePiece` vocabulary (`trgvocab.enzh.spm`).
    pub trg_vocab: ModelEntry,
    /// Lexical shortlist (`lex.50.50.enzh.s2t.bin`).
    pub lexical_shortlist: ModelEntry,
    /// Beam size for decoding (default 1).
    pub beam_size: usize,
    /// GEMM precision; must match the `.intgemm.alphas.bin` model
    /// (default `"int8shiftAlphaAll"`).
    pub gemm_precision: String,
}

/// `CTranslate2` INT8 Japanese → Chinese model family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CTranslate2ModelGroup {
    /// Engine identifier, always `"ctranslate2"`.
    pub engine: String,
    /// The converted model binary (`model.bin`).
    pub model: ModelEntry,
    /// `CTranslate2` model configuration (`config.json`).
    pub config: ModelEntry,
    /// Source vocabulary JSON (`source_vocabulary.json`).
    pub source_vocabulary: ModelEntry,
    /// Target vocabulary JSON (`target_vocabulary.json`).
    pub target_vocabulary: ModelEntry,
    /// Source `SentencePiece` model (`source.spm`).
    pub source_spm: ModelEntry,
    /// Target `SentencePiece` model (`target.spm`).
    pub target_spm: ModelEntry,
    /// Beam size for the Fast quality preset (default 1).
    pub beam_size_fast: usize,
    /// Beam size for the Balanced quality preset (default 4).
    pub beam_size_balanced: usize,
    /// Maximum source token count per request (default 256).
    pub max_input_tokens: usize,
}

/// Translation model size budget in megabytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationBudget {
    /// Hard ceiling for the whole translation directory (default 200).
    pub hard_mb: u64,
    /// Target total size (default 175).
    pub target_mb: u64,
    /// Per-pair budget for en→zh (default 65).
    pub en_zh_mb: u64,
    /// Per-pair budget for ja→zh (default 110).
    pub ja_zh_mb: u64,
}

/// A single model file entry with integrity metadata.
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
}

/// Image preprocessing parameters for OCR detection.
///
/// # Schema evolution
///
/// The OCR group structure is shared by manifest versions 1 and 2. Fields
/// added after the original v4-era release are optional and default via
/// serde; a manifest without them still deserializes, and the accessor
/// methods on this struct fall back to the PP-OCRv6 defaults documented in
/// `docs/modules/08-models.md` ("`preprocess_params (v6 默认值)`").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreprocessParams {
    /// Input image dimensions `(width, height)` for the detection model.
    pub image_size: (u32, u32),
    /// Per-channel mean for normalization. Channel order is determined by
    /// the model pipeline (PP-OCRv6 uses BGR; the Python baseline is the
    /// authority — see `docs/modules/08-models.md`, "`preprocess_params
    /// (v6 默认值)`").
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

/// Default `box_threshold` (PP-OCRv6 Small det; see the defaults table in
/// `docs/modules/08-models.md`).
pub const DEFAULT_BOX_THRESHOLD: f32 = 0.45;
/// Default `max_candidates` (PP-OCRv6 Small det; see the defaults table in
/// `docs/modules/08-models.md`).
pub const DEFAULT_MAX_CANDIDATES: usize = 3000;
/// Default `min_box_size` (see the defaults table in `docs/modules/08-models.md`).
pub const DEFAULT_MIN_BOX_SIZE: f32 = 3.0;
/// Default recognition input height (PP-OCRv6 Small rec; see the defaults
/// table in `docs/modules/08-models.md`).
pub const DEFAULT_REC_INPUT_HEIGHT: u32 = 48;
/// Default recognition input width (PP-OCRv6 Small rec; see the defaults
/// table in `docs/modules/08-models.md`).
pub const DEFAULT_REC_INPUT_WIDTH: u32 = 320;
/// Default `append_space` (PP-OCRv6 rec uses a space character; see the
/// defaults table in `docs/modules/08-models.md`).
pub const DEFAULT_REC_APPEND_SPACE: bool = true;
/// Default CTC blank index (see the defaults table in `docs/modules/08-models.md`).
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

/// Legacy inference parameters for the removed single-ONNX translation
/// model. Retained for API compatibility; the manifest v2 translation
/// group carries engine-specific parameters instead.
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
    /// Includes detection, recognition, and every translation engine file
    /// (models, vocabularies, `SentencePiece` models, configs). Optional
    /// entries (`rec_multi`, `translation`) are included only when present.
    #[must_use]
    pub fn all_entries(&self) -> Vec<&ModelEntry> {
        let mut entries = Vec::with_capacity(14);
        entries.push(&self.ocr.det);
        entries.push(&self.ocr.rec_ja);
        entries.push(&self.ocr.rec_en);
        if let Some(ref multi) = self.ocr.rec_multi {
            entries.push(multi);
        }
        if let Some(ref trans) = self.translation {
            let en_zh = &trans.engines.en_zh;
            entries.push(&en_zh.model);
            entries.push(&en_zh.src_vocab);
            entries.push(&en_zh.trg_vocab);
            entries.push(&en_zh.lexical_shortlist);
            let ja_zh = &trans.engines.ja_zh;
            entries.push(&ja_zh.model);
            entries.push(&ja_zh.config);
            entries.push(&ja_zh.source_vocabulary);
            entries.push(&ja_zh.target_vocabulary);
            entries.push(&ja_zh.source_spm);
            entries.push(&ja_zh.target_spm);
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
        "version": 2,
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
        "version": 2,
        "ocr": {
            "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
            "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
            "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
            "rec_multi": { "id": "rm", "path": "ocr/rec_multi.onnx", "sha256": "jkl", "size_bytes": 4 },
            "dicts": {},
            "preprocess_params": { "image_size": [960, 960], "mean": [0.485, 0.456, 0.406], "std": [0.229, 0.224, 0.225], "det_threshold": 0.2, "unclip_ratio": 1.4 }
        },
        "translation": {
            "target": "zh-Hans",
            "engines": {
                "en_zh": {
                    "engine": "bergamot",
                    "model": { "id": "enzh-model", "path": "translation/en-zh/model.enzh.intgemm.alphas.bin", "sha256": "mno", "size_bytes": 5 },
                    "src_vocab": { "id": "enzh-src-vocab", "path": "translation/en-zh/srcvocab.enzh.spm", "sha256": "pqr", "size_bytes": 6 },
                    "trg_vocab": { "id": "enzh-trg-vocab", "path": "translation/en-zh/trgvocab.enzh.spm", "sha256": "stu", "size_bytes": 7 },
                    "lexical_shortlist": { "id": "enzh-lex", "path": "translation/en-zh/lex.50.50.enzh.s2t.bin", "sha256": "vwx", "size_bytes": 8 },
                    "beam_size": 1,
                    "gemm_precision": "int8shiftAlphaAll"
                },
                "ja_zh": {
                    "engine": "ctranslate2",
                    "model": { "id": "jazh-model", "path": "translation/ja-zh/model.bin", "sha256": "yza", "size_bytes": 9 },
                    "config": { "id": "jazh-config", "path": "translation/ja-zh/config.json", "sha256": "bcd", "size_bytes": 10 },
                    "source_vocabulary": { "id": "jazh-src-vocab", "path": "translation/ja-zh/source_vocabulary.json", "sha256": "cde", "size_bytes": 11 },
                    "target_vocabulary": { "id": "jazh-trg-vocab", "path": "translation/ja-zh/target_vocabulary.json", "sha256": "def", "size_bytes": 12 },
                    "source_spm": { "id": "jazh-src-spm", "path": "translation/ja-zh/source.spm", "sha256": "efg", "size_bytes": 13 },
                    "target_spm": { "id": "jazh-trg-spm", "path": "translation/ja-zh/target.spm", "sha256": "fgh", "size_bytes": 14 },
                    "beam_size_fast": 1,
                    "beam_size_balanced": 4,
                    "max_input_tokens": 256
                }
            },
            "budget_mb": { "hard_mb": 200, "target_mb": 175, "en_zh_mb": 65, "ja_zh_mb": 110 },
            "metadata": {
                "model_revision": "abc123",
                "converted_with": "ctranslate2 4.8.1",
                "registry_generated": "2026-08-07T00:43:32Z"
            }
        }
    }"#;

    #[test]
    fn parse_valid_no_translation() {
        let manifest = ModelManifest::from_json_str(VALID_JSON_NO_TRANS).unwrap();
        assert_eq!(manifest.version, 2);
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
        assert_eq!(trans.target, "zh-Hans");
        assert_eq!(trans.budget_mb.hard_mb, 200);
        assert_eq!(trans.budget_mb.target_mb, 175);
        assert_eq!(trans.budget_mb.en_zh_mb, 65);
        assert_eq!(trans.budget_mb.ja_zh_mb, 110);

        let en_zh = &trans.engines.en_zh;
        assert_eq!(en_zh.engine, "bergamot");
        assert_eq!(en_zh.model.id, "enzh-model");
        assert_eq!(en_zh.src_vocab.id, "enzh-src-vocab");
        assert_eq!(en_zh.trg_vocab.id, "enzh-trg-vocab");
        assert_eq!(en_zh.lexical_shortlist.id, "enzh-lex");
        assert_eq!(en_zh.beam_size, 1);
        assert_eq!(en_zh.gemm_precision, "int8shiftAlphaAll");

        let ja_zh = &trans.engines.ja_zh;
        assert_eq!(ja_zh.engine, "ctranslate2");
        assert_eq!(ja_zh.model.id, "jazh-model");
        assert_eq!(ja_zh.config.id, "jazh-config");
        assert_eq!(ja_zh.source_vocabulary.id, "jazh-src-vocab");
        assert_eq!(ja_zh.target_vocabulary.id, "jazh-trg-vocab");
        assert_eq!(ja_zh.source_spm.id, "jazh-src-spm");
        assert_eq!(ja_zh.target_spm.id, "jazh-trg-spm");
        assert_eq!(ja_zh.beam_size_fast, 1);
        assert_eq!(ja_zh.beam_size_balanced, 4);
        assert_eq!(ja_zh.max_input_tokens, 256);

        assert_eq!(
            trans.metadata.get("model_revision").map(String::as_str),
            Some("abc123")
        );
        assert_eq!(
            trans.metadata.get("converted_with").map(String::as_str),
            Some("ctranslate2 4.8.1")
        );
        assert_eq!(
            trans.metadata.get("registry_generated").map(String::as_str),
            Some("2026-08-07T00:43:32Z")
        );
    }

    #[test]
    fn missing_required_field_returns_parse_error() {
        let json = r#"{ "version": 2 }"#;
        let result = ModelManifest::from_json_str(json);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ModelError::Parse(_)));
    }

    #[test]
    fn unsupported_future_version_returns_error() {
        let json = VALID_JSON_NO_TRANS.replace(r#""version": 2"#, r#""version": 99"#);
        let result = ModelManifest::from_json_str(&json);
        assert!(matches!(
            result.unwrap_err(),
            ModelError::UnsupportedVersion(99)
        ));
    }

    #[test]
    fn v1_manifest_is_rejected() {
        // Manifest v2 is a breaking upgrade (A4): the v1 translation group
        // (single ONNX model + tokenizer) is no longer supported. Even a
        // v1 manifest without a translation section must be rejected.
        let json = VALID_JSON_NO_TRANS.replace(r#""version": 2"#, r#""version": 1"#);
        let result = ModelManifest::from_json_str(&json);
        assert!(matches!(
            result.unwrap_err(),
            ModelError::UnsupportedVersion(1)
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
        // 4 OCR entries (incl. rec_multi) + 4 Bergamot + 6 CTranslate2 = 14.
        assert_eq!(entries.len(), 14);
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"rm"));
        assert!(ids.contains(&"enzh-model"));
        assert!(ids.contains(&"enzh-src-vocab"));
        assert!(ids.contains(&"enzh-trg-vocab"));
        assert!(ids.contains(&"enzh-lex"));
        assert!(ids.contains(&"jazh-model"));
        assert!(ids.contains(&"jazh-config"));
        assert!(ids.contains(&"jazh-src-vocab"));
        assert!(ids.contains(&"jazh-trg-vocab"));
        assert!(ids.contains(&"jazh-src-spm"));
        assert!(ids.contains(&"jazh-trg-spm"));
        assert_eq!(ids.len(), 14);
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
            "version": 2,
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
            "version": 2,
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
            "version": 2,
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
    fn translation_metadata_defaults_to_empty() {
        // `metadata` is optional; a manifest without it must deserialize
        // with an empty map, and serialization must omit it again.
        let json = r#"{
            "version": 2,
            "ocr": {
                "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
                "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
                "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
                "rec_multi": null,
                "dicts": {},
                "preprocess_params": { "image_size": [960, 960], "mean": [0.485, 0.456, 0.406], "std": [0.229, 0.224, 0.225], "det_threshold": 0.2, "unclip_ratio": 1.4 }
            },
            "translation": {
                "target": "zh-Hans",
                "engines": {
                    "en_zh": {
                        "engine": "bergamot",
                        "model": { "id": "enzh-model", "path": "translation/en-zh/model.enzh.intgemm.alphas.bin", "sha256": "mno", "size_bytes": 5 },
                        "src_vocab": { "id": "enzh-src-vocab", "path": "translation/en-zh/srcvocab.enzh.spm", "sha256": "pqr", "size_bytes": 6 },
                        "trg_vocab": { "id": "enzh-trg-vocab", "path": "translation/en-zh/trgvocab.enzh.spm", "sha256": "stu", "size_bytes": 7 },
                        "lexical_shortlist": { "id": "enzh-lex", "path": "translation/en-zh/lex.50.50.enzh.s2t.bin", "sha256": "vwx", "size_bytes": 8 },
                        "beam_size": 1,
                        "gemm_precision": "int8shiftAlphaAll"
                    },
                    "ja_zh": {
                        "engine": "ctranslate2",
                        "model": { "id": "jazh-model", "path": "translation/ja-zh/model.bin", "sha256": "yza", "size_bytes": 9 },
                        "config": { "id": "jazh-config", "path": "translation/ja-zh/config.json", "sha256": "bcd", "size_bytes": 10 },
                        "source_vocabulary": { "id": "jazh-src-vocab", "path": "translation/ja-zh/source_vocabulary.json", "sha256": "cde", "size_bytes": 11 },
                        "target_vocabulary": { "id": "jazh-trg-vocab", "path": "translation/ja-zh/target_vocabulary.json", "sha256": "def", "size_bytes": 12 },
                        "source_spm": { "id": "jazh-src-spm", "path": "translation/ja-zh/source.spm", "sha256": "efg", "size_bytes": 13 },
                        "target_spm": { "id": "jazh-trg-spm", "path": "translation/ja-zh/target.spm", "sha256": "fgh", "size_bytes": 14 },
                        "beam_size_fast": 1,
                        "beam_size_balanced": 4,
                        "max_input_tokens": 256
                    }
                },
                "budget_mb": { "hard_mb": 200, "target_mb": 175, "en_zh_mb": 65, "ja_zh_mb": 110 }
            }
        }"#;
        let manifest = ModelManifest::from_json_str(json).unwrap();
        let trans = manifest.translation.as_ref().unwrap();
        assert!(trans.metadata.is_empty());
        let serialized = serde_json::to_string(&manifest).unwrap();
        assert!(!serialized.contains("metadata"));
    }
}
