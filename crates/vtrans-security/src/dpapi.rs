#![allow(unsafe_code)]
//! Installation-local credential backend: [`DpapiFileStore`].
//!
//! # Safety
//!
//! Like [`crate::credential_store`], this module contains the only other
//! `unsafe` code in the crate: FFI calls into the Windows Data Protection API
//! (`crypt32.dll`). Every `unsafe` block carries an adjacent `// SAFETY:`
//! comment stating the invariants that make the call sound (pointer/length
//! validity, ownership of `LocalAlloc`-backed buffers).
//!
//! # Design
//!
//! [`DpapiFileStore`] keeps every credential in a single container file whose
//! path is supplied by the caller (the `data/` root is decided by the
//! application layer, not by this crate). Secrets are protected with
//! `CryptProtectData` before they touch the file, so plaintext keys never
//! appear on disk. DPAPI blobs are bound to the Windows user profile that
//! created them plus a fixed application entropy constant.
//!
//! The container is a small length-prefixed binary format:
//!
//! ```text
//! magic "VTRANCRD" | u32 LE version | u32 LE entry count |
//! per entry: u32 LE target_len | target bytes (UTF-8) |
//!            u32 LE blob_len | encrypted blob bytes
//! ```
//!
//! An empty (zero-byte) file is a valid empty container. Every mutation is
//! serialized through a mutex and written atomically (unique temporary file
//! in the same directory, flushed, then renamed over the container), matching
//! the write style of `vtrans-config`'s `ConfigManager`.

use std::collections::BTreeMap;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use tracing::{debug, info, warn};
use windows::core::{Error as Win32Error, PCWSTR};
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use crate::credential_store::{map_windows_error, CredentialStore, WindowsCredentialStore};
use crate::{mask_key, SecurityError};

/// Container file magic: identifies the file as a `VTrans` credential store.
const MAGIC: &[u8; 8] = b"VTRANCRD";
/// Container format version. Must be bumped whenever the on-disk layout
/// changes; old versions are rejected, not silently reinterpreted.
const VERSION: u32 = 1;
/// Upper bound for a stored target name (defense against corrupted lengths).
const MAX_TARGET_LEN: usize = 4096;
/// Upper bound for a plaintext secret accepted by [`DpapiFileStore::store`].
///
/// API keys are a few hundred bytes; the cap only guards against runaway
/// allocations.
const MAX_SECRET_LEN: usize = 16 * 1024 * 1024;
/// Upper bound for an encrypted blob read back from the container.
///
/// Must stay above `MAX_SECRET_LEN` plus DPAPI overhead so that every blob
/// written by this crate can be parsed back.
const MAX_BLOB_LEN: usize = 64 * 1024 * 1024;
/// Application entropy mixed into every DPAPI operation.
///
/// Not a secret: it only additionally binds the blobs to this application so
/// other software in the same user context cannot trivially decrypt them.
/// Changing this value orphans all previously stored credentials; it must
/// only change through a migration.
const ENTROPY: &[u8] = b"VTrans.CredentialFileStore.v1";

/// Credential backend that stores user-bound DPAPI-encrypted secrets in one
/// caller-provided container file.
///
/// The file path is supplied by the caller ([`new`](Self::new)); this crate
/// never assumes a fixed location, so the installation-local `data/` root
/// stays under application control. Every secret is protected with
/// `CryptProtectData` (Windows DPAPI, bound to the Windows user profile)
/// before it is written, so the container file never contains plaintext
/// keys.
///
/// All methods take `&self` and are safe to call concurrently: file access is
/// serialized by an internal mutex and every mutation is written atomically
/// (temporary file + rename), so a crash can never leave a half-written
/// container behind.
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use vtrans_security::{CredentialStore, DpapiFileStore};
///
/// let store = DpapiFileStore::new(Path::new(r"C:\VTrans\data\credentials.bin"))?;
/// store.store("VTrans:openai", b"sk-1234567890")?;
/// assert_eq!(store.load("VTrans:openai")?, Some(b"sk-1234567890".to_vec()));
/// # Ok::<(), vtrans_security::SecurityError>(())
/// ```
#[derive(Debug)]
pub struct DpapiFileStore {
    path: PathBuf,
    /// Serializes every read-modify-write cycle so concurrent calls cannot
    /// interleave and lose updates.
    lock: Mutex<()>,
}

impl DpapiFileStore {
    /// Opens or creates the container file at `path`.
    ///
    /// When the file does not exist yet an empty container is created; the
    /// real content is only written by the first [`store`](Self::store) call.
    /// An existing file is opened without truncation so previously stored
    /// credentials are preserved. The parent directory must already exist.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::FileIo`] when the file cannot be opened or
    /// created (e.g. missing parent directory or insufficient permissions).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::path::Path;
    /// use vtrans_security::DpapiFileStore;
    ///
    /// let store = DpapiFileStore::new(Path::new(r"C:\VTrans\data\credentials.bin"))?;
    /// # Ok::<(), vtrans_security::SecurityError>(())
    /// ```
    #[tracing::instrument(skip_all, fields(path = %path.display()))]
    pub fn new(path: &Path) -> Result<Self, SecurityError> {
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            // Create without truncating: an existing container must keep its
            // previously stored credentials.
            .truncate(false)
            .open(path)
            .map_err(|e| file_io_error("open credential file", e))?;
        debug!(path = %path.display(), "credential file store ready");
        Ok(Self {
            path: path.to_path_buf(),
            lock: Mutex::new(()),
        })
    }

    /// Returns the container file path supplied at construction time.
    ///
    /// # Example
    ///
    /// ```
    /// use std::path::Path;
    /// use vtrans_security::DpapiFileStore;
    ///
    /// # fn run() -> Result<(), vtrans_security::SecurityError> {
    /// let store = DpapiFileStore::new(Path::new("credentials.bin"))?;
    /// assert_eq!(store.path(), Path::new("credentials.bin"));
    /// # Ok(()) }
    /// # run().unwrap();
    /// ```
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads and parses the container into the in-memory entry map.
    ///
    /// A missing or empty file is a valid empty container; a structurally
    /// invalid file is reported as [`SecurityError::CorruptedFile`].
    fn read_entries(&self) -> Result<BTreeMap<String, Vec<u8>>, SecurityError> {
        let data = match std::fs::read(&self.path) {
            Ok(data) => data,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(file_io_error("read credential file", e)),
        };
        parse(&data)
    }

    /// Serializes `entries` and atomically replaces the container file.
    fn write_entries(&self, entries: &BTreeMap<String, Vec<u8>>) -> Result<(), SecurityError> {
        let bytes = serialize(entries)?;
        if self.path.parent().is_none() {
            return Err(file_io_error(
                "replace credential file",
                io::Error::new(
                    ErrorKind::InvalidInput,
                    "credential file path has no parent directory",
                ),
            ));
        }
        let temp_path = temp_path_for(&self.path);
        let result = (|| -> Result<(), SecurityError> {
            let mut file = std::fs::File::create(&temp_path)
                .map_err(|e| file_io_error("create temporary credential file", e))?;
            file.write_all(&bytes)
                .map_err(|e| file_io_error("write temporary credential file", e))?;
            // Flush before the rename so a crash after the rename can never
            // leave an empty or truncated container behind.
            file.sync_all()
                .map_err(|e| file_io_error("sync temporary credential file", e))?;
            // On Windows the handle must be closed before the rename replaces
            // the target.
            drop(file);
            std::fs::rename(&temp_path, &self.path)
                .map_err(|e| file_io_error("replace credential file", e))?;
            Ok(())
        })();
        if result.is_err() {
            // Best effort: a stale temp file would be overwritten by the next
            // write anyway, but keep the directory clean.
            let _ = std::fs::remove_file(&temp_path);
        }
        result
    }
}

impl CredentialStore for DpapiFileStore {
    #[tracing::instrument(skip(self, secret), fields(target = %target, path = %self.path.display()))]
    fn store(&self, target: &str, secret: &[u8]) -> Result<(), SecurityError> {
        if target.len() > MAX_TARGET_LEN {
            let message = format!("target name exceeds the {MAX_TARGET_LEN} byte limit");
            warn!(error = %message, "target name is too long to store");
            return Err(SecurityError::OperationFailed(message));
        }
        if secret.len() > MAX_SECRET_LEN {
            let message = format!("secret exceeds the {MAX_SECRET_LEN} byte limit");
            warn!(error = %message, "secret is too large to store");
            return Err(SecurityError::OperationFailed(message));
        }
        // Encrypt before taking the lock: DPAPI does not touch shared state
        // and the lock should only cover the read-modify-write cycle.
        //
        // DPAPI rejects zero-length input blobs (ERROR_INVALID_PARAMETER), so
        // an empty secret is stored as an empty blob: it contains nothing
        // secret and `load` returns the empty plaintext without a decryption
        // round trip.
        let protected = if secret.is_empty() {
            Vec::new()
        } else {
            protect(secret)?
        };

        let _guard = lock_store(&self.lock)?;
        let mut entries = self.read_entries()?;
        entries.insert(target.to_string(), protected);
        self.write_entries(&entries)?;
        debug!(target = %target, "credential stored in dpapi file store");
        Ok(())
    }

    #[tracing::instrument(skip_all, fields(target = %target, path = %self.path.display()))]
    fn load(&self, target: &str) -> Result<Option<Vec<u8>>, SecurityError> {
        let _guard = lock_store(&self.lock)?;
        let entries = self.read_entries()?;
        let Some(blob) = entries.get(target) else {
            return Ok(None);
        };
        // An empty blob is the stored form of an empty secret (DPAPI rejects
        // zero-length input), so it decrypts to the empty plaintext directly.
        let secret = if blob.is_empty() {
            Vec::new()
        } else {
            unprotect(blob)?
        };
        debug!(target = %target, "credential loaded from dpapi file store");
        Ok(Some(secret))
    }

    #[tracing::instrument(skip_all, fields(target = %target, path = %self.path.display()))]
    fn delete(&self, target: &str) -> Result<(), SecurityError> {
        let _guard = lock_store(&self.lock)?;
        let mut entries = self.read_entries()?;
        if entries.remove(target).is_none() {
            warn!(target = %target, "attempted to delete a credential that does not exist");
            return Err(SecurityError::NotFound(target.to_string()));
        }
        self.write_entries(&entries)?;
        debug!(target = %target, "credential deleted from dpapi file store");
        Ok(())
    }

    #[tracing::instrument(skip_all, fields(path = %self.path.display()))]
    fn list_targets(&self) -> Result<Vec<String>, SecurityError> {
        let _guard = lock_store(&self.lock)?;
        let entries = self.read_entries()?;
        // BTreeMap iteration is sorted; only names are returned, never blobs.
        Ok(entries.keys().cloned().collect())
    }
}

/// Migrates every `VTrans:` credential from the legacy Windows Credential
/// Manager backend into `new_store`, one entry at a time.
///
/// Entries are read from the Windows Credential Manager, written into the
/// DPAPI file store, and only then deleted from the legacy vault, so a failed
/// write never loses data. Failures of individual entries are tolerated and
/// reported in the logs; the returned count is the number of entries that
/// were successfully migrated (0 when there is nothing to migrate, which is
/// not an error). Re-running the migration is safe: already-migrated entries
/// are overwritten in the file store and their legacy entries are deleted
/// again if a previous run left them behind.
///
/// Targets are migrated under their fully-qualified `VTrans:` names so the
/// resulting store is directly usable as the backend of
/// [`CredentialManager::with_store`](crate::CredentialManager::with_store).
///
/// # Errors
///
/// Returns an error only when the legacy vault cannot be enumerated at all
/// ([`SecurityError::WindowsApi`]). Per-entry failures are tolerated, not
/// propagated.
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use vtrans_security::{migrate_windows_to_dpapi, DpapiFileStore};
///
/// let store = DpapiFileStore::new(Path::new(r"C:\VTrans\data\credentials.bin"))?;
/// let migrated = migrate_windows_to_dpapi(&store)?;
/// println!("migrated {migrated} credentials");
/// # Ok::<(), vtrans_security::SecurityError>(())
/// ```
#[tracing::instrument(skip(new_store), fields(path = %new_store.path().display()))]
pub fn migrate_windows_to_dpapi(new_store: &DpapiFileStore) -> Result<usize, SecurityError> {
    migrate_to_dpapi(&WindowsCredentialStore::new(), new_store)
}

/// Backend-agnostic core of [`migrate_windows_to_dpapi`].
///
/// Kept generic over the source store so unit tests can inject a mock legacy
/// backend without touching the real Windows Credential Manager.
#[tracing::instrument(skip(old_store, new_store), fields(path = %new_store.path().display()))]
fn migrate_to_dpapi<S: CredentialStore>(
    old_store: &S,
    new_store: &DpapiFileStore,
) -> Result<usize, SecurityError> {
    let targets = old_store.list_targets()?;
    let candidates: Vec<&str> = targets
        .iter()
        .map(String::as_str)
        .filter(|target| target.starts_with(crate::manager::TARGET_PREFIX))
        .collect();
    let candidate_count = candidates.len();

    let mut migrated = 0usize;
    for target in candidates {
        match migrate_one(old_store, new_store, target) {
            Ok(true) => migrated += 1,
            Ok(false) => {}
            Err(e) => warn!(
                target = %target,
                error = %e,
                "failed to migrate one credential; continuing with the remaining entries"
            ),
        }
    }
    info!(
        migrated,
        candidates = candidate_count,
        "windows credential manager migration finished"
    );
    Ok(migrated)
}

/// Migrates a single credential: load from the legacy store, write to the
/// file store, then delete from the legacy store.
///
/// Returns `Ok(true)` when the entry was written to the new store (even if
/// the legacy deletion afterwards failed — the value is safe and a later run
/// retries the deletion), and `Ok(false)` when the entry vanished between
/// enumeration and load.
fn migrate_one(
    old_store: &dyn CredentialStore,
    new_store: &DpapiFileStore,
    target: &str,
) -> Result<bool, SecurityError> {
    let Some(blob) = old_store.load(target)? else {
        debug!(target = %target, "credential disappeared before migration; skipping");
        return Ok(false);
    };
    let secret = String::from_utf8(blob).map_err(|e| {
        let message =
            format!("legacy credential for target '{target}' is not valid UTF-8; skipping: {e}");
        warn!(error = %message, "legacy credential is not valid UTF-8");
        SecurityError::OperationFailed(message)
    })?;

    new_store.store(target, secret.as_bytes())?;
    if let Err(e) = old_store.delete(target) {
        warn!(
            target = %target,
            error = %e,
            "credential was migrated to the file store but the legacy entry could not be deleted; a later migration run will retry"
        );
    }
    debug!(target = %target, masked_key = %mask_key(&secret), "credential migrated to the dpapi file store");
    Ok(true)
}

/// Locks the store's mutex, mapping poisoning to a backend error.
fn lock_store(lock: &Mutex<()>) -> Result<std::sync::MutexGuard<'_, ()>, SecurityError> {
    lock.lock().map_err(|_| {
        SecurityError::OperationFailed("dpapi file credential store is poisoned".to_string())
    })
}

/// Protects `secret` with DPAPI (`CryptProtectData`, user-bound, plus the
/// application entropy constant). Returns the opaque ciphertext blob.
fn protect(secret: &[u8]) -> Result<Vec<u8>, SecurityError> {
    let blob_size = u32::try_from(secret.len()).map_err(|_| {
        let message = "credential blob exceeds the 4 GiB limit".to_string();
        warn!(error = %message, "credential blob is too large to protect");
        SecurityError::OperationFailed(message)
    })?;
    // A zero-length input has no valid pointer; pass null in that case.
    let pb_data = if secret.is_empty() {
        std::ptr::null_mut()
    } else {
        secret.as_ptr().cast_mut()
    };
    let data_in = CRYPT_INTEGER_BLOB {
        cbData: blob_size,
        pbData: pb_data,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(ENTROPY.len())
            .map_err(|_| SecurityError::OperationFailed("entropy is too large".to_string()))?,
        pbData: ENTROPY.as_ptr().cast_mut(),
    };
    let mut data_out = CRYPT_INTEGER_BLOB::default();

    // SAFETY: `data_in` references `secret` and `entropy` references the
    // static ENTROPY constant; both remain alive and immutable for the
    // duration of the call. On success `data_out` receives a LocalAlloc
    // buffer that is copied out and released with LocalFree before this
    // function returns; the callee does not retain any pointer.
    let result = unsafe {
        CryptProtectData(
            &data_in,
            PCWSTR::null(),
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut data_out,
        )
    };

    let protected = match result {
        Ok(()) => {
            let copied = copy_blob(&data_out);
            // SAFETY: a successful CryptProtectData returns a buffer owned by
            // the caller that must be released with LocalFree. It is released
            // even when copying the bytes out failed so no allocation leaks.
            unsafe { LocalFree(HLOCAL(data_out.pbData.cast())) };
            copied?
        }
        Err(e) => return Err(map_windows_error(&e, "CryptProtectData")),
    };
    Ok(protected)
}

/// Unprotects a DPAPI blob produced by [`protect`] with
/// `CryptUnprotectData`, returning the plaintext secret.
fn unprotect(blob: &[u8]) -> Result<Vec<u8>, SecurityError> {
    let blob_size = u32::try_from(blob.len()).map_err(|_| {
        let message = "credential blob exceeds the 4 GiB limit".to_string();
        warn!(error = %message, "credential blob is too large to decrypt");
        SecurityError::OperationFailed(message)
    })?;
    let pb_data = if blob.is_empty() {
        std::ptr::null_mut()
    } else {
        blob.as_ptr().cast_mut()
    };
    let data_in = CRYPT_INTEGER_BLOB {
        cbData: blob_size,
        pbData: pb_data,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(ENTROPY.len())
            .map_err(|_| SecurityError::OperationFailed("entropy is too large".to_string()))?,
        pbData: ENTROPY.as_ptr().cast_mut(),
    };
    let mut data_out = CRYPT_INTEGER_BLOB::default();

    // SAFETY: `data_in` references `blob` and `entropy` references the static
    // ENTROPY constant; both remain alive and immutable for the duration of
    // the call. On success `data_out` receives a LocalAlloc buffer that is
    // copied out and released with LocalFree before this function returns;
    // the callee does not retain any pointer.
    let result = unsafe {
        CryptUnprotectData(
            &data_in,
            None,
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut data_out,
        )
    };

    match result {
        Ok(()) => {
            let copied = copy_blob(&data_out);
            // SAFETY: a successful CryptUnprotectData returns a buffer owned
            // by the caller that must be released with LocalFree. It is
            // released even when copying the bytes out failed so no
            // allocation leaks.
            unsafe { LocalFree(HLOCAL(data_out.pbData.cast())) };
            Ok(copied?)
        }
        // Any failure here means the stored blob cannot be trusted or was
        // protected under a different user context: report it as a
        // decryption failure, never as a generic OS error.
        Err(e) => Err(decryption_error(&e, "CryptUnprotectData")),
    }
}

/// Copies the bytes out of a DPAPI result blob before it is freed.
fn copy_blob(blob: &CRYPT_INTEGER_BLOB) -> Result<Vec<u8>, SecurityError> {
    let len = usize::try_from(blob.cbData).map_err(|_| {
        let message = "dpapi blob size is out of range".to_string();
        warn!(error = %message, "dpapi returned an invalid blob size");
        SecurityError::OperationFailed(message)
    })?;
    if len == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: on a successful DPAPI call `pbData` points to a readable buffer
    // of exactly `cbData` bytes allocated with LocalAlloc; it is copied out
    // before the caller releases it with LocalFree.
    Ok(unsafe { std::slice::from_raw_parts(blob.pbData, len) }.to_vec())
}

/// Maps a failed `CryptUnprotectData` call to [`SecurityError::DecryptionFailed`].
fn decryption_error(error: &Win32Error, operation: &str) -> SecurityError {
    let message = format!("{operation} failed: {error} (hr=0x{:08X})", error.code().0);
    warn!(error = %message, "stored credential blob could not be decrypted");
    SecurityError::DecryptionFailed(message)
}

/// Maps an IO failure to [`SecurityError::FileIo`] and logs the context.
fn file_io_error(operation: &str, error: io::Error) -> SecurityError {
    warn!(error = %error, operation = %operation, "credential file operation failed");
    SecurityError::FileIo(error)
}

/// Builds a unique temporary path next to the container file, so concurrent
/// threads (and, together with the pid, concurrent processes) never collide.
fn temp_path_for(path: &Path) -> PathBuf {
    static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path.file_name().map_or_else(
        || "credentials.bin".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    path.with_file_name(format!("{file_name}.tmp.{}.{seq}", std::process::id()))
}

/// Serializes the entry map into the container format.
fn serialize(entries: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, SecurityError> {
    let count = u32::try_from(entries.len()).map_err(|_| {
        let message = "too many credential entries to serialize".to_string();
        warn!(error = %message, "credential entry count exceeds the file format limit");
        SecurityError::OperationFailed(message)
    })?;
    let capacity = entries
        .iter()
        .map(|(target, blob)| target.len() + blob.len() + 8)
        .sum::<usize>()
        + MAGIC.len()
        + 8;
    let mut out = Vec::with_capacity(capacity);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    for (target, blob) in entries {
        let target_len = u32::try_from(target.len()).map_err(|_| {
            let message = "target name exceeds the 4 GiB format limit".to_string();
            warn!(error = %message, "target name is too long to serialize");
            SecurityError::OperationFailed(message)
        })?;
        let blob_len = u32::try_from(blob.len()).map_err(|_| {
            let message = "credential blob exceeds the 4 GiB format limit".to_string();
            warn!(error = %message, "credential blob is too large to serialize");
            SecurityError::OperationFailed(message)
        })?;
        out.extend_from_slice(&target_len.to_le_bytes());
        out.extend_from_slice(target.as_bytes());
        out.extend_from_slice(&blob_len.to_le_bytes());
        out.extend_from_slice(blob);
    }
    Ok(out)
}

/// Parses the container format into the entry map.
///
/// An empty (zero-byte) buffer is a valid empty container. Structural
/// problems (bad magic, unsupported version, truncation, length limits,
/// trailing garbage) are reported as [`SecurityError::CorruptedFile`] so a
/// damaged container is never silently reinterpreted as an empty one.
fn parse(data: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, SecurityError> {
    if data.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut rest = data;
    let magic = take(&mut rest, MAGIC.len())?;
    if magic != MAGIC {
        return Err(corrupted(
            "unrecognized header (not a VTrans credential container)",
        ));
    }
    let version = read_u32(&mut rest)?;
    if version != VERSION {
        return Err(corrupted(format!(
            "unsupported container version {version}"
        )));
    }
    let count =
        usize::try_from(read_u32(&mut rest)?).map_err(|_| corrupted("entry count out of range"))?;
    let mut entries = BTreeMap::new();
    for _ in 0..count {
        let target_len = usize::try_from(read_u32(&mut rest)?)
            .map_err(|_| corrupted("target length out of range"))?;
        if target_len > MAX_TARGET_LEN {
            return Err(corrupted(format!(
                "target name too long ({target_len} bytes)"
            )));
        }
        let target_bytes = take(&mut rest, target_len)?;
        let target = std::str::from_utf8(target_bytes)
            .map_err(|_| corrupted("target name is not valid UTF-8"))?
            .to_string();
        let blob_len = usize::try_from(read_u32(&mut rest)?)
            .map_err(|_| corrupted("blob length out of range"))?;
        if blob_len > MAX_BLOB_LEN {
            return Err(corrupted(format!(
                "encrypted blob too large ({blob_len} bytes)"
            )));
        }
        let blob = take(&mut rest, blob_len)?.to_vec();
        entries.insert(target, blob);
    }
    if !rest.is_empty() {
        return Err(corrupted(format!(
            "trailing bytes after {} entries",
            entries.len()
        )));
    }
    Ok(entries)
}

/// Returns a [`SecurityError::CorruptedFile`] with the given reason.
fn corrupted(reason: impl Into<String>) -> SecurityError {
    let message = reason.into();
    warn!(error = %message, "stored credential file is corrupted");
    SecurityError::CorruptedFile(message)
}

/// Takes `len` bytes off the front of `rest`, erroring on truncation.
fn take<'a>(rest: &mut &'a [u8], len: usize) -> Result<&'a [u8], SecurityError> {
    if rest.len() < len {
        return Err(corrupted("file is truncated"));
    }
    let (head, tail) = rest.split_at(len);
    *rest = tail;
    Ok(head)
}

/// Reads a little-endian `u32` off the front of `rest`.
fn read_u32(rest: &mut &[u8]) -> Result<u32, SecurityError> {
    let bytes = take(rest, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mockall::predicate::eq;

    use super::*;
    use crate::credential_store::MockCredentialStore;

    /// Minimal std-only temporary-directory guard for tests.
    ///
    /// Deliberately avoids an external `tempfile` dependency so adding these
    /// tests does not touch the workspace `Cargo.lock`.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "vtrans-security-{}-{name}-{seq}",
                std::process::id()
            ));
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

    /// Creates a store backed by a fresh temporary file.
    fn temp_store() -> (TestDir, DpapiFileStore) {
        let dir = TestDir::new("store");
        let path = dir.path().join("credentials.bin");
        let store = DpapiFileStore::new(&path).expect("store should open a fresh container");
        (dir, store)
    }

    // ---- container format (no DPAPI involved) ----

    #[test]
    fn serialize_then_parse_roundtrip() {
        let mut entries = BTreeMap::new();
        entries.insert("VTrans:openai".to_string(), b"blob-a".to_vec());
        entries.insert("VTrans:azure".to_string(), b"blob-b".to_vec());
        let bytes = serialize(&entries).unwrap();
        assert_eq!(parse(&bytes).unwrap(), entries);
    }

    #[test]
    fn empty_buffer_parses_as_empty_container() {
        assert!(parse(b"").unwrap().is_empty());
    }

    #[test]
    fn bad_magic_is_corrupted_file() {
        let bytes = serialize(&BTreeMap::new()).unwrap();
        let mut bad = bytes;
        bad[0] ^= 0xff;
        let err = parse(&bad).unwrap_err();
        assert!(matches!(err, SecurityError::CorruptedFile(_)));
    }

    #[test]
    fn unsupported_version_is_corrupted_file() {
        let mut bytes = serialize(&BTreeMap::new()).unwrap();
        bytes[MAGIC.len()] = 0xff;
        let err = parse(&bytes).unwrap_err();
        assert!(matches!(err, SecurityError::CorruptedFile(_)));
        assert!(err.to_string().contains("version"));
    }

    #[test]
    fn truncated_file_is_corrupted_file() {
        let bytes = serialize(&BTreeMap::from([(
            "VTrans:openai".to_string(),
            vec![1, 2, 3],
        )]))
        .unwrap();
        for cut in [MAGIC.len(), bytes.len() - 1] {
            let err = parse(&bytes[..cut]).unwrap_err();
            assert!(matches!(err, SecurityError::CorruptedFile(_)));
        }
    }

    #[test]
    fn trailing_garbage_is_corrupted_file() {
        let mut bytes = serialize(&BTreeMap::new()).unwrap();
        bytes.push(0xaa);
        let err = parse(&bytes).unwrap_err();
        assert!(matches!(err, SecurityError::CorruptedFile(_)));
    }

    #[test]
    fn oversized_lengths_are_rejected() {
        let oversized_target_len = u32::try_from(MAX_TARGET_LEN + 1).unwrap();
        let oversized_blob_len = u32::try_from(MAX_BLOB_LEN + 1).unwrap();

        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        // target length beyond the limit
        bytes.extend_from_slice(&oversized_target_len.to_le_bytes());
        let err = parse(&bytes).unwrap_err();
        assert!(matches!(err, SecurityError::CorruptedFile(_)));

        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(b'x');
        // blob length beyond the limit
        bytes.extend_from_slice(&oversized_blob_len.to_le_bytes());
        let err = parse(&bytes).unwrap_err();
        assert!(matches!(err, SecurityError::CorruptedFile(_)));
    }

    #[test]
    fn non_utf8_target_is_rejected() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[0xff, 0xfe]);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let err = parse(&bytes).unwrap_err();
        assert!(matches!(err, SecurityError::CorruptedFile(_)));
    }

    // ---- construction ----

    #[test]
    fn new_creates_empty_container_file() {
        let dir = TestDir::new("new-container");
        let path = dir.path().join("credentials.bin");
        assert!(!path.exists());
        let _store = DpapiFileStore::new(&path).unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    }

    #[test]
    fn new_does_not_truncate_existing_file() {
        let dir = TestDir::new("existing-file");
        let path = dir.path().join("credentials.bin");
        std::fs::write(&path, b"existing-content").unwrap();
        let _store = DpapiFileStore::new(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"existing-content");
    }

    #[test]
    fn new_fails_when_parent_directory_is_missing() {
        let dir = TestDir::new("missing-parent");
        let path = dir.path().join("no-such-dir").join("credentials.bin");
        let err = DpapiFileStore::new(&path).unwrap_err();
        assert!(matches!(err, SecurityError::FileIo(_)));
    }

    // ---- basic behavior without DPAPI (empty container) ----

    #[test]
    fn load_missing_target_on_empty_store_returns_none() {
        let (_dir, store) = temp_store();
        assert_eq!(store.load("VTrans:openai").unwrap(), None);
    }

    #[test]
    fn list_targets_on_empty_store_is_empty() {
        let (_dir, store) = temp_store();
        assert!(store.list_targets().unwrap().is_empty());
    }

    #[test]
    fn delete_missing_target_returns_not_found() {
        let (_dir, store) = temp_store();
        let err = store.delete("VTrans:openai").unwrap_err();
        assert!(matches!(err, SecurityError::NotFound(_)));
    }

    #[test]
    fn corrupted_file_yields_clear_error_on_load() {
        let dir = TestDir::new("corrupted-load");
        let path = dir.path().join("credentials.bin");
        std::fs::write(&path, b"this is not a vtrans container").unwrap();
        let store = DpapiFileStore::new(&path).unwrap();
        let err = store.load("VTrans:openai").unwrap_err();
        assert!(matches!(err, SecurityError::CorruptedFile(_)));
    }

    #[test]
    fn corrupted_file_yields_clear_error_on_list_targets() {
        let dir = TestDir::new("corrupted-list");
        let path = dir.path().join("credentials.bin");
        std::fs::write(&path, b"garbage").unwrap();
        let store = DpapiFileStore::new(&path).unwrap();
        let err = store.list_targets().unwrap_err();
        assert!(matches!(err, SecurityError::CorruptedFile(_)));
    }

    // ---- real DPAPI round trips (Windows only) ----

    #[cfg(windows)]
    #[test]
    fn store_load_roundtrip_with_real_dpapi() {
        let (_dir, store) = temp_store();
        store
            .store("VTrans:openai", b"sk-1234567890abcdef")
            .unwrap();
        assert_eq!(
            store.load("VTrans:openai").unwrap(),
            Some(b"sk-1234567890abcdef".to_vec())
        );
    }

    #[cfg(windows)]
    #[test]
    fn store_overwrites_existing_value() {
        let (_dir, store) = temp_store();
        store.store("VTrans:openai", b"old-key").unwrap();
        store.store("VTrans:openai", b"new-key").unwrap();
        assert_eq!(
            store.load("VTrans:openai").unwrap(),
            Some(b"new-key".to_vec())
        );
    }

    #[cfg(windows)]
    #[test]
    fn delete_then_load_returns_none() {
        let (_dir, store) = temp_store();
        store.store("VTrans:openai", b"sk-to-delete").unwrap();
        store.delete("VTrans:openai").unwrap();
        assert_eq!(store.load("VTrans:openai").unwrap(), None);
    }

    #[cfg(windows)]
    #[test]
    fn empty_secret_roundtrips() {
        let (_dir, store) = temp_store();
        store.store("VTrans:openai", b"").unwrap();
        assert_eq!(store.load("VTrans:openai").unwrap(), Some(Vec::new()));
    }

    #[cfg(windows)]
    #[test]
    fn stored_file_never_contains_plaintext() {
        let (_dir, store) = temp_store();
        let key = b"sk-ultra-secret-key-0123456789";
        store.store("VTrans:openai", key).unwrap();
        let bytes = std::fs::read(store.path()).unwrap();
        assert!(
            !bytes.windows(key.len()).any(|window| window == key),
            "plaintext key found in the container file"
        );
    }

    #[cfg(windows)]
    #[test]
    fn list_targets_sorted_and_without_secrets() {
        let (_dir, store) = temp_store();
        store.store("VTrans:beta", b"sk-secret-beta").unwrap();
        store.store("VTrans:alpha", b"sk-secret-alpha").unwrap();
        assert_eq!(
            store.list_targets().unwrap(),
            ["VTrans:alpha", "VTrans:beta"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn tampered_blob_returns_decryption_error() {
        let dir = TestDir::new("tampered-blob");
        let path = dir.path().join("credentials.bin");
        let store = DpapiFileStore::new(&path).unwrap();
        // Build a structurally valid container whose "encrypted" blob is not
        // a DPAPI blob at all: decryption must fail with a clear error.
        let entries = BTreeMap::from([("VTrans:openai".to_string(), b"not-a-dpapi-blob".to_vec())]);
        std::fs::write(&path, serialize(&entries).unwrap()).unwrap();
        let err = store.load("VTrans:openai").unwrap_err();
        assert!(
            matches!(err, SecurityError::DecryptionFailed(_)),
            "expected DecryptionFailed, got {err:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn store_leaves_no_temporary_files_behind() {
        let dir = TestDir::new("no-temp-files");
        let path = dir.path().join("credentials.bin");
        let store = DpapiFileStore::new(&path).unwrap();
        store.store("VTrans:openai", b"sk-1234").unwrap();
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["credentials.bin"]);
    }

    #[cfg(windows)]
    #[test]
    fn store_and_load_log_never_contain_the_raw_key() {
        let (_dir, store) = temp_store();
        let key = "sk-log-safety-0123456789";
        crate::test_log::clear_captured_log();
        store.store("VTrans:openai", key.as_bytes()).unwrap();
        store.load("VTrans:openai").unwrap();
        let log = crate::test_log::captured_log();
        assert!(
            !log.contains(key),
            "raw key leaked into the dpapi store log: {log}"
        );
    }

    // ---- migration with an injected legacy store ----

    #[test]
    fn migrate_returns_zero_when_no_prefixed_entries_exist() {
        let (_dir, new_store) = temp_store();
        let mut old = MockCredentialStore::new();
        old.expect_list_targets().once().returning(|| {
            Ok(vec![
                "OtherApp:foo".to_string(),
                "LegacyGeneric:target=bar".to_string(),
            ])
        });
        let migrated = migrate_to_dpapi(&old, &new_store).unwrap();
        assert_eq!(migrated, 0);
    }

    #[cfg(windows)]
    #[test]
    fn migrate_moves_prefixed_entries_and_deletes_them() {
        let (_dir, new_store) = temp_store();
        let mut old = MockCredentialStore::new();
        old.expect_list_targets().once().returning(|| {
            Ok(vec![
                "VTrans:openai".to_string(),
                "VTrans:azure".to_string(),
                "OtherApp:foo".to_string(),
            ])
        });
        old.expect_load()
            .with(eq("VTrans:openai"))
            .once()
            .returning(|_| Ok(Some(b"sk-old-openai-key".to_vec())));
        old.expect_load()
            .with(eq("VTrans:azure"))
            .once()
            .returning(|_| Ok(Some(b"sk-old-azure-key".to_vec())));
        old.expect_delete()
            .with(eq("VTrans:openai"))
            .once()
            .returning(|_| Ok(()));
        old.expect_delete()
            .with(eq("VTrans:azure"))
            .once()
            .returning(|_| Ok(()));

        let migrated = migrate_to_dpapi(&old, &new_store).unwrap();
        assert_eq!(migrated, 2);
        // The migrated values land in the new store under their qualified
        // names.
        assert_eq!(
            new_store.load("VTrans:openai").unwrap(),
            Some(b"sk-old-openai-key".to_vec())
        );
        assert_eq!(
            new_store.load("VTrans:azure").unwrap(),
            Some(b"sk-old-azure-key".to_vec())
        );
    }

    #[cfg(windows)]
    #[test]
    fn migrate_tolerates_a_failed_entry_and_continues() {
        let (_dir, new_store) = temp_store();
        let mut old = MockCredentialStore::new();
        old.expect_list_targets().once().returning(|| {
            Ok(vec![
                "VTrans:openai".to_string(),
                "VTrans:azure".to_string(),
            ])
        });
        old.expect_load()
            .with(eq("VTrans:openai"))
            .once()
            .returning(|_| Err(SecurityError::StoreUnavailable("vault locked".to_string())));
        old.expect_load()
            .with(eq("VTrans:azure"))
            .once()
            .returning(|_| Ok(Some(b"sk-old-azure-key".to_vec())));
        old.expect_delete()
            .with(eq("VTrans:azure"))
            .once()
            .returning(|_| Ok(()));

        let migrated = migrate_to_dpapi(&old, &new_store).unwrap();
        assert_eq!(migrated, 1);
    }

    #[test]
    fn migrate_skips_non_utf8_entries_without_writing() {
        let (_dir, new_store) = temp_store();
        let mut old = MockCredentialStore::new();
        old.expect_list_targets()
            .once()
            .returning(|| Ok(vec!["VTrans:broken".to_string()]));
        old.expect_load()
            .with(eq("VTrans:broken"))
            .once()
            .returning(|_| Ok(Some(vec![0xff, 0xfe, 0x00])));
        // No store/delete expectations: the entry must not be migrated.

        let migrated = migrate_to_dpapi(&old, &new_store).unwrap();
        assert_eq!(migrated, 0);
        assert_eq!(new_store.list_targets().unwrap().len(), 0);
    }

    #[cfg(windows)]
    #[test]
    fn migrate_does_not_delete_legacy_entry_when_write_fails() {
        let dir = TestDir::new("store-failure");
        let path = dir.path().join("credentials.bin");
        let new_store = DpapiFileStore::new(&path).unwrap();
        // Make the container un-writable while keeping the store usable.
        std::fs::remove_dir_all(dir.path()).unwrap();

        let mut old = MockCredentialStore::new();
        old.expect_list_targets()
            .once()
            .returning(|| Ok(vec!["VTrans:openai".to_string()]));
        old.expect_load()
            .with(eq("VTrans:openai"))
            .once()
            .returning(|_| Ok(Some(b"sk-old-openai-key".to_vec())));
        // No delete expectation: the legacy entry must be preserved when the
        // new store cannot persist the value.

        let migrated = migrate_to_dpapi(&old, &new_store).unwrap();
        assert_eq!(migrated, 0);
    }

    #[cfg(windows)]
    #[test]
    fn migrate_counts_entry_even_when_legacy_delete_fails() {
        let (_dir, new_store) = temp_store();
        let mut old = MockCredentialStore::new();
        old.expect_list_targets()
            .once()
            .returning(|| Ok(vec!["VTrans:openai".to_string()]));
        old.expect_load()
            .with(eq("VTrans:openai"))
            .once()
            .returning(|_| Ok(Some(b"sk-old-openai-key".to_vec())));
        old.expect_delete()
            .with(eq("VTrans:openai"))
            .once()
            .returning(|_| Err(SecurityError::WindowsApi("CredDeleteW failed".to_string())));

        // The value is safely in the new store; a later run retries the
        // legacy deletion, so the entry counts as migrated.
        let migrated = migrate_to_dpapi(&old, &new_store).unwrap();
        assert_eq!(migrated, 1);
        assert_eq!(
            new_store.load("VTrans:openai").unwrap(),
            Some(b"sk-old-openai-key".to_vec())
        );
    }

    #[cfg(windows)]
    #[test]
    fn migrate_logs_only_the_masked_key() {
        let (_dir, new_store) = temp_store();
        let key = "sk-super-secret-migration-0123456789";
        let mut old = MockCredentialStore::new();
        old.expect_list_targets()
            .once()
            .returning(|| Ok(vec!["VTrans:openai".to_string()]));
        old.expect_load()
            .with(eq("VTrans:openai"))
            .once()
            .returning(|_| Ok(Some(key.as_bytes().to_vec())));
        old.expect_delete()
            .with(eq("VTrans:openai"))
            .once()
            .returning(|_| Ok(()));

        crate::test_log::clear_captured_log();
        migrate_to_dpapi(&old, &new_store).unwrap();
        let log = crate::test_log::captured_log();
        assert!(
            log.contains("sk-s****6789"),
            "log should contain the masked key, got: {log}"
        );
        assert!(
            !log.contains(key),
            "raw key leaked into the migration log: {log}"
        );
        assert!(
            !log.contains("super-secret"),
            "raw key material leaked into the migration log: {log}"
        );
    }

    #[test]
    fn temp_path_is_next_to_the_container_and_unique() {
        let dir = TestDir::new("temp-path");
        let path = dir.path().join("credentials.bin");
        let first = temp_path_for(&path);
        let second = temp_path_for(&path);
        assert_eq!(first.parent(), Some(dir.path()));
        assert_ne!(first, second);
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("credentials.bin.tmp."));
    }

    #[test]
    fn path_accessor_returns_the_construction_path() {
        let dir = TestDir::new("path-accessor");
        let path = dir.path().join("credentials.bin");
        let store = DpapiFileStore::new(&path).unwrap();
        assert_eq!(store.path(), Path::new(&path));
    }
}
