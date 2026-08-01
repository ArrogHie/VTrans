//! Local ONNX translation provider.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use ort::ep;
use ort::session::{builder::GraphOptimizationLevel, RunOptions, Session};
use ort::value::Tensor;
use tokenizers::Tokenizer;
use tokio::task::spawn_blocking;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use vtrans_core::error::TranslationError;
use vtrans_core::traits::TranslationProvider;
use vtrans_core::types::{Language, TranslationRequest, TranslationResult};
use vtrans_models::verify::verify_entry;
use vtrans_models::{ModelManager, ModelManifest, TranslationModelGroup};

use crate::validate::validate_language_pair;

/// A local ONNX translation provider.
///
/// The provider loads one encoder-decoder ONNX model and a Hugging Face
/// tokenizer (`tokenizer.json`) from a [`ModelManifest`]. Generation uses
/// greedy decoding with cooperative cancellation; beam search configuration
/// is accepted but not yet used.
///
/// Model files are verified with SHA-256 before loading. A model load
/// failure is reported as [`TranslationError::ModelLoad`] and never falls
/// back to an API provider.
///
/// # Example
///
/// ```no_run
/// # use vtrans_models::ModelManager;
/// # use vtrans_translation::LocalTranslationProvider;
/// let manager = ModelManager::from_manifest_dir(
///     std::path::Path::new("src-tauri/resources/models"),
/// )?;
/// let provider = LocalTranslationProvider::from_manager(&manager)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct LocalTranslationProvider {
    model_id: String,
    session: Arc<Mutex<Session>>,
    tokenizer: Arc<Mutex<Tokenizer>>,
    supported_pairs: Vec<(Language, Language)>,
    max_length: usize,
    special: SpecialTokens,
    io: ModelIo,
}

impl LocalTranslationProvider {
    /// Load the provider from a manifest, resolving relative paths from the
    /// current working directory.
    ///
    /// Application code should prefer [`from_manifest_dir`](Self::from_manifest_dir)
    /// or [`from_manager`](Self::from_manager) so paths do not depend on the
    /// process working directory.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::ModelLoad`] when the manifest has no
    /// translation group, files fail SHA-256 verification, the tokenizer
    /// cannot be loaded, or ONNX Runtime cannot load the model.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use vtrans_models::ModelManifest;
    /// # use vtrans_translation::LocalTranslationProvider;
    /// let manifest = ModelManifest::from_path(
    ///     std::path::Path::new("src-tauri/resources/models/manifest.json"),
    /// )?;
    /// let provider = LocalTranslationProvider::from_manifest(&manifest)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn from_manifest(manifest: &ModelManifest) -> Result<Self, TranslationError> {
        Self::from_manifest_dir(manifest, Path::new("."))
    }

    /// Load the provider from a manifest and an explicit models directory.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::ModelLoad`] when the manifest has no
    /// translation group, files fail SHA-256 verification, the tokenizer
    /// cannot be loaded, or ONNX Runtime cannot load the model.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use std::path::Path;
    /// # use vtrans_models::ModelManifest;
    /// # use vtrans_translation::LocalTranslationProvider;
    /// let manifest = ModelManifest::from_path(
    ///     Path::new("src-tauri/resources/models/manifest.json"),
    /// )?;
    /// let provider = LocalTranslationProvider::from_manifest_dir(
    ///     &manifest,
    ///     Path::new("src-tauri/resources/models"),
    /// )?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[tracing::instrument(skip(manifest), fields(models_dir = %models_dir.display()))]
    pub fn from_manifest_dir(
        manifest: &ModelManifest,
        models_dir: &Path,
    ) -> Result<Self, TranslationError> {
        let group = translation_group(manifest)?;
        validate_group(group)?;
        verify_translation_files(models_dir, group)?;
        Self::load(group, models_dir)
    }

    /// Load the provider from a [`ModelManager`].
    ///
    /// Only the translation model and tokenizer entries are verified before
    /// loading.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::ModelLoad`] when the manifest has no
    /// translation group, files fail SHA-256 verification, the tokenizer
    /// cannot be loaded, or ONNX Runtime cannot load the model.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use vtrans_models::ModelManager;
    /// # use vtrans_translation::LocalTranslationProvider;
    /// let manager = ModelManager::from_manifest_dir(
    ///     std::path::Path::new("src-tauri/resources/models"),
    /// )?;
    /// let provider = LocalTranslationProvider::from_manager(&manager)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[tracing::instrument(skip(manager))]
    pub fn from_manager(manager: &ModelManager) -> Result<Self, TranslationError> {
        Self::from_manifest_dir(manager.manifest(), manager.manifest_dir())
    }

    /// Load sessions and tokenizer after validation has passed.
    fn load(group: &TranslationModelGroup, models_dir: &Path) -> Result<Self, TranslationError> {
        let started = Instant::now();
        let tokenizer_path = models_dir.join(&group.tokenizer.path);
        let tokenizer = load_tokenizer(&tokenizer_path)?;
        let session = load_session(&models_dir.join(&group.model.path), &group.model.id)?;
        let io = ModelIo::from_session(&session)?;
        let special = SpecialTokens::from_tokenizer(&tokenizer);

        info!(
            model_id = %group.model.id,
            supported_pairs = group.supported_pairs.len(),
            max_length = group.max_length,
            elapsed_ms = elapsed_millis(started),
            "local translation provider initialized"
        );

        Ok(Self {
            model_id: group.model.id.clone(),
            session: Arc::new(Mutex::new(session)),
            tokenizer: Arc::new(Mutex::new(tokenizer)),
            supported_pairs: group.supported_pairs.clone(),
            max_length: group.max_length,
            special,
            io,
        })
    }
}

impl fmt::Debug for LocalTranslationProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalTranslationProvider")
            .field("model_id", &self.model_id)
            .field("supported_pairs", &self.supported_pairs)
            .field("max_length", &self.max_length)
            .field("session", &self.session)
            .field("tokenizer", &self.tokenizer)
            .field("special", &self.special)
            .field("io", &self.io)
            .finish()
    }
}

#[async_trait]
impl TranslationProvider for LocalTranslationProvider {
    /// Stable provider identifier used in logs and results.
    fn id(&self) -> &'static str {
        "local-onnx"
    }

    /// Pairs declared by the model manifest.
    fn supported_pairs(&self) -> &[(Language, Language)] {
        &self.supported_pairs
    }

    #[tracing::instrument(
        skip(self, request, cancel),
        fields(
            source = %request.source.code(),
            target = %request.target.code(),
            text_len = request.text.chars().count()
        )
    )]
    async fn translate(
        &self,
        request: &TranslationRequest,
        cancel: CancellationToken,
    ) -> Result<TranslationResult, TranslationError> {
        let started = Instant::now();
        validate_language_pair(request.source, request.target, &self.supported_pairs)?;
        if cancel.is_cancelled() {
            return Err(TranslationError::Cancelled);
        }

        let run_options = Arc::new(RunOptions::new().map_err(|error| {
            TranslationError::Inference(format!("create ONNX run options: {error}"))
        })?);
        let session = Arc::clone(&self.session);
        let tokenizer = Arc::clone(&self.tokenizer);
        let text = request.text.clone();
        let max_length = self.max_length;
        let special = self.special;
        let io = self.io.clone();
        let cancel_for_task = cancel.clone();
        let run_options_for_task = Arc::clone(&run_options);
        let job = GenerationJob {
            session,
            tokenizer,
            text,
            max_length,
            special,
            io,
            run_options: run_options_for_task,
            cancel: cancel_for_task,
        };

        let handle = spawn_blocking(move || generate_translation(job));

        let result = tokio::select! {
            result = handle => match result {
                Ok(result) => result,
                Err(error) => {
                    tracing::error!(error = %error, "local translation task panicked");
                    Err(TranslationError::Inference(format!(
                        "local translation task failed: {error}"
                    )))
                }
            },
            () = cancel.cancelled() => {
                let _ = run_options.terminate();
                warn!("local translation cancelled and ONNX run terminated");
                Err(TranslationError::Cancelled)
            }
        }?;

        let elapsed_ms = elapsed_millis(started);
        info!(
            provider_id = self.id(),
            model_id = %self.model_id,
            source = %request.source.code(),
            target = %request.target.code(),
            elapsed_ms,
            text_len = result.chars().count(),
            "translation completed"
        );
        Ok(TranslationResult::new(result, self.id(), elapsed_ms))
    }
}

/// Input/output names expected from an encoder-decoder ONNX model.
#[derive(Debug, Clone)]
struct ModelIo {
    input_ids: String,
    attention_mask: Option<String>,
    decoder_input_ids: String,
    logits: String,
}

impl ModelIo {
    /// Discover tensor names from the ONNX session metadata.
    fn from_session(session: &Session) -> Result<Self, TranslationError> {
        let input_names: Vec<String> = session
            .inputs()
            .iter()
            .map(|input| input.name().to_string())
            .collect();
        let output_names: Vec<String> = session
            .outputs()
            .iter()
            .map(|output| output.name().to_string())
            .collect();

        if input_names.is_empty() {
            return Err(TranslationError::ModelLoad(
                "translation model exposes no inputs".to_string(),
            ));
        }
        if output_names.is_empty() {
            return Err(TranslationError::ModelLoad(
                "translation model exposes no outputs".to_string(),
            ));
        }

        let input_ids = input_names
            .iter()
            .find(|name| name.contains("input_ids") && !name.contains("decoder"))
            .cloned()
            .ok_or_else(|| {
                TranslationError::ModelLoad(format!(
                    "translation model input names must include an encoder input_ids-like tensor, got {input_names:?}"
                ))
            })?;
        let decoder_input_ids = input_names
            .iter()
            .find(|name| name.contains("decoder_input_ids"))
            .cloned()
            .ok_or_else(|| {
                TranslationError::ModelLoad(format!(
                    "translation model input names must include decoder_input_ids, got {input_names:?}"
                ))
            })?;
        let attention_mask = input_names
            .iter()
            .find(|name| name.contains("attention_mask"))
            .cloned();
        let logits = output_names
            .iter()
            .find(|name| name.contains("logits"))
            .or_else(|| output_names.first())
            .cloned()
            .ok_or_else(|| {
                TranslationError::ModelLoad(
                    "translation model exposes no usable output".to_string(),
                )
            })?;

        debug!(
            input_ids = %input_ids,
            decoder_input_ids = %decoder_input_ids,
            attention_mask = ?attention_mask,
            logits = %logits,
            "translation model io names"
        );

        Ok(Self {
            input_ids,
            attention_mask,
            decoder_input_ids,
            logits,
        })
    }
}

/// Special token ids required by greedy generation.
#[derive(Debug, Clone, Copy)]
struct SpecialTokens {
    eos_id: i64,
    decoder_start_id: i64,
}

impl SpecialTokens {
    /// Resolve special token ids from a tokenizer.
    fn from_tokenizer(tokenizer: &Tokenizer) -> Self {
        let id_for = |tokens: &[&str]| -> Option<i64> {
            tokens
                .iter()
                .find_map(|token| tokenizer.token_to_id(token))
                .map(i64::from)
        };
        let eos_id = id_for(&["</s>", "[EOS]", "<eos>", "<|endoftext|>", "<sep>"]).unwrap_or(2);
        let decoder_start_id =
            id_for(&["</s>", "<s>", "[BOS]", "<bos>", "<|startoftext|>"]).unwrap_or(eos_id);
        Self {
            eos_id,
            decoder_start_id,
        }
    }
}

/// Load and parse a Hugging Face tokenizer.
fn load_tokenizer(path: &Path) -> Result<Tokenizer, TranslationError> {
    let tokenizer = Tokenizer::from_file(path).map_err(|error| {
        warn!(
            path = %path.display(),
            error = %error,
            "translation tokenizer load failed"
        );
        TranslationError::ModelLoad(format!(
            "failed to load translation tokenizer {}: {error}",
            path.display()
        ))
    })?;
    info!(path = %path.display(), "translation tokenizer loaded");
    Ok(tokenizer)
}

/// Load an ONNX session with CPU execution and graph optimization.
fn load_session(path: &Path, model_id: &str) -> Result<Session, TranslationError> {
    let started = Instant::now();
    let builder = Session::builder().map_err(|error| {
        warn!(model_id, error = %error, "ONNX session builder failed");
        TranslationError::ModelLoad(error.to_string())
    })?;
    let session = builder
        .with_execution_providers([ep::CPU::default().build()])
        .map_err(|error| {
            warn!(
                model_id,
                error = %error,
                "failed to configure CPU execution provider"
            );
            TranslationError::ModelLoad(error.to_string())
        })?
        .with_intra_threads(2)
        .unwrap_or_else(|error| {
            warn!(
                model_id,
                error = %error,
                "failed to configure inference threads, using defaults"
            );
            error.recover()
        })
        .with_optimization_level(GraphOptimizationLevel::All)
        .unwrap_or_else(|error| {
            warn!(
                model_id,
                error = %error,
                "failed to enable full graph optimization, using default level"
            );
            error.recover()
        })
        .commit_from_file(path)
        .map_err(|error| {
            warn!(
                model_id,
                path = %path.display(),
                error = %error,
                "ONNX translation model load failed"
            );
            TranslationError::ModelLoad(error.to_string())
        })?;
    info!(
        model_id,
        elapsed_ms = elapsed_millis(started),
        "ONNX translation session loaded"
    );
    Ok(session)
}

/// Return the translation group or a clear load error.
fn translation_group(manifest: &ModelManifest) -> Result<&TranslationModelGroup, TranslationError> {
    manifest
        .validate()
        .map_err(|error| TranslationError::ModelLoad(format!("invalid model manifest: {error}")))?;
    manifest.translation.as_ref().ok_or_else(|| {
        TranslationError::ModelLoad(
            "manifest does not contain a translation model group".to_string(),
        )
    })
}

/// Validate translation model group fields.
fn validate_group(group: &TranslationModelGroup) -> Result<(), TranslationError> {
    if group.supported_pairs.is_empty() {
        return Err(TranslationError::ModelLoad(
            "translation model group lists no supported pairs".to_string(),
        ));
    }
    if group
        .supported_pairs
        .iter()
        .any(|&(_, target)| target.is_auto())
    {
        return Err(TranslationError::ModelLoad(
            "translation model group cannot use Auto as a target language".to_string(),
        ));
    }
    if group.max_length == 0 {
        return Err(TranslationError::ModelLoad(
            "translation model group max_length must be greater than zero".to_string(),
        ));
    }
    if group.inference_params.num_beams == 0 {
        return Err(TranslationError::ModelLoad(
            "translation model group num_beams must be greater than zero".to_string(),
        ));
    }
    if group.inference_params.max_batch_size == 0 {
        return Err(TranslationError::ModelLoad(
            "translation model group max_batch_size must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

/// Verify translation model and tokenizer files before loading.
fn verify_translation_files(
    models_dir: &Path,
    group: &TranslationModelGroup,
) -> Result<(), TranslationError> {
    verify_entry(models_dir, &group.model).map_err(|error| {
        TranslationError::ModelLoad(format!("translation model integrity check failed: {error}"))
    })?;
    verify_entry(models_dir, &group.tokenizer).map_err(|error| {
        TranslationError::ModelLoad(format!(
            "translation tokenizer integrity check failed: {error}"
        ))
    })?;
    Ok(())
}

/// Everything needed to run one local generation job on a blocking thread.
struct GenerationJob {
    session: Arc<Mutex<Session>>,
    tokenizer: Arc<Mutex<Tokenizer>>,
    text: String,
    max_length: usize,
    special: SpecialTokens,
    io: ModelIo,
    run_options: Arc<RunOptions>,
    cancel: CancellationToken,
}

/// Tokenize, run greedy generation, and decode the translated text.
fn generate_translation(job: GenerationJob) -> Result<String, TranslationError> {
    let GenerationJob {
        session,
        tokenizer,
        text,
        max_length,
        special,
        io,
        run_options,
        cancel,
    } = job;
    if cancel.is_cancelled() {
        return Err(TranslationError::Cancelled);
    }

    let source_ids = {
        let tokenizer = tokenizer
            .lock()
            .map_err(|_| TranslationError::Inference("tokenizer mutex poisoned".to_string()))?;
        let encoding = tokenizer.encode(text.as_str(), true).map_err(|error| {
            TranslationError::Inference(format!("tokenization failed: {error}"))
        })?;
        encoding
            .get_ids()
            .iter()
            .map(|&id| i64::from(id))
            .collect::<Vec<i64>>()
    };

    if source_ids.is_empty() {
        return Err(TranslationError::Inference(
            "tokenizer produced no source ids".to_string(),
        ));
    }
    let source_ids = truncate_ids(&source_ids, max_length);
    let attention_mask = vec![1_i64; source_ids.len()];
    let mut generated = vec![special.decoder_start_id];

    while generated.len() < max_length {
        if cancel.is_cancelled() {
            return Err(TranslationError::Cancelled);
        }
        let next = run_one_step(
            &session,
            &io,
            &source_ids,
            &attention_mask,
            &generated,
            &run_options,
        )?;
        if cancel.is_cancelled() {
            return Err(TranslationError::Cancelled);
        }
        generated.push(next);
        if next == special.eos_id {
            break;
        }
    }

    debug!(
        source_tokens = source_ids.len(),
        generated_tokens = generated.len(),
        "local translation generation finished"
    );

    let tokenizer = tokenizer
        .lock()
        .map_err(|_| TranslationError::Inference("tokenizer mutex poisoned".to_string()))?;
    let ids: Vec<u32> = generated
        .iter()
        .map(|&id| u32::try_from(id).unwrap_or(0))
        .collect();
    let translated = tokenizer
        .decode(&ids, true)
        .map_err(|error| TranslationError::Inference(format!("token decoding failed: {error}")))?;
    let translated = translated.trim().to_string();
    if translated.is_empty() {
        return Err(TranslationError::Inference(
            "decoder produced empty translation".to_string(),
        ));
    }
    Ok(translated)
}

/// Run one decoder step and return the next token id.
fn run_one_step(
    session: &Mutex<Session>,
    io: &ModelIo,
    source_ids: &[i64],
    attention_mask: &[i64],
    generated: &[i64],
    run_options: &RunOptions,
) -> Result<i64, TranslationError> {
    let mut session = session
        .lock()
        .map_err(|_| TranslationError::Inference("ONNX session mutex poisoned".to_string()))?;

    let input_shape = vec![1_usize, source_ids.len()];
    let input_tensor = Tensor::from_array((input_shape, source_ids.to_vec())).map_err(|error| {
        TranslationError::Inference(format!("create encoder input tensor: {error}"))
    })?;
    let decoder_shape = vec![1_usize, generated.len()];
    let decoder_tensor =
        Tensor::from_array((decoder_shape, generated.to_vec())).map_err(|error| {
            TranslationError::Inference(format!("create decoder input tensor: {error}"))
        })?;

    let mut inputs: Vec<(String, Tensor<i64>)> = vec![(io.input_ids.clone(), input_tensor)];
    if let Some(name) = &io.attention_mask {
        let mask_shape = vec![1_usize, attention_mask.len()];
        let mask_tensor =
            Tensor::from_array((mask_shape, attention_mask.to_vec())).map_err(|error| {
                TranslationError::Inference(format!("create attention mask tensor: {error}"))
            })?;
        inputs.push((name.clone(), mask_tensor));
    }
    inputs.push((io.decoder_input_ids.clone(), decoder_tensor));

    let outputs = session
        .run_with_options(inputs, run_options)
        .map_err(|error| TranslationError::Inference(format!("ONNX inference failed: {error}")))?;
    let value = outputs.get(io.logits.as_str()).ok_or_else(|| {
        TranslationError::Inference("translation model returned no logits output".to_string())
    })?;
    let (shape, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|error| TranslationError::Inference(format!("extract logits: {error}")))?;
    let shape: Vec<usize> = shape
        .iter()
        .map(|&dimension| usize::try_from(dimension).unwrap_or(0))
        .collect();
    let logits = data.to_vec();
    select_next_token(&logits, &shape)
}

/// Pick the argmax token from the last sequence position.
fn select_next_token(logits: &[f32], shape: &[usize]) -> Result<i64, TranslationError> {
    let vocab_size = *shape.last().ok_or_else(|| {
        TranslationError::Inference("logits output has no dimensions".to_string())
    })?;
    if vocab_size == 0 {
        return Err(TranslationError::Inference(
            "logits output has zero vocab size".to_string(),
        ));
    }
    let seq_len = if shape.len() >= 2 {
        shape[shape.len() - 2]
    } else {
        1
    };
    let expected_len = seq_len
        .checked_mul(vocab_size)
        .ok_or_else(|| TranslationError::Inference("logits dimensions overflow".to_string()))?;
    if logits.len() < expected_len {
        return Err(TranslationError::Inference(format!(
            "logits length {} is smaller than expected {expected_len}",
            logits.len()
        )));
    }

    let row_start = seq_len.saturating_sub(1).saturating_mul(vocab_size);
    let row = &logits[row_start..row_start + vocab_size];
    let best_index = row
        .iter()
        .enumerate()
        .fold(
            (0_usize, f32::NEG_INFINITY),
            |(best_index, best_value), (index, &value)| {
                if value > best_value {
                    (index, value)
                } else {
                    (best_index, best_value)
                }
            },
        )
        .0;
    i64::try_from(best_index)
        .map_err(|_| TranslationError::Inference("token id out of range".to_string()))
}

/// Truncate source tokens to the model's maximum sequence length.
fn truncate_ids(ids: &[i64], max_length: usize) -> Vec<i64> {
    if ids.len() <= max_length {
        ids.to_vec()
    } else {
        warn!(
            from = ids.len(),
            to = max_length,
            "source tokens truncated for local translation"
        );
        ids[..max_length].to_vec()
    }
}

/// Convert an `Instant` delta to milliseconds, saturating at `u64::MAX`.
fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn model_entry(id: &str, path: &str) -> vtrans_models::ModelEntry {
        vtrans_models::ModelEntry {
            id: id.to_string(),
            path: PathBuf::from(path),
            sha256: "abc".to_string(),
            size_bytes: 1,
        }
    }

    fn make_translation_group(
        supported_pairs: Vec<(Language, Language)>,
        max_length: usize,
    ) -> TranslationModelGroup {
        TranslationModelGroup {
            model: model_entry("trans-model", "translation/model.onnx"),
            tokenizer: model_entry("trans-tokenizer", "translation/tokenizer.json"),
            supported_pairs,
            max_length,
            inference_params: vtrans_models::InferenceParams {
                max_batch_size: 1,
                num_beams: 1,
            },
        }
    }

    fn manifest(translation: Option<TranslationModelGroup>) -> ModelManifest {
        ModelManifest {
            version: 1,
            ocr: vtrans_models::OcrModelGroup {
                det: model_entry("det", "ocr/det.onnx"),
                rec_ja: model_entry("rec-ja", "ocr/rec_ja.onnx"),
                rec_en: model_entry("rec-en", "ocr/rec_en.onnx"),
                rec_multi: None,
                dicts: std::collections::HashMap::new(),
                preprocess_params: vtrans_models::PreprocessParams {
                    image_size: (1, 1),
                    mean: [0.0; 3],
                    std: [1.0; 3],
                    det_threshold: 0.5,
                    unclip_ratio: 1.5,
                },
            },
            translation,
        }
    }

    #[test]
    fn missing_translation_group_is_model_load_error() {
        let manifest = manifest(None);
        let error = translation_group(&manifest).unwrap_err();
        assert!(matches!(error, TranslationError::ModelLoad(_)));
    }

    #[test]
    fn validate_group_rejects_empty_pairs() {
        let group = make_translation_group(Vec::new(), 128);
        assert!(validate_group(&group).is_err());
    }

    #[test]
    fn validate_group_rejects_auto_target() {
        let group = make_translation_group(vec![(Language::English, Language::Auto)], 128);
        assert!(validate_group(&group).is_err());
    }

    #[test]
    fn validate_group_rejects_zero_max_length() {
        let group = make_translation_group(vec![(Language::English, Language::Japanese)], 0);
        assert!(validate_group(&group).is_err());
    }

    #[test]
    fn truncate_ids_keeps_short_sequences() {
        assert_eq!(truncate_ids(&[1, 2, 3], 10), vec![1, 2, 3]);
    }

    #[test]
    fn truncate_ids_truncates_long_sequences() {
        assert_eq!(truncate_ids(&[1, 2, 3], 2), vec![1, 2]);
    }

    #[test]
    fn select_next_token_uses_last_row() {
        let logits = vec![
            0.1, 0.2, 0.3, // row 0
            0.4, 0.9, 0.2, // row 1
        ];
        let token = select_next_token(&logits, &[1, 2, 3]).unwrap();
        assert_eq!(token, 1);
    }

    #[test]
    fn select_next_token_rejects_short_logits() {
        let error = select_next_token(&[0.1, 0.2], &[1, 2, 3]).unwrap_err();
        assert!(matches!(error, TranslationError::Inference(_)));
    }

    #[test]
    fn tokenizer_loads_from_fixture() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tokenizer.json");
        let tokenizer = load_tokenizer(&path).unwrap();
        let special = SpecialTokens::from_tokenizer(&tokenizer);
        assert_eq!(special.eos_id, 2);
        assert!(special.decoder_start_id >= 0);
    }

    #[test]
    fn missing_tokenizer_is_model_load_error() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/missing-tokenizer.json");
        assert!(matches!(
            load_tokenizer(&path),
            Err(TranslationError::ModelLoad(_))
        ));
    }
}
