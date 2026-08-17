//! `VTrans` credential security module.
//!
//! Securely stores and reads API keys through the Windows Credential Manager
//! ([`CredentialManager`]). Keys are never written to plaintext config files
//! and never appear in logs: the only key-related value that may be logged is
//! the masked form produced by [`mask_key`].
//!
//! # Design
//!
//! - [`CredentialManager`] is the application-facing entry point. It owns the
//!   `VTrans:` target namespace and maps logical targets (e.g. `openai`) to
//!   namespaced ones (`VTrans:openai`) so credentials never collide with other
//!   applications. Cloud provider credentials use the typed
//!   [`CredentialTarget`] enum through
//!   [`store_for_provider`](CredentialManager::store_for_provider) and
//!   [`load_for_provider`](CredentialManager::load_for_provider); the legacy
//!   string-based methods remain for backward compatibility.
//! - [`CredentialStore`] is a small trait that decouples the manager from the
//!   concrete backend. [`WindowsCredentialStore`] is the legacy backend backed
//!   by the Windows Credential Manager; [`InMemoryCredentialStore`] exists for
//!   tests and non-Windows development; [`DpapiFileStore`] is the
//!   installation-local backend that keeps user-bound DPAPI-encrypted
//!   credentials in a single caller-provided file (see
//!   [`migrate_windows_to_dpapi`] for the one-time migration from the legacy
//!   vault backend).
//!
//! See `docs/modules/03-security.md` for the full module specification.

pub mod credential_store;
pub mod dpapi;
pub mod manager;
pub mod mask;
pub mod target;

#[cfg(test)]
mod test_log;

pub use credential_store::{CredentialStore, InMemoryCredentialStore, WindowsCredentialStore};
pub use dpapi::{migrate_windows_to_dpapi, DpapiFileStore};
pub use manager::CredentialManager;
pub use mask::mask_key;
pub use target::CredentialTarget;

use thiserror::Error;

/// Errors that can occur while storing or loading credentials.
#[derive(Debug, Error)]
pub enum SecurityError {
    /// The credential store backend cannot be used (e.g. the Windows
    /// Credential Manager vault is not accessible).
    #[error("credential store unavailable: {0}")]
    StoreUnavailable(String),

    /// The requested credential does not exist for the given target.
    #[error("credential not found for target: {0}")]
    NotFound(String),

    /// A credential operation failed for a non-OS reason (e.g. data stored in
    /// the vault is not valid UTF-8).
    #[error("credential operation failed: {0}")]
    OperationFailed(String),

    /// The underlying Windows API call failed.
    #[error("windows api error: {0}")]
    WindowsApi(String),

    /// An IO operation on the credential file failed (open, read, write or
    /// rename of the [`DpapiFileStore`](crate::DpapiFileStore) container).
    #[error("credential file io error: {0}")]
    FileIo(#[from] std::io::Error),

    /// The credential file is structurally invalid: truncated, not a
    /// `VTrans` container, or carrying an unsupported version. The stored
    /// secrets cannot be recovered from such a file.
    #[error("credential file is corrupted: {0}")]
    CorruptedFile(String),

    /// A stored DPAPI blob could not be decrypted, e.g. because it was
    /// tampered with or was protected under a different Windows user account.
    #[error("credential decryption failed: {0}")]
    DecryptionFailed(String),
}
