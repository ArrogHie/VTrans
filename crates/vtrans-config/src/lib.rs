//! `VTrans` configuration management.
//!
//! Defines the application config schema ([`AppConfig`] and sub-structs),
//! default values, validation rules, version migration, and the
//! [`ConfigManager`] responsible for persistence.
//!
//! The config is stored as JSON at `config_dir/vtrans/config.json` and is
//! written atomically (temporary file + rename) so an interrupted write
//! never corrupts it.
//!
//! See `docs/modules/02-config.md` for the full module specification.

pub mod defaults;
pub mod manager;
pub mod migration;
pub mod schema;
pub mod validation;

pub use manager::{default_config_path, ConfigManager, CONFIG_FILE_NAME};
pub use schema::{
    AppConfig, CaptureConfig, HotkeyConfig, OcrConfig, ResultWindowConfig, TranslationConfig,
    CURRENT_CONFIG_VERSION,
};

use std::path::PathBuf;

use thiserror::Error;

/// Errors that can occur during config loading, validation, migration, or
/// persistence.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The config file does not exist.
    #[error("config file not found: {0}")]
    NotFound(PathBuf),

    /// The config file is not valid JSON or does not match the schema.
    #[error("config parse error: {0}")]
    Parse(#[from] serde_json::Error),

    /// The config violates a validation rule.
    #[error("config validation failed: {0}")]
    Validation(String),

    /// A filesystem error occurred while reading or writing the config.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The config file version is newer than this build supports.
    #[error("unsupported config version: {0}")]
    UnsupportedVersion(u32),
}
