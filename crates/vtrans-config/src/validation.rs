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
const TRANSLATION_PROVIDERS: &[&str] = &["openai", "deepl", "google", "azure", "baidu", "local"];

/// Providers that call a remote HTTP(S) API and therefore require a valid
/// `api_endpoint`.
const REMOTE_TRANSLATION_PROVIDERS: &[&str] = &["openai", "deepl", "google", "azure", "baidu"];

/// Allowed translation quality presets.
const TRANSLATION_QUALITIES: &[&str] = &["fast", "balanced"];

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

/// Valid range for `floating_ball.size_px`.
const FLOATING_BALL_SIZE_PX_RANGE: std::ops::RangeInclusive<u32> = 32..=72;

/// Valid range for `max_boxes`.
const MAX_BOXES_RANGE: std::ops::RangeInclusive<u32> = 1..=32;

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
        self.validate_language_linkage()?;
        self.validate_result_window()?;
        self.validate_floating_ball()?;
        self.validate_hotkeys()?;
        self.validate_multi_box()?;
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

        let quality = self.translation.quality.as_str();
        if !TRANSLATION_QUALITIES.contains(&quality) {
            return Err(ConfigError::Validation(format!(
                "translation.quality must be one of {TRANSLATION_QUALITIES:?}, got {quality:?}"
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

        // Every remote provider needs an HTTP(S) endpoint; the local
        // provider ignores all of endpoint/model/region/app_id.
        if REMOTE_TRANSLATION_PROVIDERS.contains(&provider) {
            let endpoint = &self.translation.api_endpoint;
            if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
                return Err(ConfigError::Validation(format!(
                    "translation.api_endpoint must start with http:// or https://, got {endpoint:?}"
                )));
            }
        }

        // Only OpenAI requires a model id; DeepL and Google treat it as
        // optional and Azure/Baidu ignore it entirely.
        if provider == "openai" && self.translation.api_model.trim().is_empty() {
            return Err(ConfigError::Validation(
                "translation.api_model must not be empty when provider is \"openai\"".to_string(),
            ));
        }

        // Azure region: optional, but a present value must be non-empty.
        // Other providers ignore the field entirely.
        if provider == "azure" {
            if let Some(region) = &self.translation.region {
                if region.trim().is_empty() {
                    return Err(ConfigError::Validation(
                        "translation.region must not be empty when present".to_string(),
                    ));
                }
            }
        }

        // Baidu authenticates with APP ID + Secret; the APP ID lives in the
        // config (non-sensitive) and is required for the "baidu" provider.
        if provider == "baidu"
            && self
                .translation
                .app_id
                .as_deref()
                .map_or(true, |app_id| app_id.trim().is_empty())
        {
            return Err(ConfigError::Validation(
                "translation.app_id must not be empty when provider is \"baidu\"".to_string(),
            ));
        }
        Ok(())
    }

    /// Cross-field rule: the OCR recognition language and the translation
    /// source language must always agree.
    ///
    /// The two settings are linked: changing either one (via the
    /// `set_ocr_language` / `set_source_language` commands) keeps the other
    /// in sync, so a persisted config where they disagree indicates a manual
    /// edit or an old file that never went through the `v3 -> v4` migration.
    fn validate_language_linkage(&self) -> Result<(), ConfigError> {
        if self.ocr.language != self.translation.source_language {
            return Err(ConfigError::Validation(format!(
                "ocr.language ({}) and translation.source_language ({}) must be identical; \
                 they are linked settings — change either one via the set_ocr_language / \
                 set_source_language commands and both stay in sync",
                self.ocr.language.code(),
                self.translation.source_language.code(),
            )));
        }
        Ok(())
    }

    fn validate_result_window(&self) -> Result<(), ConfigError> {
        validate_opacity("result_window.opacity", self.result_window.opacity)
            .map_err(ConfigError::Validation)?;
        validate_font_size_px(self.result_window.font_size_px).map_err(ConfigError::Validation)?;
        Ok(())
    }

    fn validate_floating_ball(&self) -> Result<(), ConfigError> {
        validate_opacity("floating_ball.opacity", self.floating_ball.opacity)
            .map_err(ConfigError::Validation)?;
        validate_floating_ball_size_px(self.floating_ball.size_px)
            .map_err(ConfigError::Validation)?;
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

    fn validate_multi_box(&self) -> Result<(), ConfigError> {
        let max_boxes = self.max_boxes;
        if !MAX_BOXES_RANGE.contains(&max_boxes) {
            return Err(ConfigError::Validation(format!(
                "max_boxes must be within {MAX_BOXES_RANGE:?}, got {max_boxes}"
            )));
        }

        let warning_threshold = self.warning_threshold;
        if warning_threshold > max_boxes {
            return Err(ConfigError::Validation(format!(
                "warning_threshold ({warning_threshold}) must not exceed max_boxes ({max_boxes})"
            )));
        }

        let box_count = self.translation_boxes.len();
        if box_count > max_boxes as usize {
            return Err(ConfigError::Validation(format!(
                "translation_boxes count ({box_count}) must not exceed max_boxes ({max_boxes})"
            )));
        }

        // Check for duplicate IDs (nested-loop style, consistent with
        // hotkey duplicate validation).
        for (i, box_a) in self.translation_boxes.iter().enumerate() {
            for box_b in self.translation_boxes.iter().skip(i + 1) {
                if box_a.id == box_b.id {
                    return Err(ConfigError::Validation(format!(
                        "translation_boxes has duplicate id: {}",
                        box_a.id
                    )));
                }
            }
        }

        // Validate each box's region dimensions and color format.
        for box_config in &self.translation_boxes {
            if !box_config.region.is_valid() {
                return Err(ConfigError::Validation(format!(
                    "translation_boxes[{}].region has zero dimension",
                    box_config.id
                )));
            }
            if !is_valid_hex_color(&box_config.color) {
                return Err(ConfigError::Validation(format!(
                    "translation_boxes[{}].color must be a valid hex color (#RRGGBB), got {:?}",
                    box_config.id, box_config.color
                )));
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

/// Validates an opacity value against the `0.3..=1.0` range.
///
/// The range is inclusive on both ends; `NaN` is rejected because it never
/// satisfies a `<=` comparison.
///
/// # Arguments
///
/// * `field` - Field path used in the error message (e.g.
///   `"result_window.opacity"` or `"floating_ball.opacity"`).
/// * `opacity` - The value to validate.
///
/// # Errors
///
/// Returns `Err` with a field-specific message when `opacity` is out of
/// range.
fn validate_opacity(field: &str, opacity: f64) -> Result<(), String> {
    if OPACITY_RANGE.contains(&opacity) {
        Ok(())
    } else {
        Err(format!(
            "{field} must be within {OPACITY_RANGE:?}, got {opacity}"
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

/// Validates a floating-ball diameter against the `32..=72` range (inclusive).
///
/// # Errors
///
/// Returns `Err` with a field-specific message when `size_px` is out of
/// range.
fn validate_floating_ball_size_px(size_px: u32) -> Result<(), String> {
    if FLOATING_BALL_SIZE_PX_RANGE.contains(&size_px) {
        Ok(())
    } else {
        Err(format!(
            "floating_ball.size_px must be within {FLOATING_BALL_SIZE_PX_RANGE:?}, got {size_px}"
        ))
    }
}

/// Checks whether `color` is a valid `#RRGGBB` hex color string.
///
/// The value must start with `#`, be exactly 7 characters long, and the
/// remaining 6 characters must be ASCII hex digits (`0-9`, `a-f`, `A-F`).
fn is_valid_hex_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color.chars().skip(1).all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::TranslationBoxConfig;
    use vtrans_core::Language;
    use vtrans_core::ScreenRegion;

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
    fn provider_whitelist_accepts_known_providers() {
        for provider in ["openai", "deepl", "google", "azure", "baidu", "local"] {
            let config = config_with(|c| {
                c.translation.provider = provider.to_string();
                // "baidu" also needs an app_id; other providers validate as-is.
                if provider == "baidu" {
                    c.translation.app_id = Some("2026081000000000".to_string());
                }
            });
            assert!(
                config.validate().is_ok(),
                "provider {provider:?} must validate"
            );
        }
    }

    #[test]
    fn legacy_api_provider_is_rejected() {
        // "api" is no longer a valid config-domain id; v4 files are renamed
        // to "openai" by migration before validation sees them.
        let config = config_with(|c| c.translation.provider = "api".to_string());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("translation.provider")
        ));
    }

    #[test]
    fn quality_fast_and_balanced_are_accepted() {
        assert!(config_with(|c| c.translation.quality = "fast".to_string())
            .validate()
            .is_ok());
        assert!(
            config_with(|c| c.translation.quality = "balanced".to_string())
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn quality_invalid_value_is_rejected() {
        for invalid in ["slow", "Fast", "", "ultra"] {
            let config = config_with(|c| c.translation.quality = invalid.to_string());
            let err = config.validate().unwrap_err();
            assert!(
                matches!(
                    err,
                    ConfigError::Validation(ref msg) if msg.contains("translation.quality")
                ),
                "quality {invalid:?} should be rejected"
            );
        }
    }

    #[test]
    fn mismatched_ocr_and_source_language_is_rejected() {
        let config = config_with(|c| {
            c.ocr.language = Language::Japanese;
            c.translation.source_language = Language::English;
        });
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::Validation(msg) => {
                assert!(msg.contains("ocr.language"), "message: {msg}");
                assert!(msg.contains("source_language"), "message: {msg}");
                assert!(msg.contains("set_ocr_language"), "message: {msg}");
                assert!(msg.contains("set_source_language"), "message: {msg}");
                assert!(msg.contains("both stay in sync"), "message: {msg}");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn matched_ocr_and_source_language_is_accepted() {
        for language in Language::all_concrete() {
            let config = config_with(|c| {
                c.ocr.language = *language;
                c.translation.source_language = *language;
            });
            assert!(
                config.validate().is_ok(),
                "language pair {} must validate",
                language.code()
            );
        }
        // `auto` on both sides is also consistent.
        assert!(config_with(|c| {
            c.ocr.language = Language::Auto;
            c.translation.source_language = Language::Auto;
        })
        .validate()
        .is_ok());
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
    fn openai_provider_requires_http_endpoint() {
        let config = config_with(|c| c.translation.api_endpoint = "ftp://example.com".to_string());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("api_endpoint")
        ));
    }

    #[test]
    fn openai_provider_requires_model_name() {
        let config = config_with(|c| c.translation.api_model = "  ".to_string());
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::Validation(msg) => {
                assert!(msg.contains("api_model"), "message: {msg}");
                assert!(msg.contains("openai"), "message: {msg}");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn local_provider_ignores_endpoint_and_model() {
        let config = config_with(|c| {
            c.translation.provider = "local".to_string();
            c.translation.api_endpoint = String::new();
            c.translation.api_model = String::new();
            c.translation.region = Some("  ".to_string());
            c.translation.app_id = None;
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn deepl_and_google_allow_empty_model() {
        for provider in ["deepl", "google"] {
            let config = config_with(|c| {
                c.translation.provider = provider.to_string();
                c.translation.api_model = String::new();
            });
            assert!(
                config.validate().is_ok(),
                "provider {provider:?} with empty model must validate"
            );
        }
    }

    #[test]
    fn azure_ignores_model_and_accepts_optional_region() {
        let config = config_with(|c| {
            c.translation.provider = "azure".to_string();
            c.translation.api_model = String::new();
            c.translation.region = Some("eastasia".to_string());
        });
        assert!(config.validate().is_ok());

        let config = config_with(|c| {
            c.translation.provider = "azure".to_string();
            c.translation.api_model = String::new();
            c.translation.region = None;
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn empty_region_is_rejected_when_present() {
        let config = config_with(|c| {
            c.translation.provider = "azure".to_string();
            c.translation.region = Some("   ".to_string());
        });
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::Validation(msg) => assert!(msg.contains("translation.region")),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn baidu_requires_app_id() {
        let config = config_with(|c| c.translation.provider = "baidu".to_string());
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::Validation(msg) => {
                assert!(msg.contains("translation.app_id"), "message: {msg}");
                assert!(msg.contains("baidu"), "message: {msg}");
            }
            other => panic!("expected Validation, got {other:?}"),
        }

        let config = config_with(|c| {
            c.translation.provider = "baidu".to_string();
            c.translation.app_id = Some("   ".to_string());
        });
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("translation.app_id")
        ));

        let config = config_with(|c| {
            c.translation.provider = "baidu".to_string();
            c.translation.app_id = Some("2026081000000000".to_string());
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
        let config = config_with(|c| c.version = 7);
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
        assert!(validate_opacity("result_window.opacity", 0.3).is_ok());
        assert!(validate_opacity("floating_ball.opacity", 1.0).is_ok());
        assert!(validate_opacity("result_window.opacity", 0.29).is_err());
        assert!(validate_opacity("floating_ball.opacity", 1.01).is_err());
        assert!(validate_opacity("result_window.opacity", f64::NAN).is_err());
    }

    #[test]
    fn validate_font_size_px_pure_function() {
        assert!(validate_font_size_px(12).is_ok());
        assert!(validate_font_size_px(24).is_ok());
        assert!(validate_font_size_px(11).is_err());
        assert!(validate_font_size_px(25).is_err());
    }

    #[test]
    fn floating_ball_opacity_out_of_range_low() {
        let config = config_with(|c| c.floating_ball.opacity = 0.29);
        let err = config.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation(ref msg) if msg.contains("floating_ball.opacity")
        ));
    }

    #[test]
    fn floating_ball_opacity_out_of_range_high() {
        let config = config_with(|c| c.floating_ball.opacity = 1.01);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("floating_ball.opacity")
        ));
    }

    #[test]
    fn floating_ball_opacity_boundaries_are_valid() {
        assert!(config_with(|c| c.floating_ball.opacity = 0.3)
            .validate()
            .is_ok());
        assert!(config_with(|c| c.floating_ball.opacity = 1.0)
            .validate()
            .is_ok());
    }

    #[test]
    fn floating_ball_opacity_nan_is_rejected() {
        let config = config_with(|c| c.floating_ball.opacity = f64::NAN);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("floating_ball.opacity")
        ));
    }

    #[test]
    fn floating_ball_size_px_out_of_range_low() {
        let config = config_with(|c| c.floating_ball.size_px = 31);
        let err = config.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation(ref msg) if msg.contains("floating_ball.size_px")
        ));
    }

    #[test]
    fn floating_ball_size_px_out_of_range_high() {
        let config = config_with(|c| c.floating_ball.size_px = 73);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Validation(ref msg)) if msg.contains("floating_ball.size_px")
        ));
    }

    #[test]
    fn floating_ball_size_px_boundaries_are_valid() {
        assert!(config_with(|c| c.floating_ball.size_px = 32)
            .validate()
            .is_ok());
        assert!(config_with(|c| c.floating_ball.size_px = 72)
            .validate()
            .is_ok());
    }

    #[test]
    fn validate_floating_ball_size_px_pure_function() {
        assert!(validate_floating_ball_size_px(32).is_ok());
        assert!(validate_floating_ball_size_px(72).is_ok());
        assert!(validate_floating_ball_size_px(31).is_err());
        assert!(validate_floating_ball_size_px(73).is_err());
    }

    // Guard against the schema gaining fields that are never validated.
    #[test]
    fn result_window_schema_fields_covered() {
        let config = config_with(|c| c.result_window.opacity = 0.2);
        assert!(config.validate().is_err());
        let config = config_with(|c| c.result_window.font_size_px = 0);
        assert!(config.validate().is_err());
    }

    // Guard against the schema gaining fields that are never validated.
    #[test]
    fn floating_ball_schema_fields_covered() {
        let config = config_with(|c| c.floating_ball.opacity = 0.2);
        assert!(config.validate().is_err());
        let config = config_with(|c| c.floating_ball.size_px = 0);
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
            c.translation.source_language = Language::Japanese;
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

    // ── Multi-box validation ──

    /// Builds a valid [`TranslationBoxConfig`] with the given `id`.
    fn test_box(id: u32) -> TranslationBoxConfig {
        TranslationBoxConfig::new(id, ScreenRegion::new("m0", 0, 0, 100, 100), "#FF6B6B")
    }

    #[test]
    fn default_multi_box_config_is_valid() {
        assert!(AppConfig::default().validate().is_ok());
    }

    #[test]
    fn max_boxes_out_of_range_low() {
        let config = config_with(|c| c.max_boxes = 0);
        let err = config.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation(ref msg) if msg.contains("max_boxes")
        ));
    }

    #[test]
    fn max_boxes_out_of_range_high() {
        let config = config_with(|c| c.max_boxes = 33);
        let err = config.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Validation(ref msg) if msg.contains("max_boxes")
        ));
    }

    #[test]
    fn max_boxes_boundaries_are_valid() {
        assert!(config_with(|c| {
            c.max_boxes = 1;
            c.warning_threshold = 0;
        })
        .validate()
        .is_ok());
        assert!(config_with(|c| c.max_boxes = 32).validate().is_ok());
    }

    #[test]
    fn warning_threshold_exceeds_max_boxes_is_rejected() {
        let config = config_with(|c| {
            c.max_boxes = 8;
            c.warning_threshold = 9;
        });
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::Validation(msg) => {
                assert!(msg.contains("warning_threshold"), "message: {msg}");
                assert!(msg.contains("max_boxes"), "message: {msg}");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn warning_threshold_equal_to_max_boxes_is_valid() {
        assert!(config_with(|c| {
            c.max_boxes = 8;
            c.warning_threshold = 8;
        })
        .validate()
        .is_ok());
    }

    #[test]
    fn warning_threshold_zero_is_valid() {
        assert!(config_with(|c| {
            c.max_boxes = 8;
            c.warning_threshold = 0;
        })
        .validate()
        .is_ok());
    }

    #[test]
    fn too_many_boxes_is_rejected() {
        let config = config_with(|c| {
            c.max_boxes = 2;
            c.warning_threshold = 1;
            c.translation_boxes = vec![test_box(0), test_box(1), test_box(2)];
        });
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::Validation(msg) => {
                assert!(msg.contains("translation_boxes count"), "message: {msg}");
                assert!(msg.contains("max_boxes"), "message: {msg}");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_box_ids_are_rejected() {
        let config = config_with(|c| {
            c.translation_boxes = vec![test_box(0), test_box(0)];
        });
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::Validation(msg) => {
                assert!(msg.contains("duplicate id"), "message: {msg}");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn invalid_box_color_is_rejected() {
        let config = config_with(|c| {
            c.translation_boxes = vec![TranslationBoxConfig::new(
                0,
                ScreenRegion::new("m", 0, 0, 10, 10),
                "red",
            )];
        });
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::Validation(msg) => {
                assert!(msg.contains("color"), "message: {msg}");
                assert!(msg.contains("#RRGGBB"), "message: {msg}");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn valid_hex_colors_are_accepted() {
        for color in ["#FF6B6B", "#4ecdc4", "#000000", "#FFFFFF", "#abcdef"] {
            let config = config_with(|c| {
                c.translation_boxes = vec![TranslationBoxConfig::new(
                    0,
                    ScreenRegion::new("m", 0, 0, 10, 10),
                    color,
                )];
            });
            assert!(config.validate().is_ok(), "color {color:?} should be valid");
        }
    }

    #[test]
    fn box_region_zero_dimension_is_rejected() {
        let config = config_with(|c| {
            c.translation_boxes = vec![TranslationBoxConfig::new(
                0,
                ScreenRegion::new("m", 0, 0, 0, 100),
                "#FF6B6B",
            )];
        });
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::Validation(msg) => {
                assert!(msg.contains("zero dimension"), "message: {msg}");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn is_valid_hex_color_function() {
        assert!(is_valid_hex_color("#FF6B6B"));
        assert!(is_valid_hex_color("#4ecdc4"));
        assert!(is_valid_hex_color("#000000"));
        assert!(is_valid_hex_color("#FFFFFF"));
        assert!(is_valid_hex_color("#abcdef"));
        assert!(!is_valid_hex_color("FF6B6B"));
        assert!(!is_valid_hex_color("#FF6B6"));
        assert!(!is_valid_hex_color("#FF6B6BB"));
        assert!(!is_valid_hex_color("#GGGGGG"));
        assert!(!is_valid_hex_color(""));
        assert!(!is_valid_hex_color("#"));
    }

    #[test]
    fn multi_box_config_with_valid_boxes_is_accepted() {
        let config = config_with(|c| {
            c.translation_boxes = vec![
                test_box(0),
                TranslationBoxConfig::new(1, ScreenRegion::new("m1", 50, 50, 200, 200), "#4ECDC4"),
            ];
            c.max_boxes = 16;
            c.warning_threshold = 8;
        });
        assert!(config.validate().is_ok());
    }
}
