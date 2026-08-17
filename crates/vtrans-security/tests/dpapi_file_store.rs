//! Integration tests for the DPAPI file store.
//!
//! These tests exercise [`DpapiFileStore`] end-to-end (through
//! [`CredentialManager`]) using a real Windows DPAPI round trip and a
//! temporary container file. They never touch the Windows Credential
//! Manager: the legacy vault is covered by the injected-mock migration unit
//! tests in `src/dpapi.rs`.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vtrans_security::{
    CredentialManager, CredentialStore, CredentialTarget, DpapiFileStore, SecurityError,
};

/// Minimal std-only temporary-directory guard for tests.
///
/// Deliberately avoids an external `tempfile` dependency so adding these
/// tests does not touch the workspace `Cargo.lock`.
struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("vtrans-security-it-{}-{seq}", std::process::id()));
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

/// Creates a store in a fresh temporary directory and wraps it in a manager.
fn temp_manager() -> (TestDir, CredentialManager, PathBuf) {
    let dir = TestDir::new();
    let path = dir.path().join("credentials.bin");
    let store = Arc::new(DpapiFileStore::new(&path).expect("store should open a fresh container"));
    let manager = CredentialManager::with_store(store);
    (dir, manager, path)
}

#[test]
fn manager_roundtrip_through_dpapi_file_store() {
    let (_dir, manager, _path) = temp_manager();
    manager.store("openai", "sk-1234567890abcdef").unwrap();
    assert_eq!(
        manager.load("openai").unwrap().as_deref(),
        Some("sk-1234567890abcdef")
    );
    manager.delete("openai").unwrap();
    assert_eq!(manager.load("openai").unwrap(), None);
}

#[test]
fn load_missing_target_returns_none() {
    let (_dir, manager, _path) = temp_manager();
    assert_eq!(manager.load("openai").unwrap(), None);
}

#[test]
fn delete_missing_target_returns_not_found() {
    let (_dir, manager, _path) = temp_manager();
    let err = manager.delete("openai").unwrap_err();
    assert!(matches!(err, SecurityError::NotFound(_)));
}

#[test]
fn provider_targets_roundtrip() {
    let (_dir, manager, _path) = temp_manager();
    manager
        .store_for_provider(CredentialTarget::Azure, "sk-azure-0123456789")
        .unwrap();
    assert_eq!(
        manager
            .load_for_provider(CredentialTarget::Azure)
            .unwrap()
            .as_deref(),
        Some("sk-azure-0123456789")
    );
}

#[test]
fn container_file_never_contains_plaintext() {
    let (_dir, manager, path) = temp_manager();
    let key = "sk-integration-ultra-secret-987654321";
    manager.store("openai", key).unwrap();
    let bytes = std::fs::read(&path).expect("container file should be readable");
    assert!(
        !bytes
            .windows(key.len())
            .any(|window| window == key.as_bytes()),
        "plaintext key found in the container file"
    );
}

#[test]
fn store_survives_reopen_of_the_same_path() {
    let (_dir, _manager, path) = temp_manager();
    let store = Arc::new(DpapiFileStore::new(&path).unwrap());
    store.store("VTrans:openai", b"sk-persistent").unwrap();
    drop(store);

    let reopened = DpapiFileStore::new(&path).unwrap();
    assert_eq!(
        reopened.load("VTrans:openai").unwrap(),
        Some(b"sk-persistent".to_vec())
    );
}

#[test]
fn corrupted_container_yields_a_clear_error() {
    let (_dir, manager, path) = temp_manager();
    manager.store("openai", "sk-1234567890abcdef").unwrap();
    // Corrupt the container: the load must fail with an explicit error, never
    // panic or silently return a wrong value.
    std::fs::write(&path, b"definitely not a vtrans container").unwrap();
    let err = manager.load("openai").unwrap_err();
    assert!(
        matches!(err, SecurityError::CorruptedFile(_)),
        "expected CorruptedFile, got {err:?}"
    );
}

#[test]
fn list_targets_strips_prefix_and_never_leaks_secrets() {
    let (_dir, manager, _path) = temp_manager();
    let secret = "sk-list-targets-secret-0123456789";
    manager.store("openai", secret).unwrap();
    manager.store("azure", "sk-azure-secret").unwrap();

    let targets = manager.list_targets().unwrap();
    assert_eq!(targets, ["azure", "openai"]);
    for listed in &targets {
        assert!(
            !listed.contains("secret"),
            "listed target {listed:?} must not leak secret data"
        );
    }
}
