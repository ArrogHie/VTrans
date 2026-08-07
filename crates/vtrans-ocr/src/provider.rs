//! [`PaddleOcrProvider`]: the ONNX Runtime implementation of [`OcrProvider`].

// Provider timings and pixel-to-model conversions are bounded by the input
// image and model configuration.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless
)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use ort::ep;
use ort::session::{builder::GraphOptimizationLevel, RunOptions, Session};

use vtrans_core::error::OcrError;
use vtrans_core::traits::OcrProvider;
use vtrans_core::types::{CapturedImage, Language, OcrLine, OcrOptions, OcrResult, ScreenRegion};
use vtrans_models::{ModelEntry, ModelManager, ModelManifest, PreprocessParams};

use crate::detect::Detector;
use crate::geometry::{rotate_90_cw, warp_perspective};
use crate::postprocess::{boxes_from_map, merge_lines, sort_boxes, DetectionParams};
use crate::preprocess::{det_preprocess, rgb_region};
use crate::recognize::Recognizer;

/// PaddleOCR-style ONNX recognition provider.
///
/// The provider owns one detection session and one recognition session per
/// supported language. Sessions are initialized once during construction and
/// reused for every frame; ONNX Runtime runs are serialized internally with a
/// mutex because `Session::run` requires exclusive access. When multiple
/// recognition slots point at the same model file (PP-OCRv6 unifies
/// `rec_ja` / `rec_en` / `rec_multi`), they share a single session.
///
/// # Example
///
/// ```no_run
/// # use vtrans_models::ModelManager;
/// # use vtrans_ocr::PaddleOcrProvider;
/// let manager = ModelManager::from_manifest_dir(
///     std::path::Path::new("src-tauri/resources/models"),
/// )?;
/// let provider = PaddleOcrProvider::from_manager(&manager)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug)]
pub struct PaddleOcrProvider {
    det: Arc<Detector>,
    rec_ja: Arc<Recognizer>,
    rec_en: Arc<Recognizer>,
    rec_multi: Option<Arc<Recognizer>>,
    preprocess: PreprocessParams,
    supported_languages: Vec<Language>,
}

impl PaddleOcrProvider {
    /// Load the provider from a manifest, resolving relative paths from the
    /// current working directory.
    ///
    /// Prefer [`from_manifest_dir`](Self::from_manifest_dir) or
    /// [`from_manager`](Self::from_manager) in application code so model
    /// paths do not depend on the process working directory.
    ///
    /// # Errors
    ///
    /// Returns [`OcrError::InvalidManifest`] when the manifest is invalid and
    /// [`OcrError::ModelLoad`] when model files are missing, corrupt, or
    /// cannot be loaded by ONNX Runtime.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use vtrans_models::ModelManifest;
    /// # use vtrans_ocr::PaddleOcrProvider;
    /// let manifest = ModelManifest::from_json_str(r#"{
    ///   "version": 1,
    ///   "ocr": {
    ///     "det": { "id": "det", "path": "ocr/det.onnx", "sha256": "abc", "size_bytes": 1 },
    ///     "rec_ja": { "id": "rj", "path": "ocr/rec_ja.onnx", "sha256": "def", "size_bytes": 2 },
    ///     "rec_en": { "id": "re", "path": "ocr/rec_en.onnx", "sha256": "ghi", "size_bytes": 3 },
    ///     "rec_multi": null,
    ///     "dicts": { "ja": "ocr/dict_ja.txt", "en": "ocr/dict_en.txt" },
    ///     "preprocess_params": {
    ///       "image_size": [960, 960],
    ///       "mean": [0.485, 0.456, 0.406],
    ///       "std": [0.229, 0.224, 0.225],
    ///       "det_threshold": 0.3,
    ///       "unclip_ratio": 1.5
    ///     }
    ///   },
    ///   "translation": null
    /// }"#)?;
    /// let provider = PaddleOcrProvider::from_manifest(&manifest)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn from_manifest(manifest: &ModelManifest) -> Result<Self, OcrError> {
        Self::from_manifest_dir(manifest, Path::new("."))
    }

    /// Load the provider from a manifest and a models directory.
    ///
    /// Before loading, all referenced model files are verified with SHA-256
    /// and dictionary files are checked for existence.
    ///
    /// # Errors
    ///
    /// Returns [`OcrError::InvalidManifest`] when the manifest is invalid or
    /// required dictionaries are missing, and [`OcrError::ModelLoad`] when
    /// files fail verification or ONNX Runtime loading.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use std::path::Path;
    /// # use vtrans_models::ModelManifest;
    /// # use vtrans_ocr::PaddleOcrProvider;
    /// let manifest = ModelManifest::from_path(
    ///     Path::new("src-tauri/resources/models/manifest.json"),
    /// )?;
    /// let provider = PaddleOcrProvider::from_manifest_dir(
    ///     &manifest,
    ///     Path::new("src-tauri/resources/models"),
    /// )?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[tracing::instrument(skip(manifest), fields(models_dir = %models_dir.display()))]
    pub fn from_manifest_dir(
        manifest: &ModelManifest,
        models_dir: &Path,
    ) -> Result<Self, OcrError> {
        verify_manifest_files(manifest, models_dir)?;
        Self::load(manifest, models_dir)
    }

    /// Load the provider from a [`ModelManager`].
    ///
    /// Only the OCR entries (detection, recognition, and their dictionaries)
    /// are verified before loading; translation models are not required.
    ///
    /// # Errors
    ///
    /// Returns [`OcrError::ModelLoad`] when integrity verification fails or
    /// ONNX Runtime cannot load a model.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use vtrans_models::ModelManager;
    /// # use vtrans_ocr::PaddleOcrProvider;
    /// let manager = ModelManager::from_manifest_dir(
    ///     std::path::Path::new("src-tauri/resources/models"),
    /// )?;
    /// let provider = PaddleOcrProvider::from_manager(&manager)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[tracing::instrument(skip(manager))]
    pub fn from_manager(manager: &ModelManager) -> Result<Self, OcrError> {
        verify_manifest_files(manager.manifest(), manager.manifest_dir())?;
        Self::load(manager.manifest(), manager.manifest_dir())
    }

    /// Build the provider without a second integrity check.
    fn load(manifest: &ModelManifest, models_dir: &Path) -> Result<Self, OcrError> {
        manifest
            .validate()
            .map_err(|e| OcrError::InvalidManifest(e.to_string()))?;

        let started = Instant::now();
        let det_session = load_session(
            &models_dir.join(&manifest.ocr.det.path),
            &manifest.ocr.det.id,
        )?;
        let det = Arc::new(Detector::new(det_session)?);

        let preprocess = manifest.ocr.preprocess_params.clone();
        let rec_ja_path = &manifest.ocr.rec_ja.path;
        let rec_en_shared = manifest.ocr.rec_en.path == *rec_ja_path;
        let rec_multi_shared = manifest
            .ocr
            .rec_multi
            .as_ref()
            .is_some_and(|entry| entry.path == *rec_ja_path);

        let rec_ja = load_recognizer(
            models_dir,
            &manifest.ocr.rec_ja,
            manifest.ocr.dicts.get("ja"),
            &preprocess,
            "ja",
        )?;
        let rec_en = if rec_en_shared {
            tracing::debug!("rec_en shares the unified PP-OCRv6 recognition model");
            Arc::clone(&rec_ja)
        } else {
            load_recognizer(
                models_dir,
                &manifest.ocr.rec_en,
                manifest.ocr.dicts.get("en"),
                &preprocess,
                "en",
            )?
        };
        let rec_multi = match &manifest.ocr.rec_multi {
            Some(entry) => {
                let dict_key = dict_key_for_multi(&manifest.ocr.dicts).ok_or_else(|| {
                    OcrError::InvalidManifest(
                        "rec_multi requires a dict keyed by 'auto', 'multi', or 'zh-CN'"
                            .to_string(),
                    )
                })?;
                if rec_multi_shared {
                    tracing::debug!("rec_multi shares the unified PP-OCRv6 recognition model");
                    Some(Arc::clone(&rec_ja))
                } else {
                    Some(load_recognizer(
                        models_dir,
                        entry,
                        manifest.ocr.dicts.get(dict_key),
                        &preprocess,
                        "multi",
                    )?)
                }
            }
            None => None,
        };

        let mut supported_languages = vec![Language::Japanese, Language::English];
        if rec_multi.is_some() {
            supported_languages.push(Language::ChineseSimplified);
        }

        tracing::info!(
            det_id = %manifest.ocr.det.id,
            rec_ja_id = %manifest.ocr.rec_ja.id,
            rec_en_id = %manifest.ocr.rec_en.id,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "OCR provider initialized"
        );

        Ok(Self {
            det,
            rec_ja,
            rec_en,
            rec_multi,
            preprocess,
            supported_languages,
        })
    }

    /// Pick the recognizer for a language and the language to report.
    fn select_recognizer(
        &self,
        language: Language,
    ) -> Result<(Arc<Recognizer>, Option<Language>), OcrError> {
        let choice = choose_recognizer(language, self.rec_multi.is_some())?;
        match choice {
            RecognizerChoice::Japanese => Ok((Arc::clone(&self.rec_ja), Some(Language::Japanese))),
            RecognizerChoice::English => Ok((Arc::clone(&self.rec_en), Some(Language::English))),
            RecognizerChoice::Multi => {
                let multi = self.rec_multi.as_ref().ok_or_else(|| {
                    OcrError::Inference(format!(
                        "no recognition model configured for language {}",
                        language.code()
                    ))
                })?;
                let detected = match language {
                    Language::Auto => None,
                    Language::ChineseSimplified => Some(Language::ChineseSimplified),
                    Language::Japanese | Language::English => {
                        unreachable!("single-language choices never map to the multi model")
                    }
                };
                Ok((Arc::clone(multi), detected))
            }
        }
    }
}

/// Which recognition model slot to use for a requested language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecognizerChoice {
    /// Japanese single-language model.
    Japanese,
    /// English single-language model.
    English,
    /// Multi-language model (also serves `zh-CN` and `auto`).
    Multi,
}

/// Pick the recognizer slot for a language.
///
/// `auto` requires the multi-language model: silently falling back to a
/// single-language model would recognize the wrong script (e.g. the Japanese
/// model on English text) without the caller knowing. When no multi-language
/// model is configured, `auto` returns [`OcrError::Inference`] with an
/// actionable message instead.
fn choose_recognizer(language: Language, has_multi: bool) -> Result<RecognizerChoice, OcrError> {
    match language {
        Language::Japanese => Ok(RecognizerChoice::Japanese),
        Language::English => Ok(RecognizerChoice::English),
        Language::ChineseSimplified | Language::Auto if has_multi => Ok(RecognizerChoice::Multi),
        Language::ChineseSimplified => Err(OcrError::Inference(format!(
            "no recognition model configured for language {}",
            language.code()
        ))),
        Language::Auto => Err(OcrError::Inference(
            "auto language detection requires a multi-language recognition model; \
             please select a language manually"
                .to_string(),
        )),
    }
}

#[async_trait]
impl OcrProvider for PaddleOcrProvider {
    /// Stable provider identifier used in logs and UI.
    fn id(&self) -> &'static str {
        "pp-ocr"
    }

    /// Languages configured by the loaded manifest.
    fn supported_languages(&self) -> &[Language] {
        &self.supported_languages
    }

    #[tracing::instrument(
        skip(self, image, cancel),
        fields(
            language = %options.language.code(),
            region = %format!("{}x{}", region.width, region.height)
        )
    )]
    async fn recognize(
        &self,
        image: &CapturedImage,
        region: &ScreenRegion,
        options: &OcrOptions,
        cancel: CancellationToken,
    ) -> Result<OcrResult, OcrError> {
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        let (recognizer, detected_language) = match self.select_recognizer(options.language) {
            Ok(selected) => selected,
            Err(error) => {
                tracing::warn!(
                    language = %options.language.code(),
                    error = %error,
                    "recognizer selection failed"
                );
                return Err(error);
            }
        };
        let run_options = Arc::new(
            RunOptions::new()
                .map_err(|e| OcrError::OrtRuntime(format!("create ONNX run options: {e}")))?,
        );
        let run_options_for_task = Arc::clone(&run_options);
        let det = Arc::clone(&self.det);
        let preprocess = self.preprocess.clone();
        let options = options.clone();
        let cancel_for_task = cancel.clone();
        let image = image.clone();
        let region = region.clone();

        let handle = tokio::task::spawn_blocking(move || {
            run_ocr_pipeline(
                det,
                recognizer,
                image,
                region,
                preprocess,
                options,
                detected_language,
                cancel_for_task,
                run_options_for_task,
            )
        });

        tokio::select! {
            result = handle => match result {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!(error = %e, "OCR blocking task panicked");
                    Err(OcrError::Inference(format!("OCR task failed: {e}")))
                }
            },
            () = cancel.cancelled() => {
                let _ = run_options.terminate();
                tracing::warn!("OCR cancelled and ONNX run terminated");
                Err(OcrError::Cancelled)
            }
        }
    }
}

/// Execute the full OCR pipeline on a blocking thread.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::needless_pass_by_value
)]
fn run_ocr_pipeline(
    det: Arc<Detector>,
    recognizer: Arc<Recognizer>,
    image: CapturedImage,
    region: ScreenRegion,
    preprocess: PreprocessParams,
    options: OcrOptions,
    detected_language: Option<Language>,
    cancel: CancellationToken,
    run_options: Arc<RunOptions>,
) -> Result<OcrResult, OcrError> {
    let started = Instant::now();
    if cancel.is_cancelled() {
        return Err(OcrError::Cancelled);
    }
    let rgb = rgb_region(&image, &region)?;

    let det_input = det_preprocess(&rgb, &preprocess)?;
    if cancel.is_cancelled() {
        return Err(OcrError::Cancelled);
    }
    let probability = det.run(&det_input.tensor, &run_options)?;
    if cancel.is_cancelled() {
        return Err(OcrError::Cancelled);
    }

    let params = DetectionParams {
        threshold: preprocess.det_threshold,
        box_threshold: preprocess.box_threshold,
        max_candidates: preprocess.max_candidates,
        min_box_size: preprocess.min_box_size,
        unclip_ratio: preprocess.unclip_ratio,
    };
    let boxes = boxes_from_map(
        &probability,
        params,
        det_input.ratio_x,
        det_input.ratio_y,
        rgb.width(),
        rgb.height(),
    );
    tracing::debug!(box_count = boxes.len(), "text boxes detected");
    if boxes.is_empty() {
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            "OCR completed without text"
        );
        return Ok(OcrResult::empty());
    }

    let boxes = sort_boxes(boxes, &options);
    let mut recognized_lines = Vec::with_capacity(boxes.len());
    for (reading_order, box_) in boxes.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        let vertical = options.detect_vertical && box_.height > box_.width * 1.5;
        let line_width = if vertical { box_.height } else { box_.width };
        let line_height = if vertical { box_.width } else { box_.height };
        // Warp to the line's proportional width at the fixed recognition
        // height. `Recognizer::run` recognizes the crop at its natural width
        // (PP-OCRv6 rec input has a dynamic width dimension) and only falls
        // back to overlapping chunks for pathological ultra-wide crops. The
        // crop width is bounded by the input image dimensions because
        // detected boxes are clipped to the image bounds.
        let rec_height = preprocess.rec_input_height;
        let target_width = ((line_width / line_height.max(1.0)) * rec_height as f32)
            .round()
            .max(rec_height as f32) as u32;
        let crop = warp_perspective(&rgb, box_.polygon, target_width, rec_height);
        let crop = if vertical { rotate_90_cw(&crop) } else { crop };
        let line = recognizer.run(&crop, &run_options)?;
        tracing::debug!(
            reading_order,
            confidence = line.confidence,
            text_len = line.text.chars().count(),
            box_width = box_.width,
            box_height = box_.height,
            crop_width = crop.width(),
            crop_height = crop.height(),
            "line recognized"
        );
        recognized_lines.push((box_.clone(), line));
    }

    let total_lines = recognized_lines.len();
    let recognized_lines = recognized_lines
        .into_iter()
        .filter(|(_, line)| line.confidence >= options.min_confidence)
        .collect::<Vec<_>>();
    tracing::debug!(
        total = total_lines,
        kept = recognized_lines.len(),
        dropped = total_lines - recognized_lines.len(),
        min_confidence = options.min_confidence,
        "confidence filter applied"
    );
    let lines: Vec<OcrLine> = recognized_lines
        .iter()
        .enumerate()
        .map(|(index, (box_, line))| {
            OcrLine::new(line.text.clone(), line.confidence, box_.polygon, index)
        })
        .collect();
    let merged_text = merge_lines(&lines);
    let detected_language = if lines.is_empty() {
        None
    } else {
        detected_language
    };
    let elapsed_ms = started.elapsed().as_millis() as u64;
    tracing::info!(lines = lines.len(), elapsed_ms, "OCR completed");
    Ok(OcrResult {
        lines,
        merged_text,
        detected_language,
        elapsed_ms,
    })
}

/// Load an ONNX session with CPU execution and graph optimization.
fn load_session(path: &Path, id: &str) -> Result<Session, OcrError> {
    let started = Instant::now();
    let builder = Session::builder().map_err(|e| {
        tracing::error!(model_id = id, error = %e, "ONNX session builder failed");
        OcrError::ModelLoad(e.to_string())
    })?;
    let session = builder
        .with_execution_providers([ep::CPU::default().build()])
        .map_err(|e| {
            tracing::error!(model_id = id, error = %e, "failed to configure CPU execution provider");
            OcrError::ModelLoad(e.to_string())
        })?
        .with_intra_threads(2)
        .unwrap_or_else(|e| {
            tracing::warn!(
                model_id = id,
                error = %e,
                "failed to configure inference threads, using defaults"
            );
            e.recover()
        })
        .with_optimization_level(GraphOptimizationLevel::All)
        .unwrap_or_else(|e| {
            tracing::warn!(
                model_id = id,
                error = %e,
                "failed to enable full graph optimization, using default level"
            );
            e.recover()
        })
        .commit_from_file(path)
        .map_err(|e| {
            tracing::error!(
                model_id = id,
                path = %path.display(),
                error = %e,
                "ONNX model load failed"
            );
            OcrError::ModelLoad(e.to_string())
        })?;
    tracing::info!(
        model_id = id,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "ONNX session loaded"
    );
    Ok(session)
}

/// Load a recognition session plus its dictionary.
///
/// The character table embedded in the ONNX metadata (`character` key) takes
/// priority when present; otherwise the manifest dictionary file is used.
/// The final character table length is validated against the model output
/// class count at load time (fail-fast, guide §9.4).
fn load_recognizer(
    models_dir: &Path,
    entry: &ModelEntry,
    dict_relative: Option<&PathBuf>,
    preprocess: &PreprocessParams,
    language: &str,
) -> Result<Arc<Recognizer>, OcrError> {
    let dict_path = dict_relative.ok_or_else(|| {
        OcrError::InvalidManifest(format!("missing dictionary for language {language}"))
    })?;
    let dict_file = models_dir.join(dict_path);
    let session = load_session(&models_dir.join(&entry.path), &entry.id)?;
    let raw_lines = session
        .metadata()
        .ok()
        .and_then(|metadata| metadata.custom("character"))
        .map(|raw| {
            raw.lines()
                .map(|line| line.trim_end_matches('\r').to_string())
                .collect::<Vec<String>>()
        })
        // PP-OCRv6 ONNX models carry no character table; ort reports a
        // missing custom attribute as an empty string, so an empty embedded
        // table must not shadow the manifest dictionary file.
        .filter(|lines| !lines.is_empty())
        .or_else(|| load_dict_lines(&dict_file).ok())
        .ok_or_else(|| {
            OcrError::InvalidManifest(format!("no usable character table for language {language}"))
        })?;
    let dict_file_lines = raw_lines.len();
    let dict = build_character_dict(
        raw_lines,
        preprocess.rec_append_space,
        preprocess.rec_blank_index,
    );
    let output_shape = session
        .outputs()
        .first()
        .map(|output| {
            output
                .dtype()
                .tensor_shape()
                .map(|shape| shape.to_vec())
                .unwrap_or_default()
        })
        .ok_or_else(|| OcrError::InvalidManifest("recognition model has no outputs".to_string()))?;
    verify_rec_classes(
        &output_shape,
        &dict,
        dict_file_lines,
        preprocess,
        &dict_file,
    )?;
    Ok(Arc::new(Recognizer::new(
        session,
        dict,
        preprocess.rec_input_height,
        preprocess.rec_input_width,
    )?))
}

/// Read the raw lines of a dictionary file.
fn load_dict_lines(path: &Path) -> Result<Vec<String>, OcrError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        tracing::error!(path = %path.display(), error = %e, "dictionary load failed");
        OcrError::ModelLoad(e.to_string())
    })?;
    let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
    let lines: Vec<String> = content
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect();
    if lines.is_empty() {
        return Err(OcrError::InvalidManifest(format!(
            "dictionary is empty: {}",
            path.display()
        )));
    }
    tracing::info!(
        chars = lines.len(),
        path = %path.display(),
        "dictionary loaded"
    );
    Ok(lines)
}

/// Build the final CTC character table for a PP-OCR recognition model.
///
/// Follows the PP-OCR `CTCLabelDecode` convention: append a trailing space
/// when `append_space` is set (space is not in the character file for these
/// models) and insert the blank token at `blank_index` when the slot is not
/// already blank. The defaults for PP-OCRv6 are `append_space = true` and
/// `blank_index = 0`, yielding `1 + 18708 + 1 = 18710` classes.
fn build_character_dict(
    mut lines: Vec<String>,
    append_space: bool,
    blank_index: usize,
) -> Vec<String> {
    if append_space {
        lines.push(' '.to_string());
    }
    if blank_index <= lines.len()
        && lines
            .get(blank_index)
            .is_some_and(|entry| !entry.is_empty())
    {
        lines.insert(blank_index, String::new());
    }
    lines
}

/// Fail-fast class-count check for a recognition model (guide §9.4).
///
/// The model output's last dimension must equal the final character table
/// length. When the output shape is fully dynamic the static check is
/// skipped; the runtime decode check in [`Recognizer`] still applies.
fn verify_rec_classes(
    output_shape: &[i64],
    dict: &[String],
    dict_file_lines: usize,
    preprocess: &PreprocessParams,
    dict_path: &Path,
) -> Result<(), OcrError> {
    let Some(&classes) = output_shape.last() else {
        return Ok(());
    };
    if classes <= 0 || classes as usize == dict.len() {
        return Ok(());
    }
    Err(OcrError::InvalidManifest(format!(
        "recognition class count mismatch: model output shape {output_shape:?} has {classes} classes, \
         character table has {} entries (dictionary file lines: {dict_file_lines}, \
         append_space: {}, blank_index: {}, dictionary: {})",
        dict.len(),
        preprocess.rec_append_space,
        preprocess.rec_blank_index,
        dict_path.display()
    )))
}

/// Find the dictionary key used by the optional multi-language model.
fn dict_key_for_multi(dicts: &HashMap<String, PathBuf>) -> Option<&'static str> {
    ["auto", "multi", "zh-CN"]
        .into_iter()
        .find(|key| dicts.contains_key(*key))
}

/// Verify model hashes and dictionary existence before loading.
fn verify_manifest_files(manifest: &ModelManifest, models_dir: &Path) -> Result<(), OcrError> {
    manifest
        .validate()
        .map_err(|e| OcrError::InvalidManifest(e.to_string()))?;
    verify_dict("ja", &manifest.ocr.dicts, models_dir)?;
    verify_dict("en", &manifest.ocr.dicts, models_dir)?;
    if manifest.ocr.rec_multi.is_some() && dict_key_for_multi(&manifest.ocr.dicts).is_none() {
        return Err(OcrError::InvalidManifest(
            "rec_multi requires a dict keyed by 'auto', 'multi', or 'zh-CN'".to_string(),
        ));
    }
    verify_entry(&manifest.ocr.det, models_dir)?;
    verify_entry(&manifest.ocr.rec_ja, models_dir)?;
    verify_entry(&manifest.ocr.rec_en, models_dir)?;
    if let Some(entry) = &manifest.ocr.rec_multi {
        verify_entry(entry, models_dir)?;
    }
    Ok(())
}

fn verify_entry(entry: &ModelEntry, models_dir: &Path) -> Result<(), OcrError> {
    vtrans_models::verify::verify_entry(models_dir, entry).map_err(|e| {
        tracing::error!(
            model_id = %entry.id,
            error = %e,
            "model integrity check failed"
        );
        OcrError::ModelLoad(e.to_string())
    })
}

fn verify_dict(
    language: &str,
    dicts: &HashMap<String, PathBuf>,
    models_dir: &Path,
) -> Result<(), OcrError> {
    let relative = dicts.get(language).ok_or_else(|| {
        tracing::error!(language, "missing dictionary entry");
        OcrError::InvalidManifest(format!("missing dictionary for language {language}"))
    })?;
    let path = models_dir.join(relative);
    if !path.exists() {
        tracing::error!(
            language,
            path = %path.display(),
            "dictionary file not found"
        );
        return Err(OcrError::InvalidManifest(format!(
            "dictionary file not found: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn dict_key_for_multi_prefers_auto() {
        let mut dicts = HashMap::new();
        dicts.insert("auto".to_string(), PathBuf::from("d.txt"));
        dicts.insert("multi".to_string(), PathBuf::from("m.txt"));
        assert_eq!(dict_key_for_multi(&dicts), Some("auto"));
    }

    #[test]
    fn dict_key_for_multi_falls_back() {
        let mut dicts = HashMap::new();
        dicts.insert("zh-CN".to_string(), PathBuf::from("d.txt"));
        assert_eq!(dict_key_for_multi(&dicts), Some("zh-CN"));
    }

    #[test]
    fn dict_key_for_multi_missing() {
        let dicts = HashMap::new();
        assert!(dict_key_for_multi(&dicts).is_none());
    }

    #[test]
    fn build_character_dict_matches_ppocrv6_class_count() {
        // 18708 dictionary lines + blank at index 0 + appended space = 18710
        // classes, matching the v6 rec ONNX output (inspect_report.json).
        let embedded: Vec<String> = (0..18_708).map(|index| index.to_string()).collect();
        let dict = build_character_dict(embedded, true, 0);
        assert_eq!(dict.len(), 18_710);
        assert_eq!(dict[0], "");
        assert_eq!(dict.last().map(String::as_str), Some(" "));
    }

    #[test]
    fn build_character_dict_respects_append_space_and_blank_index() {
        let lines = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            build_character_dict(lines.clone(), false, 0),
            vec!["", "a", "b"]
        );
        assert_eq!(
            build_character_dict(lines.clone(), true, 0),
            vec!["", "a", "b", " "]
        );
        // blank_index = 1 moves the blank into the middle of the table.
        assert_eq!(
            build_character_dict(lines, true, 1),
            vec!["a", "", "b", " "]
        );
    }

    #[test]
    fn load_dict_lines_reads_raw_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dict.txt");
        std::fs::write(&path, "a\nb\n").unwrap();
        assert_eq!(load_dict_lines(&path).unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn load_dict_lines_rejects_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dict.txt");
        std::fs::write(&path, "").unwrap();
        assert!(matches!(
            load_dict_lines(&path),
            Err(OcrError::InvalidManifest(_))
        ));
    }

    #[test]
    fn verify_rec_classes_accepts_matching_count() {
        let dict = build_character_dict(vec!["a".to_string(), "b".to_string()], true, 0);
        let preprocess = PreprocessParams {
            image_size: (640, 640),
            mean: [0.485; 3],
            std: [0.229; 3],
            det_threshold: 0.2,
            unclip_ratio: 1.4,
            box_threshold: 0.45,
            max_candidates: 3000,
            min_box_size: 3.0,
            rec_input_height: 48,
            rec_input_width: 320,
            rec_append_space: true,
            rec_blank_index: 0,
        };
        assert!(verify_rec_classes(&[1, 40, 4], &dict, 2, &preprocess, Path::new("d.txt")).is_ok());
    }

    #[test]
    fn verify_rec_classes_fails_fast_with_diagnostics() {
        let dict = build_character_dict(vec!["a".to_string(), "b".to_string()], true, 0);
        let preprocess = PreprocessParams {
            image_size: (640, 640),
            mean: [0.485; 3],
            std: [0.229; 3],
            det_threshold: 0.2,
            unclip_ratio: 1.4,
            box_threshold: 0.45,
            max_candidates: 3000,
            min_box_size: 3.0,
            rec_input_height: 48,
            rec_input_width: 320,
            rec_append_space: true,
            rec_blank_index: 0,
        };
        let error = verify_rec_classes(
            &[1, 40, 5],
            &dict,
            2,
            &preprocess,
            Path::new("ocr/ppocrv6_dict.txt"),
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("class count mismatch"));
        assert!(message.contains("[1, 40, 5]"));
        assert!(message.contains("5 classes"));
        assert!(message.contains("dictionary file lines: 2"));
        assert!(message.contains("append_space: true"));
        assert!(message.contains("blank_index: 0"));
        assert!(message.contains("ocr/ppocrv6_dict.txt"));
    }

    #[test]
    fn verify_rec_classes_skips_dynamic_dimension() {
        let dict = build_character_dict(vec!["a".to_string()], false, 0);
        let preprocess = PreprocessParams {
            image_size: (640, 640),
            mean: [0.485; 3],
            std: [0.229; 3],
            det_threshold: 0.2,
            unclip_ratio: 1.4,
            box_threshold: 0.45,
            max_candidates: 3000,
            min_box_size: 3.0,
            rec_input_height: 48,
            rec_input_width: 320,
            rec_append_space: false,
            rec_blank_index: 0,
        };
        // A fully dynamic last dimension (<= 0) cannot be statically checked.
        assert!(
            verify_rec_classes(&[1, -1, -1], &dict, 1, &preprocess, Path::new("d.txt")).is_ok()
        );
    }

    #[test]
    fn auto_without_multi_returns_inference_error() {
        let error = choose_recognizer(Language::Auto, false).unwrap_err();
        assert!(matches!(error, OcrError::Inference(_)));
        assert!(error.to_string().contains("multi-language"));
        assert!(error.to_string().contains("select a language manually"));
    }

    #[test]
    fn auto_with_multi_uses_multi_model() {
        assert_eq!(
            choose_recognizer(Language::Auto, true).unwrap(),
            RecognizerChoice::Multi
        );
    }

    #[test]
    fn explicit_languages_route_to_their_models() {
        assert_eq!(
            choose_recognizer(Language::Japanese, false).unwrap(),
            RecognizerChoice::Japanese
        );
        assert_eq!(
            choose_recognizer(Language::English, false).unwrap(),
            RecognizerChoice::English
        );
        assert_eq!(
            choose_recognizer(Language::ChineseSimplified, true).unwrap(),
            RecognizerChoice::Multi
        );
    }

    #[test]
    fn explicit_languages_ignore_multi_presence() {
        // Explicit single-language choices never consult the multi model.
        assert_eq!(
            choose_recognizer(Language::Japanese, true).unwrap(),
            RecognizerChoice::Japanese
        );
        assert_eq!(
            choose_recognizer(Language::English, true).unwrap(),
            RecognizerChoice::English
        );
    }

    #[test]
    fn chinese_simplified_without_multi_returns_error() {
        let error = choose_recognizer(Language::ChineseSimplified, false).unwrap_err();
        assert!(matches!(error, OcrError::Inference(_)));
        assert!(error.to_string().contains("zh-CN"));
    }
}
