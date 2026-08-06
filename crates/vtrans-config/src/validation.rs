//! Validation rules for [`AppConfig`].
//!
//! [`AppConfig::validate`] is invoked before a config is persisted or
//! returned to the caller, so out-of-range or inconsistent values surface
//! as [`ConfigError::Validation`] with a field-specific message instead of
//! silently degrading at runtime.

use crate::schema::{AppConfig, CURRENT_CONFIG_VERSION};
use crate::ConfigError;

/// Allowed `log_level` values, mirroring the `tracing` filter levels.
const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error", "off"];

/// Allowed translation provider identifiers.
const TRANSLATION_PROVIDERS: &[&str] = &["api", "local"];

/// Valid range for `capture.interval_ms`.
const INTERVAL_MS_RANGE: std::ops::RangeInclusive<u32> = 250..=2000;

/// Valid range for `translation.timeout_seconds`.
const TIMEOUT_SECONDS_RANGE: std::ops::RangeInclusive<u32> = 1..=3600;

/// Valid range for `translation.max_retries`.
const MAX_RETRIES_RANGE: std::ops::RangeInclusive<u32> = 0..=10;

/// Valid range for `result_window.opacity`.
const OPACITY_RANGE: std::ops::RangeInclusive<f64> = 0.3..=1.0;

/// Valid range for `result_window.font_size_px`.
const FONT_SIZE_PX_RANGE: std::ops::RangeInclusive<u32> = 12..=24;

impl AppConfig {
    /// Validates every configuration field against its documented rules.
    ///
    /// Returns the first violation found. All ranges are inclusive.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Validation`] with a field-specific message
    /// when any rule is violated.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_config::AppConfig;
    ///
    /// assert!(AppConfig::default().validate().is_ok());
    /// ```
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_capture()?;
        self.validate_ocr()?;
        self.validate_translation()?;
        self.validate_result_window()?;
        self.validate_hotkeys()?;
        self.validate_common()?;
        Ok(())
    }

    fn validate_capture(&self) -> Result<(), ConfigError> {
        let interval_ms = self.capture.interval_ms;
        if !INTERVAL_MS_RANGE.contains(&interval_ms) {
            return Err(ConfigError::Validation(format!(
                "capture.interval_ms must be within {INTERVAL_MS_RANGE:?}, got {interval_ms}"
            )));
        }
        let threshold = self.capture.difference_threshold;
        if !(0.0..=1.0).contains(&threshold) {
            return Err(ConfigError::Validation(format!(
                "capture.difference_threshold must be within 0.0..=1.0, got {threshold}"
            )));
        }
        Ok(())
    }

    fn validate_ocr(&self) -> Result<(), ConfigError> {
        let confidence = self.ocr.min_confidence;
        if !(0.0..=1.0).contains(&confidence) {
            return Err(ConfigError::Validation(format!(
                "ocr.min_confidence must be within 0.0..=1.0, got {confidence}"
            )));
        }
        Ok(())
    }

    fn validate_translation(&self) -> Result<(), ConfigError> {
        let provider = self.translation.provider.as_str();
        if !TRANSLATION_PROVIDERS.contains(&provider) {
            return Err(ConfigError::Validation(format!(
                "translation.provider must be one of {TRANSLATION_PROVIDERS:?}, got {provider:?}"
            )));
        }

        if self.translation.target_language.is_auto() {
            return Err(ConfigError::Validation(
                "translation.target_language must not be \"auto\"".to_string(),
            ));
        }

        let timeout_seconds = self.translation.timeout_seconds;
        if !TIMEOUT_SECONDS_RANGE.contains(&timeout_seconds) {
            return Err(ConfigError::Validation(format!(
                "translation.timeout_seconds must be within {TIMEOUT_SECONDS_RANGE:?}, got {timeout_seconds}"
            )));
        }

        let max_retries = self.translation.max_retries;
        if !MAX_RETRIES_RANGE.contains(&max_retries) {
            return Err(ConfigError::Validation(format!(
                "translation.max_retries must be within {MAX_RETRIES_RANGE:?}, got {max_retries}"
            )));
        }

        // Endpoint and model only matter for the remote "api" provider.
        if provider == "api" {
            let endpoint = &self.translation.api_endpoint;
            if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
                return Err(ConfigError::Validation(format!(
                    "translation.api_endpoint must start with http:// or https://, got {endpoint:?}"
                )));
            }
            if self.translation.api_model.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "translation.api_model must not be empty".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn validate_result_window(&self) -> Result<(), ConfigError> {
        validate_opacity(self.result_window.opacity).map_err(ConfigError::Validation)?;
        validate_font_size_px(self.result_window.font_size_px).map_err(ConfigError::Validation)?;
        Ok(())
    }

    fn validate_hotkeys(&self) -> Result<(), ConfigError> {
        let hotkeys = [
            ("select_and_translate", &self.hotkeys.select_and_translate),
            ("live_translate", &self.hotkeys.live_translate),
            ("stop_live", &self.hotkeys.stop_live),
        ];

        for (name, value) in hotkeys {
            if value.trim().is_empty() {
                return Err(ConfigError::Validation(format!(
                    "hotkeys.{name} must not be empty"
                )));
            }
        }

        for (index, (name_a, value_a)) in hotkeys.iter().enumerate() {
            for (name_b, value_b) in hotkeys.iter().skip(index + 1) {
                if value_a == value_b {
                    return Err(ConfigError::Validation(format!(
                        "hotkeys.{name_a} and hotkeys.{name_b} must not be identical ({value_a:?})"
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_common(&self) -> Result<(), ConfigError> {
        if !LOG_LEVELS.contains(&self.log_level.as_str()) {
            return Err(ConfigError::Validation(format!(
                "log_level must be one of {LOG_LEVELS:?}, got {:?}",
                self.log_level
            )));
        }
        if self.version != CURRENT_CONFIG_VERSION {
            return Err(ConfigError::Validation(format!(
                "version must be {CURRENT_CONFIG_VERSION}, got {}",
                self.version
            )));
        }
        Ok(())
    }
}

/// Validates a result-window opacity value against the `0.3..=1.0` range.
///
/// The range is inclusive on both ends; `NaN` is rejected because it never
/// satisfies a `<=` comparison.
///
/// # Errors
///
/// Returns `Err` with a field-specific message when `opacity` is out of
/// range.
fn validate_opacity(opacity: f64) -> Result<(), String> {
    if OPACITY_RANGE.contains(&opacity) {
        Ok(())
    } else {
        Err(format!(
            "result_window.opacity must be within {OPACITY_RANGE:?}, got {opacity}"
        ))
    }
}

/// Validates a result-window font size against the `12..=24` range (inclusive).
///
/// # Errors
///
/// Returns `Err` with a field-specific message when `font_size_px` is out
/// of range.
fn validate_font_size_px(font_size_px: u32) -> Result<(), String> {
    if FONT_SIZE_PX_RANGE.contains(&font_size_px) {
        Ok(())
    } else {
        Err(format!(
            "result_window.font_size_px must be within {FONT_SIZE_PX_RANGE:?}, got {font_size_px}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtrans_core::Language;

    fn config_with(mutator: impl FnOnce(&mut AppConfig)) -> AppConfig {
        let mut config = AppConfig::default();
        mutator(&mut config);
        config
    }

    #[test]
    fn default_config_is_valid() {
        assert!(AppConfig::default().validate().is_ok());
    }

    #[test]
    fn interval_ms_out_of_range_low() {
        let config = config_with(|c| c.capture.interval_ms = 249);
        let err = config.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation(ref msg) if msg.contains("capture.interval_ms")
        ));
    }

    #[test]
    fn interval_ms_out_of_range_high() {
        let config = config_with(|c| c.capture.interval_ms = 2001);
        assert!(matches!(config.validate(), Err(ConfigError::Validation(_))));
    }

    #[test]
    fn interval_ms_boundaries_are_valid() {
        assert!(config_with(|c| c.capture.interval_ms = 250)
            .validate()
            .is_ok());
        assert!(config_with(|c| c.capture.interval_ms = 2000)
            .validate()
            .is_ok());
    }

    #[test]
    fn difference_threshold_out_of_range() {
        let config = config_with(|c| c.capture.difference_threshold = 1.5);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("difference_threshold")
        ));
    }

    #[test]
    fn difference_threshold_nan_is_rejected() {
        let config = config_with(|c| c.capture.difference_threshold = f32::NAN);
        assert!(matches!(config.validate(), Err(ConfigError::Validation(_))));
    }

    #[test]
    fn min_confidence_out_of_range() {
        let config = config_with(|c| c.ocr.min_confidence = 1.1);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("min_confidence")
        ));
    }

    #[test]
    fn invalid_provider_is_rejected() {
        let config = config_with(|c| c.translation.provider = "deepseek".to_string());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("translation.provider")
        ));
    }

    #[test]
    fn auto_target_language_is_rejected() {
        let config = config_with(|c| c.translation.target_language = Language::Auto);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("target_language")
        ));
    }

    #[test]
    fn timeout_seconds_out_of_range() {
        let config = config_with(|c| c.translation.timeout_seconds = 0);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("timeout_seconds")
        ));
    }

    #[test]
    fn max_retries_out_of_range() {
        let config = config_with(|c| c.translation.max_retries = 11);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("max_retries")
        ));
    }

    #[test]
    fn api_provider_requires_http_endpoint() {
        let config = config_with(|c| c.translation.api_endpoint = "ftp://example.com".to_string());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("api_endpoint")
        ));
    }

    #[test]
    fn api_provider_requires_model_name() {
        let config = config_with(|c| c.translation.api_model = "  ".to_string());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("api_model")
        ));
    }

    #[test]
    fn local_provider_ignores_endpoint_and_model() {
        let config = config_with(|c| {
            c.translation.provider = "local".to_string();
            c.translation.api_endpoint = String::new();
            c.translation.api_model = String::new();
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn empty_hotkey_is_rejected() {
        let config = config_with(|c| c.hotkeys.stop_live = "  ".to_string());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("hotkeys.stop_live")
        ));
    }

    #[test]
    fn duplicate_hotkeys_are_rejected() {
        let config = config_with(|c| c.hotkeys.live_translate = "Alt+Shift+A".to_string());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("must not be identical")
        ));
    }

    #[test]
    fn invalid_log_level_is_rejected() {
        let config = config_with(|c| c.log_level = "verbose".to_string());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("log_level")
        ));
    }

    #[test]
    fn invalid_version_is_rejected() {
        let config = config_with(|c| c.version = 3);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("version")
        ));
    }

    #[test]
    fn opacity_out_of_range_low() {
        let config = config_with(|c| c.result_window.opacity = 0.29);
        let err = config.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation(ref msg) if msg.contains("result_window.opacity")
        ));
    }

    #[test]
    fn opacity_out_of_range_high() {
        let config = config_with(|c| c.result_window.opacity = 1.01);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("result_window.opacity")
        ));
    }

    #[test]
    fn opacity_boundaries_are_valid() {
        assert!(config_with(|c| c.result_window.opacity = 0.3)
            .validate()
            .is_ok());
        assert!(config_with(|c| c.result_window.opacity = 1.0)
            .validate()
            .is_ok());
    }

    #[test]
    fn opacity_nan_is_rejected() {
        let config = config_with(|c| c.result_window.opacity = f64::NAN);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("result_window.opacity")
        ));
    }

    #[test]
    fn font_size_px_out_of_range_low() {
        let config = config_with(|c| c.result_window.font_size_px = 11);
        let err = config.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation(ref msg) if msg.contains("result_window.font_size_px")
        ));
    }

    #[test]
    fn font_size_px_out_of_range_high() {
        let config = config_with(|c| c.result_window.font_size_px = 25);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("result_window.font_size_px")
        ));
    }

    #[test]
    fn font_size_px_boundaries_are_valid() {
        assert!(config_with(|c| c.result_window.font_size_px = 12)
            .validate()
            .is_ok());
        assert!(config_with(|c| c.result_window.font_size_px = 24)
            .validate()
            .is_ok());
    }

    #[test]
    fn validate_opacity_pure_function() {
        assert!(validate_opacity(0.3).is_ok());
        assert!(validate_opacity(1.0).is_ok());
        assert!(validate_opacity(0.29).is_err());
        assert!(validate_opacity(1.01).is_err());
        assert!(validate_opacity(f64::NAN).is_err());
    }

    #[test]
    fn validate_font_size_px_pure_function() {
        assert!(validate_font_size_px(12).is_ok());
        assert!(validate_font_size_px(24).is_ok());
        assert!(validate_font_size_px(11).is_err());
        assert!(validate_font_size_px(25).is_err());
    }

    // Guard against the schema gaining fields that are never validated.
    #[test]
    fn result_window_schema_fields_covered() {
        let config = config_with(|c| c.result_window.opacity = 0.2);
        assert!(config.validate().is_err());
        let config = config_with(|c| c.result_window.font_size_px = 0);
        assert!(config.validate().is_err());
    }

    #[test]
    fn first_violation_is_reported() {
        let config = config_with(|c| {
            c.capture.interval_ms = 10;
            c.ocr.min_confidence = 2.0;
        });
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::Validation(msg) => assert!(msg.contains("capture.interval_ms")),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn all_fields_validated_together() {
        // A config exercising every section at once must validate.
        let config = config_with(|c| {
            c.capture.interval_ms = 800;
            c.ocr.language = Language::Japanese;
            c.translation.provider = "local".to_string();
            c.translation.target_language = Language::English;
            c.hotkeys.live_translate = "Ctrl+Shift+L".to_string();
            c.log_level = "debug".to_string();
        });
        assert!(config.validate().is_ok());
    }

    // Guard against the schema gaining fields that are never validated.
    #[test]
    fn capture_schema_fields_covered() {
        let config = config_with(|c| c.capture.interval_ms = 0);
        assert!(config.validate().is_err());
        let config = config_with(|c| c.capture.difference_threshold = -0.1);
        assert!(config.validate().is_err());
    }

    #[test]
    fn ocr_schema_fields_covered() {
        let config = config_with(|c| c.ocr.min_confidence = -0.1);
        assert!(config.validate().is_err());
    }
}
