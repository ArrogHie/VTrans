//! Model integrity verification CLI.
//!
//! Loads `manifest.json` from a models directory and verifies the SHA-256
//! hash of every referenced model file, mirroring the verification run by
//! the `load_local_models` Tauri command. Exits non-zero when any required
//! file is missing or corrupted. Optional entries (`"optional": true`) that
//! are not installed are reported as skipped — not failures — and do not
//! affect the exit code.
//!
//! ```powershell
//! # 默认目录（仓库根目录下）
//! cargo run --bin vtrans-verify-models
//!
//! # 显式指定模型目录
//! cargo run --bin vtrans-verify-models -- --models src-tauri/resources/models
//! ```

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use vtrans_models::ModelManager;
use vtrans_models::VerifyReport;

const DEFAULT_MODELS_DIR: &str = "src-tauri/resources/models";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let models_dir = parse_args(&args);
    let Some(models_dir) = models_dir else {
        print_usage();
        return ExitCode::from(2);
    };
    if !models_dir.exists() {
        eprintln!(
            "error: models directory not found: {}",
            models_dir.display()
        );
        eprintln!("hint: place model files under the directory or pass --models <dir>");
        return ExitCode::from(2);
    }

    let manager = match ModelManager::from_manifest_dir(&models_dir) {
        Ok(manager) => manager,
        Err(error) => {
            eprintln!(
                "error: failed to load manifest from {}: {error}",
                models_dir.display()
            );
            return ExitCode::from(1);
        }
    };

    let report = match manager.verify_integrity() {
        Ok(report) => report,
        Err(error) => {
            eprintln!("error: integrity verification failed: {error}");
            return ExitCode::from(1);
        }
    };

    print_report(&report, &models_dir)
}

/// Prints the verification summary and returns the process exit code.
///
/// Skipped optional entries are printed to stdout as informational lines;
/// failures are printed to stderr and cause a non-zero exit code.
fn print_report(report: &VerifyReport, models_dir: &std::path::Path) -> ExitCode {
    println!(
        "verified {}/{} model files under {}",
        report.passed,
        report.checked,
        models_dir.display()
    );
    for skipped_id in &report.skipped {
        println!("skipped: {skipped_id} (optional, not installed)");
    }
    for failure in &report.failed {
        eprintln!("failed: {failure}");
    }
    if report.failed.is_empty() {
        if report.skipped.is_empty() {
            println!("all model files are valid");
        } else {
            println!(
                "all required model files are valid ({} optional entries not installed)",
                report.skipped.len()
            );
        }
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Parses `--models <dir>` (also honoring the `VTRANS_MODEL_DIR`
/// environment variable used by the application) and returns `None` when
/// an unknown flag is passed.
fn parse_args(args: &[String]) -> Option<PathBuf> {
    let mut index = 0;
    let mut explicit = None;
    while index < args.len() {
        match args[index].as_str() {
            "--models" => {
                index += 1;
                explicit = Some(PathBuf::from(args.get(index)?));
            }
            "--help" | "-h" => return None,
            other => {
                eprintln!("error: unknown argument: {other}");
                return None;
            }
        }
        index += 1;
    }
    Some(explicit.unwrap_or_else(|| {
        env::var("VTRANS_MODEL_DIR")
            .map_or_else(|_| PathBuf::from(DEFAULT_MODELS_DIR), PathBuf::from)
    }))
}

fn print_usage() {
    eprintln!(
        "vtrans-verify-models: verify SHA-256 integrity of all model files

Usage:
  vtrans-verify-models [--models <dir>]

Options:
  --models <dir>   models directory containing manifest.json (default:
                   src-tauri/resources/models, or $VTRANS_MODEL_DIR)
  --help           show this help"
    );
}
