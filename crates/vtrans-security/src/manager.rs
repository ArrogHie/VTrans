//! [`CredentialManager`]: application-facing credential storage.
//!
//! The manager owns the `VTrans:` target namespace. Logical targets such as
//! `openai` are qualified to `VTrans:openai` before hitting the
//! [`CredentialStore`] backend, and [`list_targets`](CredentialManager::list_targets)
//! strips the prefix again, so callers never see implementation details and
//! credentials can never collide with other applications' entries in the
//! Windows Credential Manager.
//!
//! Two API families are provided:
//!
//! - The typed provider API ([`store_for_provider`](CredentialManager::store_for_provider),
//!   [`load_for_provider`](CredentialManager::load_for_provider),
//!   [`delete_for_provider`](CredentialManager::delete_for_provider)) takes a
//!   [`CredentialTarget`] and is the recommended entry point for cloud
//!   provider credentials.
//! - The legacy string API ([`store`](CredentialManager::store),
//!   [`load`](CredentialManager::load), [`delete`](CredentialManager::delete),
//!   [`list_targets`](CredentialManager::list_targets)) remains fully
//!   supported for backward compatibility, including the historical
//!   `translation` target.

use std::sync::Arc;

use tracing::{debug, warn};

use crate::credential_store::{CredentialStore, WindowsCredentialStore};
use crate::{mask_key, CredentialTarget, SecurityError};

/// Namespace prefix applied to every credential target stored in the
/// Windows Credential Manager, so `VTrans` entries never collide with entries
/// written by other applications.
pub const TARGET_PREFIX: &str = "VTrans:";

/// Stores and reads API keys through a [`CredentialStore`].
///
/// By default the manager uses the Windows Credential Manager backend; use
/// [`with_store`](Self::with_store) to inject a different backend (for tests
/// or non-Windows development). Instances are cheap to clone conceptually but
/// intentionally not `Clone`: share one manager behind an `Arc` so a single
/// store instance backs all callers.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
/// use vtrans_security::credential_store::InMemoryCredentialStore;
/// use vtrans_security::CredentialManager;
///
/// let manager = CredentialManager::with_store(Arc::new(InMemoryCredentialStore::new()));
/// manager.store("openai", "sk-1234567890").unwrap();
/// assert_eq!(manager.load("openai").unwrap().as_deref(), Some("sk-1234567890"));
/// manager.delete("openai").unwrap();
/// ```
pub struct CredentialManager {
    store: Arc<dyn CredentialStore>,
}

impl CredentialManager {
    /// Creates a manager backed by the Windows Credential Manager.
    ///
    /// Construction does not touch the vault; a store that is unavailable
    /// surfaces as [`SecurityError`] from the first operation.
    ///
    /// # Errors
    ///
    /// Kept as `Result` by the module specification so a future availability
    /// probe can report [`SecurityError::StoreUnavailable`] without an API
    /// break; today construction cannot fail.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use vtrans_security::CredentialManager;
    ///
    /// let manager = CredentialManager::new().unwrap();
    /// # drop(manager);
    /// ```
    #[tracing::instrument(skip_all)]
    pub fn new() -> Result<Self, SecurityError> {
        Ok(Self::with_store(Arc::new(WindowsCredentialStore::new())))
    }

    /// Creates a manager around a caller-provided store backend.
    ///
    /// This is primarily useful for tests (e.g. an
    /// [`InMemoryCredentialStore`]) and for embedding the manager in custom
    /// backends. The `Arc` keeps the backend shareable, so the same store can
    /// be inspected from outside the manager.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::Arc;
    /// use vtrans_security::credential_store::InMemoryCredentialStore;
    /// use vtrans_security::{CredentialManager, CredentialStore};
    ///
    /// let store = Arc::new(InMemoryCredentialStore::new());
    /// let manager = CredentialManager::with_store(Arc::clone(&store));
    /// manager.store("openai", "sk-1234").unwrap();
    /// assert!(store.load("VTrans:openai").unwrap().is_some());
    /// ```
    #[must_use]
    pub fn with_store<S>(store: Arc<S>) -> Self
    where
        S: CredentialStore + 'static,
    {
        Self { store }
    }

    /// Stores an API key for `target`, overwriting any previous value.
    ///
    /// The key is persisted by the backend (Windows Credential Manager by
    /// default) and never written to a plaintext file or a log by this crate.
    ///
    /// # Arguments
    ///
    /// * `target` - logical name of the provider, e.g. `"openai"`. An empty
    ///   target is stored under the bare `VTrans:` prefix.
    /// * `api_key` - the secret to store. It is skipped by the tracing span
    ///   and never logged.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::OperationFailed`] or
    /// [`SecurityError::WindowsApi`] when the backend cannot persist the key.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::Arc;
    /// use vtrans_security::credential_store::InMemoryCredentialStore;
    /// use vtrans_security::CredentialManager;
    ///
    /// let manager = CredentialManager::with_store(Arc::new(InMemoryCredentialStore::new()));
    /// manager.store("openai", "sk-1234567890").unwrap();
    /// ```
    #[tracing::instrument(skip(self, api_key), fields(target = %target))]
    pub fn store(&self, target: &str, api_key: &str) -> Result<(), SecurityError> {
        let stored_target = qualify_target(target);
        self.store.store(&stored_target, api_key.as_bytes())?;
        debug!(target = %target, "api key stored");
        Ok(())
    }

    /// Reads the API key stored for `target`.
    ///
    /// Returns `Ok(None)` when no key is stored for `target`; a missing
    /// credential is not an error.
    ///
    /// # Arguments
    ///
    /// * `target` - logical name of the provider, e.g. `"openai"`.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::OperationFailed`] when the stored blob is not
    /// valid UTF-8, and [`SecurityError::WindowsApi`] when the backend cannot
    /// read the credential.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::Arc;
    /// use vtrans_security::credential_store::InMemoryCredentialStore;
    /// use vtrans_security::CredentialManager;
    ///
    /// let manager = CredentialManager::with_store(Arc::new(InMemoryCredentialStore::new()));
    /// assert_eq!(manager.load("openai").unwrap(), None);
    /// manager.store("openai", "sk-1234567890").unwrap();
    /// assert_eq!(manager.load("openai").unwrap().as_deref(), Some("sk-1234567890"));
    /// ```
    #[tracing::instrument(skip(self), fields(target = %target))]
    pub fn load(&self, target: &str) -> Result<Option<String>, SecurityError> {
        let stored_target = qualify_target(target);
        match self.store.load(&stored_target)? {
            Some(blob) => {
                let api_key = String::from_utf8(blob).map_err(|e| {
                    let message =
                        format!("stored credential for target '{target}' is not valid UTF-8: {e}");
                    warn!(error = %message, "stored credential is not valid UTF-8");
                    SecurityError::OperationFailed(message)
                })?;
                debug!(target = %target, masked_key = %mask_key(&api_key), "api key loaded");
                Ok(Some(api_key))
            }
            None => Ok(None),
        }
    }

    /// Deletes the API key stored for `target`.
    ///
    /// Deleting a target that has no stored key is reported as
    /// [`SecurityError::NotFound`], which lets callers distinguish "removed"
    /// from "there was nothing to remove".
    ///
    /// # Arguments
    ///
    /// * `target` - logical name of the provider, e.g. `"openai"`.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::NotFound`] when no credential exists for
    /// `target`, or a backend error when the deletion itself fails.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::Arc;
    /// use vtrans_security::credential_store::InMemoryCredentialStore;
    /// use vtrans_security::CredentialManager;
    ///
    /// let manager = CredentialManager::with_store(Arc::new(InMemoryCredentialStore::new()));
    /// manager.store("openai", "sk-1234567890").unwrap();
    /// manager.delete("openai").unwrap();
    /// assert_eq!(manager.load("openai").unwrap(), None);
    /// ```
    #[tracing::instrument(skip(self), fields(target = %target))]
    pub fn delete(&self, target: &str) -> Result<(), SecurityError> {
        let stored_target = qualify_target(target);
        self.store.delete(&stored_target)?;
        debug!(target = %target, "api key deleted");
        Ok(())
    }

    /// Lists all stored targets, sorted and deduplicated.
    ///
    /// Only targets inside the `VTrans:` namespace are returned and the
    /// prefix is stripped, so callers receive the same logical names they
    /// passed to [`store`](Self::store). No secret data is ever returned.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::WindowsApi`] when enumeration fails.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::Arc;
    /// use vtrans_security::credential_store::InMemoryCredentialStore;
    /// use vtrans_security::CredentialManager;
    ///
    /// let manager = CredentialManager::with_store(Arc::new(InMemoryCredentialStore::new()));
    /// manager.store("openai", "sk-1").unwrap();
    /// manager.store("azure", "sk-2").unwrap();
    /// assert_eq!(manager.list_targets().unwrap(), ["azure", "openai"]);
    /// ```
    #[tracing::instrument(skip_all)]
    pub fn list_targets(&self) -> Result<Vec<String>, SecurityError> {
        let mut targets: Vec<String> = self
            .store
            .list_targets()?
            .into_iter()
            .filter_map(|stored| stored.strip_prefix(TARGET_PREFIX).map(str::to_owned))
            .collect();
        targets.sort();
        targets.dedup();
        Ok(targets)
    }

    /// Stores a credential for a cloud provider target, overwriting any
    /// previous value.
    ///
    /// This is the typed counterpart of [`store`](Self::store): `target` is a
    /// [`CredentialTarget`] variant instead of a raw string, so provider
    /// names are checked at compile time and always use the canonical
    /// spelling. The secret is persisted by the backend (Windows Credential
    /// Manager by default) and never written to a plaintext file or a log by
    /// this crate.
    ///
    /// # Arguments
    ///
    /// * `target` - the provider credential target, e.g.
    ///   [`CredentialTarget::OpenAI`].
    /// * `api_key` - the secret to store. It is skipped by the tracing span
    ///   and never logged.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::OperationFailed`] or
    /// [`SecurityError::WindowsApi`] when the backend cannot persist the key.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::Arc;
    /// use vtrans_security::credential_store::InMemoryCredentialStore;
    /// use vtrans_security::{CredentialManager, CredentialTarget};
    ///
    /// let manager = CredentialManager::with_store(Arc::new(InMemoryCredentialStore::new()));
    /// manager
    ///     .store_for_provider(CredentialTarget::OpenAI, "sk-1234567890")
    ///     .unwrap();
    /// assert_eq!(
    ///     manager
    ///         .load_for_provider(CredentialTarget::OpenAI)
    ///         .unwrap()
    ///         .as_deref(),
    ///     Some("sk-1234567890")
    /// );
    /// ```
    #[tracing::instrument(skip(self, api_key), fields(target = %target))]
    pub fn store_for_provider(
        &self,
        target: CredentialTarget,
        api_key: &str,
    ) -> Result<(), SecurityError> {
        self.store(target.as_str(), api_key)
    }

    /// Reads the credential stored for a cloud provider target.
    ///
    /// This is the typed counterpart of [`load`](Self::load). Returns
    /// `Ok(None)` when no credential is stored for `target`; a missing
    /// credential is not an error.
    ///
    /// # Arguments
    ///
    /// * `target` - the provider credential target, e.g.
    ///   [`CredentialTarget::BaiduSecret`].
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::OperationFailed`] when the stored blob is not
    /// valid UTF-8, and [`SecurityError::WindowsApi`] when the backend cannot
    /// read the credential.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::Arc;
    /// use vtrans_security::credential_store::InMemoryCredentialStore;
    /// use vtrans_security::{CredentialManager, CredentialTarget};
    ///
    /// let manager = CredentialManager::with_store(Arc::new(InMemoryCredentialStore::new()));
    /// manager
    ///     .store_for_provider(CredentialTarget::DeepL, "sk-deepl-1234")
    ///     .unwrap();
    /// assert_eq!(
    ///     manager
    ///         .load_for_provider(CredentialTarget::DeepL)
    ///         .unwrap()
    ///         .as_deref(),
    ///     Some("sk-deepl-1234")
    /// );
    /// ```
    #[tracing::instrument(skip(self), fields(target = %target))]
    pub fn load_for_provider(
        &self,
        target: CredentialTarget,
    ) -> Result<Option<String>, SecurityError> {
        self.load(target.as_str())
    }

    /// Deletes the credential stored for a cloud provider target.
    ///
    /// This is the typed counterpart of [`delete`](Self::delete). Deleting a
    /// target that has no stored credential is reported as
    /// [`SecurityError::NotFound`], which lets callers distinguish "removed"
    /// from "there was nothing to remove".
    ///
    /// # Arguments
    ///
    /// * `target` - the provider credential target, e.g.
    ///   [`CredentialTarget::BaiduAppId`].
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::NotFound`] when no credential exists for
    /// `target`, or a backend error when the deletion itself fails.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::Arc;
    /// use vtrans_security::credential_store::InMemoryCredentialStore;
    /// use vtrans_security::{CredentialManager, CredentialTarget};
    ///
    /// let manager = CredentialManager::with_store(Arc::new(InMemoryCredentialStore::new()));
    /// manager
    ///     .store_for_provider(CredentialTarget::Azure, "sk-azure-1234")
    ///     .unwrap();
    /// manager
    ///     .delete_for_provider(CredentialTarget::Azure)
    ///     .unwrap();
    /// assert_eq!(
    ///     manager.load_for_provider(CredentialTarget::Azure).unwrap(),
    ///     None
    /// );
    /// ```
    #[tracing::instrument(skip(self), fields(target = %target))]
    pub fn delete_for_provider(&self, target: CredentialTarget) -> Result<(), SecurityError> {
        self.delete(target.as_str())
    }
}

/// Returns `target` with the `VTrans:` namespace prefix applied exactly once.
fn qualify_target(target: &str) -> String {
    if target.starts_with(TARGET_PREFIX) {
        target.to_string()
    } else {
        format!("{TARGET_PREFIX}{target}")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, OnceLock};

    use mockall::predicate::eq;
    use tracing_subscriber::fmt;

    use super::*;
    use crate::credential_store::{InMemoryCredentialStore, MockCredentialStore};

    /// A `MakeWriter` that records everything written to it so tests can
    /// assert on log output.
    #[derive(Clone, Default)]
    struct CapturingWriter(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for CapturingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("capture lock should not be poisoned")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl fmt::MakeWriter<'_> for CapturingWriter {
        type Writer = Self;

        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    /// Process-wide buffer that receives every test log event.
    static TEST_LOG_BUFFER: OnceLock<Arc<std::sync::Mutex<Vec<u8>>>> = OnceLock::new();

    /// Installs a process-global tracing subscriber exactly once and returns
    /// the shared log buffer.
    ///
    /// A thread-local `with_default` subscriber is not used here because
    /// `tracing` caches callsite interest globally: while a capture is active,
    /// another test thread may register the same callsite against the no-op
    /// dispatcher and permanently cache `Interest::never()` for it, silently
    /// dropping the events we want to assert on. A global default is shared by
    /// every thread, so callsite interest is always computed against the
    /// capturing subscriber and the assertions are deterministic.
    fn install_test_log_subscriber() -> &'static Arc<std::sync::Mutex<Vec<u8>>> {
        TEST_LOG_BUFFER.get_or_init(|| {
            let buffer = Arc::new(std::sync::Mutex::new(Vec::new()));
            let subscriber = fmt()
                .with_writer(CapturingWriter(Arc::clone(&buffer)))
                .with_max_level(tracing::Level::DEBUG)
                .without_time()
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .expect("test subscriber should be installed exactly once");
            buffer
        })
    }

    fn in_memory_manager() -> (CredentialManager, Arc<InMemoryCredentialStore>) {
        let store = Arc::new(InMemoryCredentialStore::new());
        let manager = CredentialManager::with_store(Arc::clone(&store));
        (manager, store)
    }

    #[test]
    fn store_then_load_roundtrip() {
        let (manager, _) = in_memory_manager();
        manager.store("openai", "sk-1234567890").unwrap();
        assert_eq!(
            manager.load("openai").unwrap().as_deref(),
            Some("sk-1234567890")
        );
    }

    #[test]
    fn load_missing_returns_none() {
        let (manager, _) = in_memory_manager();
        assert_eq!(manager.load("missing").unwrap(), None);
    }

    #[test]
    fn store_overwrites_existing_value() {
        let (manager, _) = in_memory_manager();
        manager.store("openai", "old-key").unwrap();
        manager.store("openai", "new-key").unwrap();
        assert_eq!(manager.load("openai").unwrap().as_deref(), Some("new-key"));
    }

    #[test]
    fn delete_then_load_returns_none() {
        let (manager, _) = in_memory_manager();
        manager.store("openai", "sk-1234567890").unwrap();
        manager.delete("openai").unwrap();
        assert_eq!(manager.load("openai").unwrap(), None);
    }

    #[test]
    fn delete_missing_returns_not_found() {
        let (manager, _) = in_memory_manager();
        let err = manager.delete("missing").unwrap_err();
        assert!(matches!(err, SecurityError::NotFound(_)));
    }

    #[test]
    fn targets_are_namespaced_with_prefix() {
        let (manager, raw) = in_memory_manager();
        manager.store("openai", "sk-1").unwrap();
        assert!(raw.load("VTrans:openai").unwrap().is_some());
        assert_eq!(raw.load("openai").unwrap(), None);
    }

    #[test]
    fn load_accepts_already_prefixed_target_without_double_prefix() {
        let (manager, raw) = in_memory_manager();
        manager.store("VTrans:openai", "sk-1").unwrap();
        assert_eq!(raw.list_targets().unwrap(), ["VTrans:openai"]);
        assert_eq!(
            manager.load("VTrans:openai").unwrap().as_deref(),
            Some("sk-1")
        );
    }

    #[test]
    fn list_targets_strips_prefix_and_sorts() {
        let (manager, _) = in_memory_manager();
        manager.store("openai", "sk-1").unwrap();
        manager.store("azure", "sk-2").unwrap();
        manager.store("deepseek", "sk-3").unwrap();
        assert_eq!(
            manager.list_targets().unwrap(),
            ["azure", "deepseek", "openai"]
        );
    }

    #[test]
    fn list_targets_excludes_foreign_entries() {
        let (manager, raw) = in_memory_manager();
        raw.store("OtherApp:foo", b"1").unwrap();
        manager.store("openai", "sk-1").unwrap();
        assert_eq!(manager.list_targets().unwrap(), ["openai"]);
    }

    #[test]
    fn non_utf8_blob_returns_operation_failed() {
        let (manager, raw) = in_memory_manager();
        raw.store("VTrans:openai", &[0xff, 0xfe, 0x00]).unwrap();
        let err = manager.load("openai").unwrap_err();
        assert!(matches!(err, SecurityError::OperationFailed(_)));
    }

    #[test]
    fn store_propagates_backend_errors() {
        let mut mock = MockCredentialStore::new();
        mock.expect_store()
            .with(eq("VTrans:openai"), eq(b"sk-1".as_slice()))
            .once()
            .returning(|_, _| Err(SecurityError::WindowsApi("boom".to_string())));
        let manager = CredentialManager::with_store(Arc::new(mock));
        let err = manager.store("openai", "sk-1").unwrap_err();
        assert!(matches!(err, SecurityError::WindowsApi(_)));
    }

    #[test]
    fn load_propagates_backend_errors() {
        let mut mock = MockCredentialStore::new();
        mock.expect_load()
            .with(eq("VTrans:openai"))
            .once()
            .returning(|_| Err(SecurityError::StoreUnavailable("vault locked".to_string())));
        let manager = CredentialManager::with_store(Arc::new(mock));
        let err = manager.load("openai").unwrap_err();
        assert!(matches!(err, SecurityError::StoreUnavailable(_)));
    }

    #[test]
    fn delete_propagates_backend_errors() {
        let mut mock = MockCredentialStore::new();
        mock.expect_delete()
            .with(eq("VTrans:openai"))
            .once()
            .returning(|_| Err(SecurityError::NotFound("VTrans:openai".to_string())));
        let manager = CredentialManager::with_store(Arc::new(mock));
        let err = manager.delete("openai").unwrap_err();
        assert!(matches!(err, SecurityError::NotFound(_)));
    }

    #[test]
    fn list_targets_propagates_backend_errors() {
        let mut mock = MockCredentialStore::new();
        mock.expect_list_targets()
            .once()
            .returning(|| Err(SecurityError::WindowsApi("enumeration failed".to_string())));
        let manager = CredentialManager::with_store(Arc::new(mock));
        assert!(manager.list_targets().is_err());
    }

    #[test]
    fn qualify_target_is_idempotent() {
        assert_eq!(qualify_target("openai"), "VTrans:openai");
        assert_eq!(qualify_target("VTrans:openai"), "VTrans:openai");
    }

    #[test]
    fn provider_store_load_roundtrip_for_every_target() {
        let (manager, raw) = in_memory_manager();
        for (i, target) in CredentialTarget::ALL.iter().enumerate() {
            let key = format!("sk-provider-{i}-0123456789");
            manager.store_for_provider(*target, &key).unwrap();
            assert_eq!(
                manager.load_for_provider(*target).unwrap().as_deref(),
                Some(key.as_str()),
                "roundtrip failed for {target}"
            );
            // The secret lands under the namespaced logical target.
            let stored = format!("VTrans:{}", target.as_str());
            assert!(
                raw.load(&stored).unwrap().is_some(),
                "expected {stored:?} in the raw store"
            );
        }
    }

    #[test]
    fn provider_targets_are_namespace_isolated() {
        let (manager, raw) = in_memory_manager();
        manager
            .store_for_provider(CredentialTarget::OpenAI, "sk-openai-key")
            .unwrap();
        manager
            .store_for_provider(CredentialTarget::Azure, "sk-azure-key")
            .unwrap();

        assert_eq!(
            raw.load("VTrans:openai").unwrap().as_deref(),
            Some(b"sk-openai-key".as_slice())
        );
        assert_eq!(
            raw.load("VTrans:azure").unwrap().as_deref(),
            Some(b"sk-azure-key".as_slice())
        );
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::OpenAI)
                .unwrap()
                .as_deref(),
            Some("sk-openai-key")
        );
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::Azure)
                .unwrap()
                .as_deref(),
            Some("sk-azure-key")
        );
    }

    #[test]
    fn baidu_app_id_and_secret_are_stored_independently() {
        let (manager, raw) = in_memory_manager();
        manager
            .store_for_provider(CredentialTarget::BaiduAppId, "202401010001")
            .unwrap();
        manager
            .store_for_provider(CredentialTarget::BaiduSecret, "sk-baidu-secret-0123456789")
            .unwrap();

        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::BaiduAppId)
                .unwrap()
                .as_deref(),
            Some("202401010001")
        );
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::BaiduSecret)
                .unwrap()
                .as_deref(),
            Some("sk-baidu-secret-0123456789")
        );
        assert_eq!(
            raw.load("VTrans:baidu_app_id").unwrap().as_deref(),
            Some(b"202401010001".as_slice())
        );
        assert_eq!(
            raw.load("VTrans:baidu_secret").unwrap().as_deref(),
            Some(b"sk-baidu-secret-0123456789".as_slice())
        );

        // Deleting one target leaves the other intact.
        manager
            .delete_for_provider(CredentialTarget::BaiduSecret)
            .unwrap();
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::BaiduSecret)
                .unwrap(),
            None
        );
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::BaiduAppId)
                .unwrap()
                .as_deref(),
            Some("202401010001")
        );
    }

    #[test]
    fn tencent_target_is_reserved_but_functional() {
        let (manager, _) = in_memory_manager();
        assert_eq!(CredentialTarget::Tencent.as_str(), "tencent");
        manager
            .store_for_provider(CredentialTarget::Tencent, "tk-0123456789")
            .unwrap();
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::Tencent)
                .unwrap()
                .as_deref(),
            Some("tk-0123456789")
        );
    }

    #[test]
    fn provider_load_missing_returns_none() {
        let (manager, _) = in_memory_manager();
        for target in CredentialTarget::ALL {
            assert_eq!(
                manager.load_for_provider(target).unwrap(),
                None,
                "target {target} should be absent"
            );
        }
    }

    #[test]
    fn provider_delete_missing_returns_not_found() {
        let (manager, _) = in_memory_manager();
        let err = manager
            .delete_for_provider(CredentialTarget::DeepL)
            .unwrap_err();
        assert!(matches!(err, SecurityError::NotFound(_)));
    }

    #[test]
    fn provider_store_overwrites_existing_value() {
        let (manager, _) = in_memory_manager();
        manager
            .store_for_provider(CredentialTarget::Google, "sk-old-key")
            .unwrap();
        manager
            .store_for_provider(CredentialTarget::Google, "sk-new-key")
            .unwrap();
        assert_eq!(
            manager
                .load_for_provider(CredentialTarget::Google)
                .unwrap()
                .as_deref(),
            Some("sk-new-key")
        );
    }

    #[test]
    fn provider_delete_then_load_returns_none() {
        let (manager, _) = in_memory_manager();
        manager
            .store_for_provider(CredentialTarget::DeepL, "sk-deepl-0123456789")
            .unwrap();
        manager
            .delete_for_provider(CredentialTarget::DeepL)
            .unwrap();
        assert_eq!(
            manager.load_for_provider(CredentialTarget::DeepL).unwrap(),
            None
        );
    }

    #[test]
    fn provider_load_non_utf8_blob_returns_operation_failed() {
        let (manager, raw) = in_memory_manager();
        raw.store("VTrans:google", &[0xff, 0xfe, 0x00]).unwrap();
        let err = manager
            .load_for_provider(CredentialTarget::Google)
            .unwrap_err();
        assert!(matches!(err, SecurityError::OperationFailed(_)));
    }

    #[test]
    fn provider_store_does_not_transform_secret() {
        let (manager, raw) = in_memory_manager();
        let key = "sk-raw-secret-0123456789";
        manager
            .store_for_provider(CredentialTarget::Azure, key)
            .unwrap();
        assert_eq!(raw.load("VTrans:azure").unwrap().unwrap(), key.as_bytes());
    }

    #[test]
    fn provider_load_logs_only_masked_key() {
        let (manager, _) = in_memory_manager();
        let key = "sk-super-secret-value-0123456789";
        manager
            .store_for_provider(CredentialTarget::OpenAI, key)
            .unwrap();

        let buffer = install_test_log_subscriber();
        buffer
            .lock()
            .expect("capture lock should not be poisoned")
            .clear();

        manager
            .load_for_provider(CredentialTarget::OpenAI)
            .expect("load should succeed");

        let log = String::from_utf8(
            buffer
                .lock()
                .expect("capture lock should not be poisoned")
                .clone(),
        )
        .expect("captured log should be valid UTF-8");
        assert!(
            log.contains("sk-s****6789"),
            "log should contain the masked key, got: {log}"
        );
        assert!(!log.contains(key), "raw key leaked into log: {log}");
        assert!(
            !log.contains("super-secret"),
            "raw key material leaked into log: {log}"
        );
    }
}
