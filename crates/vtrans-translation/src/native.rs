//! Native multi-engine translation provider.
//!
//! [`NativeTranslationProvider`] implements [`vtrans_core::TranslationProvider`]
//! on top of the C++ bridge in `native/translation_bridge/`:
//!
//! * en → zh-CN via `Bergamot` (Marian INT8, `model.enzh.intgemm.alphas.bin`)
//! * ja → zh-CN via `CTranslate2` INT8 with `SentencePiece` encode/decode
//!
//! The provider holds one engine for its lifetime (integration guide
//! section 15). Construction is a blocking, heavy operation and must run
//! in `spawn_blocking`; the engine is shared and thread-safe afterwards.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use vtrans_core::error::TranslationError;
use vtrans_core::traits::TranslationProvider;
use vtrans_core::types::{Language, TranslationRequest, TranslationResult};
use vtrans_models::ModelManager;

use crate::ffi::{locate_library, NativeTranslator};
use crate::validate::validate_language_pair;

/// Provider identifier advertised by [`NativeTranslationProvider`] (decision A2).
pub const NATIVE_PROVIDER_ID: &str = "local-native";

/// Translation quality preset (decision A1).
///
/// The preset maps to beam sizes inside the native bridge: `Bergamot`
/// fast 1 / balanced 2 and `CTranslate2` fast 1 / balanced 4. The default is
/// [`Self::Fast`], matching `AppConfig.translation.quality`'s default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationQuality {
    /// Low latency, smaller beam (default).
    #[default]
    Fast,
    /// Higher quality, larger beam.
    Balanced,
}

impl TranslationQuality {
    /// Return the string form used by `AppConfig.translation.quality`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Balanced => "balanced",
        }
    }

    /// Bergamot beam size for this preset (kept in sync with
    /// `native/translation_bridge/translation_bridge.cpp`).
    #[must_use]
    pub const fn bergamot_beam_size(self) -> usize {
        match self {
            Self::Fast => 1,
            Self::Balanced => 2,
        }
    }

    /// `CTranslate2` beam size for this preset (integration guide section 7).
    #[must_use]
    pub const fn ctranslate2_beam_size(self) -> usize {
        match self {
            Self::Fast => 1,
            Self::Balanced => 4,
        }
    }

    /// Maximum source token budget shared by both engines
    /// (integration guide sections 7 and 9.3).
    #[must_use]
    pub const fn max_input_tokens(self) -> usize {
        256
    }
}

impl std::fmt::Display for TranslationQuality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TranslationQuality {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "fast" => Ok(Self::Fast),
            "balanced" => Ok(Self::Balanced),
            _ => Err("unknown translation quality: expected \"fast\" or \"balanced\""),
        }
    }
}

/// The two language pairs served by the native engines.
fn native_supported_pairs() -> Vec<(Language, Language)> {
    vec![
        (Language::English, Language::ChineseSimplified),
        (Language::Japanese, Language::ChineseSimplified),
    ]
}

/// Local multi-engine translation provider (`Bergamot` en-zh + `CTranslate2` ja-zh).
///
/// Loaded from a manifest v2 via [`Self::from_manager`]. The provider
/// rejects `Auto` sources (the caller resolves the concrete source
/// language before translating) and any pair outside en→zh-CN / ja→zh-CN.
///
/// # Example
///
/// ```no_run
/// # use vtrans_models::ModelManager;
/// # use vtrans_translation::{NativeTranslationProvider, TranslationQuality};
/// let manager = ModelManager::from_manifest_dir(
///     std::path::Path::new("src-tauri/resources/models"),
/// )?;
/// let provider = NativeTranslationProvider::from_manager(&manager)?
///     .with_quality(TranslationQuality::Balanced)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct NativeTranslationProvider {
    translator: Arc<NativeTranslator>,
    quality: TranslationQuality,
    supported_pairs: Vec<(Language, Language)>,
    model_id: String,
}

impl NativeTranslationProvider {
    /// Load the dual-engine provider from a manifest v2.
    ///
    /// Resolves the `Bergamot` and `CTranslate2` directories through
    /// [`ModelManager`], locates `translation_bridge.dll` next to the
    /// models directory (or in the packaged resources), and creates the
    /// engine with the default (Fast) quality preset.
    ///
    /// This is a blocking, heavy operation (model + tokenizer loading);
    /// call it from `spawn_blocking`, not from the async runtime directly.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::ModelLoad`] when the manifest has no
    /// translation section, the bridge DLL cannot be found or loaded, the
    /// ABI version mismatches, or the engine fails to initialize.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use vtrans_models::ModelManager;
    /// # use vtrans_translation::NativeTranslationProvider;
    /// let manager = ModelManager::from_manifest_dir(
    ///     std::path::Path::new("src-tauri/resources/models"),
    /// )?;
    /// let provider = NativeTranslationProvider::from_manager(&manager)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[tracing::instrument(skip(manager))]
    pub fn from_manager(manager: &ModelManager) -> Result<Self, TranslationError> {
        let translation = manager.manifest().translation.as_ref().ok_or_else(|| {
            TranslationError::ModelLoad(
                "manifest has no translation section (manifest v2 required)".to_string(),
            )
        })?;
        let en_zh = manager.en_zh_paths().ok_or_else(|| {
            TranslationError::ModelLoad("manifest has no en_zh engine paths".to_string())
        })?;
        let ja_zh = manager.ja_zh_paths().ok_or_else(|| {
            TranslationError::ModelLoad("manifest has no ja_zh engine paths".to_string())
        })?;

        // The bridge takes model *directories*; the manifest resolves the
        // first file of each engine family, whose parent is the package dir.
        let en_zh_dir = en_zh.model.parent().ok_or_else(|| {
            TranslationError::ModelLoad(format!(
                "en_zh model path has no parent: {}",
                en_zh.model.display()
            ))
        })?;
        let ja_zh_dir = ja_zh.model.parent().ok_or_else(|| {
            TranslationError::ModelLoad(format!(
                "ja_zh model path has no parent: {}",
                ja_zh.model.display()
            ))
        })?;

        let library_path = locate_library(manager.manifest_dir())?;
        let translator = NativeTranslator::load(&library_path, en_zh_dir, ja_zh_dir)?;
        let model_id = format!(
            "{}+{}",
            translation.engines.en_zh.model.id, translation.engines.ja_zh.model.id
        );

        info!(
            provider_id = NATIVE_PROVIDER_ID,
            model_id = %model_id,
            library = %library_path.display(),
            en_zh = %en_zh_dir.display(),
            ja_zh = %ja_zh_dir.display(),
            "native translation provider loaded"
        );
        Ok(Self {
            translator: Arc::new(translator),
            quality: TranslationQuality::default(),
            supported_pairs: native_supported_pairs(),
            model_id,
        })
    }

    /// Switch the quality preset of this provider.
    ///
    /// The bridge maps the preset to beam sizes (`Bergamot` 1/2,
    /// `CTranslate2` 1/4). When the preset changes to `Balanced`, the bridge
    /// rebuilds the Bergamot model, which is a blocking operation; call
    /// this once at provider assembly time, not per request.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::ModelLoad`] when the bridge cannot
    /// apply the preset.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use vtrans_models::ModelManager;
    /// # use vtrans_translation::{NativeTranslationProvider, TranslationQuality};
    /// let manager = ModelManager::from_manifest_dir(
    ///     std::path::Path::new("src-tauri/resources/models"),
    /// )?;
    /// let provider = NativeTranslationProvider::from_manager(&manager)?
    ///     .with_quality(TranslationQuality::Fast)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn with_quality(mut self, quality: TranslationQuality) -> Result<Self, TranslationError> {
        self.translator.set_quality(quality.as_str())?;
        info!(
            provider_id = NATIVE_PROVIDER_ID,
            model_id = %self.model_id,
            quality = %quality,
            "native translation quality preset applied"
        );
        self.quality = quality;
        Ok(self)
    }

    /// Current quality preset of this provider.
    #[must_use]
    pub const fn quality(&self) -> TranslationQuality {
        self.quality
    }

    /// Stable identifier of the loaded engine family (for logs and status).
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
}

#[async_trait]
impl TranslationProvider for NativeTranslationProvider {
    /// Stable provider identifier (decision A2).
    fn id(&self) -> &'static str {
        NATIVE_PROVIDER_ID
    }

    /// Pairs supported by the native engines.
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
        validate_language_pair(request.source, request.target, &self.supported_pairs)?;
        // The native engines cannot auto-detect the source; `Auto` must be
        // resolved by the pipeline before reaching the provider.
        if request.source.is_auto() {
            return Err(TranslationError::UnsupportedPair {
                src: request.source,
                target: request.target,
            });
        }
        if cancel.is_cancelled() {
            return Err(TranslationError::Cancelled);
        }

        let started = Instant::now();
        let translator = Arc::clone(&self.translator);
        let text = request.text.clone();
        let source = request.source;
        let quality = self.quality;
        let model_id = self.model_id.clone();

        // Native inference is not interruptible (decision B3); the
        // cancellation token is checked before and after the blocking call.
        let outcome = spawn_blocking(move || {
            if cancel.is_cancelled() {
                return Err(TranslationError::Cancelled);
            }
            let translated = translator.translate_blocking(source, &text)?;
            if cancel.is_cancelled() {
                return Err(TranslationError::Cancelled);
            }
            Ok::<String, TranslationError>(translated)
        })
        .await
        .map_err(|join_error| {
            TranslationError::Inference(format!("native translation task failed: {join_error}"))
        })?;
        let translated = match outcome {
            Ok(text) => text,
            Err(error) => {
                warn!(
                    provider_id = self.id(),
                    model_id = %model_id,
                    source = %request.source.code(),
                    target = %request.target.code(),
                    error = %error,
                    "native translation failed"
                );
                return Err(error);
            }
        };

        let elapsed_ms = elapsed_millis(started);
        info!(
            provider_id = self.id(),
            model_id = %model_id,
            source = %request.source.code(),
            target = %request.target.code(),
            quality = %quality,
            elapsed_ms,
            text_len = translated.chars().count(),
            "translation completed"
        );
        Ok(TranslationResult::new(translated, self.id(), elapsed_ms))
    }
}

/// Convert an `Instant` delta to milliseconds, saturating at `u64::MAX`.
fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_is_local_native() {
        assert_eq!(NATIVE_PROVIDER_ID, "local-native");
    }

    #[test]
    fn supported_pairs_are_en_zh_and_ja_zh() {
        let pairs = native_supported_pairs();
        assert_eq!(
            pairs,
            vec![
                (Language::English, Language::ChineseSimplified),
                (Language::Japanese, Language::ChineseSimplified),
            ]
        );
    }

    #[test]
    fn quality_default_is_fast() {
        assert_eq!(TranslationQuality::default(), TranslationQuality::Fast);
    }

    #[test]
    fn quality_serde_roundtrip() {
        let json = serde_json::to_string(&TranslationQuality::Fast).unwrap();
        assert_eq!(json, r#""fast""#);
        let json = serde_json::to_string(&TranslationQuality::Balanced).unwrap();
        assert_eq!(json, r#""balanced""#);
        assert_eq!(
            serde_json::from_str::<TranslationQuality>(r#""balanced""#).unwrap(),
            TranslationQuality::Balanced
        );
        assert!(serde_json::from_str::<TranslationQuality>(r#""ultra""#).is_err());
    }

    #[test]
    fn quality_string_roundtrip() {
        assert_eq!(TranslationQuality::Fast.as_str(), "fast");
        assert_eq!(TranslationQuality::Balanced.as_str(), "balanced");
        assert_eq!(
            "fast".parse::<TranslationQuality>().unwrap(),
            TranslationQuality::Fast
        );
        assert_eq!(
            "balanced".parse::<TranslationQuality>().unwrap(),
            TranslationQuality::Balanced
        );
        assert!("Fast".parse::<TranslationQuality>().is_err());
        assert!("".parse::<TranslationQuality>().is_err());
        assert_eq!(TranslationQuality::Balanced.to_string(), "balanced");
    }

    #[test]
    fn quality_maps_to_beam_sizes_per_engine() {
        // Kept in sync with native/translation_bridge/translation_bridge.cpp.
        assert_eq!(TranslationQuality::Fast.bergamot_beam_size(), 1);
        assert_eq!(TranslationQuality::Balanced.bergamot_beam_size(), 2);
        assert_eq!(TranslationQuality::Fast.ctranslate2_beam_size(), 1);
        assert_eq!(TranslationQuality::Balanced.ctranslate2_beam_size(), 4);
        assert_eq!(TranslationQuality::Fast.max_input_tokens(), 256);
        assert_eq!(TranslationQuality::Balanced.max_input_tokens(), 256);
    }

    #[test]
    fn quality_display_is_lowercase() {
        assert_eq!(TranslationQuality::Fast.to_string(), "fast");
        assert_eq!(TranslationQuality::Balanced.to_string(), "balanced");
    }
}
