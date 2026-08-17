//! End-to-end tests for the `vtrans-verify-models` CLI.
//!
//! These tests run the compiled binary against temporary models directories
//! and assert the exit code and output for the optional-entry semantics:
//! missing optional entries are reported as skipped (exit success), while
//! corrupted optional entries and missing required entries still fail.

use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};
use tempfile::TempDir;

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

/// Build a models directory with OCR files, a tokenizer, and a translation
/// model entry flagged `optional: true` with download metadata.
///
/// * `install_trans_model` — whether `translation/model.onnx` is written.
/// * `corrupt_trans_model` — write wrong bytes for the translation model.
/// * `remove_det` — remove the required detection model (must fail).
fn build_models_dir(
    install_trans_model: bool,
    corrupt_trans_model: bool,
    remove_det: bool,
) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let det_sha = write_file(dir.path(), "ocr/det.onnx", b"det");
    let rec_ja_sha = write_file(dir.path(), "ocr/rec_ja.onnx", b"rj");
    let rec_en_sha = write_file(dir.path(), "ocr/rec_en.onnx", b"re");
    let tokenizer_sha = write_file(dir.path(), "translation/tokenizer.json", b"{}");

    // Expected SHA-256 of the properly installed translation model.
    let mut hasher = Sha256::new();
    hasher.update(b"trans model");
    let trans_sha = format!("{:x}", hasher.finalize());
    if corrupt_trans_model {
        write_file(dir.path(), "translation/model.onnx", b"corrupted");
    } else if install_trans_model {
        write_file(dir.path(), "translation/model.onnx", b"trans model");
    }
    if remove_det {
        std::fs::remove_file(dir.path().join("ocr/det.onnx")).unwrap();
    }

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
  "translation": {{
    "model": {{ "id": "tm", "path": "translation/model.onnx", "sha256": "{trans_sha}", "size_bytes": 11, "optional": true, "download_url": "https://example.com/translation-model.onnx", "download_size_bytes": 11 }},
    "tokenizer": {{ "id": "tk", "path": "translation/tokenizer.json", "sha256": "{tokenizer_sha}", "size_bytes": 2 }},
    "supported_pairs": [["en", "zh-CN"]],
    "max_length": 512,
    "inference_params": {{ "max_batch_size": 1, "num_beams": 4 }}
  }}
}}"#
    );
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
    dir
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vtrans-verify-models"))
        .args(args)
        .output()
        .expect("failed to spawn vtrans-verify-models")
}

#[test]
fn cli_optional_missing_exits_success_and_reports_skipped() {
    let dir = build_models_dir(false, false, false);
    let output = run_cli(&["--models", dir.path().to_str().unwrap()]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("skipped: tm"), "stdout was: {stdout}");
    assert!(stdout.contains("1 optional entries not installed"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("failed:"), "stderr was: {stderr}");
}

#[test]
fn cli_optional_corrupted_exits_failure() {
    let dir = build_models_dir(false, true, false);
    let output = run_cli(&["--models", dir.path().to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("sha256 mismatch"), "stderr was: {stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("skipped:"), "stdout was: {stdout}");
}

#[test]
fn cli_required_missing_exits_failure() {
    let dir = build_models_dir(true, false, true);
    let output = run_cli(&["--models", dir.path().to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("model file not found"),
        "stderr was: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("skipped:"), "stdout was: {stdout}");
}

#[test]
fn cli_honors_vtrans_model_dir_env_var() {
    // The `VTRANS_MODEL_DIR` environment variable must keep working as the
    // models directory source when `--models` is not passed.
    let dir = build_models_dir(false, false, false);
    let output = Command::new(env!("CARGO_BIN_EXE_vtrans-verify-models"))
        .env("VTRANS_MODEL_DIR", dir.path())
        .output()
        .expect("failed to spawn vtrans-verify-models");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("skipped: tm"), "stdout was: {stdout}");
}
