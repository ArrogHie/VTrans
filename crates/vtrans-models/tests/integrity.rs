//! Integration tests for batch model integrity verification.
//!
//! These tests exercise the full `ModelManager` pipeline: creating a
//! temporary models directory with fake model files, writing a manifest,
//! and verifying that `verify_integrity` correctly aggregates results
//! across multiple files.

use std::path::Path;

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use vtrans_models::{ModelError, ModelManager};

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

/// Build a complete models directory with OCR + translation files.
fn build_models_dir() -> (TempDir, String, String, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let det_sha = write_file(dir.path(), "ocr/det.onnx", b"detection model bytes");
    let rec_ja_sha = write_file(dir.path(), "ocr/rec_ja.onnx", b"japanese rec model");
    let rec_en_sha = write_file(dir.path(), "ocr/rec_en.onnx", b"english rec model");
    write_file(dir.path(), "ocr/dict_ja.txt", b"ja\ndict\n");
    write_file(dir.path(), "ocr/dict_en.txt", b"en\ndict\n");
    let trans_model_sha = write_file(dir.path(), "translation/model.onnx", b"translation model");
    let tokenizer_sha = write_file(
        dir.path(),
        "translation/tokenizer.json",
        b"{\"tokenizer\": true}",
    );

    let manifest = format!(
        r#"{{
  "version": 1,
  "ocr": {{
    "det": {{ "id": "det", "path": "ocr/det.onnx", "sha256": "{det_sha}", "size_bytes": 20 }},
    "rec_ja": {{ "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "{rec_ja_sha}", "size_bytes": 18 }},
    "rec_en": {{ "id": "re", "path": "ocr/rec_en.onnx", "sha256": "{rec_en_sha}", "size_bytes": 18 }},
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
  "translation": {{
    "model": {{ "id": "tm", "path": "translation/model.onnx", "sha256": "{trans_model_sha}", "size_bytes": 17 }},
    "tokenizer": {{ "id": "tk", "path": "translation/tokenizer.json", "sha256": "{tokenizer_sha}", "size_bytes": 18 }},
    "supported_pairs": [["en", "zh-CN"]],
    "max_length": 512,
    "inference_params": {{ "max_batch_size": 1, "num_beams": 4 }}
  }}
}}"#
    );
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
    (
        dir,
        det_sha,
        rec_ja_sha,
        rec_en_sha,
        trans_model_sha,
        tokenizer_sha,
    )
}

#[test]
fn batch_verify_all_pass() {
    let (dir, _, _, _, _, _) = build_models_dir();
    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let report = manager.verify_integrity().unwrap();

    // 5 model entries (det, rec_ja, rec_en, translation model, tokenizer)
    // + 2 dict files = 7 total checked.
    assert_eq!(report.checked, 7);
    assert_eq!(report.passed, 7);
    assert!(report.failed.is_empty());
    assert!(report.is_ok());
}

#[test]
fn batch_verify_one_hash_mismatch() {
    let (dir, _, _, _, _, _) = build_models_dir();
    // Corrupt one model file.
    std::fs::write(dir.path().join("ocr/rec_en.onnx"), b"corrupted content").unwrap();

    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let report = manager.verify_integrity().unwrap();

    assert_eq!(report.checked, 7);
    assert_eq!(report.passed, 6);
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0].contains("sha256 mismatch"));
    assert!(!report.is_ok());
}

#[test]
fn batch_verify_missing_file_and_dict() {
    let (dir, _, _, _, _, _) = build_models_dir();
    // Remove a model file and a dict file.
    std::fs::remove_file(dir.path().join("translation/model.onnx")).unwrap();
    std::fs::remove_file(dir.path().join("ocr/dict_ja.txt")).unwrap();

    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let report = manager.verify_integrity().unwrap();

    assert_eq!(report.checked, 7);
    assert_eq!(report.passed, 5);
    assert_eq!(report.failed.len(), 2);
    let combined = report.failed.join("; ");
    assert!(combined.contains("model file not found"));
    assert!(combined.contains("dict file not found"));
}

#[test]
fn batch_verify_multiple_failures() {
    let (dir, _, _, _, _, _) = build_models_dir();
    // Corrupt two files and delete one dict.
    std::fs::write(dir.path().join("ocr/det.onnx"), b"wrong").unwrap();
    std::fs::write(dir.path().join("ocr/rec_ja.onnx"), b"also wrong").unwrap();
    std::fs::remove_file(dir.path().join("ocr/dict_en.txt")).unwrap();

    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let report = manager.verify_integrity().unwrap();

    assert_eq!(report.checked, 7);
    assert_eq!(report.passed, 4);
    assert_eq!(report.failed.len(), 3);
}

#[test]
fn manifest_not_found_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let result = ModelManager::from_manifest_dir(dir.path());
    assert!(matches!(result, Err(ModelError::ManifestNotFound(_))));
}

#[test]
fn model_path_resolution() {
    let (dir, _, _, _, _, _) = build_models_dir();
    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();

    let det_entry = &manager.manifest().ocr.det;
    let det_path = manager.model_path(det_entry);
    assert!(det_path.ends_with("ocr/det.onnx"));

    let trans = manager.manifest().translation.as_ref().unwrap();
    let tk_path = manager.model_path(&trans.tokenizer);
    assert!(tk_path.ends_with("translation/tokenizer.json"));
}

#[test]
fn manifest_with_no_translation() {
    let dir = tempfile::tempdir().unwrap();
    let det_sha = write_file(dir.path(), "ocr/det.onnx", b"det");
    let rec_ja_sha = write_file(dir.path(), "ocr/rec_ja.onnx", b"rj");
    let rec_en_sha = write_file(dir.path(), "ocr/rec_en.onnx", b"re");

    let manifest = format!(
        r#"{{
  "version": 1,
  "ocr": {{
    "det": {{ "id": "det", "path": "ocr/det.onnx", "sha256": "{det_sha}", "size_bytes": 3 }},
    "rec_ja": {{ "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "{rec_ja_sha}", "size_bytes": 2 }},
    "rec_en": {{ "id": "re", "path": "ocr/rec_en.onnx", "sha256": "{rec_en_sha}", "size_bytes": 2 }},
    "rec_multi": null,
    "dicts": {{}},
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
    );
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();

    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    assert!(manager.manifest().translation.is_none());
    let report = manager.verify_integrity().unwrap();
    assert_eq!(report.checked, 3);
    assert_eq!(report.passed, 3);
    assert!(report.is_ok());
}

#[test]
fn legacy_v4_manifest_still_deserializes_with_v6_defaults() {
    // The schema is backward compatible: a v4-era manifest (no det/rec
    // extension fields) must still load, and the new PreprocessParams
    // fields must fall back to the PP-OCRv6 defaults.
    let dir = tempfile::tempdir().unwrap();
    let det_sha = write_file(dir.path(), "ocr/det.onnx", b"det");
    let rec_ja_sha = write_file(dir.path(), "ocr/rec_ja.onnx", b"rj");
    let rec_en_sha = write_file(dir.path(), "ocr/rec_en.onnx", b"re");

    let manifest = format!(
        r#"{{"version": 1,
  "ocr": {{
    "det": {{ "id": "det", "path": "ocr/det.onnx", "sha256": "{det_sha}", "size_bytes": 3 }},
    "rec_ja": {{ "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "{rec_ja_sha}", "size_bytes": 2 }},
    "rec_en": {{ "id": "re", "path": "ocr/rec_en.onnx", "sha256": "{rec_en_sha}", "size_bytes": 2 }},
    "rec_multi": null,
    "dicts": {{}},
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
    );
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();

    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let pp = &manager.manifest().ocr.preprocess_params;
    assert!((pp.det_threshold - 0.3).abs() < f32::EPSILON);
    assert!((pp.unclip_ratio - 2.0).abs() < f32::EPSILON);
    assert!((pp.box_threshold - 0.45).abs() < f32::EPSILON);
    assert_eq!(pp.max_candidates, 3000);
    assert!((pp.min_box_size - 3.0).abs() < f32::EPSILON);
    assert_eq!(pp.rec_input_height, 48);
    assert_eq!(pp.rec_input_width, 320);
    assert!(pp.rec_append_space);
    assert_eq!(pp.rec_blank_index, 0);

    let report = manager.verify_integrity().unwrap();
    assert!(report.is_ok());
}

#[test]
fn shared_rec_path_verifies_with_single_file() {
    // rec_ja / rec_en / rec_multi share one physical file (ocr/rec.onnx).
    // Verification must pass, and the report must count each manifest
    // entry (and each dict) rather than each unique file.
    let dir = tempfile::tempdir().unwrap();
    let det_sha = write_file(dir.path(), "ocr/det.onnx", b"det");
    let rec_sha = write_file(dir.path(), "ocr/rec.onnx", b"shared rec model");
    write_file(dir.path(), "ocr/ppocrv6_dict.txt", b"dict\n");
    let trans_model_sha = write_file(dir.path(), "translation/model.onnx", b"translation model");
    let tokenizer_sha = write_file(dir.path(), "translation/tokenizer.json", b"{}");

    let manifest = format!(
        r#"{{
  "version": 1,
  "ocr": {{
    "det": {{ "id": "det", "path": "ocr/det.onnx", "sha256": "{det_sha}", "size_bytes": 3 }},
    "rec_ja": {{ "id": "ppocr-rec-v6", "path": "ocr/rec.onnx", "sha256": "{rec_sha}", "size_bytes": 16 }},
    "rec_en": {{ "id": "ppocr-rec-v6-en", "path": "ocr/rec.onnx", "sha256": "{rec_sha}", "size_bytes": 16 }},
    "rec_multi": {{ "id": "ppocr-rec-v6-multi", "path": "ocr/rec.onnx", "sha256": "{rec_sha}", "size_bytes": 16 }},
    "dicts": {{ "ja": "ocr/ppocrv6_dict.txt", "en": "ocr/ppocrv6_dict.txt", "auto": "ocr/ppocrv6_dict.txt" }},
    "preprocess_params": {{
      "image_size": [640, 640],
      "mean": [0.485, 0.456, 0.406],
      "std": [0.229, 0.224, 0.225],
      "det_threshold": 0.2,
      "unclip_ratio": 1.4
    }}
  }},
  "translation": {{
    "model": {{ "id": "tm", "path": "translation/model.onnx", "sha256": "{trans_model_sha}", "size_bytes": 17 }},
    "tokenizer": {{ "id": "tk", "path": "translation/tokenizer.json", "sha256": "{tokenizer_sha}", "size_bytes": 2 }},
    "supported_pairs": [["en", "zh-CN"]],
    "max_length": 512,
    "inference_params": {{ "max_batch_size": 1, "num_beams": 4 }}
  }}
}}"#
    );
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();

    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let ocr = &manager.manifest().ocr;
    assert_eq!(ocr.rec_ja.path, ocr.rec_en.path);
    assert_eq!(ocr.rec_en.path, ocr.rec_multi.as_ref().unwrap().path);

    let report = manager.verify_integrity().unwrap();
    // 6 model entries (det + 3 rec + translation model + tokenizer) + 3 dicts.
    assert_eq!(report.checked, 9);
    assert_eq!(report.passed, 9);
    assert!(report.failed.is_empty());
    assert!(report.is_ok());
}
