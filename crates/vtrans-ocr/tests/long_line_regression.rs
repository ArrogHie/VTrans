//! Regression tests for long-line recognition and the `auto` language policy.
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

/// The default confidence threshold from `OcrOptions` (frozen in core).
const DEFAULT_MIN_CONFIDENCE: f32 = 0.55;

#[tokio::test]
#[ignore = "requires local PP-OCR ONNX models"]
async fn long_english_lines_are_recognized_completely() {
    let image = load_fixture("test1_lines.png");
    let region = ScreenRegion::new("test", 0, 0, image.width, image.height);
    let options = OcrOptions {
        language: Language::English,
        min_confidence: DEFAULT_MIN_CONFIDENCE,
        detect_vertical: true,
    };
    let result = provider()
        .recognize(&image, &region, &options, CancellationToken::new())
        .await
        .unwrap();

    // Every kept line passes the default threshold...
    assert!(
        result
            .lines
            .iter()
            .all(|line| line.confidence >= DEFAULT_MIN_CONFIDENCE),
        "a line slipped below the confidence threshold"
    );
    // ...and the long bullet sentences survive. Before the long-line fix this
    // fixture yielded only a handful of short fragments; the wide boxes were
    // compressed to ~13px text height and dropped by the confidence filter.
    assert!(
        result.lines.len() >= 12,
        "expected close to the full text, got {} lines",
        result.lines.len()
    );
    for phrase in [
        "Huns serving in",
        "Varsha Deshpand",
        "Darwin",
        "Darrenkamp",
        "Wakidi sold a paint",
        "Barbie doll",
    ] {
        assert!(
            result.merged_text.contains(phrase),
            "long sentence fragment missing: {phrase}"
        );
    }
}
