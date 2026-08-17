//! First-boot model provisioning and read-only model status reporting.
//!
//! [`ensure_data_models`] is the self-healing bootstrap: every non-optional
//! model file (and the manifest itself) that is missing or fails its SHA-256
//! check in `{data}/models` is re-copied from the read-only bundled source
//! (Tauri `resource_dir()/resources/models`). Optional entries
//! (`translation.model`) are **never** copied — they have no bundled source
//! and are installed through the download flow instead.
//!
//! [`model_status_report`] maps the same verification semantics into the
//! [`ModelStatusReport`] IPC DTO without modifying anything on disk, so
//! `get_model_status` stays strictly read-only.

use std::path::Path;

use serde::Serialize;
use tracing::{debug, info, warn};
use vtrans_models::manifest::{ModelEntry, ModelManifest};
use vtrans_models::verify::verify_entry;
use vtrans_models::{ModelError, ModelManager};

use crate::error::AppError;

/// State of a single model file as reported to the frontend.
///
/// Serialized as `"ready"`, `"missing"`, or `"invalid"`:
/// `ready` means the file exists and its SHA-256 matches the manifest,
/// `missing` means the file does not exist (for optional entries this is the
/// expected "not installed" state), and `invalid` means the file exists but
/// fails verification (or cannot be read).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelState {
    /// The file exists and its SHA-256 matches the manifest.
    Ready,
    /// The file does not exist.
    Missing,
    /// The file exists but fails verification or cannot be read.
    Invalid,
}

/// Status of one manifest model entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelEntryStatus {
    /// Stable manifest entry id (e.g. `"ppocr-det-v6"`).
    pub id: String,
    /// Verification state of the entry's file.
    pub state: ModelState,
    /// Whether the entry is optional (missing optional entries are the
    /// expected "not installed" state, never a failure).
    pub optional: bool,
}

/// Snapshot of model availability returned by `get_model_status` and
/// `retry_model_setup`.
///
/// The per-entry states use the same classification as
/// [`ModelManager::verify_integrity`](vtrans_models::ModelManager::verify_integrity)
/// (optional-missing is "skipped"/`missing`, existing-but-bad is a
/// failure/`invalid`); `ocr_ready` and `translation_ready` are the derived
/// group-level flags the frontend uses for the startup error banner.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ModelStatusReport {
    /// Status of every manifest entry (OCR + translation, optional included).
    pub entries: Vec<ModelEntryStatus>,
    /// Whether every OCR model and dictionary is ready.
    pub ocr_ready: bool,
    /// Whether the local translation model and tokenizer are ready.
    pub translation_ready: bool,
}

/// Outcome of an [`ensure_data_models`] pass.
///
/// The report is produced by the same verification walk so callers get a
/// fresh snapshot without a second hashing pass; `errors` collects the
/// repair failures that degraded the result.
pub(crate) struct ModelSetupOutcome {
    /// Fresh model status after the repair pass.
    pub report: ModelStatusReport,
    /// Human-readable repair errors (empty when everything is consistent).
    pub errors: Vec<String>,
}

/// Classifies one model entry by running the same verification used by
/// `verify_integrity`, without modifying anything on disk.
fn classify_entry(base_dir: &Path, entry: &ModelEntry) -> ModelEntryStatus {
    let state = match verify_entry(base_dir, entry) {
        Ok(()) => ModelState::Ready,
        Err(ModelError::FileNotFound(_)) => ModelState::Missing,
        Err(error) => {
            debug!(
                entry_id = %entry.id,
                error = %error,
                "model entry failed verification"
            );
            ModelState::Invalid
        }
    };
    ModelEntryStatus {
        id: entry.id.clone(),
        state,
        optional: entry.optional,
    }
}

/// Aggregates per-entry states into a [`ModelStatusReport`].
fn aggregate_report(
    manifest: &ModelManifest,
    entry_states: &[ModelEntryStatus],
    dicts_ok: bool,
) -> ModelStatusReport {
    let is_ready = |id: &str| {
        entry_states
            .iter()
            .any(|status| status.id == id && status.state == ModelState::Ready)
    };
    let ocr_entry_ids = std::iter::once(manifest.ocr.det.id.as_str())
        .chain(std::iter::once(manifest.ocr.rec_ja.id.as_str()))
        .chain(std::iter::once(manifest.ocr.rec_en.id.as_str()))
        .chain(
            manifest
                .ocr
                .rec_multi
                .as_ref()
                .map(|entry| entry.id.as_str()),
        );
    let ocr_ready = dicts_ok && ocr_entry_ids.clone().all(is_ready);
    let translation_ready = manifest
        .translation
        .as_ref()
        .is_some_and(|group| is_ready(&group.model.id) && is_ready(&group.tokenizer.id));
    ModelStatusReport {
        entries: entry_states.to_vec(),
        ocr_ready,
        translation_ready,
    }
}

/// Copies one file, creating missing parent directories.
fn copy_file(source: &Path, target: &Path) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source, target)?;
    Ok(())
}

/// Loads the data-side manifest, repairing it from the bundled source when
/// it is missing or unparsable. Returns `None` (with an error recorded)
/// when no usable manifest can be obtained.
fn load_or_repair_manifest(
    data_models_dir: &Path,
    bundled_models_dir: Option<&Path>,
    errors: &mut Vec<String>,
) -> Option<ModelManifest> {
    let manifest_path = data_models_dir.join("manifest.json");
    if let Ok(manifest) = ModelManifest::from_path(&manifest_path) {
        return Some(manifest);
    }
    let Some(bundled) = bundled_models_dir else {
        let message = format!(
            "model manifest is missing or invalid at {} and no bundled source is available",
            manifest_path.display()
        );
        warn!(error = %message, "model manifest unavailable");
        errors.push(message);
        return None;
    };
    let source = bundled.join("manifest.json");
    if let Err(error) = copy_file(&source, &manifest_path) {
        let message = format!(
            "failed to repair model manifest from {}: {error}",
            source.display()
        );
        warn!(error = %message, "model manifest repair failed");
        errors.push(message);
        return None;
    }
    match ModelManifest::from_path(&manifest_path) {
        Ok(manifest) => {
            info!("model manifest repaired from the bundled source");
            Some(manifest)
        }
        Err(error) => {
            let message = format!("bundled model manifest is invalid: {error}");
            warn!(error = %message, "bundled model manifest rejected");
            errors.push(message);
            None
        }
    }
}

/// Repairs one non-optional entry by copying it from the bundled source and
/// re-verifying the copy.
fn repair_from_bundled(
    data_models_dir: &Path,
    bundled_models_dir: Option<&Path>,
    entry: &ModelEntry,
) -> Result<(), String> {
    let Some(bundled) = bundled_models_dir else {
        return Err(format!(
            "no bundled model source available to repair {}",
            entry.id
        ));
    };
    let source = bundled.join(&entry.path);
    if !source.exists() {
        return Err(format!(
            "bundled source missing for {}: {}",
            entry.id,
            source.display()
        ));
    }
    let target = data_models_dir.join(&entry.path);
    copy_file(&source, &target)
        .map_err(|error| format!("failed to repair {}: {error}", entry.id))?;
    match verify_entry(data_models_dir, entry) {
        Ok(()) => {
            info!(entry_id = %entry.id, "model entry repaired from the bundled source");
            Ok(())
        }
        Err(error) => {
            // The bundled copy still fails verification: drop it so the next
            // boot (or `retry_model_setup`) retries from a clean state.
            let _ = std::fs::remove_file(&target);
            Err(format!(
                "repaired {} still fails verification: {error}",
                entry.id
            ))
        }
    }
}

/// Provisions `{data}/models` from the bundled read-only source.
///
/// Runs for every boot and for `retry_model_setup`: the manifest and every
/// non-optional entry that is missing or fails its SHA-256 check are
/// re-copied from `bundled_models_dir`; dictionary files are copied when
/// missing. Optional entries (e.g. `translation.model`) are never copied and
/// simply classified (missing → `missing`). The pass is idempotent and
/// self-healing: deleting or corrupting `{data}/models` and restarting (or
/// retrying) restores the bundled files.
///
/// Never fails the caller: degradation is reported through
/// [`ModelSetupOutcome::errors`] and the accompanying report.
#[tracing::instrument(skip(data_models_dir, bundled_models_dir))]
pub(crate) fn ensure_data_models(
    data_models_dir: &Path,
    bundled_models_dir: Option<&Path>,
) -> ModelSetupOutcome {
    let mut errors = Vec::new();
    let Some(manifest) = load_or_repair_manifest(data_models_dir, bundled_models_dir, &mut errors)
    else {
        return ModelSetupOutcome {
            report: ModelStatusReport::default(),
            errors,
        };
    };

    let mut entry_states = Vec::with_capacity(manifest.all_entries().len());
    for entry in manifest.all_entries() {
        if entry.optional {
            // Optional entries have no bundled source: never copy, only
            // classify (missing stays missing until downloaded).
            entry_states.push(classify_entry(data_models_dir, entry));
            continue;
        }
        let status = match verify_entry(data_models_dir, entry) {
            Ok(()) => ModelEntryStatus {
                id: entry.id.clone(),
                state: ModelState::Ready,
                optional: entry.optional,
            },
            Err(ModelError::Io(error)) => {
                let message = format!("cannot read {}: {error}", entry.id);
                warn!(error = %message, "model entry is unreadable");
                errors.push(message);
                ModelEntryStatus {
                    id: entry.id.clone(),
                    state: ModelState::Invalid,
                    optional: entry.optional,
                }
            }
            Err(_) => {
                // Missing or hash mismatch: repair from the bundled source.
                match repair_from_bundled(data_models_dir, bundled_models_dir, entry) {
                    Ok(()) => ModelEntryStatus {
                        id: entry.id.clone(),
                        state: ModelState::Ready,
                        optional: entry.optional,
                    },
                    Err(message) => {
                        warn!(error = %message, "model entry repair failed");
                        errors.push(message);
                        classify_entry(data_models_dir, entry)
                    }
                }
            }
        };
        entry_states.push(status);
    }

    // Dictionaries carry no hash in the manifest: existence check + copy.
    let mut dicts_ok = true;
    for (dict_name, relative) in manifest.dict_paths() {
        let target = data_models_dir.join(relative);
        if target.exists() {
            continue;
        }
        let mut missing = true;
        if let Some(bundled) = bundled_models_dir {
            let source = bundled.join(relative);
            if source.exists() {
                match copy_file(&source, &target) {
                    Ok(()) => missing = false,
                    Err(error) => warn!(
                        dict = dict_name,
                        error = %error,
                        "failed to repair dictionary file"
                    ),
                }
            }
        }
        if missing {
            let message = format!("dictionary file unavailable: {dict_name}");
            warn!(error = %message, "dictionary file missing");
            errors.push(message);
            dicts_ok = false;
        } else {
            debug!(
                dict = dict_name,
                "dictionary file repaired from the bundled source"
            );
        }
    }

    let report = aggregate_report(&manifest, &entry_states, dicts_ok);
    info!(
        ocr_ready = report.ocr_ready,
        translation_ready = report.translation_ready,
        repair_errors = errors.len(),
        "model provisioning pass finished"
    );
    ModelSetupOutcome { report, errors }
}

/// Builds a [`ModelStatusReport`] from a loaded manager without modifying
/// anything on disk.
///
/// This is the read-only counterpart used by `get_model_status`; the
/// classification is the same as `verify_integrity`'s (missing optional
/// entries are "not installed", existing-but-bad entries are failures).
pub(crate) fn model_status_report(manager: &ModelManager) -> ModelStatusReport {
    let manifest = manager.manifest();
    let base_dir = manager.manifest_dir();
    let entry_states: Vec<ModelEntryStatus> = manifest
        .all_entries()
        .iter()
        .map(|entry| classify_entry(base_dir, entry))
        .collect();
    let dicts_ok = manifest
        .dict_paths()
        .iter()
        .all(|(_, relative)| base_dir.join(relative).exists());
    let report = aggregate_report(manifest, &entry_states, dicts_ok);
    debug!(
        ocr_ready = report.ocr_ready,
        translation_ready = report.translation_ready,
        entries = report.entries.len(),
        "read-only model status report built"
    );
    report
}

/// Maps a model provisioning outcome into a command result, keeping the
/// "never fails the caller" contract of [`ensure_data_models`].
pub(crate) fn ensure_outcome_result(
    outcome: ModelSetupOutcome,
) -> Result<ModelStatusReport, AppError> {
    if !outcome.errors.is_empty() {
        return Err(AppError::ModelNotReady(format!(
            "模型就位失败: {}",
            outcome.errors.join("; ")
        )));
    }
    Ok(outcome.report)
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// Minimal std-only temporary-directory guard (parallel-test safe).
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "vtrans-app-model-setup-{name}-{}-{seq}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir should be created");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const DET_CONTENT: &[u8] = b"det-model-bytes";
    const REC_CONTENT: &[u8] = b"rec-model-bytes";
    const TOK_CONTENT: &[u8] = b"tokenizer-json-bytes";
    const MODEL_CONTENT: &[u8] = b"translation-model-bytes";
    const DICT_CONTENT: &[u8] = b"ppocrv6-dict";

    fn sha256_of(content: &[u8]) -> String {
        format!("{:x}", Sha256::digest(content))
    }

    /// Manifest JSON with `translation.model` marked optional and carrying
    /// download metadata.
    fn manifest_json() -> String {
        format!(
            r#"{{
  "version": 1,
  "ocr": {{
    "det": {{ "id": "det", "path": "ocr/det.onnx", "sha256": "{}", "size_bytes": {} }},
    "rec_ja": {{ "id": "rj", "path": "ocr/rec.onnx", "sha256": "{}", "size_bytes": {} }},
    "rec_en": {{ "id": "re", "path": "ocr/rec.onnx", "sha256": "{}", "size_bytes": {} }},
    "rec_multi": null,
    "dicts": {{ "ja": "ocr/dict.txt" }},
    "preprocess_params": {{
      "image_size": [640, 640],
      "mean": [0.485, 0.456, 0.406],
      "std": [0.229, 0.224, 0.225],
      "det_threshold": 0.2,
      "unclip_ratio": 1.4
    }}
  }},
  "translation": {{
    "model": {{
      "id": "tm",
      "path": "translation/model.onnx",
      "sha256": "{}",
      "size_bytes": {},
      "optional": true,
      "download_url": "https://example.com/translation-model.onnx",
      "download_size_bytes": {}
    }},
    "tokenizer": {{ "id": "tk", "path": "translation/tokenizer.json", "sha256": "{}", "size_bytes": {} }},
    "supported_pairs": [["en", "zh-CN"]],
    "max_length": 512,
    "inference_params": {{ "max_batch_size": 1, "num_beams": 4 }}
  }}
}}"#,
            sha256_of(DET_CONTENT),
            DET_CONTENT.len(),
            sha256_of(REC_CONTENT),
            REC_CONTENT.len(),
            sha256_of(REC_CONTENT),
            REC_CONTENT.len(),
            sha256_of(MODEL_CONTENT),
            MODEL_CONTENT.len(),
            MODEL_CONTENT.len(),
            sha256_of(TOK_CONTENT),
            TOK_CONTENT.len(),
        )
    }

    /// Writes the complete bundled model source into `root` (the content of
    /// `resource_dir()/resources/models`), optionally including the optional
    /// translation model.
    fn write_bundled_source(root: &Path, include_optional_model: bool) {
        let models = root.join("models");
        std::fs::create_dir_all(models.join("ocr")).unwrap();
        std::fs::create_dir_all(models.join("translation")).unwrap();
        std::fs::write(models.join("manifest.json"), manifest_json()).unwrap();
        std::fs::write(models.join("ocr/det.onnx"), DET_CONTENT).unwrap();
        std::fs::write(models.join("ocr/rec.onnx"), REC_CONTENT).unwrap();
        std::fs::write(models.join("ocr/dict.txt"), DICT_CONTENT).unwrap();
        std::fs::write(models.join("translation/tokenizer.json"), TOK_CONTENT).unwrap();
        if include_optional_model {
            std::fs::write(models.join("translation/model.onnx"), MODEL_CONTENT).unwrap();
        }
    }

    fn entry<'a>(report: &'a ModelStatusReport, id: &str) -> &'a ModelEntryStatus {
        report
            .entries
            .iter()
            .find(|status| status.id == id)
            .unwrap_or_else(|| panic!("entry {id} missing from report"))
    }

    #[test]
    fn ensure_copies_manifest_and_bundled_files_on_first_boot() {
        let bundled = TestDir::new("bundled-first");
        let data = TestDir::new("data-first");
        write_bundled_source(bundled.path(), false);
        let data_models = data.path().join("models");
        std::fs::create_dir_all(&data_models).unwrap();

        let outcome = ensure_data_models(&data_models, Some(&bundled.path().join("models")));
        assert!(outcome.errors.is_empty(), "errors: {:?}", outcome.errors);

        assert!(data_models.join("manifest.json").exists());
        assert_eq!(
            std::fs::read(data_models.join("ocr/det.onnx")).unwrap(),
            DET_CONTENT
        );
        assert_eq!(
            std::fs::read(data_models.join("ocr/rec.onnx")).unwrap(),
            REC_CONTENT
        );
        assert_eq!(
            std::fs::read(data_models.join("ocr/dict.txt")).unwrap(),
            DICT_CONTENT
        );
        assert_eq!(
            std::fs::read(data_models.join("translation/tokenizer.json")).unwrap(),
            TOK_CONTENT
        );
        // Optional entry: never copied, reported as missing.
        assert!(!data_models.join("translation/model.onnx").exists());
        assert_eq!(entry(&outcome.report, "tm").state, ModelState::Missing);
        assert!(entry(&outcome.report, "tm").optional);
        assert_eq!(entry(&outcome.report, "tk").state, ModelState::Ready);
        assert!(outcome.report.ocr_ready);
        assert!(!outcome.report.translation_ready);
    }

    #[test]
    fn ensure_is_idempotent_and_never_overwrites_valid_files() {
        let bundled = TestDir::new("bundled-idem");
        let data = TestDir::new("data-idem");
        write_bundled_source(bundled.path(), false);
        let data_models = data.path().join("models");
        std::fs::create_dir_all(&data_models).unwrap();
        let bundled_models = bundled.path().join("models");

        ensure_data_models(&data_models, Some(&bundled_models));

        // Tamper with the bundled source: a second pass must NOT clobber the
        // already-valid data files with the changed bundled content.
        std::fs::write(bundled_models.join("ocr/det.onnx"), b"tampered-bundled").unwrap();
        let outcome = ensure_data_models(&data_models, Some(&bundled_models));
        assert!(outcome.errors.is_empty(), "errors: {:?}", outcome.errors);
        assert_eq!(
            std::fs::read(data_models.join("ocr/det.onnx")).unwrap(),
            DET_CONTENT
        );
        assert!(outcome.report.ocr_ready);
    }

    #[test]
    fn ensure_self_heals_a_deleted_file() {
        let bundled = TestDir::new("bundled-delete");
        let data = TestDir::new("data-delete");
        write_bundled_source(bundled.path(), false);
        let data_models = data.path().join("models");
        std::fs::create_dir_all(&data_models).unwrap();
        let bundled_models = bundled.path().join("models");
        ensure_data_models(&data_models, Some(&bundled_models));

        std::fs::remove_file(data_models.join("ocr/rec.onnx")).unwrap();
        let outcome = ensure_data_models(&data_models, Some(&bundled_models));
        assert!(outcome.errors.is_empty(), "errors: {:?}", outcome.errors);
        assert_eq!(
            std::fs::read(data_models.join("ocr/rec.onnx")).unwrap(),
            REC_CONTENT
        );
        assert_eq!(entry(&outcome.report, "rj").state, ModelState::Ready);
        assert!(outcome.report.ocr_ready);
    }

    #[test]
    fn ensure_re_copies_a_corrupted_file() {
        let bundled = TestDir::new("bundled-corrupt");
        let data = TestDir::new("data-corrupt");
        write_bundled_source(bundled.path(), false);
        let data_models = data.path().join("models");
        std::fs::create_dir_all(&data_models).unwrap();
        let bundled_models = bundled.path().join("models");
        ensure_data_models(&data_models, Some(&bundled_models));

        std::fs::write(data_models.join("ocr/det.onnx"), b"garbage-bytes").unwrap();
        let outcome = ensure_data_models(&data_models, Some(&bundled_models));
        assert!(outcome.errors.is_empty(), "errors: {:?}", outcome.errors);
        assert_eq!(
            std::fs::read(data_models.join("ocr/det.onnx")).unwrap(),
            DET_CONTENT
        );
        assert_eq!(entry(&outcome.report, "det").state, ModelState::Ready);
        assert!(outcome.report.ocr_ready);
    }

    #[test]
    fn ensure_never_copies_optional_entries_even_when_bundled() {
        let bundled = TestDir::new("bundled-optional");
        let data = TestDir::new("data-optional");
        write_bundled_source(bundled.path(), true);
        let data_models = data.path().join("models");
        std::fs::create_dir_all(&data_models).unwrap();

        let outcome = ensure_data_models(&data_models, Some(&bundled.path().join("models")));
        assert!(!data_models.join("translation/model.onnx").exists());
        assert_eq!(entry(&outcome.report, "tm").state, ModelState::Missing);
        assert!(!outcome.report.translation_ready);
    }

    #[test]
    fn ensure_without_bundled_source_reports_errors_and_keeps_missing_state() {
        let data = TestDir::new("data-nobundled");
        let data_models = data.path().join("models");
        std::fs::create_dir_all(&data_models).unwrap();

        let outcome = ensure_data_models(&data_models, None);
        assert!(!outcome.errors.is_empty());
        assert_eq!(outcome.report, ModelStatusReport::default());
        // The missing manifest must not be fabricated.
        assert!(!data_models.join("manifest.json").exists());
    }

    #[test]
    fn ensure_repairs_a_corrupted_manifest() {
        let bundled = TestDir::new("bundled-manifest");
        let data = TestDir::new("data-manifest");
        write_bundled_source(bundled.path(), false);
        let data_models = data.path().join("models");
        std::fs::create_dir_all(&data_models).unwrap();
        std::fs::write(data_models.join("manifest.json"), b"{ not json").unwrap();

        let outcome = ensure_data_models(&data_models, Some(&bundled.path().join("models")));
        assert!(outcome.errors.is_empty(), "errors: {:?}", outcome.errors);
        let repaired = std::fs::read_to_string(data_models.join("manifest.json")).unwrap();
        assert!(repaired.contains(r#""optional": true"#));
        assert!(outcome.report.ocr_ready);
    }

    #[test]
    fn status_report_classifies_ready_missing_and_invalid_without_repairing() {
        let dir = TestDir::new("status");
        let data_models = dir.path().join("models");
        std::fs::create_dir_all(data_models.join("ocr")).unwrap();
        std::fs::create_dir_all(data_models.join("translation")).unwrap();
        std::fs::write(data_models.join("manifest.json"), manifest_json()).unwrap();
        std::fs::write(data_models.join("ocr/det.onnx"), DET_CONTENT).unwrap();
        // rec.onnx exists but is corrupt → invalid; the model is missing
        // (optional) → missing; tokenizer ready; dict ready.
        std::fs::write(data_models.join("ocr/rec.onnx"), b"corrupted").unwrap();
        std::fs::write(data_models.join("ocr/dict.txt"), DICT_CONTENT).unwrap();
        std::fs::write(data_models.join("translation/tokenizer.json"), TOK_CONTENT).unwrap();

        let manager = ModelManager::from_manifest_dir(&data_models).unwrap();
        let report = model_status_report(&manager);

        assert_eq!(entry(&report, "det").state, ModelState::Ready);
        assert_eq!(entry(&report, "rj").state, ModelState::Invalid);
        assert_eq!(entry(&report, "re").state, ModelState::Invalid);
        assert_eq!(entry(&report, "tm").state, ModelState::Missing);
        assert!(entry(&report, "tm").optional);
        assert_eq!(entry(&report, "tk").state, ModelState::Ready);
        assert!(!report.ocr_ready);
        assert!(!report.translation_ready);

        // Strictly read-only: the corrupted file is untouched and no file
        // was created or repaired.
        assert_eq!(
            std::fs::read(data_models.join("ocr/rec.onnx")).unwrap(),
            b"corrupted"
        );
        assert!(!data_models.join("translation/model.onnx").exists());
    }

    #[test]
    fn status_report_is_fully_ready_when_everything_verifies() {
        let bundled = TestDir::new("bundled-ready");
        let data = TestDir::new("data-ready");
        write_bundled_source(bundled.path(), false);
        let data_models = data.path().join("models");
        std::fs::create_dir_all(&data_models).unwrap();
        ensure_data_models(&data_models, Some(&bundled.path().join("models")));
        // Install the optional model manually (simulates a completed download).
        std::fs::write(data_models.join("translation/model.onnx"), MODEL_CONTENT).unwrap();

        let manager = ModelManager::from_manifest_dir(&data_models).unwrap();
        let report = model_status_report(&manager);
        assert_eq!(entry(&report, "tm").state, ModelState::Ready);
        assert!(report.ocr_ready);
        assert!(report.translation_ready);
    }

    #[test]
    fn model_state_serializes_to_lowercase_names() {
        assert_eq!(
            serde_json::to_string(&ModelState::Ready).unwrap(),
            r#""ready""#
        );
        assert_eq!(
            serde_json::to_string(&ModelState::Missing).unwrap(),
            r#""missing""#
        );
        assert_eq!(
            serde_json::to_string(&ModelState::Invalid).unwrap(),
            r#""invalid""#
        );
    }

    #[test]
    fn model_status_report_serializes_with_frontend_field_names() {
        let report = ModelStatusReport {
            entries: vec![ModelEntryStatus {
                id: "det".to_string(),
                state: ModelState::Ready,
                optional: false,
            }],
            ocr_ready: true,
            translation_ready: false,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains(r#""entries""#));
        assert!(json.contains(r#""id":"det""#));
        assert!(json.contains(r#""state":"ready""#));
        assert!(json.contains(r#""optional":false"#));
        assert!(json.contains(r#""ocr_ready":true"#));
        assert!(json.contains(r#""translation_ready":false"#));
    }
}
