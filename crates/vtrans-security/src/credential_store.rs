#![allow(unsafe_code)]
//! Credential store abstraction and backends.
//!
//! # Safety
//!
//! This module contains the only `unsafe` code in the crate: FFI calls into
//! the Windows Credential Manager. `unsafe_code` is allowed here (the
//! workspace default is `warn`) because the FFI boundary is unavoidable; every
//! `unsafe` block carries an adjacent `// SAFETY:` comment stating the
//! invariants that make the call sound.
//!
//! [`CredentialStore`] decouples [`crate::CredentialManager`] from the
//! concrete backend. Two backends ship with the crate:
//!
//! - [`WindowsCredentialStore`] — production backend backed by the Windows
//!   Credential Manager (`advapi32.dll`). Credentials are stored with
//!   `CRED_TYPE_GENERIC`, persisted locally, and the blob is never written to
//!   disk by this crate (the OS vault handles persistence).
//! - [`InMemoryCredentialStore`] — test/development backend that keeps
//!   secrets in process memory only.

use std::collections::HashMap;
use std::sync::Mutex;

use tracing::warn;
use windows::core::{Error as Win32Error, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredEnumerateW, CredFree, CredReadW, CredWriteW, CREDENTIALW,
    CRED_ENUMERATE_ALL_CREDENTIALS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};

use crate::SecurityError;

/// Backend used to persist credential blobs.
///
/// Implementations store opaque bytes keyed by a fully-qualified target name
/// (the `VTrans:` prefix is applied by [`crate::CredentialManager`], not by
/// the store). All methods are synchronous and take `&self` because the
/// Windows backend is stateless (the OS vault owns the state) and the
/// in-memory backend uses interior mutability.
///
/// # Example
///
/// ```
/// use vtrans_security::credential_store::InMemoryCredentialStore;
/// use vtrans_security::CredentialStore;
///
/// let store = InMemoryCredentialStore::new();
/// store.store("openai", b"sk-1234").unwrap();
/// assert_eq!(store.load("openai").unwrap(), Some(b"sk-1234".to_vec()));
/// assert_eq!(store.load("missing").unwrap(), None);
/// ```
#[cfg_attr(test, mockall::automock)]
pub trait CredentialStore: Send + Sync {
    /// Stores `secret` under `target`, overwriting any existing value.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::OperationFailed`] or
    /// [`SecurityError::WindowsApi`] when the backend cannot persist the
    /// credential.
    fn store(&self, target: &str, secret: &[u8]) -> Result<(), SecurityError>;

    /// Reads the secret stored under `target`.
    ///
    /// Returns `Ok(None)` when no credential exists for `target`.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::OperationFailed`] or
    /// [`SecurityError::WindowsApi`] when the backend cannot read the
    /// credential.
    fn load(&self, target: &str) -> Result<Option<Vec<u8>>, SecurityError>;

    /// Deletes the credential stored under `target`.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::NotFound`] when no credential exists for
    /// `target`, or a backend error when the deletion itself fails.
    fn delete(&self, target: &str) -> Result<(), SecurityError>;

    /// Lists all stored target names, without any secret data.
    ///
    /// # Errors
    ///
    /// Returns a backend error when enumeration fails.
    fn list_targets(&self) -> Result<Vec<String>, SecurityError>;
}

/// Backend backed by the Windows Credential Manager.
///
/// Wraps `CredWriteW` / `CredReadW` / `CredDeleteW` / `CredEnumerateW` from
/// `advapi32.dll`. Credentials are stored as `CRED_TYPE_GENERIC` with
/// `CRED_PERSIST_LOCAL_MACHINE` persistence and no roaming, keeping keys on
/// the local machine.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsCredentialStore;

impl WindowsCredentialStore {
    /// Creates a new backend.
    ///
    /// Construction never touches the OS vault; actual availability is
    /// reported by the first operation.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl CredentialStore for WindowsCredentialStore {
    fn store(&self, target: &str, secret: &[u8]) -> Result<(), SecurityError> {
        let blob_size = u32::try_from(secret.len()).map_err(|_| {
            SecurityError::OperationFailed("credential blob exceeds 4 GiB limit".to_string())
        })?;
        let target_wide = to_wide(target);
        let empty_wide = to_wide("");

        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target_wide.as_ptr().cast_mut()),
            CredentialBlobSize: blob_size,
            CredentialBlob: secret.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(empty_wide.as_ptr().cast_mut()),
            ..Default::default()
        };

        // SAFETY: `credential` is a fully initialized CREDENTIALW whose string
        // and blob pointers reference buffers that remain alive for the
        // duration of the call. CredWriteW copies the blob and strings into
        // the vault synchronously and does not retain any of the pointers.
        // The secret buffer is only read by the OS; the const-to-mut cast is
        // therefore safe because the callee never writes through it.
        unsafe { CredWriteW(&credential, 0) }.map_err(|e| map_windows_error(&e, "CredWriteW"))?;

        Ok(())
    }

    fn load(&self, target: &str) -> Result<Option<Vec<u8>>, SecurityError> {
        let target_wide = to_wide(target);
        let mut credential: *mut CREDENTIALW = std::ptr::null_mut();

        // SAFETY: `target_wide` is a null-terminated UTF-16 buffer that stays
        // alive for the call. `credential` receives a heap allocation owned by
        // the OS on success and must be released with CredFree.
        let result = unsafe {
            CredReadW(
                PCWSTR(target_wide.as_ptr()),
                CRED_TYPE_GENERIC,
                0,
                &mut credential,
            )
        };

        if let Err(e) = result {
            if is_not_found(&e) {
                return Ok(None);
            }
            return Err(map_windows_error(&e, "CredReadW"));
        }

        let blob = {
            // SAFETY: a successful CredReadW guarantees `credential` is
            // non-null and points to a valid CREDENTIALW owned by the OS.
            let cred = unsafe { &*credential };
            read_blob(cred)
        };

        // SAFETY: CredFree releases the buffer that CredReadW allocated. It is
        // called even when copying the blob failed so no allocation leaks.
        unsafe { CredFree(credential.cast::<core::ffi::c_void>()) };

        blob.map(Some)
    }

    fn delete(&self, target: &str) -> Result<(), SecurityError> {
        let target_wide = to_wide(target);

        // SAFETY: `target_wide` is a null-terminated UTF-16 buffer that stays
        // alive for the call.
        let result = unsafe { CredDeleteW(PCWSTR(target_wide.as_ptr()), CRED_TYPE_GENERIC, 0) };

        match result {
            Ok(()) => Ok(()),
            Err(e) if is_not_found(&e) => {
                warn!(target = %target, "attempted to delete a credential that does not exist");
                Err(SecurityError::NotFound(target.to_string()))
            }
            Err(e) => Err(map_windows_error(&e, "CredDeleteW")),
        }
    }

    fn list_targets(&self) -> Result<Vec<String>, SecurityError> {
        let mut count: u32 = 0;
        let mut credentials: *mut *mut CREDENTIALW = std::ptr::null_mut();

        // SAFETY: a null filter enumerates every credential in the user's
        // vault. On success `credentials` points to an array of `count`
        // CREDENTIALW pointers that must be released with CredFree.
        unsafe {
            CredEnumerateW(
                PCWSTR::null(),
                CRED_ENUMERATE_ALL_CREDENTIALS,
                &mut count,
                &mut credentials,
            )
        }
        .map_err(|e| map_windows_error(&e, "CredEnumerateW"))?;

        let count = usize::try_from(count).map_err(|_| {
            SecurityError::OperationFailed("credential count out of range".to_string())
        })?;
        let targets = {
            // SAFETY: a successful CredEnumerateW guarantees `credentials`
            // points to an array of exactly `count` pointers.
            let slice = unsafe { std::slice::from_raw_parts(credentials, count) };

            let mut targets = Vec::with_capacity(slice.len());
            for entry in slice {
                if entry.is_null() {
                    continue;
                }
                // SAFETY: each element of the array is a valid CREDENTIALW.
                let cred = unsafe { &**entry };
                if cred.Type != CRED_TYPE_GENERIC {
                    continue;
                }
                // SAFETY: TargetName is a null-terminated UTF-16 string
                // allocated by the OS. For generic credentials Windows reports
                // a qualified name such as `LegacyGeneric:target=<name>`;
                // stripping that wrapper yields the name passed to `store`.
                let name = unsafe { cred.TargetName.as_wide() };
                let name = String::from_utf16_lossy(name);
                targets.push(
                    name.strip_prefix("LegacyGeneric:target=")
                        .unwrap_or(&name)
                        .to_string(),
                );
            }
            targets
        };

        // SAFETY: CredFree releases the array allocated by CredEnumerateW.
        unsafe { CredFree(credentials.cast::<core::ffi::c_void>()) };

        Ok(targets)
    }
}

/// Test/dev backend that keeps credentials in process memory.
///
/// Nothing is ever persisted; the store is emptied when the process exits.
/// Useful for unit tests and for running the application on platforms where
/// the Windows Credential Manager is unavailable.
#[derive(Debug, Default)]
pub struct InMemoryCredentialStore {
    secrets: Mutex<HashMap<String, Vec<u8>>>,
}

impl InMemoryCredentialStore {
    /// Creates an empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn store(&self, target: &str, secret: &[u8]) -> Result<(), SecurityError> {
        let mut secrets = lock_secrets(&self.secrets)?;
        secrets.insert(target.to_string(), secret.to_vec());
        Ok(())
    }

    fn load(&self, target: &str) -> Result<Option<Vec<u8>>, SecurityError> {
        let secrets = lock_secrets(&self.secrets)?;
        Ok(secrets.get(target).cloned())
    }

    fn delete(&self, target: &str) -> Result<(), SecurityError> {
        let mut secrets = lock_secrets(&self.secrets)?;
        if secrets.remove(target).is_some() {
            Ok(())
        } else {
            Err(SecurityError::NotFound(target.to_string()))
        }
    }

    fn list_targets(&self) -> Result<Vec<String>, SecurityError> {
        let secrets = lock_secrets(&self.secrets)?;
        let mut targets: Vec<String> = secrets.keys().cloned().collect();
        targets.sort();
        Ok(targets)
    }
}

fn lock_secrets(
    mutex: &Mutex<HashMap<String, Vec<u8>>>,
) -> Result<std::sync::MutexGuard<'_, HashMap<String, Vec<u8>>>, SecurityError> {
    mutex.lock().map_err(|_| {
        SecurityError::OperationFailed("in-memory credential store is poisoned".to_string())
    })
}

/// Encodes a string as a null-terminated UTF-16 buffer for FFI calls.
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copies the credential blob out of a CREDENTIALW returned by `CredReadW`.
///
/// The caller is responsible for calling `CredFree` on the owning buffer even
/// when this function returns an error.
fn read_blob(credential: &CREDENTIALW) -> Result<Vec<u8>, SecurityError> {
    let blob_size = usize::try_from(credential.CredentialBlobSize).map_err(|_| {
        SecurityError::OperationFailed("credential blob size is out of range".to_string())
    })?;
    if blob_size == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: the OS guarantees CredentialBlob points to a readable buffer of
    // CredentialBlobSize bytes for the lifetime of the CREDENTIALW returned by
    // CredReadW. The buffer is copied before the caller frees it.
    Ok(unsafe { std::slice::from_raw_parts(credential.CredentialBlob, blob_size) }.to_vec())
}

/// Maps a `windows` crate error to [`SecurityError::WindowsApi`].
fn map_windows_error(error: &Win32Error, operation: &str) -> SecurityError {
    let message = format!("{operation} failed: {error} (hr=0x{:08X})", error.code().0);
    warn!(error = %message, "windows credential api call failed");
    SecurityError::WindowsApi(message)
}

/// Returns `true` when the Windows error is `ERROR_NOT_FOUND` (1168), which
/// is how the credential APIs report a missing credential.
fn is_not_found(error: &Win32Error) -> bool {
    error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0)
}

#[cfg(test)]
mod tests {
    use super::{CredentialStore, InMemoryCredentialStore};

    #[test]
    fn in_memory_store_roundtrip() {
        let store = InMemoryCredentialStore::new();
        store.store("openai", b"sk-1234").unwrap();
        assert_eq!(store.load("openai").unwrap(), Some(b"sk-1234".to_vec()));
    }

    #[test]
    fn in_memory_store_load_missing_returns_none() {
        let store = InMemoryCredentialStore::new();
        assert_eq!(store.load("missing").unwrap(), None);
    }

    #[test]
    fn in_memory_store_overwrites_existing_value() {
        let store = InMemoryCredentialStore::new();
        store.store("openai", b"old").unwrap();
        store.store("openai", b"new").unwrap();
        assert_eq!(store.load("openai").unwrap(), Some(b"new".to_vec()));
    }

    #[test]
    fn in_memory_store_delete() {
        let store = InMemoryCredentialStore::new();
        store.store("openai", b"sk-1234").unwrap();
        store.delete("openai").unwrap();
        assert_eq!(store.load("openai").unwrap(), None);
    }

    #[test]
    fn in_memory_store_delete_missing_returns_not_found() {
        let store = InMemoryCredentialStore::new();
        let err = store.delete("missing").unwrap_err();
        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn in_memory_store_list_targets_sorted() {
        let store = InMemoryCredentialStore::new();
        store.store("beta", b"1").unwrap();
        store.store("alpha", b"2").unwrap();
        store.store("gamma", b"3").unwrap();
        assert_eq!(store.list_targets().unwrap(), ["alpha", "beta", "gamma"]);
    }
}
