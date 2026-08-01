//! Model path resolution utilities.
//!
//! Manifest entries store paths relative to the models directory. These
//! helpers join a base directory with a relative path to produce the
//! absolute filesystem path used at runtime.

use std::path::{Path, PathBuf};

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
