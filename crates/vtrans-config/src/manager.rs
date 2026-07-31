//! [`ConfigManager`]: load, save, update, and migrate the application config.
//!
//! Persistence follows two invariants:
//!
//! 1. **Atomic writes** — the config is first written to a temporary file in
//!    the same directory, flushed to disk, then renamed over the target. An
//!    interrupted write can therefore never leave a truncated config file.
//! 2. **Serialized read-modify-write** — [`ConfigManager::update`] takes an
//!    internal `RwLock` so concurrent callers never lose mutations.

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use serde_json::Value;
use tracing::{debug, error, info, warn};

use crate::migration::{migrate_value, raw_version};
use crate::schema::{AppConfig, CURRENT_CONFIG_VERSION};
use crate::ConfigError;

/// File name used for the application config inside the config directory.
pub const CONFIG_FILE_NAME: &str = "config.json";

/// Unique suffix counter for temporary files used in atomic saves.
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Manages the application config file.
///
/// The manager is not Clone by design: callers share one instance
/// behind an `Arc` so the internal update lock is effective process-wide.
///
/// # Example
///
/// ```
/// use vtrans_config::ConfigManager;
///
/// # let dir = tempfile::tempdir().unwrap();
/// let manager = ConfigManager::new(dir.path()).unwrap();
/// let config = manager.load().unwrap();          // creates default on first run
/// manager.update(|c| c.log_level = "debug".to_string()).unwrap();
/// ```
#[derive(Debug)]
pub struct ConfigManager {
    /// Resolved path of the config file (`config_dir/config.json`).
    config_path: PathBuf,
    /// Serializes read-modify-write operations performed by `update`.
    lock: RwLock<()>,
}

impl ConfigManager {
    /// Creates a manager for the given config directory.
    ///
    /// The directory is created if missing. The config file itself is not
    /// touched until [`load`](Self::load) or [`save`](Self::save) is called.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Io`] when the config directory cannot be
    /// created.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_config::ConfigManager;
    ///
    /// # let dir = tempfile::tempdir().unwrap();
    /// let manager = ConfigManager::new(dir.path()).unwrap();
    /// ```
    #[tracing::instrument(skip(config_dir))]
    pub fn new(config_dir: &Path) -> Result<Self, ConfigError> {
        fs::create_dir_all(config_dir).map_err(|e| {
            error!(
                error = %e,
                config_dir = %config_dir.display(),
                "failed to create config directory"
            );
            ConfigError::Io(e)
        })?;
        let config_path = config_dir.join(CONFIG_FILE_NAME);
        debug!(config_path = %config_path.display(), "config manager initialized");
        Ok(Self {
            config_path,
            lock: RwLock::new(()),
        })
    }

    /// Returns the resolved config file path.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_config::ConfigManager;
    ///
    /// # let dir = tempfile::tempdir().unwrap();
    /// let manager = ConfigManager::new(dir.path()).unwrap();
    /// assert!(manager.config_path().ends_with("config.json"));
    /// ```
    #[must_use]
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Loads the config, creating and persisting a default config on first run.
    ///
    /// The file is migrated to the current schema version before being
    /// returned. See [`migrate`](Self::migrate) for the exact behavior.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Parse`] for malformed JSON,
    /// [`ConfigError::Validation`] for content that violates validation
    /// rules, and [`ConfigError::UnsupportedVersion`] when the file version
    /// is newer than this build supports.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_config::ConfigManager;
    ///
    /// # let dir = tempfile::tempdir().unwrap();
    /// let manager = ConfigManager::new(dir.path()).unwrap();
    /// let config = manager.load().unwrap(); // creates defaults on first run
    /// assert_eq!(config.log_level, "info");
    /// ```
    #[tracing::instrument(skip(self))]
    pub fn load(&self) -> Result<AppConfig, ConfigError> {
        self.migrate()
    }

    /// Loads the config and upgrades it to the current schema version.
    ///
    /// When the config file does not exist, a default config is created and
    /// persisted. When it exists but carries an older version, the migration
    /// result is persisted back to disk. A config already at the current
    /// version is returned unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Parse`] for malformed JSON,
    /// [`ConfigError::Validation`] for content that violates validation
    /// rules, and [`ConfigError::UnsupportedVersion`] when the file version
    /// is newer than this build supports.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_config::ConfigManager;
    ///
    /// # let dir = tempfile::tempdir().unwrap();
    /// let manager = ConfigManager::new(dir.path()).unwrap();
    /// let config = manager.migrate().unwrap();
    /// assert_eq!(config.version, vtrans_config::CURRENT_CONFIG_VERSION);
    /// ```
    #[tracing::instrument(skip(self))]
    pub fn migrate(&self) -> Result<AppConfig, ConfigError> {
        let Some(raw) = self.read_raw()? else {
            info!(
                config_path = %self.config_path.display(),
                "config file not found; creating default config"
            );
            let config = AppConfig::default();
            self.save(&config)?;
            return Ok(config);
        };

        let from_version = raw_version(&raw);
        let config = self.migrate_value_logged(raw)?;
        if from_version < CURRENT_CONFIG_VERSION {
            info!(
                from_version,
                to_version = CURRENT_CONFIG_VERSION,
                config_path = %self.config_path.display(),
                "config migrated"
            );
            self.save(&config)?;
        }
        Ok(config)
    }

    /// Validates and persists the given config.
    ///
    /// The write is atomic: content is written to a temporary file in the
    /// same directory, synced, and renamed over the target. Concurrent
    /// `save` calls are atomic but unordered (last writer wins); use
    /// [`update`](Self::update) when a change depends on the currently
    /// persisted state.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Validation`] when the config violates a
    /// validation rule, and [`ConfigError::Io`] or [`ConfigError::Parse`]
    /// when persisting fails.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_config::{AppConfig, ConfigManager};
    ///
    /// # let dir = tempfile::tempdir().unwrap();
    /// let manager = ConfigManager::new(dir.path()).unwrap();
    /// manager.save(&AppConfig::default()).unwrap();
    /// ```
    #[tracing::instrument(skip(self, config))]
    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigError> {
        config.validate().map_err(|e| {
            warn!(error = %e, "config validation failed before save");
            e
        })?;
        let json = serde_json::to_string_pretty(config)?;
        atomic_write(&self.config_path, &json)?;
        debug!(config_path = %self.config_path.display(), "config saved");
        Ok(())
    }

    /// Atomically applies a mutation to the persisted config.
    ///
    /// The read-modify-write cycle is serialized by an internal `RwLock`,
    /// so concurrent `update` calls never lose each other's mutations. The
    /// config file must already exist — call [`load`](Self::load) first —
    /// otherwise [`ConfigError::NotFound`] is returned. When the mutated
    /// config fails validation, nothing is persisted and the error is
    /// returned.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::NotFound`] when no config file exists,
    /// [`ConfigError::Parse`] for malformed JSON, [`ConfigError::Validation`]
    /// when the mutated config violates a validation rule, and
    /// [`ConfigError::Io`] when persisting fails.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_config::{AppConfig, ConfigManager};
    ///
    /// # let dir = tempfile::tempdir().unwrap();
    /// let manager = ConfigManager::new(dir.path()).unwrap();
    /// manager.save(&AppConfig::default()).unwrap();
    /// manager
    ///     .update(|c| c.capture.interval_ms = 1000)
    ///     .unwrap();
    /// ```
    #[tracing::instrument(skip(self, f))]
    pub fn update<F>(&self, f: F) -> Result<(), ConfigError>
    where
        F: FnOnce(&mut AppConfig),
    {
        // The lock guards no shared memory, only the serialization of the
        // read-modify-write cycle, so recovering from a poisoned lock (a
        // panic in another thread) is safe: the file on disk is still a
        // consistent snapshot.
        let _guard = self
            .lock
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let raw = self.read_strict()?;
        let mut config = self.migrate_value_logged(raw)?;
        f(&mut config);
        self.save(&config)
    }

    /// Migrates raw config JSON, logging any failure with file context.
    fn migrate_value_logged(&self, raw: Value) -> Result<AppConfig, ConfigError> {
        migrate_value(raw).map_err(|e| {
            warn!(
                error = %e,
                config_path = %self.config_path.display(),
                "config migration or validation failed"
            );
            e
        })
    }

    /// Reads the raw JSON of the config file, if it exists.
    fn read_raw(&self) -> Result<Option<Value>, ConfigError> {
        match fs::read_to_string(&self.config_path) {
            Ok(contents) => {
                let raw = serde_json::from_str(&contents).map_err(|e| {
                    warn!(
                        error = %e,
                        config_path = %self.config_path.display(),
                        "config file is not valid JSON"
                    );
                    ConfigError::Parse(e)
                })?;
                Ok(Some(raw))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => {
                error!(
                    error = %e,
                    config_path = %self.config_path.display(),
                    "failed to read config file"
                );
                Err(ConfigError::Io(e))
            }
        }
    }

    /// Reads the config file, failing with [`ConfigError::NotFound`] when
    /// it does not exist.
    fn read_strict(&self) -> Result<Value, ConfigError> {
        self.read_raw()?.ok_or_else(|| {
            warn!(
                config_path = %self.config_path.display(),
                "config file not found; call load() before update()"
            );
            ConfigError::NotFound(self.config_path.clone())
        })
    }
}

/// Returns the platform default config file path (`config_dir/vtrans/config.json`).
///
/// Returns `None` when no config directory can be determined for the
/// current user (e.g. missing `HOME`/`APPDATA`).
///
/// # Example
///
/// ```
/// use vtrans_config::default_config_path;
///
/// if let Some(path) = default_config_path() {
///     assert!(path.ends_with("config.json"));
/// }
/// ```
#[must_use]
pub fn default_config_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|base| base.config_dir().join("vtrans").join(CONFIG_FILE_NAME))
}

/// Writes `contents` to `path` atomically (temp file + rename).
fn atomic_write(path: &Path, contents: &str) -> Result<(), ConfigError> {
    let parent = path.parent().ok_or_else(|| {
        ConfigError::Io(io::Error::new(
            ErrorKind::InvalidInput,
            "config path has no parent directory",
        ))
    })?;
    fs::create_dir_all(parent)?;

    let temp_path = temp_path_for(path);
    let result = (|| -> Result<(), ConfigError> {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(contents.as_bytes())?;
        // Flush to disk before the rename so a crash after the rename can
        // never leave an empty or truncated config file behind.
        file.sync_all()?;
        fs::rename(&temp_path, path)?;
        Ok(())
    })();

    if let Err(e) = &result {
        let _ = fs::remove_file(&temp_path);
        error!(
            error = %e,
            config_path = %path.display(),
            "failed to write config file"
        );
    }
    result
}

/// Builds a unique temporary path next to the target config file.
fn temp_path_for(path: &Path) -> PathBuf {
    let seq = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path.file_name().map_or_else(
        || CONFIG_FILE_NAME.to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    path.with_file_name(format!("{file_name}.tmp.{}.{seq}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_config_dir() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("vtrans");
        let manager = ConfigManager::new(&dir).unwrap();
        assert!(dir.is_dir());
        assert_eq!(manager.config_path(), dir.join(CONFIG_FILE_NAME));
    }

    #[test]
    fn config_path_points_to_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::new(dir.path()).unwrap();
        assert_eq!(manager.config_path(), dir.path().join("config.json"));
    }

    #[test]
    fn save_rejects_invalid_config() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::new(dir.path()).unwrap();
        let mut config = AppConfig::default();
        config.capture.interval_ms = 10;
        let err = manager.save(&config).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
        assert!(!manager.config_path().exists());
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::new(dir.path()).unwrap();
        let mut config = AppConfig::default();
        config.capture.interval_ms = 1200;
        config.hotkeys.stop_live = "Ctrl+Shift+X".to_string();
        manager.save(&config).unwrap();
        assert_eq!(manager.load().unwrap(), config);
    }

    #[test]
    fn load_creates_default_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::new(dir.path()).unwrap();
        let config = manager.load().unwrap();
        assert_eq!(config, AppConfig::default());
        assert!(manager.config_path().is_file());
    }

    #[test]
    fn update_on_missing_file_returns_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::new(dir.path()).unwrap();
        let err = manager
            .update(|c| c.log_level = "debug".to_string())
            .unwrap_err();
        assert!(matches!(err, ConfigError::NotFound(_)));
    }

    #[test]
    fn update_applies_mutation_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::new(dir.path()).unwrap();
        manager.save(&AppConfig::default()).unwrap();
        manager
            .update(|c| {
                c.capture.interval_ms = 900;
                c.translation.provider = "local".to_string();
            })
            .unwrap();
        let loaded = manager.load().unwrap();
        assert_eq!(loaded.capture.interval_ms, 900);
        assert_eq!(loaded.translation.provider, "local");
    }

    #[test]
    fn update_rejects_invalid_mutation_without_persisting() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::new(dir.path()).unwrap();
        manager.save(&AppConfig::default()).unwrap();
        let err = manager.update(|c| c.capture.interval_ms = 10).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
        assert_eq!(manager.load().unwrap().capture.interval_ms, 500);
    }

    #[test]
    fn migrate_upgrades_legacy_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(CONFIG_FILE_NAME),
            r#"{"capture":{"interval_ms":1000}}"#,
        )
        .unwrap();
        let manager = ConfigManager::new(dir.path()).unwrap();
        let config = manager.migrate().unwrap();
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.capture.interval_ms, 1000);
        // The migrated file is persisted with the new version.
        let persisted: Value =
            serde_json::from_str(&fs::read_to_string(manager.config_path()).unwrap()).unwrap();
        assert_eq!(
            persisted["version"].as_u64(),
            Some(u64::from(CURRENT_CONFIG_VERSION))
        );
    }

    #[test]
    fn migrate_rejects_newer_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CONFIG_FILE_NAME), r#"{"version":99}"#).unwrap();
        let manager = ConfigManager::new(dir.path()).unwrap();
        assert!(matches!(
            manager.migrate(),
            Err(ConfigError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn default_config_path_has_expected_suffix() {
        if let Some(path) = default_config_path() {
            let expected = Path::new("vtrans").join(CONFIG_FILE_NAME);
            assert!(
                path.ends_with(&expected),
                "unexpected path: {}",
                path.display()
            );
        }
    }

    #[test]
    fn atomic_write_leaves_no_temp_files_behind() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("config.json");
        atomic_write(&target, "{}").unwrap();
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
    }
}
