//! Regression tests for long-line recognition and the `auto` language policy
//! with the PP-OCRv6 models.
//!
//! These tests require locally downloaded PP-OCR models (see
//! `scripts/download_models.ps1`) and the model directory referenced by the
//! repository `manifest.json`. They are ignored by default; run them with:
//!
//! ```text
//! cargo test -p vtrans-ocr --test long_line_regression -- --ignored
//! ```

use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use vtrans_core::error::OcrError;
use vtrans_core::types::{CapturedImage, Language, OcrOptions, PixelFormat, ScreenRegion};
use vtrans_core::OcrProvider;
use vtrans_models::ModelManager;
use vtrans_ocr::PaddleOcrProvider;

/// Path to the locally downloaded model directory.
fn models_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources/models")
}

/// Load a fixture PNG as a captured image.
fn load_fixture(name: &str) -> CapturedImage {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let rgba = image::open(&path).unwrap().to_rgba8();
    let (width, height) = rgba.dimensions();
    CapturedImage::new(width, height, PixelFormat::Rgba8, rgba.into_raw()).unwrap()
}

/// Build a provider from the local models.
fn provider() -> PaddleOcrProvider {
    let manager = ModelManager::from_manifest_dir(&models_dir()).unwrap();
    PaddleOcrProvider::from_manager(&manager).unwrap()
}

#[tokio::test]
#[ignore = "requires local PP-OCR ONNX models"]
async fn long_english_lines_are_recognized_completely() {
    let image = load_fixture("test1_lines.png");
    let region = ScreenRegion::new("test", 0, 0, image.width, image.height);
    let options = OcrOptions::new(Language::English);
    let result = provider()
        .recognize(&image, &region, &options, CancellationToken::new())
        .await
        .unwrap();

    // The long bullet sentences survive completely. PP-OCRv6 rec uses the
    // model's dynamic input width, so no chunk-seam artifacts appear and no
    // long line is compressed away.
    assert!(
        result.lines.len() >= 12,
        "expected close to the full text, got {} lines",
        result.lines.len()
    );
    for phrase in [
        "Huns serving in",
        "Varsha Deshpande",
        "Darwin",
        "Darrenkamp",
        "Wakidi sold a painting",
        "Barbie doll",
        "Noble Sproat Heaney",
        "Tricamarum",
    ] {
        assert!(
            result.merged_text.contains(phrase),
            "long sentence fragment missing: {phrase}"
        );
    }
}

#[tokio::test]
#[ignore = "requires local PP-OCR ONNX models"]
async fn auto_language_uses_multi_model_and_reads_chinese() {
    let image = load_fixture("test1_zh.png");
    let region = ScreenRegion::new("test", 0, 0, image.width, image.height);
    let options = OcrOptions::new(Language::Auto);
    let result = provider()
        .recognize(&image, &region, &options, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(result.lines.len(), 4, "all four Chinese lines detected");
    for phrase in [
        "欢迎使用 VTrans 屏幕翻译工具",
        "这是一段用于",
        "识别验证的中文文本",
        "模型升级后自动检测与简体中文识别均已解锁",
        "数字与标点",
        "测试通过",
    ] {
        assert!(
            result.merged_text.contains(phrase),
            "Chinese phrase missing: {phrase}"
        );
    }
}

#[tokio::test]
#[ignore = "requires local PP-OCR ONNX models"]
async fn chinese_simplified_language_reads_chinese() {
    let image = load_fixture("test1_zh.png");
    let region = ScreenRegion::new("test", 0, 0, image.width, image.height);
    let options = OcrOptions::new(Language::ChineseSimplified);
    let result = provider()
        .recognize(&image, &region, &options, CancellationToken::new())
        .await
        .unwrap();

    assert!(
        result.merged_text.contains("欢迎使用 VTrans 屏幕翻译工具"),
        "zh-CN recognition failed: {}",
        result.merged_text
    );
}

#[tokio::test]
#[ignore = "requires local PP-OCR ONNX models"]
async fn vertical_text_does_not_crash() {
    let image = load_fixture("test1_vertical.png");
    let region = ScreenRegion::new("test", 0, 0, image.width, image.height);
    let options = OcrOptions::new(Language::ChineseSimplified);
    // Vertical-line quality is not part of the PP-OCRv6 acceptance bar
    // (registered as a known limitation); the pipeline must only complete
    // without panicking or erroring.
    let result = provider()
        .recognize(&image, &region, &options, CancellationToken::new())
        .await
        .unwrap();
    assert!(result.lines.is_empty() || !result.merged_text.is_empty());
}

#[tokio::test]
#[ignore = "requires local PP-OCR ONNX models"]
async fn auto_without_multi_model_returns_actionable_error() {
    let image = load_fixture("test1_lines.png");
    let region = ScreenRegion::new("test", 0, 0, image.width, image.height);
    // Simulate a manifest without a multi-language model by pointing the
    // provider at a manifest where `rec_multi` is absent.
    let manager = ModelManager::from_manifest_dir(&models_dir()).unwrap();
    let manifest = manager.manifest().clone();
    let mut manifest = manifest;
    manifest.ocr.rec_multi = None;
    let provider_without_multi =
        PaddleOcrProvider::from_manifest_dir(&manifest, &models_dir()).unwrap();

    let options = OcrOptions::new(Language::Auto);
    let error = provider_without_multi
        .recognize(&image, &region, &options, CancellationToken::new())
        .await
        .unwrap_err();

    match &error {
        OcrError::Inference(message) => {
            assert!(message.contains("multi-language"));
            assert!(message.contains("select a language manually"));
        }
        other => panic!("expected OcrError::Inference, got {other:?}"),
    }
}
