//! Integration tests for provider construction from a model manifest.

use std::fs;
use std::path::Path;

use tempfile::tempdir;

use vtrans_core::error::OcrError;
use vtrans_models::ModelManager;
use vtrans_ocr::PaddleOcrProvider;

const PREPROCESS_JSON: &str = r#"
    "preprocess_params": {
      "image_size": [640, 640],
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
"#;

/// Write a minimal manifest with the given dictionary JSON.
fn write_manifest(dir: &Path, dicts: &str) {
    let json = format!(
        r#"{{
  "version": 1,
  "ocr": {{
    "det": {{ "id": "det", "path": "ocr/det.onnx", "sha256": "000", "size_bytes": 1 }},
    "rec_ja": {{ "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "000", "size_bytes": 1 }},
    "rec_en": {{ "id": "re", "path": "ocr/rec_en.onnx", "sha256": "000", "size_bytes": 1 }},
    "rec_multi": null,
    "dicts": {dicts},
    {PREPROCESS_JSON}
  }},
  "translation": null
}}"#
    );
    fs::write(dir.join("manifest.json"), json).unwrap();
}

fn write_model_files(dir: &Path) {
    fs::create_dir_all(dir.join("ocr")).unwrap();
    fs::write(dir.join("ocr/det.onnx"), b"not an onnx model").unwrap();
    fs::write(dir.join("ocr/rec_ja.onnx"), b"not an onnx model").unwrap();
    fs::write(dir.join("ocr/rec_en.onnx"), b"not an onnx model").unwrap();
}

#[test]
fn missing_dictionary_returns_invalid_manifest() {
    let dir = tempdir().unwrap();
    write_model_files(dir.path());
    write_manifest(dir.path(), "{}");

    let manifest = ModelManager::from_manifest_dir(dir.path())
        .unwrap()
        .manifest()
        .clone();
    let result = PaddleOcrProvider::from_manifest_dir(&manifest, dir.path());
    assert!(matches!(result, Err(OcrError::InvalidManifest(_))));
}

#[test]
fn missing_model_file_returns_model_load() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("ocr")).unwrap();
    fs::write(dir.path().join("ocr/dict_ja.txt"), "a\nb\n").unwrap();
    fs::write(dir.path().join("ocr/dict_en.txt"), "a\nb\n").unwrap();
    write_manifest(
        dir.path(),
        r#"{ "ja": "ocr/dict_ja.txt", "en": "ocr/dict_en.txt" }"#,
    );

    let manifest = ModelManager::from_manifest_dir(dir.path())
        .unwrap()
        .manifest()
        .clone();
    let result = PaddleOcrProvider::from_manifest_dir(&manifest, dir.path());
    assert!(matches!(result, Err(OcrError::ModelLoad(_))));
}

#[test]
fn corrupt_model_returns_model_load() {
    let dir = tempdir().unwrap();
    write_model_files(dir.path());
    fs::write(dir.path().join("ocr/dict_ja.txt"), "a\nb\n").unwrap();
    fs::write(dir.path().join("ocr/dict_en.txt"), "a\nb\n").unwrap();
    write_manifest(
        dir.path(),
        r#"{ "ja": "ocr/dict_ja.txt", "en": "ocr/dict_en.txt" }"#,
    );

    let manifest = ModelManager::from_manifest_dir(dir.path())
        .unwrap()
        .manifest()
        .clone();
    let result = PaddleOcrProvider::from_manifest_dir(&manifest, dir.path());
    assert!(matches!(result, Err(OcrError::ModelLoad(_))));
}

#[test]
fn from_manager_reports_missing_dictionary() {
    let dir = tempdir().unwrap();
    write_manifest(
        dir.path(),
        r#"{ "ja": "ocr/dict_ja.txt", "en": "ocr/dict_en.txt" }"#,
    );
    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let result = PaddleOcrProvider::from_manager(&manager);
    assert!(matches!(result, Err(OcrError::InvalidManifest(_))));
}

#[test]
fn from_manager_requires_ocr_model_files() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("ocr")).unwrap();
    fs::write(dir.path().join("ocr/dict_ja.txt"), "a\nb\n").unwrap();
    fs::write(dir.path().join("ocr/dict_en.txt"), "a\nb\n").unwrap();
    write_manifest(
        dir.path(),
        r#"{ "ja": "ocr/dict_ja.txt", "en": "ocr/dict_en.txt" }"#,
    );
    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let result = PaddleOcrProvider::from_manager(&manager);
    assert!(matches!(result, Err(OcrError::ModelLoad(_))));
}
