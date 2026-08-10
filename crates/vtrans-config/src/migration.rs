//! Version migration for config files.
//!
//! Config files carry an integer `version` field. Files written before the
//! `version` field existed (or without it) are treated as version `0` and
//! upgraded step by step to [`CURRENT_CONFIG_VERSION`]. Missing fields are
//! filled with defaults during deserialization (see [`crate::defaults`]),
//! so migrations only need to handle structural changes and version
//! stamping — with the exceptions of `v3 -> v4` (re-synchronizes the OCR
//! language and the translation source language; see [`migrate_v3_to_v4`])
//! and `v4 -> v5` (renames the old `"api"` provider to `"openai"`; see
//! [`migrate_v4_to_v5`]).

use serde::Deserialize;
use serde_json::Value;

use crate::schema::{AppConfig, CURRENT_CONFIG_VERSION};
use crate::ConfigError;

/// A single migration step from one schema version to the next.
struct Migration {
    /// Version this step upgrades from.
    from: u32,
    /// Version this step upgrades to.
    to: u32,
    /// The migration function itself.
    apply: fn(&mut AppConfig),
}

/// All known migration steps, ordered by `from` version.
const MIGRATIONS: &[Migration] = &[
    Migration {
        from: 0,
        to: 1,
        apply: migrate_v0_to_v1,
    },
    Migration {
        from: 1,
        to: 2,
        apply: migrate_v1_to_v2,
    },
    Migration {
        from: 2,
        to: 3,
        apply: migrate_v2_to_v3,
    },
    Migration {
        from: 3,
        to: 4,
        apply: migrate_v3_to_v4,
    },
    Migration {
        from: 4,
        to: 5,
        apply: migrate_v4_to_v5,
    },
];

/// The outcome of migrating raw config JSON.
#[derive(Debug)]
pub(crate) struct MigratedConfig {
    /// The migrated, validated config at [`CURRENT_CONFIG_VERSION`].
    pub config: AppConfig,
    /// The schema version the file was migrated from (`0` for legacy files).
    pub from_version: u32,
    /// `true` when a version migration was applied; the caller should then
    /// persist the result back to disk.
    pub migrated: bool,
}

/// Migrates raw config JSON to the latest schema version and validates it.
///
/// Missing fields are filled with defaults during deserialization. Returns
/// [`ConfigError::UnsupportedVersion`] when the file is newer than this
/// build supports and [`ConfigError::Validation`] when the migrated config
/// violates a validation rule.
pub(crate) fn migrate_value(raw: Value) -> Result<MigratedConfig, ConfigError> {
    // Probe the version first: a malformed `version` field surfaces as a
    // clear parse error instead of being silently treated as legacy.
    let probe: VersionProbe = serde_json::from_value(raw.clone())?;
    if probe.version > CURRENT_CONFIG_VERSION {
        return Err(ConfigError::UnsupportedVersion(probe.version));
    }

    let migrated = probe.version < CURRENT_CONFIG_VERSION;
    let mut config: AppConfig = serde_json::from_value(raw)?;
    if migrated {
        apply_migrations(&mut config, probe.version)?;
    }
    config.validate()?;
    Ok(MigratedConfig {
        config,
        from_version: probe.version,
        migrated,
    })
}

/// Applies every migration step needed to reach [`CURRENT_CONFIG_VERSION`].
fn apply_migrations(config: &mut AppConfig, from_version: u32) -> Result<(), ConfigError> {
    let mut current = from_version;
    while current < CURRENT_CONFIG_VERSION {
        let step = MIGRATIONS
            .iter()
            .find(|migration| migration.from == current)
            .ok_or(ConfigError::UnsupportedVersion(current))?;
        if step.to > CURRENT_CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion(step.to));
        }
        (step.apply)(config);
        current = step.to;
    }
    Ok(())
}

/// Migrates a legacy (pre-versioning) config to version `1`.
///
/// v0 files predate the `version` field. Their missing fields are already
/// filled with defaults during deserialization, so this step only stamps
/// version `1`; the `v1 -> v2` step runs afterwards.
fn migrate_v0_to_v1(config: &mut AppConfig) {
    config.version = 1;
}

/// Migrates a version-`1` config to version `2`.
///
/// v2 adds `result_window.opacity`, `result_window.font_size_px`, and the
/// `floating_ball` section. Fields absent from v1 files are backfilled with
/// defaults by `serde(default)` during deserialization (see
/// [`crate::defaults`]), while fields a v1 file happens to contain are
/// preserved; this step only stamps the new version.
fn migrate_v1_to_v2(config: &mut AppConfig) {
    config.version = 2;
}

/// Migrates a version-`2` config to version `3`.
///
/// v3 adds `floating_ball.opacity` and `floating_ball.size_px`. Fields
/// absent from v2 files are backfilled with defaults by `serde(default)`
/// during deserialization (see [`crate::defaults`]), while fields a v2 file
/// happens to contain are preserved; this step only stamps the new version.
fn migrate_v2_to_v3(config: &mut AppConfig) {
    config.version = 3;
}

/// Migrates a version-`3` config to version `4`.
///
/// v4 adds `translation.quality`, which defaults to `"fast"`. The default is
/// applied by `serde(default)` during deserialization, so a v3 file without
/// the field is backfilled automatically while an explicit value (should one
/// exist in the file) is preserved.
///
/// v4 also requires `translation.source_language` to equal `ocr.language`.
/// Historical v3 files may have drifted apart, so the source language is
/// re-synchronized here with the OCR language as the authoritative value.
///
/// The step is idempotent: applying it to an already-synchronized config
/// leaves every field unchanged except the version stamp.
fn migrate_v3_to_v4(config: &mut AppConfig) {
    config.translation.source_language = config.ocr.language;
    config.version = 4;
}

/// Migrates a version-`4` config to version `5`.
///
/// v5 renames the old `"api"` provider id to `"openai"` and adds
/// `translation.region` / `translation.app_id`. The new fields are
/// backfilled with `None` by `serde(default)` during deserialization (see
/// [`crate::defaults`]); any explicit values present in the file are
/// preserved because the current schema parses them.
///
/// The step is idempotent: an already-renamed provider and already-set
/// `region` / `app_id` values are left untouched, so re-applying the step
/// only ever bumps the version stamp.
fn migrate_v4_to_v5(config: &mut AppConfig) {
    if config.translation.provider == "api" {
        config.translation.provider = "openai".to_string();
    }
    config.version = 5;
}

/// Minimal deserialization target used to read the `version` field.
#[derive(Deserialize)]
struct VersionProbe {
    /// The stored schema version; `0` when absent.
    #[serde(default)]
    version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtrans_core::Language;

    #[test]
    fn versionless_json_is_migrated_from_v0() {
        let raw = serde_json::json!({
            "capture": { "interval_ms": 1000 }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.capture.interval_ms, 1000);
        assert!((config.capture.difference_threshold - 0.03).abs() < f32::EPSILON);
        assert_eq!(config.ocr.language, Language::Auto);
        assert_eq!(
            config.translation.target_language,
            Language::ChineseSimplified
        );
        assert_eq!(config.translation.provider, "openai");
        assert_eq!(config.log_level, "info");
    }

    #[test]
    fn explicit_v0_is_migrated() {
        let raw = serde_json::json!({
            "version": 0,
            "translation": { "provider": "local", "target_language": "ja" }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.translation.provider, "local");
        assert_eq!(config.translation.target_language, Language::Japanese);
        assert_eq!(config.capture.interval_ms, 500);
    }

    #[test]
    fn current_version_passes_through() {
        let raw = serde_json::json!({
            "version": 5,
            "capture": { "interval_ms": 700 }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.version, 5);
        assert_eq!(config.capture.interval_ms, 700);
    }

    #[test]
    fn newer_version_is_rejected() {
        let raw = serde_json::json!({ "version": 6 });
        let err = migrate_value(raw).unwrap_err();
        assert!(matches!(err, ConfigError::UnsupportedVersion(6)));
    }

    #[test]
    fn malformed_version_is_a_parse_error() {
        let raw = serde_json::json!({ "version": "two" });
        assert!(matches!(migrate_value(raw), Err(ConfigError::Parse(_))));
    }

    #[test]
    fn negative_version_is_a_parse_error() {
        let raw = serde_json::json!({ "version": -1 });
        assert!(matches!(migrate_value(raw), Err(ConfigError::Parse(_))));
    }

    #[test]
    fn migrated_config_is_validated() {
        // v0 with an out-of-range interval must fail validation.
        let raw = serde_json::json!({
            "version": 0,
            "capture": { "interval_ms": 10 }
        });
        assert!(matches!(
            migrate_value(raw),
            Err(ConfigError::Validation(_))
        ));
    }

    #[test]
    fn migrated_flag_is_true_for_v0() {
        let raw = serde_json::json!({ "version": 0 });
        let migrated = migrate_value(raw).unwrap();
        assert!(migrated.migrated);
        assert_eq!(migrated.from_version, 0);
        assert_eq!(migrated.config.version, CURRENT_CONFIG_VERSION);
    }

    #[test]
    fn migrated_flag_is_false_for_current_version() {
        let raw = serde_json::json!({ "version": 5 });
        let migrated = migrate_value(raw).unwrap();
        assert!(!migrated.migrated);
        assert_eq!(migrated.from_version, 5);
    }

    #[test]
    fn versionless_file_is_reported_as_v0() {
        let raw = serde_json::json!({});
        let migrated = migrate_value(raw).unwrap();
        assert!(migrated.migrated);
        assert_eq!(migrated.from_version, 0);
    }

    #[test]
    fn v1_missing_new_fields_is_migrated_with_defaults() {
        let raw = serde_json::json!({
            "version": 1,
            "result_window": { "always_on_top": false }
        });
        let migrated = migrate_value(raw).unwrap();
        assert!(migrated.migrated);
        assert_eq!(migrated.from_version, 1);
        assert_eq!(migrated.config.version, CURRENT_CONFIG_VERSION);
        assert!(!migrated.config.result_window.always_on_top);
        assert!((migrated.config.result_window.opacity - 0.95).abs() < f64::EPSILON);
        assert_eq!(migrated.config.result_window.font_size_px, 14);
        assert!(!migrated.config.floating_ball.enabled);
    }

    #[test]
    fn v1_present_new_fields_are_preserved() {
        let raw = serde_json::json!({
            "version": 1,
            "result_window": { "opacity": 0.8, "font_size_px": 18 },
            "floating_ball": { "enabled": true }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
        assert!((config.result_window.opacity - 0.8).abs() < f64::EPSILON);
        assert_eq!(config.result_window.font_size_px, 18);
        assert!(config.floating_ball.enabled);
    }

    #[test]
    fn v2_missing_new_fields_is_migrated_with_defaults() {
        let raw = serde_json::json!({
            "version": 2,
            "floating_ball": { "enabled": true }
        });
        let migrated = migrate_value(raw).unwrap();
        assert!(migrated.migrated);
        assert_eq!(migrated.from_version, 2);
        assert_eq!(migrated.config.version, CURRENT_CONFIG_VERSION);
        assert!(migrated.config.floating_ball.enabled);
        assert!((migrated.config.floating_ball.opacity - 1.0).abs() < f64::EPSILON);
        assert_eq!(migrated.config.floating_ball.size_px, 48);
    }

    #[test]
    fn v2_present_new_fields_are_preserved() {
        let raw = serde_json::json!({
            "version": 2,
            "floating_ball": { "enabled": true, "opacity": 0.85, "size_px": 64 }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
        assert!(config.floating_ball.enabled);
        assert!((config.floating_ball.opacity - 0.85).abs() < f64::EPSILON);
        assert_eq!(config.floating_ball.size_px, 64);
        assert_eq!(config.translation.quality, "fast");
        assert_eq!(config.ocr.language, config.translation.source_language);
    }

    #[test]
    fn v0_config_is_migrated_through_current() {
        let raw = serde_json::json!({ "version": 0 });
        let migrated = migrate_value(raw).unwrap();
        assert_eq!(migrated.from_version, 0);
        assert_eq!(migrated.config.version, CURRENT_CONFIG_VERSION);
        assert!((migrated.config.result_window.opacity - 0.95).abs() < f64::EPSILON);
        assert_eq!(migrated.config.result_window.font_size_px, 14);
        assert!(!migrated.config.floating_ball.enabled);
        assert!((migrated.config.floating_ball.opacity - 1.0).abs() < f64::EPSILON);
        assert_eq!(migrated.config.floating_ball.size_px, 48);
        assert_eq!(migrated.config.translation.quality, "fast");
        assert_eq!(
            migrated.config.ocr.language,
            migrated.config.translation.source_language
        );
        assert_eq!(migrated.config.translation.provider, "openai");
    }

    #[test]
    fn v3_config_with_inconsistent_languages_is_synced() {
        let raw = serde_json::json!({
            "version": 3,
            "ocr": { "language": "ja" },
            "translation": {
                "source_language": "en",
                "target_language": "zh-CN"
            }
        });
        let migrated = migrate_value(raw).unwrap();
        assert!(migrated.migrated);
        assert_eq!(migrated.from_version, 3);
        assert_eq!(migrated.config.version, CURRENT_CONFIG_VERSION);
        assert_eq!(migrated.config.ocr.language, Language::Japanese);
        assert_eq!(
            migrated.config.translation.source_language,
            Language::Japanese
        );
        assert_eq!(migrated.config.translation.quality, "fast");
        assert_eq!(
            migrated.config.translation.target_language,
            Language::ChineseSimplified
        );
    }

    #[test]
    fn v3_config_missing_quality_defaults_to_fast() {
        let raw = serde_json::json!({
            "version": 3,
            "translation": { "provider": "local" }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.translation.quality, "fast");
    }

    #[test]
    fn v3_config_explicit_quality_is_preserved() {
        // A v3 file that already carries a quality value keeps it; only a
        // missing field is backfilled with the default.
        let raw = serde_json::json!({
            "version": 3,
            "translation": { "quality": "balanced" }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.translation.quality, "balanced");
    }

    #[test]
    fn v3_sync_when_ocr_language_is_missing() {
        // When only `source_language` was set in a v3 file, the OCR default
        // (`auto`) is authoritative and the source language follows it.
        let raw = serde_json::json!({
            "version": 3,
            "translation": { "source_language": "en" }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.ocr.language, Language::Auto);
        assert_eq!(config.translation.source_language, Language::Auto);
    }

    #[test]
    fn v5_config_re_migration_is_idempotent() {
        // A v5 config passes through `migrate_value` untouched: no version
        // bump, no provider rename, no field changes.
        let raw = serde_json::json!({
            "version": 5,
            "ocr": { "language": "ja" },
            "translation": {
                "provider": "openai",
                "quality": "balanced",
                "region": "eastasia",
                "app_id": "2026081000000000",
                "source_language": "ja",
                "target_language": "zh-CN"
            }
        });
        let migrated = migrate_value(raw).unwrap();
        assert!(!migrated.migrated);
        assert_eq!(migrated.config.version, 5);
        assert_eq!(migrated.config.translation.provider, "openai");
        assert_eq!(migrated.config.ocr.language, Language::Japanese);
        assert_eq!(
            migrated.config.translation.source_language,
            Language::Japanese
        );
        assert_eq!(migrated.config.translation.quality, "balanced");
        assert_eq!(
            migrated.config.translation.region.as_deref(),
            Some("eastasia")
        );
        assert_eq!(
            migrated.config.translation.app_id.as_deref(),
            Some("2026081000000000")
        );
    }

    #[test]
    fn v4_config_with_api_provider_is_renamed_to_openai() {
        let raw = serde_json::json!({
            "version": 4,
            "translation": {
                "provider": "api",
                "quality": "balanced",
                "target_language": "zh-CN"
            }
        });
        let migrated = migrate_value(raw).unwrap();
        assert!(migrated.migrated);
        assert_eq!(migrated.from_version, 4);
        assert_eq!(migrated.config.version, CURRENT_CONFIG_VERSION);
        assert_eq!(migrated.config.translation.provider, "openai");
        assert_eq!(migrated.config.translation.quality, "balanced");
        assert_eq!(migrated.config.translation.region, None);
        assert_eq!(migrated.config.translation.app_id, None);
    }

    #[test]
    fn v4_config_with_local_provider_is_preserved() {
        let raw = serde_json::json!({
            "version": 4,
            "translation": { "provider": "local" }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.translation.provider, "local");
    }

    #[test]
    fn v4_config_with_openai_provider_is_preserved() {
        // A v4 file that already uses the new id keeps it; the migration
        // only rewrites the legacy "api" value.
        let raw = serde_json::json!({
            "version": 4,
            "translation": { "provider": "openai" }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.translation.provider, "openai");
    }

    #[test]
    fn v4_config_region_and_app_id_are_preserved() {
        // The v5 schema parses region/app_id, so values present in a v4
        // file survive the migration instead of being dropped.
        let raw = serde_json::json!({
            "version": 4,
            "translation": {
                "provider": "api",
                "region": "eastasia",
                "app_id": "2026081000000000"
            }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.translation.provider, "openai");
        assert_eq!(config.translation.region.as_deref(), Some("eastasia"));
        assert_eq!(
            config.translation.app_id.as_deref(),
            Some("2026081000000000")
        );
    }

    #[test]
    fn migrate_v4_to_v5_is_idempotent() {
        let mut config = AppConfig {
            translation: crate::schema::TranslationConfig {
                provider: "api".to_string(),
                region: Some("eastasia".to_string()),
                app_id: Some("2026081000000000".to_string()),
                ..Default::default()
            },
            version: 4,
            ..Default::default()
        };
        migrate_v4_to_v5(&mut config);
        let after_first = config.clone();
        migrate_v4_to_v5(&mut config);
        assert_eq!(config, after_first);
        assert_eq!(config.version, 5);
        assert_eq!(config.translation.provider, "openai");
        assert_eq!(config.translation.region.as_deref(), Some("eastasia"));
        assert_eq!(
            config.translation.app_id.as_deref(),
            Some("2026081000000000")
        );
    }

    #[test]
    fn migrate_v3_to_v4_is_idempotent() {
        let mut config = AppConfig {
            ocr: crate::schema::OcrConfig {
                language: Language::Japanese,
                ..Default::default()
            },
            translation: crate::schema::TranslationConfig {
                quality: "fast".to_string(),
                source_language: Language::Japanese,
                ..Default::default()
            },
            version: 3,
            ..Default::default()
        };
        migrate_v3_to_v4(&mut config);
        let after_first = config.clone();
        migrate_v3_to_v4(&mut config);
        assert_eq!(config, after_first);
        assert_eq!(config.version, 4);
    }
}
