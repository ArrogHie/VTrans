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
//!     [--no-vertical] \
//!     [--dump-det-input path.npy]
//! ```

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use vtrans_core::types::{CapturedImage, Language, OcrOptions, PixelFormat, ScreenRegion};
use vtrans_core::OcrProvider;
use vtrans_models::ModelManager;
use vtrans_ocr::preprocess::{det_preprocess, rgb_region};
use vtrans_ocr::PaddleOcrProvider;

struct CliArgs {
    models_dir: PathBuf,
    image_path: PathBuf,
    language: Language,
    min_confidence: f32,
    detect_vertical: bool,
    dump_det_input: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&args)?;
    let image = load_image(&args.image_path)?;
    let manager = ModelManager::from_manifest_dir(&args.models_dir)?;
    let provider = PaddleOcrProvider::from_manager(&manager)?;

    let region = ScreenRegion::new("cli", 0, 0, image.width, image.height);
    if let Some(path) = &args.dump_det_input {
        let rgb = rgb_region(&image, &region)?;
        let input = det_preprocess(&rgb, &manager.manifest().ocr.preprocess_params)?;
        write_npy(
            path,
            input.tensor.shape(),
            input.tensor.as_slice().unwrap_or_default(),
        )?;
        println!("det input dumped to {}", path.display());
        return Ok(());
    }
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
        let polygon = &line.polygon;
        let width = (polygon[1][0] - polygon[0][0])
            .max(polygon[3][0] - polygon[2][0])
            .abs();
        let height = (polygon[3][1] - polygon[0][1])
            .max(polygon[2][1] - polygon[1][1])
            .abs();
        let points = polygon
            .iter()
            .map(|point| format!("{:.0},{:.0}", point[0], point[1]))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "[{:.6}] box={width:.0}x{height:.0} poly=[{points}] {}",
            line.confidence, line.text
        );
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
    let mut dump_det_input = None;

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
            "--dump-det-input" => {
                dump_det_input = Some(PathBuf::from(next_value(
                    args,
                    &mut index,
                    "--dump-det-input",
                )?));
            }
            "--help" | "-h" => {
                println!(
                    "usage: ocr_verify --models <dir> --image <path> [--language ja|en|zh-CN|auto] [--min-confidence 0.55] [--no-vertical] [--dump-det-input path.npy]"
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
        dump_det_input,
    })
}

/// Write a float32 array in `NumPy` `.npy` format (C order).
///
/// Used by the verification CLI to export the normalized detection input
/// tensor so it can be compared stage by stage against the Python baseline
/// artifacts produced by `scripts/ppocrv6/baseline_ocr.py` (guide §14).
///
/// # Errors
///
/// Returns an I/O error if the file cannot be created or written.
fn write_npy(path: &Path, shape: &[usize], data: &[f32]) -> std::io::Result<()> {
    let header = format!(
        "{{'descr': '<f4', 'fortran_order': False, 'shape': ({}), }}",
        shape
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut bytes = Vec::with_capacity(header.len() + 1 + data.len() * 4);
    bytes.extend_from_slice(b"\x93NUMPY");
    bytes.push(1); // major version
    bytes.push(0); // minor version
    let header_len = u16::try_from(header.len() + 1)
        .map_err(|_| std::io::Error::other("npy header too long"))?;
    bytes.extend_from_slice(&header_len.to_le_bytes());
    bytes.extend_from_slice(header.as_bytes());
    bytes.push(b'\n');
    for value in data {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut file = File::create(path)?;
    file.write_all(&bytes)
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
