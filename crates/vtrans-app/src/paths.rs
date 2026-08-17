//! Portable data-root resolution and legacy configuration migration.
//!
//! `VTrans` keeps **all** mutable state under a single `data/` directory next
//! to the executable (`{exe}/data`), so an installed copy (and a dev build)
//! never touches `%APPDATA%` / `%LOCALAPPDATA%`. This module owns the
//! resolution of that root plus the one-time migration of a configuration
//! written by previous versions into the roaming application directory.
//!
//! Decision logic is kept in pure functions ([`resolve_data_root_for`],
//! [`migrate_legacy_config`]) so it is unit-testable without inspecting the
//! real process executable or touching the user profile.

use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::error::AppError;

/// Name of the directory holding all portable application data.
pub(crate) const DATA_DIR_NAME: &str = "data";

/// Legacy roaming configuration directory identifier used by the pre-portable
/// layout (`%APPDATA%\com.vtrans.app`).
const LEGACY_CONFIG_DIR_NAME: &str = "com.vtrans.app";

/// Returns the portable data root for the given executable path.
///
/// The root is `{exe_dir}/data`; the returned path is not created and the
/// executable directory is not checked (pure path arithmetic). See
/// [`resolve_data_root`] for the production resolution that also creates
/// the directory.
///
/// # Example
///
/// ```
/// use std::path::Path;
/// use vtrans_app::paths::resolve_data_root_for;
///
/// assert_eq!(
///     resolve_data_root_for(Path::new(r"C:\VTrans\vtrans.exe")),
///     Path::new(r"C:\VTrans\data")
/// );
/// ```
#[must_use]
pub fn resolve_data_root_for(current_exe: &Path) -> PathBuf {
    current_exe
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(DATA_DIR_NAME)
}

/// Resolves the production data root from the running executable.
///
/// The directory is created when possible; a creation failure is logged and
/// tolerated here (downstream consumers such as `ConfigManager::new` surface
/// a clear error if the directory is really unusable), so path arithmetic
/// never panics.
///
/// # Errors
///
/// Returns an application error only when the executable path itself cannot
/// be resolved (`current_exe` failed).
#[tracing::instrument]
pub fn resolve_data_root() -> Result<PathBuf, AppError> {
    let current_exe = std::env::current_exe()
        .map_err(|error| AppError::Tauri(format!("failed to locate the executable: {error}")))?;
    let root = resolve_data_root_for(&current_exe);
    match std::fs::create_dir_all(&root) {
        Ok(()) => info!(data_root = %root.display(), "portable data root ready"),
        Err(error) => warn!(
            error = %error,
            data_root = %root.display(),
            "failed to create the portable data root; startup continues and configuration may be read-only"
        ),
    }
    Ok(root)
}

/// Returns the legacy roaming config directory for a `%APPDATA%` value.
///
/// Pure path arithmetic kept separate from the environment lookup so tests
/// can pin the exact legacy location without touching the user profile.
///
/// # Example
///
/// ```
/// use std::path::Path;
/// use vtrans_app::paths::legacy_config_dir_for_appdata;
///
/// assert_eq!(
///     legacy_config_dir_for_appdata(r"C:\Users\me\AppData\Roaming"),
///     Path::new(r"C:\Users\me\AppData\Roaming\com.vtrans.app")
/// );
/// ```
#[must_use]
pub fn legacy_config_dir_for_appdata(appdata: &str) -> PathBuf {
    Path::new(appdata).join(LEGACY_CONFIG_DIR_NAME)
}

/// Returns the legacy roaming config directory, or `None` when the
/// `APPDATA` environment variable is unset (non-Windows or exotic setups).
#[must_use]
pub fn legacy_config_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|value| legacy_config_dir_for_appdata(&value.to_string_lossy()))
}

/// Copies a legacy roaming `config.json` into the portable data root once.
///
/// The copy only happens when the legacy file exists **and** the portable
/// file is still missing, so a fresh portable installation is never
/// overwritten by stale roaming state. Failures are reported to the caller
/// (the startup path logs them with `warn!` and continues).
///
/// # Returns
///
/// `Ok(true)` when a file was migrated, `Ok(false)` when there was nothing
/// to migrate or the target already existed, and `Err` when an actual copy
/// (or probe) failure occurred.
///
/// # Errors
///
/// Returns the underlying IO error when the legacy file cannot be read or
/// the portable copy cannot be written; the startup caller logs it with
/// `warn!` and continues.
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use vtrans_app::paths::migrate_legacy_config;
///
/// let migrated = migrate_legacy_config(
///     Path::new(r"C:\Users\me\AppData\Roaming\com.vtrans.app\config.json"),
///     Path::new(r"D:\VTrans\data\config.json"),
/// )?;
/// assert!(migrated);
/// # Ok::<(), std::io::Error>(())
/// ```
#[tracing::instrument(skip(legacy_config, portable_config))]
pub fn migrate_legacy_config(
    legacy_config: &Path,
    portable_config: &Path,
) -> std::io::Result<bool> {
    if portable_config.exists() || !legacy_config.exists() {
        return Ok(false);
    }
    std::fs::copy(legacy_config, portable_config)?;
    info!(
        legacy = %legacy_config.display(),
        portable = %portable_config.display(),
        "legacy roaming configuration migrated to the portable data root"
    );
    Ok(true)
}

/// Runs the one-time legacy configuration migration against the given data
/// root, tolerating every failure (log only, never blocks startup).
pub(crate) fn migrate_legacy_config_if_needed(data_root: &Path) {
    let Some(legacy_dir) = legacy_config_dir() else {
        return;
    };
    let legacy_config = legacy_dir.join("config.json");
    let portable_config = data_root.join("config.json");
    if let Err(error) = migrate_legacy_config(&legacy_config, &portable_config) {
        warn!(
            error = %error,
            legacy = %legacy_config.display(),
            portable = %portable_config.display(),
            "legacy configuration migration failed; continuing with a fresh configuration"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// Minimal std-only temporary-directory guard (parallel-test safe).
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "vtrans-app-paths-{name}-{}-{seq}",
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

    #[test]
    fn data_root_is_exe_parent_plus_data() {
        assert_eq!(
            resolve_data_root_for(Path::new(r"C:\VTrans\vtrans.exe")),
            Path::new(r"C:\VTrans\data")
        );
        // Dev builds live in target/debug (or release), so their data root
        // lands inside the target directory — the accepted dev behavior.
        assert_eq!(
            resolve_data_root_for(Path::new(r"D:\~~~rust\VTrans\target\debug\vtrans.exe")),
            Path::new(r"D:\~~~rust\VTrans\target\debug\data")
        );
    }

    #[test]
    fn legacy_dir_is_roaming_appdata_plus_identifier() {
        assert_eq!(
            legacy_config_dir_for_appdata(r"C:\Users\me\AppData\Roaming"),
            Path::new(r"C:\Users\me\AppData\Roaming\com.vtrans.app")
        );
        // Trailing separators must not change the result.
        assert_eq!(
            legacy_config_dir_for_appdata(r"C:\Users\me\AppData\Roaming\"),
            Path::new(r"C:\Users\me\AppData\Roaming\com.vtrans.app")
        );
    }

    #[test]
    fn migration_copies_once_when_legacy_exists_and_target_missing() {
        let legacy = TestDir::new("legacy");
        let portable = TestDir::new("portable");
        std::fs::write(legacy.path().join("config.json"), b"legacy-content").unwrap();

        let migrated = migrate_legacy_config(
            &legacy.path().join("config.json"),
            &portable.path().join("config.json"),
        )
        .unwrap();
        assert!(migrated);
        assert_eq!(
            std::fs::read(portable.path().join("config.json")).unwrap(),
            b"legacy-content"
        );

        // Re-running must not overwrite the portable file.
        std::fs::write(portable.path().join("config.json"), b"portable-content").unwrap();
        let migrated = migrate_legacy_config(
            &legacy.path().join("config.json"),
            &portable.path().join("config.json"),
        )
        .unwrap();
        assert!(!migrated);
        assert_eq!(
            std::fs::read(portable.path().join("config.json")).unwrap(),
            b"portable-content"
        );
    }

    #[test]
    fn migration_skips_when_legacy_config_is_missing() {
        let legacy = TestDir::new("legacy-missing");
        let portable = TestDir::new("portable-missing");

        let migrated = migrate_legacy_config(
            &legacy.path().join("config.json"),
            &portable.path().join("config.json"),
        )
        .unwrap();
        assert!(!migrated);
        assert!(!portable.path().join("config.json").exists());
    }

    #[test]
    fn migration_skips_when_portable_config_already_exists() {
        let legacy = TestDir::new("legacy-exists");
        let portable = TestDir::new("portable-exists");
        std::fs::write(legacy.path().join("config.json"), b"legacy").unwrap();
        std::fs::write(portable.path().join("config.json"), b"portable").unwrap();

        let migrated = migrate_legacy_config(
            &legacy.path().join("config.json"),
            &portable.path().join("config.json"),
        )
        .unwrap();
        assert!(!migrated);
        assert_eq!(
            std::fs::read(portable.path().join("config.json")).unwrap(),
            b"portable"
        );
    }

    #[test]
    fn migration_reports_copy_failures() {
        let legacy = TestDir::new("legacy-fail");
        let portable = TestDir::new("portable-fail");
        std::fs::write(legacy.path().join("config.json"), b"legacy").unwrap();
        // The portable parent directory does not exist; the copy must fail
        // instead of being silently swallowed.
        let error = migrate_legacy_config(
            &legacy.path().join("config.json"),
            &portable.path().join("no-such-dir").join("config.json"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
}
