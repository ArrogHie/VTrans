//! Local ONNX translation provider.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use ort::ep;
use ort::session::{builder::GraphOptimizationLevel, RunOptions, Session, SessionInputValue};
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

/// Validate a language pair for the local provider.
///
/// The local provider cannot auto-detect the source language; `Auto` is only
/// accepted when the manifest explicitly declares an `(Auto, target)` pair.
fn validate_local_pair(
    source: Language,
    target: Language,
    supported: &[(Language, Language)],
) -> Result<(), TranslationError> {
    validate_language_pair(source, target, supported)?;
    if source.is_auto() && !supported.contains(&(Language::Auto, target)) {
        return Err(TranslationError::UnsupportedPair {
            src: source,
            target,
        });
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelKind {
    /// Decoder-loop model: feed `decoder_input_ids` per step and read logits.
    Stepwise,
    /// Whole-graph generation model: feed generation params and read sequences.
    Generation,
}

/// A local ONNX translation provider.
///
/// The provider loads one encoder-decoder ONNX model and a Hugging Face
/// tokenizer (`tokenizer.json`) from a [`ModelManifest`]. The model performs
/// generation internally; cancellation is cooperative and can terminate an
/// in-flight ONNX run. Language pairs from the manifest are used
/// for validation only; the model itself must handle multilingual generation.
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
    num_beams: usize,
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
        let num_beams = normalize_num_beams(group.inference_params.num_beams);

        info!(
            model_id = %group.model.id,
            model_kind = ?io.kind,
            supported_pairs = group.supported_pairs.len(),
            max_length = group.max_length,
            num_beams,
            elapsed_ms = elapsed_millis(started),
            "local translation provider initialized"
        );

        Ok(Self {
            model_id: group.model.id.clone(),
            session: Arc::new(Mutex::new(session)),
            tokenizer: Arc::new(Mutex::new(tokenizer)),
            supported_pairs: group.supported_pairs.clone(),
            max_length: group.max_length,
            num_beams,
            special,
            io,
        })
    }
}

impl fmt::Debug for LocalTranslationProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalTranslationProvider")
            .field("model_id", &self.model_id)
            .field("model_kind", &self.io.kind)
            .field("supported_pairs", &self.supported_pairs)
            .field("max_length", &self.max_length)
            .field("num_beams", &self.num_beams)
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
        validate_local_pair(request.source, request.target, &self.supported_pairs)?;
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
        let num_beams = self.num_beams;
        let kind = self.io.kind;
        let special = self.special;
        let io = self.io.clone();
        let cancel_for_task = cancel.clone();
        let run_options_for_task = Arc::clone(&run_options);
        let job = GenerationJob {
            kind,
            session,
            tokenizer,
            text,
            max_length,
            num_beams,
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

/// Input/output names expected from an ONNX translation model.
#[derive(Debug, Clone)]
struct ModelIo {
    kind: ModelKind,
    input_ids: String,
    attention_mask: Option<String>,
    decoder_input_ids: Option<String>,
    logits: Option<String>,
    generation_params: Option<GenerationParams>,
    sequences_output: Option<String>,
}

/// Names of generation parameters for whole-graph models.
#[derive(Debug, Clone)]
struct GenerationParams {
    num_beams: String,
    min_length: String,
    max_length: String,
    length_penalty: String,
    repetition_penalty: String,
}

impl GenerationParams {
    fn probe(input_names: &[String]) -> Option<Self> {
        let find = |needle: &str| {
            input_names
                .iter()
                .find(|name| name.contains(needle))
                .cloned()
        };
        Some(Self {
            num_beams: find("num_beams")?,
            min_length: find("min_length")?,
            max_length: find("max_length")?,
            length_penalty: find("length_penalty")?,
            repetition_penalty: find("repetition_penalty")?,
        })
    }
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
        probe_io(&input_names, &output_names)
    }
}

/// Classify a translation model as generation or stepwise based on I/O names.
fn probe_io(input_names: &[String], output_names: &[String]) -> Result<ModelIo, TranslationError> {
    let input_ids = input_names
        .iter()
        .find(|name| name.contains("input_ids") && !name.contains("decoder"))
        .cloned()
        .ok_or_else(|| {
            TranslationError::ModelLoad(format!("translation model input names must include an encoder input_ids-like tensor, got {input_names:?}"))
        })?;
    let attention_mask = input_names
        .iter()
        .find(|name| name.contains("attention_mask"))
        .cloned();

    let generation_params = GenerationParams::probe(input_names);
    let sequences_output = output_names
        .iter()
        .find(|name| name.contains("sequences"))
        .cloned()
        .or_else(|| (output_names.len() == 1).then(|| output_names[0].clone()));

    if let (Some(generation_params), Some(sequences_output)) = (generation_params, sequences_output)
    {
        let attention_mask = attention_mask.ok_or_else(|| {
            TranslationError::ModelLoad(
                "generation translation model requires attention_mask input".to_string(),
            )
        })?;
        debug!(
            input_ids = %input_ids,
            attention_mask = ?attention_mask,
            sequences_output = %sequences_output,
            "detected generation translation model"
        );
        return Ok(ModelIo {
            kind: ModelKind::Generation,
            input_ids,
            attention_mask: Some(attention_mask),
            decoder_input_ids: None,
            logits: None,
            generation_params: Some(generation_params),
            sequences_output: Some(sequences_output),
        });
    }

    let decoder_input_ids = input_names
        .iter()
        .find(|name| name.contains("decoder_input_ids"))
        .cloned();
    let logits = output_names
        .iter()
        .find(|name| name.contains("logits"))
        .cloned()
        .or_else(|| output_names.first().cloned());

    if let (Some(decoder_input_ids), Some(logits)) = (decoder_input_ids, logits) {
        debug!(
            input_ids = %input_ids,
            attention_mask = ?attention_mask,
            decoder_input_ids = %decoder_input_ids,
            logits = %logits,
            "detected stepwise translation model"
        );
        return Ok(ModelIo {
            kind: ModelKind::Stepwise,
            input_ids,
            attention_mask,
            decoder_input_ids: Some(decoder_input_ids),
            logits: Some(logits),
            generation_params: None,
            sequences_output: None,
        });
    }

    Err(TranslationError::ModelLoad(format!(
        "unsupported translation model I/O; generation expects inputs \\
         [input_ids, attention_mask, num_beams, min_length, max_length, \\
         length_penalty, repetition_penalty] and a sequences output; stepwise \\
         expects [input_ids, attention_mask, decoder_input_ids] and a logits \\
         output; actual inputs {input_names:?}, outputs {output_names:?}"
    )))
}

/// Special token ids used to trim generated sequences.
#[derive(Debug, Clone, Copy)]
struct SpecialTokens {
    eos_id: i32,
    decoder_start_id: i32,
}

impl SpecialTokens {
    /// Resolve special token ids from a tokenizer.
    fn from_tokenizer(tokenizer: &Tokenizer) -> Self {
        let id_for = |tokens: &[&str]| -> Option<i32> {
            tokens.iter().find_map(|token| {
                tokenizer
                    .token_to_id(token)
                    .and_then(|id| i32::try_from(id).ok())
            })
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
    kind: ModelKind,
    session: Arc<Mutex<Session>>,
    tokenizer: Arc<Mutex<Tokenizer>>,
    text: String,
    max_length: usize,
    num_beams: usize,
    special: SpecialTokens,
    io: ModelIo,
    run_options: Arc<RunOptions>,
    cancel: CancellationToken,
}

/// Dispatch to the I/O contract detected at load time.
fn generate_translation(job: GenerationJob) -> Result<String, TranslationError> {
    match job.kind {
        ModelKind::Stepwise => run_stepwise_translation(job),
        ModelKind::Generation => run_generation_translation(job),
    }
}

/// Run decoder-loop inference one token at a time.
fn run_stepwise_translation(job: GenerationJob) -> Result<String, TranslationError> {
    let GenerationJob {
        kind: _,
        session,
        tokenizer,
        text,
        max_length,
        num_beams: _,
        special,
        io,
        run_options,
        cancel,
    } = job;
    if cancel.is_cancelled() {
        return Err(TranslationError::Cancelled);
    }

    let (source_ids, attention_mask) = tokenize_source(&tokenizer, &text, max_length)?;
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
        "stepwise translation generation finished"
    );
    decode_generated_ids(&tokenizer, &generated)
}

/// Run whole-graph generation and decode the first returned sequence.
fn run_generation_translation(job: GenerationJob) -> Result<String, TranslationError> {
    let GenerationJob {
        kind: _,
        session,
        tokenizer,
        text,
        max_length,
        num_beams,
        special,
        io,
        run_options,
        cancel,
    } = job;
    if cancel.is_cancelled() {
        return Err(TranslationError::Cancelled);
    }

    let (source_ids, attention_mask) = tokenize_source(&tokenizer, &text, max_length)?;
    if cancel.is_cancelled() {
        return Err(TranslationError::Cancelled);
    }

    let generation_params = io.generation_params.as_ref().ok_or_else(|| {
        TranslationError::Inference("generation model io is missing generation params".to_string())
    })?;
    let sequences_output = io.sequences_output.as_ref().ok_or_else(|| {
        TranslationError::Inference("generation model io is missing sequences output".to_string())
    })?;

    let mut session = session
        .lock()
        .map_err(|_| TranslationError::Inference("ONNX session mutex poisoned".to_string()))?;
    let mut inputs: Vec<(String, SessionInputValue<'_>)> = Vec::with_capacity(7);
    let input_shape = vec![1_usize, source_ids.len()];
    inputs.push((
        io.input_ids.clone(),
        Tensor::from_array((input_shape, source_ids.clone()))
            .map_err(|error| {
                TranslationError::Inference(format!("create encoder input tensor: {error}"))
            })?
            .into(),
    ));
    let attention_mask_name = io.attention_mask.as_ref().ok_or_else(|| {
        TranslationError::Inference("generation model io is missing attention_mask".to_string())
    })?;
    let mask_shape = vec![1_usize, attention_mask.len()];
    inputs.push((
        attention_mask_name.clone(),
        Tensor::from_array((mask_shape, attention_mask.clone()))
            .map_err(|error| {
                TranslationError::Inference(format!("create attention mask tensor: {error}"))
            })?
            .into(),
    ));

    let beams = i32::try_from(normalize_num_beams(num_beams))
        .map_err(|_| TranslationError::Inference("num_beams out of range".to_string()))?;
    let max_len = i32::try_from(max_length)
        .map_err(|_| TranslationError::Inference("max_length out of range".to_string()))?;
    push_i32_param(
        &mut inputs,
        &generation_params.num_beams,
        beams,
        "num_beams",
    )?;
    push_i32_param(&mut inputs, &generation_params.min_length, 0, "min_length")?;
    push_i32_param(
        &mut inputs,
        &generation_params.max_length,
        max_len,
        "max_length",
    )?;
    push_f32_param(
        &mut inputs,
        &generation_params.length_penalty,
        1.0,
        "length_penalty",
    )?;
    push_f32_param(
        &mut inputs,
        &generation_params.repetition_penalty,
        1.0,
        "repetition_penalty",
    )?;

    let outputs = session
        .run_with_options(inputs, &run_options)
        .map_err(|error| TranslationError::Inference(format!("ONNX inference failed: {error}")))?;
    let value = outputs.get(sequences_output.as_str()).ok_or_else(|| {
        TranslationError::Inference("translation model returned no sequences output".to_string())
    })?;
    let (shape, data) = value
        .try_extract_tensor::<i32>()
        .map_err(|error| TranslationError::Inference(format!("extract sequences: {error}")))?;
    let shape: Vec<usize> = shape
        .iter()
        .map(|&dimension| usize::try_from(dimension).unwrap_or(0))
        .collect();
    let sequence = extract_first_sequence(data, &shape)?;
    let sequence = truncate_at_eos(&sequence, special.eos_id);

    debug!(
        source_tokens = source_ids.len(),
        sequence_tokens = sequence.len(),
        "generation translation finished"
    );
    decode_generated_ids(&tokenizer, &sequence)
}

/// Push a scalar `i32` generation parameter tensor into the inputs vector.
fn push_i32_param(
    inputs: &mut Vec<(String, SessionInputValue<'_>)>,
    name: &str,
    value: i32,
    label: &str,
) -> Result<(), TranslationError> {
    inputs.push((
        name.to_string(),
        Tensor::from_array((vec![1_usize], vec![value]))
            .map_err(|error| {
                TranslationError::Inference(format!("create {label} tensor: {error}"))
            })?
            .into(),
    ));
    Ok(())
}

/// Push a scalar `f32` generation parameter tensor into the inputs vector.
fn push_f32_param(
    inputs: &mut Vec<(String, SessionInputValue<'_>)>,
    name: &str,
    value: f32,
    label: &str,
) -> Result<(), TranslationError> {
    inputs.push((
        name.to_string(),
        Tensor::from_array((vec![1_usize], vec![value]))
            .map_err(|error| {
                TranslationError::Inference(format!("create {label} tensor: {error}"))
            })?
            .into(),
    ));
    Ok(())
}

/// Tokenize source text and build its attention mask.
fn tokenize_source(
    tokenizer: &Mutex<Tokenizer>,
    text: &str,
    max_length: usize,
) -> Result<(Vec<i32>, Vec<i32>), TranslationError> {
    let tokenizer = tokenizer
        .lock()
        .map_err(|_| TranslationError::Inference("tokenizer mutex poisoned".to_string()))?;
    let encoding = tokenizer
        .encode(text, true)
        .map_err(|error| TranslationError::Inference(format!("tokenization failed: {error}")))?;
    let source_ids: Vec<i32> = encoding
        .get_ids()
        .iter()
        .map(|&id| {
            i32::try_from(id)
                .map_err(|_| TranslationError::Inference("token id out of range".to_string()))
        })
        .collect::<Result<_, _>>()?;
    if source_ids.is_empty() {
        return Err(TranslationError::Inference(
            "tokenizer produced no source ids".to_string(),
        ));
    }
    let source_ids = truncate_ids(&source_ids, max_length);
    let attention_mask = vec![1_i32; source_ids.len()];
    Ok((source_ids, attention_mask))
}

/// Decode generated ids and reject empty translations.
fn decode_generated_ids(
    tokenizer: &Mutex<Tokenizer>,
    ids: &[i32],
) -> Result<String, TranslationError> {
    let tokenizer = tokenizer
        .lock()
        .map_err(|_| TranslationError::Inference("tokenizer mutex poisoned".to_string()))?;
    let ids: Vec<u32> = ids
        .iter()
        .map(|&id| {
            u32::try_from(id)
                .map_err(|_| TranslationError::Inference("token id out of range".to_string()))
        })
        .collect::<Result<_, _>>()?;
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

/// Take the first sequence from a rank-3 `[batch, num_return_sequences, seq]`
/// output by reading the `[0][0]` row.
fn extract_first_sequence(data: &[i32], shape: &[usize]) -> Result<Vec<i32>, TranslationError> {
    match shape {
        [batch, return_sequences, seq_len] => {
            if *batch == 0 || *return_sequences == 0 || *seq_len == 0 {
                return Err(TranslationError::Inference(
                    "sequences output dimensions must be non-zero".to_string(),
                ));
            }
            let expected = batch
                .checked_mul(*return_sequences)
                .and_then(|value| value.checked_mul(*seq_len))
                .ok_or_else(|| {
                    TranslationError::Inference("sequences dimensions overflow".to_string())
                })?;
            if data.len() != expected {
                return Err(TranslationError::Inference(format!(
                    "sequences data length {} does not match shape {shape:?}",
                    data.len()
                )));
            }
            Ok(data[..*seq_len].to_vec())
        }
        _ => Err(TranslationError::Inference(
            "sequences output must be rank 3 [batch, num_return_sequences, max_length]".to_string(),
        )),
    }
}

/// Cut a generated sequence at the first EOS token (exclusive).
fn truncate_at_eos(ids: &[i32], eos_id: i32) -> Vec<i32> {
    match ids.iter().position(|&id| id == eos_id) {
        Some(position) => ids[..position].to_vec(),
        None => ids.to_vec(),
    }
}

/// Map manifest beam count to a value the graph accepts (0 or 1 = greedy).
fn normalize_num_beams(num_beams: usize) -> usize {
    usize::max(1, num_beams)
}

/// Run one decoder step and return the next token id.
fn run_one_step(
    session: &Mutex<Session>,
    io: &ModelIo,
    source_ids: &[i32],
    attention_mask: &[i32],
    generated: &[i32],
    run_options: &RunOptions,
) -> Result<i32, TranslationError> {
    let decoder_input_ids = io.decoder_input_ids.as_ref().ok_or_else(|| {
        TranslationError::Inference("stepwise model io is missing decoder_input_ids".to_string())
    })?;
    let logits = io.logits.as_ref().ok_or_else(|| {
        TranslationError::Inference("stepwise model io is missing logits output".to_string())
    })?;
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

    let mut inputs: Vec<(String, Tensor<i32>)> = vec![(io.input_ids.clone(), input_tensor)];
    if let Some(name) = &io.attention_mask {
        let mask_shape = vec![1_usize, attention_mask.len()];
        let mask_tensor =
            Tensor::from_array((mask_shape, attention_mask.to_vec())).map_err(|error| {
                TranslationError::Inference(format!("create attention mask tensor: {error}"))
            })?;
        inputs.push((name.clone(), mask_tensor));
    }
    inputs.push((decoder_input_ids.clone(), decoder_tensor));

    let outputs = session
        .run_with_options(inputs, run_options)
        .map_err(|error| TranslationError::Inference(format!("ONNX inference failed: {error}")))?;
    let value = outputs.get(logits.as_str()).ok_or_else(|| {
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
///
/// The last `vocab_size` elements are used, so the function is correct for
/// any logits layout whose final dimension is the vocabulary, including
/// shapes with leading batch dimensions.
fn select_next_token(logits: &[f32], shape: &[usize]) -> Result<i32, TranslationError> {
    let vocab_size = *shape.last().ok_or_else(|| {
        TranslationError::Inference("logits output has no dimensions".to_string())
    })?;
    if vocab_size == 0 {
        return Err(TranslationError::Inference(
            "logits output has zero vocab size".to_string(),
        ));
    }
    if logits.is_empty() || logits.len() % vocab_size != 0 {
        return Err(TranslationError::Inference(format!(
            "logits length {} is not a multiple of vocab size {vocab_size}",
            logits.len()
        )));
    }

    let row_start = logits.len() - vocab_size;
    let row = &logits[row_start..];
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
    i32::try_from(best_index)
        .map_err(|_| TranslationError::Inference("token id out of range".to_string()))
}

/// Truncate source tokens to the model's maximum sequence length.
fn truncate_ids(ids: &[i32], max_length: usize) -> Vec<i32> {
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
    fn select_next_token_uses_last_batch_row() {
        let logits = vec![
            0.1, 0.2, 0.3, // batch 0, seq 0
            0.4, 0.5, 0.6, // batch 0, seq 1
            0.7, 0.8, 0.9, // batch 1, seq 0
            0.2, 0.9, 0.1, // batch 1, seq 1
        ];
        let token = select_next_token(&logits, &[2, 1, 2, 3]).unwrap();
        assert_eq!(token, 1);
    }

    #[test]
    fn local_pair_requires_explicit_auto_source() {
        let supported = [(Language::English, Language::Japanese)];
        assert!(validate_local_pair(Language::English, Language::Japanese, &supported).is_ok());
        assert!(matches!(
            validate_local_pair(Language::Auto, Language::Japanese, &supported),
            Err(TranslationError::UnsupportedPair { .. })
        ));

        let supported_with_auto = [(Language::Auto, Language::Japanese)];
        assert!(
            validate_local_pair(Language::Auto, Language::Japanese, &supported_with_auto).is_ok()
        );
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

    #[test]
    fn probe_detects_generation_model() {
        let inputs = vec![
            "input_ids".to_string(),
            "attention_mask".to_string(),
            "num_beams".to_string(),
            "min_length".to_string(),
            "max_length".to_string(),
            "length_penalty".to_string(),
            "repetition_penalty".to_string(),
        ];
        let outputs = vec!["sequences".to_string()];
        let io = probe_io(&inputs, &outputs).unwrap();
        assert_eq!(io.kind, ModelKind::Generation);
        assert!(io.generation_params.is_some());
        assert!(io.sequences_output.is_some());
        assert!(io.decoder_input_ids.is_none());
    }

    #[test]
    fn probe_detects_stepwise_model() {
        let inputs = vec![
            "input_ids".to_string(),
            "attention_mask".to_string(),
            "decoder_input_ids".to_string(),
        ];
        let outputs = vec!["logits".to_string()];
        let io = probe_io(&inputs, &outputs).unwrap();
        assert_eq!(io.kind, ModelKind::Stepwise);
        assert!(io.decoder_input_ids.is_some());
        assert!(io.logits.is_some());
        assert!(io.generation_params.is_none());
    }

    #[test]
    fn probe_error_lists_both_supported_forms() {
        let inputs = vec!["input_ids".to_string()];
        let outputs = vec!["unexpected".to_string()];
        let error = probe_io(&inputs, &outputs).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("generation expects inputs"));
        assert!(message.contains("stepwise"));
        assert!(message.contains("input_ids"));
    }

    #[test]
    fn extract_first_sequence_takes_first_return_sequence() {
        let data = vec![1, 3, 7, 2, 5, 6, 7, 8, 9, 10, 11, 12];
        assert_eq!(
            extract_first_sequence(&data, &[2, 2, 3]).unwrap(),
            vec![1, 3, 7]
        );
    }

    #[test]
    fn extract_first_sequence_rejects_non_three_dimensional() {
        let error = extract_first_sequence(&[1, 3, 7, 4], &[2, 2]).unwrap_err();
        assert!(matches!(error, TranslationError::Inference(_)));
    }

    #[test]
    fn truncate_at_eos_stops_at_first_eos() {
        assert_eq!(truncate_at_eos(&[1, 3, 4, 2, 5], 2), vec![1, 3, 4]);
        assert_eq!(truncate_at_eos(&[1, 3, 4], 2), vec![1, 3, 4]);
    }

    #[test]
    fn normalize_num_beams_maps_zero_and_one_to_greedy() {
        assert_eq!(normalize_num_beams(0), 1);
        assert_eq!(normalize_num_beams(1), 1);
        assert_eq!(normalize_num_beams(4), 4);
    }

    #[test]
    fn validate_group_accepts_zero_beams_for_graph() {
        let mut group = make_translation_group(vec![(Language::English, Language::Japanese)], 128);
        group.inference_params.num_beams = 0;
        assert!(validate_group(&group).is_ok());
    }

    fn encode_varint(value: u64, out: &mut Vec<u8>) {
        let mut value = value;
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
    }

    fn encode_key(field: u32, wire_type: u8, out: &mut Vec<u8>) {
        encode_varint((u64::from(field) << 3) | u64::from(wire_type), out);
    }

    fn encode_varint_field(field: u32, value: u64, out: &mut Vec<u8>) {
        encode_key(field, 0, out);
        encode_varint(value, out);
    }

    fn encode_len_field(field: u32, bytes: &[u8], out: &mut Vec<u8>) {
        encode_key(field, 2, out);
        encode_varint(bytes.len() as u64, out);
        out.extend_from_slice(bytes);
    }

    fn encode_string_field(field: u32, value: &str, out: &mut Vec<u8>) {
        encode_len_field(field, value.as_bytes(), out);
    }

    fn encode_message_field(field: u32, value: &[u8], out: &mut Vec<u8>) {
        encode_len_field(field, value, out);
    }

    // Casts are correct: protobuf varints encode int32 as unsigned two's complement.
    #[allow(clippy::cast_sign_loss)]
    fn encode_tensor_int32(name: &str, dims: &[i64], data: &[i32]) -> Vec<u8> {
        let mut out = Vec::new();
        for &dim in dims {
            encode_varint_field(1, dim as u64, &mut out);
        }
        encode_varint_field(2, 6, &mut out); // TensorProto::INT32
        let mut payload = Vec::with_capacity(data.len() * 4);
        for &value in data {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        encode_len_field(9, &payload, &mut out); // raw_data
        encode_string_field(8, name, &mut out);
        out
    }

    // Casts are correct: protobuf varints encode int64 as unsigned two's complement.
    #[allow(clippy::cast_sign_loss)]
    fn encode_tensor_float(name: &str, dims: &[i64], data: &[f32]) -> Vec<u8> {
        let mut out = Vec::new();
        for &dim in dims {
            encode_varint_field(1, dim as u64, &mut out);
        }
        encode_varint_field(2, 1, &mut out); // TensorProto::FLOAT
        let mut payload = Vec::with_capacity(data.len() * 4);
        for value in data {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        encode_len_field(4, &payload, &mut out); // packed float_data
        encode_string_field(8, name, &mut out);
        out
    }

    // Casts are correct: protobuf varints encode int64 as unsigned two's complement.
    #[allow(clippy::cast_sign_loss)]
    fn encode_type(elem_type: i64, shape: &[Option<i64>]) -> Vec<u8> {
        let mut tensor = Vec::new();
        encode_varint_field(1, elem_type as u64, &mut tensor);
        if !shape.is_empty() {
            let mut shape_msg = Vec::new();
            for dim in shape {
                let mut dim_msg = Vec::new();
                match dim {
                    Some(value) => encode_varint_field(1, *value as u64, &mut dim_msg),
                    None => encode_string_field(2, "seq", &mut dim_msg),
                }
                encode_message_field(1, &dim_msg, &mut shape_msg);
            }
            encode_message_field(2, &shape_msg, &mut tensor);
        }
        let mut out = Vec::new();
        encode_message_field(1, &tensor, &mut out);
        out
    }

    fn encode_value_info(name: &str, type_msg: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        encode_string_field(1, name, &mut out);
        encode_message_field(2, type_msg, &mut out);
        out
    }

    fn encode_node(name: &str, op_type: &str, inputs: &[&str], outputs: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for input in inputs {
            encode_string_field(1, input, &mut out);
        }
        for output in outputs {
            encode_string_field(2, output, &mut out);
        }
        encode_string_field(3, name, &mut out);
        encode_string_field(4, op_type, &mut out);
        out
    }

    fn encode_model(graph: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        encode_varint_field(1, 8, &mut out); // ir_version
        let mut opset = Vec::new();
        encode_string_field(1, "ai.onnx", &mut opset);
        encode_varint_field(2, 17, &mut opset);
        encode_message_field(8, &opset, &mut out);
        encode_message_field(7, graph, &mut out);
        out
    }

    fn generation_model_bytes() -> Vec<u8> {
        let inputs = vec![
            encode_value_info("input_ids", &encode_type(6, &[Some(1), None])),
            encode_value_info("attention_mask", &encode_type(6, &[Some(1), None])),
            encode_value_info("num_beams", &encode_type(6, &[Some(1)])),
            encode_value_info("min_length", &encode_type(6, &[Some(1)])),
            encode_value_info("max_length", &encode_type(6, &[Some(1)])),
            encode_value_info("length_penalty", &encode_type(1, &[Some(1)])),
            encode_value_info("repetition_penalty", &encode_type(1, &[Some(1)])),
        ];
        let initializer = encode_tensor_int32("fixed_sequence", &[1, 1, 5], &[1, 3, 7, 4, 2]);
        let node = encode_node("identity", "Identity", &["fixed_sequence"], &["sequences"]);
        let output = encode_value_info("sequences", &encode_type(6, &[Some(1), Some(1), Some(5)]));
        let mut graph = Vec::new();
        encode_message_field(1, &node, &mut graph);
        encode_message_field(5, &initializer, &mut graph);
        for input in &inputs {
            encode_message_field(11, input, &mut graph);
        }
        encode_message_field(12, &output, &mut graph);
        encode_model(&graph)
    }

    fn stepwise_model_bytes() -> Vec<u8> {
        let inputs = vec![
            encode_value_info("input_ids", &encode_type(6, &[Some(1), None])),
            encode_value_info("attention_mask", &encode_type(6, &[Some(1), None])),
            encode_value_info("decoder_input_ids", &encode_type(6, &[Some(1), None])),
        ];
        let initializer =
            encode_tensor_float("fixed_logits", &[1, 1, 5], &[0.1, 0.2, 0.1, 0.9, 0.1]);
        let node = encode_node("identity", "Identity", &["fixed_logits"], &["logits"]);
        let output = encode_value_info("logits", &encode_type(1, &[Some(1), Some(1), Some(5)]));
        let mut graph = Vec::new();
        encode_message_field(1, &node, &mut graph);
        encode_message_field(5, &initializer, &mut graph);
        for input in &inputs {
            encode_message_field(11, input, &mut graph);
        }
        encode_message_field(12, &output, &mut graph);
        encode_model(&graph)
    }

    fn provider_from_dummy(session: Session, max_length: usize) -> LocalTranslationProvider {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tokenizer.json");
        let tokenizer = Tokenizer::from_file(path).unwrap();
        let special = SpecialTokens::from_tokenizer(&tokenizer);
        let io = ModelIo::from_session(&session).unwrap();
        LocalTranslationProvider {
            model_id: "dummy".to_string(),
            session: Arc::new(Mutex::new(session)),
            tokenizer: Arc::new(Mutex::new(tokenizer)),
            supported_pairs: vec![(Language::English, Language::Japanese)],
            max_length,
            num_beams: 4,
            special,
            io,
        }
    }

    #[tokio::test]
    async fn generation_dummy_model_translates() {
        let bytes = generation_model_bytes();
        let session = Session::builder()
            .unwrap()
            .commit_from_memory(&bytes)
            .unwrap();
        let provider = provider_from_dummy(session, 16);
        let request = TranslationRequest::new("hello", Language::English, Language::Japanese);
        let result = provider
            .translate(&request, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.translated_text, "hello world");
    }

    #[tokio::test]
    async fn stepwise_dummy_model_translates() {
        let bytes = stepwise_model_bytes();
        let session = Session::builder()
            .unwrap()
            .commit_from_memory(&bytes)
            .unwrap();
        let provider = provider_from_dummy(session, 2);
        let request = TranslationRequest::new("hello", Language::English, Language::Japanese);
        let result = provider
            .translate(&request, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.translated_text, "hello");
    }
}
