//! Application configuration schema.
//!
//! [`AppConfig`] is the root configuration structure. It is serialized to
//! and from JSON. Every field carries a `serde` default so that config
//! files with missing fields deserialize successfully with sensible values;
//! see [`crate::defaults`] for the canonical default values.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vtrans_core::Language;

/// The current config file format version.
///
/// Bump this constant when the schema changes and add a corresponding
/// migration step in [`crate::migration`].
pub const CURRENT_CONFIG_VERSION: u32 = 1;

/// Root application configuration.
///
/// All sub-structures default to sensible values when absent from the JSON
/// file, so a minimal config file only needs to override what differs.
///
/// # Example
///
/// ```
/// use vtrans_config::AppConfig;
///
/// let config = AppConfig::default();
/// let json = serde_json::to_string_pretty(&config).unwrap();
/// let back: AppConfig = serde_json::from_str(&json).unwrap();
/// assert_eq!(back.version, config.version);
/// assert_eq!(back.capture.interval_ms, config.capture.interval_ms);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Screen capture settings.
    #[serde(default = "CaptureConfig::default")]
    pub capture: CaptureConfig,

    /// OCR recognition settings.
    #[serde(default = "OcrConfig::default")]
    pub ocr: OcrConfig,

    /// Translation engine settings.
    #[serde(default = "TranslationConfig::default")]
    pub translation: TranslationConfig,

    /// Result window display settings.
    #[serde(default = "ResultWindowConfig::default")]
    pub result_window: ResultWindowConfig,

    /// Global hotkey bindings.
    #[serde(default = "HotkeyConfig::default")]
    pub hotkeys: HotkeyConfig,

    /// Log level filter (e.g. `"info"`, `"debug"`). Defaults to `"info"`.
    #[serde(default = "crate::defaults::default_log_level")]
    pub log_level: String,

    /// Override directory for model files; `None` means the default path.
    #[serde(default)]
    pub model_dir: Option<PathBuf>,

    /// Config file format version, used by [`crate::migration`].
    #[serde(default = "crate::defaults::default_version")]
    pub version: u32,
}

/// Screen capture settings for live translation mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureConfig {
    /// Capture interval in milliseconds. Must be within `250..=2000`.
    #[serde(default = "crate::defaults::default_interval_ms")]
    pub interval_ms: u32,

    /// Frame-difference threshold in `0.0..=1.0`; changes below this are
    /// ignored by the live pipeline.
    #[serde(default = "crate::defaults::default_difference_threshold")]
    pub difference_threshold: f32,
}

/// OCR recognition settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcrConfig {
    /// Target language for recognition. Defaults to [`Language::Auto`].
    #[serde(default = "crate::defaults::default_language_auto")]
    pub language: Language,

    /// Minimum recognition confidence in `0.0..=1.0`; lines below this are
    /// discarded by the OCR provider.
    #[serde(default = "crate::defaults::default_min_confidence")]
    pub min_confidence: f32,
}

/// Translation engine settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslationConfig {
    /// Provider identifier: `"api"` or `"local"`.
    #[serde(default = "crate::defaults::default_provider")]
    pub provider: String,

    /// Source language; [`Language::Auto`] enables auto-detection.
    #[serde(default = "crate::defaults::default_source_language")]
    pub source_language: Language,

    /// Target language; must not be [`Language::Auto`].
    #[serde(default = "crate::defaults::default_target_language")]
    pub target_language: Language,

    /// Per-request timeout in seconds. Must be within `1..=3600`.
    #[serde(default = "crate::defaults::default_timeout_seconds")]
    pub timeout_seconds: u32,

    /// Chat-completions endpoint used by the `"api"` provider.
    #[serde(default = "crate::defaults::default_api_endpoint")]
    pub api_endpoint: String,

    /// Model identifier used by the `"api"` provider.
    #[serde(default = "crate::defaults::default_api_model")]
    pub api_model: String,

    /// Number of retry attempts for transient API failures. Must be within
    /// `0..=10`.
    #[serde(default = "crate::defaults::default_max_retries")]
    pub max_retries: u32,
}

/// Result window display settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultWindowConfig {
    /// Keep the result window above other windows.
    #[serde(default = "crate::defaults::default_always_on_top")]
    pub always_on_top: bool,
}

/// Global hotkey bindings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Hotkey for manual region select and translate.
    #[serde(default = "crate::defaults::default_select_and_translate")]
    pub select_and_translate: String,

    /// Hotkey for starting live region translation.
    #[serde(default = "crate::defaults::default_live_translate")]
    pub live_translate: String,

    /// Hotkey for stopping live translation.
    #[serde(default = "crate::defaults::default_stop_live")]
    pub stop_live: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_config_serde_round_trip() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, back);
    }

    #[test]
    fn missing_sections_are_filled_with_defaults() {
        let json = r#"{"version":1}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config, AppConfig::default());
    }

    #[test]
    fn missing_version_defaults_to_current() {
        let json = "{}";
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{"version":1,"unknown_field":123}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config, AppConfig::default());
    }

    #[test]
    fn partial_section_keeps_present_fields() {
        let json = r#"{"capture":{"interval_ms":1000},"version":1}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.capture.interval_ms, 1000);
        assert!((config.capture.difference_threshold - 0.03).abs() < f32::EPSILON);
    }

    #[test]
    fn language_codes_round_trip() {
        let config = AppConfig {
            ocr: OcrConfig {
                language: Language::Japanese,
                ..Default::default()
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""language":"ja""#));
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ocr.language, Language::Japanese);
    }

    #[test]
    fn invalid_language_code_is_rejected() {
        let json = r#"{"ocr":{"language":"klingon"},"version":1}"#;
        assert!(serde_json::from_str::<AppConfig>(json).is_err());
    }

    #[test]
    fn model_dir_round_trip() {
        let config = AppConfig {
            model_dir: Some(PathBuf::from(r"C:\models")),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model_dir, Some(PathBuf::from(r"C:\models")));
    }

    #[test]
    fn nested_defaults_apply_independently() {
        let json = r#"{
            "capture": {"difference_threshold": 0.5},
            "translation": {"target_language": "en"},
            "version": 1
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.capture.interval_ms, 500);
        assert!((config.capture.difference_threshold - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.translation.target_language, Language::English);
        assert_eq!(config.translation.provider, "api");
        assert_eq!(config.hotkeys.select_and_translate, "Alt+Shift+A");
    }
}
