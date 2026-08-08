//! Verification CLI for translation providers.
//!
//! Usage:
//!
//! ```text
//! # Native dual-engine mode (needs models + translation_bridge.dll):
//! cargo run --example translation_verify -- \
//!     --text "hello" --source en --target zh-CN \
//!     --models src-tauri/resources/models [--quality fast|balanced]
//!
//! # API mode:
//! cargo run --example translation_verify -- \
//!     --text "hello" --source en --target ja \
//!     --api-endpoint https://api.example.com/v1/chat/completions \
//!     --api-model translator --api-key sk-... [--timeout 30] [--retries 2]
//! ```
//!
//! The native mode only supports `en -> zh-CN` and `ja -> zh-CN`; the
//! source language must be concrete (`auto` is resolved by the pipeline,
//! not by the provider).

use std::path::PathBuf;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use vtrans_core::types::{Language, TranslationRequest};
use vtrans_core::TranslationProvider;
use vtrans_models::ModelManager;
use vtrans_translation::{ApiTranslationProvider, NativeTranslationProvider, TranslationQuality};

enum Mode {
    Api {
        endpoint: String,
        model: String,
        api_key: String,
        timeout: Duration,
        retries: u32,
    },
    Native {
        models_dir: PathBuf,
        quality: TranslationQuality,
    },
}

struct CliArgs {
    mode: Mode,
    text: String,
    source: Language,
    target: Language,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&args)?;
    let request = TranslationRequest::new(&args.text, args.source, args.target);

    let cancel = CancellationToken::new();
    let result = match &args.mode {
        Mode::Api {
            endpoint,
            model,
            api_key,
            timeout,
            retries,
        } => {
            let provider =
                ApiTranslationProvider::new(endpoint, model, api_key, *timeout, *retries);
            provider.translate(&request, cancel).await?
        }
        Mode::Native {
            models_dir,
            quality,
        } => {
            let manager = ModelManager::from_manifest_dir(models_dir)?;
            let provider =
                NativeTranslationProvider::from_manager(&manager)?.with_quality(*quality)?;
            provider.translate(&request, cancel).await?
        }
    };

    println!("provider_id: {}", result.provider_id);
    println!("elapsed_ms: {}", result.elapsed_ms);
    println!("--- translated ---");
    println!("{}", result.translated_text);
    Ok(())
}

/// Parse command-line arguments.
fn parse_args(args: &[String]) -> Result<CliArgs, String> {
    let mut text = None;
    let mut source = Language::Auto;
    let mut target = None;
    let mut endpoint = None;
    let mut model = None;
    let mut api_key = None;
    let mut timeout = Duration::from_secs(30);
    let mut retries = 2;
    let mut models_dir = None;
    let mut quality = TranslationQuality::default();

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--text" => text = Some(next_value(args, &mut index, "--text")?),
            "--source" => {
                let code = next_value(args, &mut index, "--source")?;
                source = Language::from_code(&code)
                    .ok_or_else(|| format!("unsupported language: {code}"))?;
            }
            "--target" => {
                let code = next_value(args, &mut index, "--target")?;
                target = Some(
                    Language::from_code(&code)
                        .ok_or_else(|| format!("unsupported language: {code}"))?,
                );
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
            "--models" => {
                models_dir = Some(PathBuf::from(next_value(args, &mut index, "--models")?));
            }
            "--quality" => {
                let value = next_value(args, &mut index, "--quality")?;
                quality = value
                    .parse::<TranslationQuality>()
                    .map_err(|_| format!("invalid quality: {value} (expected fast|balanced)"))?;
            }
            "--help" | "-h" => {
                println!(
                    "usage: translation_verify --text <text> --source <code> --target <code> \n\
                     (--api-endpoint <url> --api-model <model> [--api-key <key>] [--timeout <secs>] [--retries <n>] \n\
                     | --models <dir> [--quality fast|balanced])"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 1;
    }

    let text = text.ok_or("missing required argument --text <text>")?;
    let target = target.ok_or("missing required argument --target <code>")?;

    let mode = if let Some(models_dir) = models_dir {
        if endpoint.is_some() || model.is_some() || api_key.is_some() {
            return Err("--models cannot be combined with API arguments".to_string());
        }
        Mode::Native {
            models_dir,
            quality,
        }
    } else {
        let endpoint = endpoint.ok_or("missing required argument --api-endpoint <url>")?;
        let model = model.ok_or("missing required argument --api-model <model>")?;
        let api_key = match api_key {
            Some(key) => key,
            None => std::env::var("VTRANS_API_KEY")
                .map_err(|_| "missing --api-key or VTRANS_API_KEY environment variable")?,
        };
        Mode::Api {
            endpoint,
            model,
            api_key,
            timeout,
            retries,
        }
    };

    Ok(CliArgs {
        mode,
        text,
        source,
        target,
    })
}

/// Read the value following a `--flag`.
fn next_value(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("missing value for {flag}"))
}
