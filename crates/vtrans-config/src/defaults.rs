//! Default values for every configuration field.
//!
//! This module is the single source of truth for defaults: the `Default`
//! implementations of the schema types and the `serde` `default = "..."`
//! attributes used by [`crate::schema`] both resolve to the functions here,
//! so a freshly created config and a config deserialized from a file with
//! missing fields always agree.

use std::path::PathBuf;

use vtrans_core::Language;

use crate::schema::{
    AppConfig, CaptureConfig, FloatingBallConfig, HotkeyConfig, OcrConfig, ResultWindowConfig,
    TranslationConfig, CURRENT_CONFIG_VERSION,
};

/// Default capture interval in milliseconds.
pub(crate) const fn default_interval_ms() -> u32 {
    500
}

/// Default frame-difference threshold.
pub(crate) const fn default_difference_threshold() -> f32 {
    0.03
}

/// Default minimum OCR confidence.
pub(crate) const fn default_min_confidence() -> f32 {
    0.55
}

/// Default translation provider identifier.
pub(crate) fn default_provider() -> String {
    "api".to_string()
}

/// Default translation quality preset.
pub(crate) fn default_translation_quality() -> String {
    "fast".to_string()
}

/// Default OCR language (auto-detection).
pub(crate) const fn default_language_auto() -> Language {
    Language::Auto
}

/// Default source language (auto-detection).
pub(crate) const fn default_source_language() -> Language {
    Language::Auto
}

/// Default target language.
pub(crate) const fn default_target_language() -> Language {
    Language::ChineseSimplified
}

/// Default per-request translation timeout in seconds.
pub(crate) const fn default_timeout_seconds() -> u32 {
    30
}

/// Default chat-completions endpoint.
pub(crate) fn default_api_endpoint() -> String {
    "https://api.openai.com/v1/chat/completions".to_string()
}

/// Default API model identifier.
pub(crate) fn default_api_model() -> String {
    "gpt-4o-mini".to_string()
}

/// Default number of API retry attempts.
pub(crate) const fn default_max_retries() -> u32 {
    3
}

/// Default "keep result window on top" flag.
pub(crate) const fn default_always_on_top() -> bool {
    true
}

/// Default result-window opacity.
pub(crate) const fn default_opacity() -> f64 {
    0.95
}

/// Default result-window font size in pixels.
pub(crate) const fn default_font_size_px() -> u32 {
    14
}

/// Default floating-ball visibility (hidden).
pub(crate) const fn default_floating_ball_enabled() -> bool {
    false
}

/// Default floating-ball opacity (fully opaque).
pub(crate) const fn default_floating_ball_opacity() -> f64 {
    1.0
}

/// Default floating-ball diameter in pixels.
pub(crate) const fn default_floating_ball_size_px() -> u32 {
    48
}

/// Default hotkey for manual region select and translate.
pub(crate) fn default_select_and_translate() -> String {
    "Alt+Shift+A".to_string()
}

/// Default hotkey for live region translation.
pub(crate) fn default_live_translate() -> String {
    "Alt+Shift+R".to_string()
}

/// Default hotkey for stopping live translation.
pub(crate) fn default_stop_live() -> String {
    "Alt+Shift+S".to_string()
}

/// Default log level filter.
pub(crate) fn default_log_level() -> String {
    "info".to_string()
}

/// Default model directory override (`None` = built-in default path).
pub(crate) const fn default_model_dir() -> Option<PathBuf> {
    None
}

/// Default config file format version.
pub(crate) const fn default_version() -> u32 {
    CURRENT_CONFIG_VERSION
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            interval_ms: default_interval_ms(),
            difference_threshold: default_difference_threshold(),
        }
    }
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            language: default_language_auto(),
            min_confidence: default_min_confidence(),
        }
    }
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            quality: default_translation_quality(),
            source_language: default_source_language(),
            target_language: default_target_language(),
            timeout_seconds: default_timeout_seconds(),
            api_endpoint: default_api_endpoint(),
            api_model: default_api_model(),
            max_retries: default_max_retries(),
        }
    }
}

impl Default for ResultWindowConfig {
    fn default() -> Self {
        Self {
            always_on_top: default_always_on_top(),
            opacity: default_opacity(),
            font_size_px: default_font_size_px(),
        }
    }
}

impl Default for FloatingBallConfig {
    fn default() -> Self {
        Self {
            enabled: default_floating_ball_enabled(),
            opacity: default_floating_ball_opacity(),
            size_px: default_floating_ball_size_px(),
        }
    }
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            select_and_translate: default_select_and_translate(),
            live_translate: default_live_translate(),
            stop_live: default_stop_live(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            capture: CaptureConfig::default(),
            ocr: OcrConfig::default(),
            translation: TranslationConfig::default(),
            result_window: ResultWindowConfig::default(),
            floating_ball: FloatingBallConfig::default(),
            hotkeys: HotkeyConfig::default(),
            log_level: default_log_level(),
            model_dir: default_model_dir(),
            version: default_version(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_defaults() {
        let config = CaptureConfig::default();
        assert_eq!(config.interval_ms, 500);
        assert!((config.difference_threshold - 0.03).abs() < f32::EPSILON);
    }

    #[test]
    fn ocr_defaults() {
        let config = OcrConfig::default();
        assert_eq!(config.language, Language::Auto);
        assert!((config.min_confidence - 0.55).abs() < f32::EPSILON);
    }

    #[test]
    fn translation_defaults() {
        let config = TranslationConfig::default();
        assert_eq!(config.provider, "api");
        assert_eq!(config.quality, "fast");
        assert_eq!(config.source_language, Language::Auto);
        assert_eq!(config.target_language, Language::ChineseSimplified);
        assert_eq!(config.timeout_seconds, 30);
        assert_eq!(
            config.api_endpoint,
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(config.api_model, "gpt-4o-mini");
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn result_window_defaults() {
        let config = ResultWindowConfig::default();
        assert!(config.always_on_top);
        assert!((config.opacity - 0.95).abs() < f64::EPSILON);
        assert_eq!(config.font_size_px, 14);
    }

    #[test]
    fn floating_ball_defaults() {
        let config = FloatingBallConfig::default();
        assert!(!config.enabled);
        assert!((config.opacity - 1.0).abs() < f64::EPSILON);
        assert_eq!(config.size_px, 48);
    }

    #[test]
    fn hotkey_defaults() {
        let config = HotkeyConfig::default();
        assert_eq!(config.select_and_translate, "Alt+Shift+A");
        assert_eq!(config.live_translate, "Alt+Shift+R");
        assert_eq!(config.stop_live, "Alt+Shift+S");
    }

    #[test]
    fn app_config_defaults() {
        let config = AppConfig::default();
        assert_eq!(config.capture, CaptureConfig::default());
        assert_eq!(config.ocr, OcrConfig::default());
        assert_eq!(config.translation, TranslationConfig::default());
        assert_eq!(config.result_window, ResultWindowConfig::default());
        assert_eq!(config.floating_ball, FloatingBallConfig::default());
        assert_eq!(config.hotkeys, HotkeyConfig::default());
        assert_eq!(config.log_level, "info");
        assert_eq!(config.model_dir, None);
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
    }

    #[test]
    fn serde_defaults_agree_with_impl_default() {
        // Fields dropped from a JSON file must deserialize to exactly the
        // same values the `Default` implementations produce.
        let json = "{}";
        let from_json: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(from_json, AppConfig::default());
    }
}
