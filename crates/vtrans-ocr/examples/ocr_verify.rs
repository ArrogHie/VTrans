//! Verification CLI for the PP-OCR ONNX provider.
//!
//! Usage:
//!
//! ```text
//! cargo run --example ocr_verify -- \
//!     --models src-tauri/resources/models \
//!     --image path/to/image.png \
//!     [--language ja|en|zh-CN|auto] \
//!     [--min-confidence 0.55] \
//!     [--no-vertical]
//! ```

use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use vtrans_core::types::{CapturedImage, Language, OcrOptions, PixelFormat, ScreenRegion};
use vtrans_core::OcrProvider;
use vtrans_models::ModelManager;
use vtrans_ocr::PaddleOcrProvider;

struct CliArgs {
    models_dir: PathBuf,
    image_path: PathBuf,
    language: Language,
    min_confidence: f32,
    detect_vertical: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&args)?;
    let image = load_image(&args.image_path)?;
    let manager = ModelManager::from_manifest_dir(&args.models_dir)?;
    let provider = PaddleOcrProvider::from_manager(&manager)?;

    let region = ScreenRegion::new("cli", 0, 0, image.width, image.height);
    let options = OcrOptions {
        language: args.language,
        min_confidence: args.min_confidence,
        detect_vertical: args.detect_vertical,
    };
    let result = provider
        .recognize(&image, &region, &options, CancellationToken::new())
        .await?;

    println!("elapsed_ms: {}", result.elapsed_ms);
    println!("lines: {}", result.lines.len());
    for line in &result.lines {
        println!("[{}] {}", line.confidence, line.text);
    }
    println!("--- merged ---");
    println!("{}", result.merged_text);
    Ok(())
}

/// Parse the command-line arguments.
fn parse_args(args: &[String]) -> Result<CliArgs, String> {
    let mut models_dir = None;
    let mut image_path = None;
    let mut language = Language::Auto;
    let mut min_confidence = 0.55;
    let mut detect_vertical = true;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--models" => {
                models_dir = Some(PathBuf::from(next_value(args, &mut index, "--models")?));
            }
            "--image" => {
                image_path = Some(PathBuf::from(next_value(args, &mut index, "--image")?));
            }
            "--language" => {
                let code = next_value(args, &mut index, "--language")?;
                language = Language::from_code(&code).ok_or_else(|| {
                    format!("unsupported language: {code}; expected ja, en, zh-CN, or auto")
                })?;
            }
            "--min-confidence" => {
                let value = next_value(args, &mut index, "--min-confidence")?;
                min_confidence = value
                    .parse()
                    .map_err(|_| format!("invalid confidence: {value}"))?;
            }
            "--no-vertical" => detect_vertical = false,
            "--help" | "-h" => {
                println!(
                    "usage: ocr_verify --models <dir> --image <path> [--language ja|en|zh-CN|auto] [--min-confidence 0.55] [--no-vertical]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 1;
    }

    Ok(CliArgs {
        models_dir: models_dir.ok_or("missing required argument --models <dir>")?,
        image_path: image_path.ok_or("missing required argument --image <path>")?,
        language,
        min_confidence,
        detect_vertical,
    })
}

/// Read the value following a `--flag`.
fn next_value(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("missing value for {flag}"))
}

/// Load an image file as an RGBA captured image.
fn load_image(path: &Path) -> Result<CapturedImage, Box<dyn std::error::Error>> {
    let rgba = image::open(path)?.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(CapturedImage::new(
        width,
        height,
        PixelFormat::Rgba8,
        rgba.into_raw(),
    )?)
}
