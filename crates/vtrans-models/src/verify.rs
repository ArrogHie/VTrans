//! SHA-256 integrity verification for model files.
//!
//! Provides [`verify_entry`] for checking individual model files and
//! [`VerifyReport`] for aggregating batch verification results.

use std::io::{BufReader, Read};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::manifest::ModelEntry;
use crate::ModelError;

/// Report from a batch integrity verification pass.
///
/// Produced by [`ModelManager::verify_integrity`](crate::manager::ModelManager::verify_integrity).
/// Each checked file contributes to `checked`; matching files increment
/// `passed`; failures are recorded as human-readable strings in `failed`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VerifyReport {
    /// Total number of files checked.
    pub checked: usize,
    /// Number of files that passed verification.
    pub passed: usize,
    /// Human-readable descriptions of failures.
    pub failed: Vec<String>,
}

impl VerifyReport {
    /// Create an empty report.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if every checked file passed.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Compute the SHA-256 hash of a file as a lowercase hex string.
///
/// Reads the file in 8 KiB chunks to avoid loading large model files
/// (potentially hundreds of MiB) entirely into memory.
///
/// # Errors
/// Returns [`ModelError::Io`] if the file cannot be opened or read.
fn compute_sha256(path: &Path) -> Result<String, ModelError> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify a single model entry: the file exists and its SHA-256 matches.
///
/// # Arguments
/// * `base_dir` - Directory that model entry paths are relative to.
/// * `entry` - The model entry to verify.
///
/// # Errors
/// Returns [`ModelError::FileNotFound`] if the file does not exist,
/// [`ModelError::Io`] if it cannot be read, or [`ModelError::HashMismatch`]
/// if the computed hash differs from the expected value.
#[tracing::instrument(skip(entry), fields(entry_id = %entry.id))]
pub fn verify_entry(base_dir: &Path, entry: &ModelEntry) -> Result<(), ModelError> {
    let file_path = base_dir.join(&entry.path);
    if !file_path.exists() {
        warn!(
            entry_id = %entry.id,
            path = %file_path.display(),
            "model file not found"
        );
        return Err(ModelError::FileNotFound(file_path));
    }
    let actual = compute_sha256(&file_path)?;
    if actual != entry.sha256 {
        warn!(
            entry_id = %entry.id,
            expected = %entry.sha256,
            actual = %actual,
            "SHA-256 mismatch"
        );
        return Err(ModelError::HashMismatch {
            id: entry.id.clone(),
            expected: entry.sha256.clone(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    /// Build a `ModelEntry` pointing at `rel_path` with the hash of `content`.
    fn make_entry(dir: &Path, rel_path: &str, content: &[u8]) -> (ModelEntry, std::path::PathBuf) {
        let full = dir.join(rel_path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, content).unwrap();

        let mut hasher = Sha256::new();
        hasher.update(content);
        let sha = format!("{:x}", hasher.finalize());

        let entry = ModelEntry {
            id: rel_path.to_string(),
            path: std::path::PathBuf::from(rel_path),
            sha256: sha,
            size_bytes: content.len() as u64,
        };
        (entry, full)
    }

    #[test]
    fn verify_entry_ok() {
        let dir = tempdir().unwrap();
        let (entry, _) = make_entry(dir.path(), "model.onnx", b"hello world");
        assert!(verify_entry(dir.path(), &entry).is_ok());
    }

    #[test]
    fn verify_entry_hash_mismatch() {
        let dir = tempdir().unwrap();
        let (mut entry, _) = make_entry(dir.path(), "model.onnx", b"hello world");
        entry.sha256 =
            "0000000000000000000000000000000000000000000000000000000000000000".to_string();
        let result = verify_entry(dir.path(), &entry);
        assert!(matches!(result, Err(ModelError::HashMismatch { .. })));
    }

    #[test]
    fn verify_entry_file_not_found() {
        let dir = tempdir().unwrap();
        let entry = ModelEntry {
            id: "missing".to_string(),
            path: std::path::PathBuf::from("nonexistent.onnx"),
            sha256: "abc".to_string(),
            size_bytes: 0,
        };
        let result = verify_entry(dir.path(), &entry);
        assert!(matches!(result, Err(ModelError::FileNotFound(_))));
    }

    #[test]
    fn compute_sha256_known_value() {
        // SHA-256 of empty string
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty");
        std::fs::write(&path, b"").unwrap();
        let hash = compute_sha256(&path).unwrap();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn compute_sha256_small_file() {
        // SHA-256 of "abc" (NIST test vector)
        let dir = tempdir().unwrap();
        let path = dir.path().join("abc");
        std::fs::write(&path, b"abc").unwrap();
        let hash = compute_sha256(&path).unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn compute_sha256_large_file_chunked() {
        // File larger than the 8 KiB buffer to exercise chunked reading.
        let dir = tempdir().unwrap();
        let path = dir.path().join("large");
        let mut file = std::fs::File::create(&path).unwrap();
        let chunk = vec![0xAB_u8; 8192];
        for _ in 0..4 {
            file.write_all(&chunk).unwrap();
        }
        file.sync_all().unwrap();
        drop(file);

        // Compute expected hash independently.
        let mut hasher = Sha256::new();
        hasher.update(chunk.repeat(4));
        let expected = format!("{:x}", hasher.finalize());

        let actual = compute_sha256(&path).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn verify_report_new_is_empty() {
        let report = VerifyReport::new();
        assert_eq!(report.checked, 0);
        assert_eq!(report.passed, 0);
        assert!(report.failed.is_empty());
        assert!(report.is_ok());
    }

    #[test]
    fn verify_report_is_ok_with_failures() {
        let report = VerifyReport {
            checked: 3,
            passed: 2,
            failed: vec!["bad file".to_string()],
        };
        assert!(!report.is_ok());
    }
}
