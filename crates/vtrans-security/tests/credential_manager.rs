//! Integration tests against the real Windows Credential Manager.
//!
//! Each test writes under a unique target derived from the process id so
//! parallel test runs never collide, and every test cleans its entries up
//! (best-effort) even when it panics. These tests only run on Windows.

#![cfg(windows)]

use vtrans_security::credential_store::WindowsCredentialStore;
use vtrans_security::{CredentialManager, CredentialStore, SecurityError};

/// Namespace prefix applied by the manager.
const STORED_PREFIX: &str = "VTrans:";

/// Builds a unique logical target for this test binary/process.
fn unique_target(name: &str) -> String {
    format!("test_{}_{name}", std::process::id())
}

/// Removes a target's credential, ignoring "not found" so cleanup is idempotent.
fn cleanup(manager: &CredentialManager, target: &str) {
    match manager.delete(target) {
        Ok(()) | Err(SecurityError::NotFound(_)) => {}
        Err(e) => eprintln!("warning: cleanup failed for '{target}': {e}"),
    }
}

/// Removes the target when the guard is dropped, including during unwinding
/// from a panicking test body.
struct CleanupGuard {
    manager: CredentialManager,
    target: String,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        cleanup(&self.manager, &self.target);
    }
}

/// Runs `body` with a fresh manager and guarantees the test target is removed
/// afterwards, even when `body` panics.
fn with_cleanup<T>(name: &str, body: impl FnOnce(&CredentialManager, &str) -> T) -> T {
    let manager = CredentialManager::new().expect("credential manager should initialize");
    let target = unique_target(name);
    let guard = CleanupGuard {
        manager,
        target: target.clone(),
    };
    body(&guard.manager, &target)
}

#[test]
fn store_then_load_roundtrip() {
    with_cleanup("store_load_roundtrip", |manager, target| {
        let key = "sk-1234567890abcdef";
        manager.store(target, key).expect("store should succeed");
        assert_eq!(
            manager
                .load(target)
                .expect("load should succeed")
                .as_deref(),
            Some(key)
        );
    });
}

#[test]
fn load_missing_target_returns_none() {
    with_cleanup("load_missing", |manager, target| {
        // The target is guaranteed to be absent: it was never stored.
        assert_eq!(
            manager.load(target).expect("load should succeed"),
            None,
            "a never-stored target must yield Ok(None)"
        );
    });
}

#[test]
fn delete_then_load_returns_none() {
    with_cleanup("delete_then_load", |manager, target| {
        manager
            .store(target, "sk-to-be-deleted")
            .expect("store should succeed");
        manager.delete(target).expect("delete should succeed");
        assert_eq!(
            manager.load(target).expect("load should succeed"),
            None,
            "deleted credential must no longer be readable"
        );
    });
}

#[test]
fn delete_missing_target_returns_not_found() {
    with_cleanup("delete_missing", |manager, target| {
        let err = manager
            .delete(target)
            .expect_err("delete of a missing target must fail");
        assert!(
            matches!(err, SecurityError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );
    });
}

#[test]
fn store_overwrites_existing_value() {
    with_cleanup("overwrite", |manager, target| {
        manager
            .store(target, "sk-old-value")
            .expect("first store should succeed");
        manager
            .store(target, "sk-new-value")
            .expect("second store should succeed");
        assert_eq!(
            manager
                .load(target)
                .expect("load should succeed")
                .as_deref(),
            Some("sk-new-value")
        );
    });
}

#[test]
fn list_targets_contains_stored_target_without_leaking_secrets() {
    with_cleanup("list_targets", |manager, target| {
        let secret = "sk-ultra-secret-value-987654321";
        manager.store(target, secret).expect("store should succeed");

        let targets = manager.list_targets().expect("list should succeed");
        assert!(
            targets.iter().any(|t| t == target),
            "listed targets should contain {target:?}, got {targets:?}"
        );

        for listed in &targets {
            assert!(
                !listed.contains("ultra-secret"),
                "listed target {listed:?} must not leak the secret value"
            );
        }
    });
}

#[test]
fn stored_credential_uses_namespaced_target() {
    with_cleanup("namespaced", |manager, target| {
        manager
            .store(target, "sk-1234")
            .expect("store should succeed");

        // The raw Windows vault entry carries the VTrans: namespace prefix.
        let raw_store = WindowsCredentialStore::new();
        let raw_target = format!("{STORED_PREFIX}{target}");
        assert!(
            raw_store
                .load(&raw_target)
                .expect("raw load should succeed")
                .is_some(),
            "raw vault should contain {raw_target:?}"
        );

        // The manager's list_targets strips the prefix again.
        let targets = manager.list_targets().expect("list should succeed");
        assert!(
            targets.iter().any(|t| t == target),
            "listed targets should contain the logical target, got {targets:?}"
        );
        assert!(
            !targets.iter().any(|t| t.starts_with(STORED_PREFIX)),
            "listed targets must not expose the namespace prefix, got {targets:?}"
        );
    });
}
