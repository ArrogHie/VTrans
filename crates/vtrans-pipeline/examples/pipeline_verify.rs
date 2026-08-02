//! Full-pipeline verification CLI: capture -> OCR -> translate -> output.
//!
//! Drives the real [`Pipeline`] with the concrete capture, OCR, and
//! translation providers and prints every stage to stdout. The recognized
//! and translated text printed by this tool is its deliverable (it is not
//! application logging), mirroring `ocr_verify` and `translation_verify`.
//!
//! Usage:
//!
//! ```text
//! # 本地翻译模型（需先运行 scripts/download_models.ps1 下载模型）
//! cargo run -p vtrans-pipeline --example pipeline_verify -- \
//!     --models src-tauri/resources/models \
//!     --language ja --target zh-CN --mode single
//!
//! # 实时模式：固定区域持续翻译，Ctrl+C 停止
//! cargo run -p vtrans-pipeline --example pipeline_verify -- \
//!     --models src-tauri/resources/models \
//!     --language ja --target zh-CN --mode live \
//!     --region 100,100,800,400 --interval-ms 500
//!
//! # API 翻译（无需本地翻译模型，OCR 仍需要模型目录）
//! cargo run -p vtrans-pipeline --example pipeline_verify -- \
//!     --models src-tauri/resources/models \
//!     --api-endpoint https://api.example.com/v1/chat/completions \
//!     --api-model translator --api-key sk-... \
//!     --language en --target zh-CN --mode single
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use vtrans_capture::WindowsCaptureSource;
use vtrans_core::types::{Language, OcrOptions, ScreenRegion, TranslationRequest};
use vtrans_models::ModelManager;
use vtrans_ocr::PaddleOcrProvider;
use vtrans_pipeline::{Pipeline, PipelineConfig, PipelineDeps, PipelineEvent};
use vtrans_translation::{ApiTranslationProvider, LocalTranslationProvider};

/// Pipeline operating mode.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Single,
    Live,
}

/// Translation backend choice.
enum TranslationChoice {
    Local,
    Api {
        endpoint: String,
        model: String,
        api_key: String,
        timeout: Duration,
        retries: u32,
    },
}

/// Parsed command-line arguments.
struct CliArgs {
    models_dir: PathBuf,
    mode: Mode,
    region: Option<ScreenRegion>,
    language: Language,
    target: Language,
    min_confidence: f32,
    interval_ms: u32,
    threshold: f32,
    translation: TranslationChoice,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&args)?;

    // 1. Capture source; resolve the region against the primary monitor.
    let capture = WindowsCaptureSource::new()?;
    let mut region = match args.region.clone() {
        Some(region) => region,
        None => default_region(&capture),
    };
    if region.monitor_id.is_empty() {
        region.monitor_id = primary_monitor_id(&capture);
    }
    region.validate()?;
    println!(
        "capture region: monitor={} {}x{} at ({},{})",
        region.monitor_id, region.width, region.height, region.x, region.y
    );

    // 2. OCR provider (model loading is a blocking heavyweight step).
    let manager = ModelManager::from_manifest_dir(&args.models_dir)?;
    let ocr = PaddleOcrProvider::from_manager(&manager)?;

    // 3. Translation provider: local ONNX or OpenAI-compatible API.
    let translation = build_translation(&args, &manager)?;

    // 4. Assemble and run the pipeline.
    let options = OcrOptions {
        language: args.language,
        min_confidence: args.min_confidence,
        detect_vertical: true,
    };
    let request = TranslationRequest::new("", args.language, args.target);
    let config = match args.mode {
        Mode::Single => PipelineConfig::single(region, options, request),
        Mode::Live => {
            PipelineConfig::live(region, args.interval_ms, args.threshold, options, request)
        }
    };
    let pipeline = Arc::new(Pipeline::new(
        config,
        PipelineDeps::new(Box::new(capture), Box::new(ocr), translation),
    ));

    let (tx, rx) = mpsc::channel(64);
    let printer = tokio::spawn(print_events(rx));
    match args.mode {
        Mode::Single => {
            let result = pipeline.run(tx).await;
            let _ = printer.await;
            result?;
            println!("[pipeline] single capture completed");
        }
        Mode::Live => {
            let handle = tokio::spawn({
                let pipeline = pipeline.clone();
                async move { pipeline.run(tx).await }
            });
            println!("[pipeline] live translation running; press Ctrl+C to stop");
            tokio::signal::ctrl_c().await?;
            println!("[pipeline] stopping...");
            pipeline.stop().await?;
            handle.await??;
            let _ = printer.await;
            println!("[pipeline] stopped");
        }
    }
    Ok(())
}

/// Builds the translation provider from the CLI arguments.
fn build_translation(
    args: &CliArgs,
    manager: &ModelManager,
) -> Result<Box<dyn vtrans_core::TranslationProvider>, Box<dyn std::error::Error>> {
    match &args.translation {
        TranslationChoice::Local => {
            let provider = LocalTranslationProvider::from_manager(manager)?;
            Ok(Box::new(provider))
        }
        TranslationChoice::Api {
            endpoint,
            model,
            api_key,
            timeout,
            retries,
        } => {
            let provider =
                ApiTranslationProvider::new(endpoint, model, api_key, *timeout, *retries);
            Ok(Box::new(provider))
        }
    }
}

/// Picks a centered 800x600 region on the primary monitor (or the first
/// monitor when none is marked primary).
fn default_region(capture: &WindowsCaptureSource) -> ScreenRegion {
    let monitors = capture.list_monitors();
    let Some(primary) = monitors
        .iter()
        .find(|m| m.is_primary)
        .or_else(|| monitors.first())
    else {
        return ScreenRegion::new("default", 0, 0, 800, 600);
    };
    let width = primary.width.min(800);
    let height = primary.height.min(600);
    let x = i32::try_from((primary.width - width) / 2).unwrap_or_default();
    let y = i32::try_from((primary.height - height) / 2).unwrap_or_default();
    ScreenRegion::new(primary.id.clone(), x, y, width, height)
}

/// Returns the id of the primary monitor, or the first monitor.
fn primary_monitor_id(capture: &WindowsCaptureSource) -> String {
    let monitors = capture.list_monitors();
    match monitors
        .iter()
        .find(|m| m.is_primary)
        .or_else(|| monitors.first())
    {
        Some(monitor) => monitor.id.clone(),
        None => "default".to_string(),
    }
}

/// Prints every pipeline event, including the recognized and translated
/// text, to stdout.
async fn print_events(mut rx: mpsc::Receiver<PipelineEvent>) {
    while let Some(event) = rx.recv().await {
        match event {
            PipelineEvent::CaptureStarted => println!("[capture] frame captured"),
            PipelineEvent::OcrStarted => println!("[ocr] started"),
            PipelineEvent::OcrCompleted(result) => {
                println!(
                    "[ocr] completed in {} ms, {} lines",
                    result.elapsed_ms,
                    result.lines.len()
                );
                println!("--- recognized text ---");
                println!("{}", result.merged_text);
            }
            PipelineEvent::TranslationStarted => println!("[translate] started"),
            PipelineEvent::TranslationCompleted(result) => {
                println!(
                    "[translate] completed in {} ms via {}",
                    result.elapsed_ms, result.provider_id
                );
                println!("--- translated text ---");
                println!("{}", result.translated_text);
            }
            PipelineEvent::Error(error) => eprintln!("[pipeline] error: {error}"),
            PipelineEvent::Stopped => println!("[pipeline] stopped"),
        }
    }
}

/// Parses the command-line arguments.
///
/// A long argument table is clearer than splitting it across helpers.
#[allow(clippy::too_many_lines)]
fn parse_args(args: &[String]) -> Result<CliArgs, String> {
    let mut models_dir = None;
    let mut mode = Mode::Single;
    let mut region = None;
    let mut language = Language::Auto;
    let mut target = Language::ChineseSimplified;
    let mut min_confidence = 0.55;
    let mut interval_ms = 250;
    let mut threshold = vtrans_pipeline::DEFAULT_DIFFERENCE_THRESHOLD;
    let mut endpoint = None;
    let mut model = None;
    let mut api_key = None;
    let mut timeout = Duration::from_secs(30);
    let mut retries = 2;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--help" | "-h" => return Err(usage()),
            "--models" => {
                models_dir = Some(PathBuf::from(next_value(args, &mut index, "--models")?));
            }
            "--mode" => {
                let value = next_value(args, &mut index, "--mode")?;
                mode = match value.as_str() {
                    "single" => Mode::Single,
                    "live" => Mode::Live,
                    other => {
                        return Err(format!("unsupported mode: {other} (expected single|live)"))
                    }
                };
            }
            "--region" => {
                let value = next_value(args, &mut index, "--region")?;
                region = Some(parse_region(&value)?);
            }
            "--language" => {
                let code = next_value(args, &mut index, "--language")?;
                language = Language::from_code(&code)
                    .ok_or_else(|| format!("unsupported language: {code}"))?;
            }
            "--target" => {
                let code = next_value(args, &mut index, "--target")?;
                target = Language::from_code(&code)
                    .ok_or_else(|| format!("unsupported target language: {code}"))?;
            }
            "--min-confidence" => {
                let value = next_value(args, &mut index, "--min-confidence")?;
                min_confidence = value
                    .parse()
                    .map_err(|_| format!("invalid min confidence: {value}"))?;
            }
            "--interval-ms" => {
                let value = next_value(args, &mut index, "--interval-ms")?;
                interval_ms = value
                    .parse()
                    .map_err(|_| format!("invalid interval ms: {value}"))?;
            }
            "--threshold" => {
                let value = next_value(args, &mut index, "--threshold")?;
                threshold = value
                    .parse()
                    .map_err(|_| format!("invalid threshold: {value}"))?;
            }
            "--api-endpoint" => endpoint = Some(next_value(args, &mut index, "--api-endpoint")?),
            "--api-model" => model = Some(next_value(args, &mut index, "--api-model")?),
            "--api-key" => api_key = Some(next_value(args, &mut index, "--api-key")?),
            "--timeout" => {
                let value = next_value(args, &mut index, "--timeout")?;
                timeout = Duration::from_secs(
                    value
                        .parse()
                        .map_err(|_| format!("invalid timeout seconds: {value}"))?,
                );
            }
            "--retries" => {
                let value = next_value(args, &mut index, "--retries")?;
                retries = value
                    .parse()
                    .map_err(|_| format!("invalid retries: {value}"))?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 1;
    }

    let models_dir = models_dir.ok_or_else(|| "missing required --models <dir>".to_string())?;
    let translation = match (endpoint, model, api_key) {
        (Some(endpoint), Some(model), Some(api_key)) => TranslationChoice::Api {
            endpoint,
            model,
            api_key,
            timeout,
            retries,
        },
        (None, None, None) => TranslationChoice::Local,
        _ => {
            return Err(
                "--api-endpoint, --api-model and --api-key must be provided together".to_string(),
            )
        }
    };
    if target.is_auto() {
        return Err("--target cannot be auto".to_string());
    }
    Ok(CliArgs {
        models_dir,
        mode,
        region,
        language,
        target,
        min_confidence,
        interval_ms,
        threshold,
        translation,
    })
}

/// Parses a `x,y,width,height` region string.
fn parse_region(value: &str) -> Result<ScreenRegion, String> {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() != 4 {
        return Err(format!("--region expects x,y,width,height, got {value:?}"));
    }
    let x = parse_int(parts[0], "x")?;
    let y = parse_int(parts[1], "y")?;
    let width = parse_uint(parts[2], "width")?;
    let height = parse_uint(parts[3], "height")?;
    Ok(ScreenRegion::new("", x, y, width, height))
}

fn parse_int(raw: &str, name: &str) -> Result<i32, String> {
    raw.trim()
        .parse()
        .map_err(|_| format!("invalid region {name}: {raw}"))
}

fn parse_uint(raw: &str, name: &str) -> Result<u32, String> {
    raw.trim()
        .parse()
        .map_err(|_| format!("invalid region {name}: {raw}"))
}

/// Returns the value of the option at `index`, advancing the cursor.
fn next_value(args: &[String], index: &mut usize, name: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

/// Returns the usage text.
fn usage() -> String {
    "pipeline_verify: capture -> OCR -> translate full-pipeline verification

Usage:
  pipeline_verify --models <models-dir> [options]

Required:
  --models <dir>            directory containing manifest.json + model files

Options:
  --mode single|live        pipeline mode (default: single)
  --region x,y,w,h          capture region relative to the primary monitor
                            (default: centered 800x600 on the primary monitor)
  --language ja|en|zh-CN|auto   OCR language and translation source (default: auto)
  --target ja|en|zh-CN      translation target (default: zh-CN)
  --min-confidence <f>      OCR minimum confidence (default: 0.55)
  --interval-ms <ms>        live capture interval (default: 250)
  --threshold <f>           live frame-difference threshold (default: 0.02)
  --api-endpoint <url>      use API translation with this chat-completions endpoint
  --api-model <name>        API model name
  --api-key <key>           API key (prefer the VTRANS_API_KEY environment
                            variable; the app uses CredentialManager)
  --timeout <sec>           API timeout (default: 30)
  --retries <n>             API retries (default: 2)
  --help                    show this help"
        .to_string()
}
