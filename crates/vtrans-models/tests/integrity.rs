//! Integration tests for batch model integrity verification (manifest v2).
//!
//! These tests exercise the full `ModelManager` pipeline against a
//! temporary models directory with fake model files: writing a v2 manifest
//! (OCR + dual-engine translation), verifying that `verify_integrity`
//! aggregates results across all entries, and resolving the per-engine
//! absolute paths consumed by `vtrans-translation`.

use std::collections::HashMap;
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

/// Slot name → relative path for the ten translation engine files.
const TRANSLATION_FILES: [(&str, &str); 10] = [
    (
        "enzh_model",
        "translation/en-zh/model.enzh.intgemm.alphas.bin",
    ),
    ("enzh_src_vocab", "translation/en-zh/srcvocab.enzh.spm"),
    ("enzh_trg_vocab", "translation/en-zh/trgvocab.enzh.spm"),
    ("enzh_lex", "translation/en-zh/lex.50.50.enzh.s2t.bin"),
    ("jazh_model", "translation/ja-zh/model.bin"),
    ("jazh_config", "translation/ja-zh/config.json"),
    ("jazh_src_vocab", "translation/ja-zh/source_vocabulary.json"),
    ("jazh_trg_vocab", "translation/ja-zh/target_vocabulary.json"),
    ("jazh_src_spm", "translation/ja-zh/source.spm"),
    ("jazh_trg_spm", "translation/ja-zh/target.spm"),
];

/// Render the translation section of a v2 manifest from slot → sha256.
fn translation_json(hashes: &HashMap<&str, String>) -> String {
    format!(
        r#"{{
    "target": "zh-Hans",
    "engines": {{
      "en_zh": {{
        "engine": "bergamot",
        "model": {{ "id": "enzh-model", "path": "translation/en-zh/model.enzh.intgemm.alphas.bin", "sha256": "{0}", "size_bytes": 1 }},
        "src_vocab": {{ "id": "enzh-src-vocab", "path": "translation/en-zh/srcvocab.enzh.spm", "sha256": "{1}", "size_bytes": 2 }},
        "trg_vocab": {{ "id": "enzh-trg-vocab", "path": "translation/en-zh/trgvocab.enzh.spm", "sha256": "{2}", "size_bytes": 3 }},
        "lexical_shortlist": {{ "id": "enzh-lex", "path": "translation/en-zh/lex.50.50.enzh.s2t.bin", "sha256": "{3}", "size_bytes": 4 }},
        "beam_size": 1,
        "gemm_precision": "int8shiftAlphaAll"
      }},
      "ja_zh": {{
        "engine": "ctranslate2",
        "model": {{ "id": "jazh-model", "path": "translation/ja-zh/model.bin", "sha256": "{4}", "size_bytes": 5 }},
        "config": {{ "id": "jazh-config", "path": "translation/ja-zh/config.json", "sha256": "{5}", "size_bytes": 6 }},
        "source_vocabulary": {{ "id": "jazh-src-vocab", "path": "translation/ja-zh/source_vocabulary.json", "sha256": "{6}", "size_bytes": 7 }},
        "target_vocabulary": {{ "id": "jazh-trg-vocab", "path": "translation/ja-zh/target_vocabulary.json", "sha256": "{7}", "size_bytes": 8 }},
        "source_spm": {{ "id": "jazh-src-spm", "path": "translation/ja-zh/source.spm", "sha256": "{8}", "size_bytes": 9 }},
        "target_spm": {{ "id": "jazh-trg-spm", "path": "translation/ja-zh/target.spm", "sha256": "{9}", "size_bytes": 10 }},
        "beam_size_fast": 1,
        "beam_size_balanced": 4,
        "max_input_tokens": 256
      }}
    }},
    "budget_mb": {{ "hard_mb": 200, "target_mb": 175, "en_zh_mb": 65, "ja_zh_mb": 110 }}
  }}"#,
        hashes["enzh_model"],
        hashes["enzh_src_vocab"],
        hashes["enzh_trg_vocab"],
        hashes["enzh_lex"],
        hashes["jazh_model"],
        hashes["jazh_config"],
        hashes["jazh_src_vocab"],
        hashes["jazh_trg_vocab"],
        hashes["jazh_src_spm"],
        hashes["jazh_trg_spm"],
    )
}

/// Build a complete models directory with OCR + dual-engine translation.
///
/// Returns the temp dir together with the hashes of the ten translation
/// files (slot name → sha256) so tests can corrupt specific entries.
fn build_models_dir() -> (TempDir, HashMap<&'static str, String>) {
    let dir = tempfile::tempdir().unwrap();
    let det_sha = write_file(dir.path(), "ocr/det.onnx", b"detection model bytes");
    let rec_ja_sha = write_file(dir.path(), "ocr/rec_ja.onnx", b"japanese rec model");
    let rec_en_sha = write_file(dir.path(), "ocr/rec_en.onnx", b"english rec model");
    write_file(dir.path(), "ocr/dict_ja.txt", b"ja\ndict\n");
    write_file(dir.path(), "ocr/dict_en.txt", b"en\ndict\n");

    let mut trans = HashMap::new();
    for (slot, path) in TRANSLATION_FILES {
        let sha = write_file(dir.path(), path, format!("{slot} bytes").as_bytes());
        trans.insert(slot, sha);
    }

    let manifest = format!(
        r#"{{
  "version": 2,
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
  "translation": {}
}}"#,
        translation_json(&trans),
    );
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
    (dir, trans)
}

/// Total checked files: 3 OCR entries + 10 translation entries + 2 dicts.
const TOTAL_CHECKED: usize = 15;

#[test]
fn batch_verify_all_pass() {
    let (dir, _) = build_models_dir();
    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let report = manager.verify_integrity().unwrap();

    assert_eq!(report.checked, TOTAL_CHECKED);
    assert_eq!(report.passed, TOTAL_CHECKED);
    assert!(report.failed.is_empty());
    assert!(report.is_ok());
}

#[test]
fn batch_verify_one_hash_mismatch() {
    let (dir, _) = build_models_dir();
    // Corrupt an OCR model file.
    std::fs::write(dir.path().join("ocr/rec_en.onnx"), b"corrupted content").unwrap();

    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let report = manager.verify_integrity().unwrap();

    assert_eq!(report.checked, TOTAL_CHECKED);
    assert_eq!(report.passed, TOTAL_CHECKED - 1);
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0].contains("sha256 mismatch"));
    assert!(!report.is_ok());
}

#[test]
fn batch_verify_translation_hash_mismatch() {
    let (dir, _) = build_models_dir();
    // Corrupt a ja→zh engine file; the failure must name its entry id.
    std::fs::write(dir.path().join("translation/ja-zh/config.json"), b"{}").unwrap();

    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let report = manager.verify_integrity().unwrap();

    assert_eq!(report.checked, TOTAL_CHECKED);
    assert_eq!(report.passed, TOTAL_CHECKED - 1);
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0].contains("sha256 mismatch"));
    assert!(report.failed[0].contains("jazh-config"));
}

#[test]
fn batch_verify_missing_file_and_dict() {
    let (dir, _) = build_models_dir();
    // Remove a translation model file and a dict file.
    std::fs::remove_file(dir.path().join("translation/ja-zh/model.bin")).unwrap();
    std::fs::remove_file(dir.path().join("ocr/dict_ja.txt")).unwrap();

    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let report = manager.verify_integrity().unwrap();

    assert_eq!(report.checked, TOTAL_CHECKED);
    assert_eq!(report.passed, TOTAL_CHECKED - 2);
    assert_eq!(report.failed.len(), 2);
    let combined = report.failed.join("; ");
    assert!(combined.contains("model file not found"));
    assert!(combined.contains("ja-zh"));
    assert!(combined.contains("dict file not found"));
}

#[test]
fn batch_verify_multiple_failures() {
    let (dir, _) = build_models_dir();
    // Corrupt two files and delete one dict.
    std::fs::write(dir.path().join("ocr/det.onnx"), b"wrong").unwrap();
    std::fs::write(
        dir.path().join("translation/en-zh/srcvocab.enzh.spm"),
        b"also wrong",
    )
    .unwrap();
    std::fs::remove_file(dir.path().join("ocr/dict_en.txt")).unwrap();

    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let report = manager.verify_integrity().unwrap();

    assert_eq!(report.checked, TOTAL_CHECKED);
    assert_eq!(report.passed, TOTAL_CHECKED - 3);
    assert_eq!(report.failed.len(), 3);
}

#[test]
fn manifest_not_found_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let result = ModelManager::from_manifest_dir(dir.path());
    assert!(matches!(result, Err(ModelError::ManifestNotFound(_))));
}

#[test]
fn v1_manifest_rejected_with_unsupported_version() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("manifest.json"),
        r#"{
  "version": 1,
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
    assert!(matches!(result, Err(ModelError::UnsupportedVersion(1))));
}

#[test]
fn model_path_resolution() {
    let (dir, _) = build_models_dir();
    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();

    let det_entry = &manager.manifest().ocr.det;
    let det_path = manager.model_path(det_entry);
    assert!(det_path.ends_with("ocr/det.onnx"));

    let en_zh_paths = manager.en_zh_paths().expect("translation configured");
    assert!(en_zh_paths
        .model
        .ends_with("translation/en-zh/model.enzh.intgemm.alphas.bin"));
    assert!(en_zh_paths
        .lexical_shortlist
        .ends_with("lex.50.50.enzh.s2t.bin"));

    let ja_zh_paths = manager.ja_zh_paths().expect("translation configured");
    assert!(ja_zh_paths.model.ends_with("translation/ja-zh/model.bin"));
    assert!(ja_zh_paths
        .target_spm
        .ends_with("translation/ja-zh/target.spm"));
}

#[test]
fn manifest_with_no_translation() {
    let dir = tempfile::tempdir().unwrap();
    let det_sha = write_file(dir.path(), "ocr/det.onnx", b"det");
    let rec_ja_sha = write_file(dir.path(), "ocr/rec_ja.onnx", b"rj");
    let rec_en_sha = write_file(dir.path(), "ocr/rec_en.onnx", b"re");

    let manifest = format!(
        r#"{{
  "version": 2,
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
    assert!(manager.en_zh_paths().is_none());
    assert!(manager.ja_zh_paths().is_none());
    let report = manager.verify_integrity().unwrap();
    assert_eq!(report.checked, 3);
    assert_eq!(report.passed, 3);
    assert!(report.is_ok());
}

#[test]
fn legacy_v4_ocr_manifest_still_deserializes_with_v6_defaults() {
    // The OCR group is backward compatible across v1 → v2: a v4-era OCR
    // block (no det/rec extension fields) must still load, with the new
    // PreprocessParams fields falling back to the PP-OCRv6 defaults.
    let dir = tempfile::tempdir().unwrap();
    let det_sha = write_file(dir.path(), "ocr/det.onnx", b"det");
    let rec_ja_sha = write_file(dir.path(), "ocr/rec_ja.onnx", b"rj");
    let rec_en_sha = write_file(dir.path(), "ocr/rec_en.onnx", b"re");

    let manifest = format!(
        r#"{{"version": 2,
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

    let mut trans = HashMap::new();
    for (slot, path) in TRANSLATION_FILES {
        let sha = write_file(dir.path(), path, format!("{slot} bytes").as_bytes());
        trans.insert(slot, sha);
    }

    let manifest = format!(
        r#"{{
  "version": 2,
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
  "translation": {}
}}"#,
        translation_json(&trans),
    );
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();

    let manager = ModelManager::from_manifest_dir(dir.path()).unwrap();
    let ocr = &manager.manifest().ocr;
    assert_eq!(ocr.rec_ja.path, ocr.rec_en.path);
    assert_eq!(ocr.rec_en.path, ocr.rec_multi.as_ref().unwrap().path);

    let report = manager.verify_integrity().unwrap();
    // 4 OCR entries (det + 3 rec) + 10 translation entries + 3 dicts.
    assert_eq!(report.checked, 17);
    assert_eq!(report.passed, 17);
    assert!(report.failed.is_empty());
    assert!(report.is_ok());
}
