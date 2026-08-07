//! Model path resolution utilities.
//!
//! Manifest entries store paths relative to the models directory. These
//! helpers join a base directory with a relative path to produce the
//! absolute filesystem path used at runtime.

use std::path::{Path, PathBuf};

/// Resolved absolute paths for the Bergamot en→zh engine.
///
/// Produced by [`ModelManager::en_zh_paths`](crate::manager::ModelManager::en_zh_paths)
/// for consumption by the translation engine (`vtrans-translation`). The
/// paths are resolved against the manifest directory; use
/// [`ModelManager::verify_integrity`](crate::manager::ModelManager::verify_integrity)
/// to check that the files exist and match the manifest hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BergamotPaths {
    /// Absolute path of the quantized Marian model (`model.enzh.intgemm.alphas.bin`).
    pub model: PathBuf,
    /// Absolute path of the source `SentencePiece` vocabulary (`srcvocab.enzh.spm`).
    pub src_vocab: PathBuf,
    /// Absolute path of the target `SentencePiece` vocabulary (`trgvocab.enzh.spm`).
    pub trg_vocab: PathBuf,
    /// Absolute path of the lexical shortlist (`lex.50.50.enzh.s2t.bin`).
    pub lexical_shortlist: PathBuf,
}

/// Resolved absolute paths for the `CTranslate2` ja→zh engine.
///
/// Produced by [`ModelManager::ja_zh_paths`](crate::manager::ModelManager::ja_zh_paths)
/// for consumption by the translation engine (`vtrans-translation`). The
/// paths are resolved against the manifest directory; use
/// [`ModelManager::verify_integrity`](crate::manager::ModelManager::verify_integrity)
/// to check that the files exist and match the manifest hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CTranslate2Paths {
    /// Absolute path of the converted `CTranslate2` model (`model.bin`).
    pub model: PathBuf,
    /// Absolute path of the `CTranslate2` model configuration (`config.json`).
    pub config: PathBuf,
    /// Absolute path of the source vocabulary (`source_vocabulary.json`).
    pub source_vocabulary: PathBuf,
    /// Absolute path of the target vocabulary (`target_vocabulary.json`).
    pub target_vocabulary: PathBuf,
    /// Absolute path of the source `SentencePiece` model (`source.spm`).
    pub source_spm: PathBuf,
    /// Absolute path of the target `SentencePiece` model (`target.spm`).
    pub target_spm: PathBuf,
}

/// Resolve a relative model path against a base directory.
///
/// # Example
///
/// ```
/// # use std::path::{Path, PathBuf};
/// # use vtrans_models::path::resolve_model_path;
/// let base = Path::new("/app/models");
/// let resolved = resolve_model_path(base, Path::new("ocr/det.onnx"));
/// assert_eq!(resolved, PathBuf::from("/app/models/ocr/det.onnx"));
/// ```
#[must_use]
pub fn resolve_model_path(base: &Path, relative: &Path) -> PathBuf {
    base.join(relative)
}

/// Check whether a path is relative (not absolute).
///
/// # Example
///
/// ```
/// # use std::path::Path;
/// # use vtrans_models::path::is_relative;
/// assert!(is_relative(Path::new("ocr/det.onnx")));
/// assert!(is_relative(Path::new("det.onnx")));
/// ```
#[must_use]
pub fn is_relative(path: &Path) -> bool {
    path.is_relative()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_relative_path() {
        let base = Path::new("/models");
        let resolved = resolve_model_path(base, Path::new("ocr/det.onnx"));
        assert_eq!(resolved, PathBuf::from("/models/ocr/det.onnx"));
    }

    #[test]
    fn resolve_nested_relative_path() {
        let base = Path::new("/app/models");
        let resolved = resolve_model_path(base, Path::new("translation/sub/tokenizer.json"));
        assert_eq!(
            resolved,
            PathBuf::from("/app/models/translation/sub/tokenizer.json")
        );
    }

    #[test]
    fn resolve_with_empty_relative() {
        let base = Path::new("/models");
        let resolved = resolve_model_path(base, Path::new(""));
        assert_eq!(resolved, PathBuf::from("/models"));
    }

    #[test]
    fn is_relative_true() {
        assert!(is_relative(Path::new("ocr/det.onnx")));
        assert!(is_relative(Path::new("det.onnx")));
    }

    #[test]
    fn is_relative_false() {
        #[cfg(unix)]
        assert!(!is_relative(Path::new("/etc/hosts")));
        #[cfg(windows)]
        assert!(!is_relative(Path::new("C:\\Windows\\System32")));
    }
}
