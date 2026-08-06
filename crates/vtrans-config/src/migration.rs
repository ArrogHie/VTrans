//! Version migration for config files.
//!
//! Config files carry an integer `version` field. Files written before the
//! `version` field existed (or without it) are treated as version `0` and
//! upgraded step by step to [`CURRENT_CONFIG_VERSION`]. Missing fields are
//! filled with defaults during deserialization (see [`crate::defaults`]),
//! so migrations only need to handle structural changes and version
//! stamping; see [`migrate_v0_to_v1`].

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
            "version": 2,
            "capture": { "interval_ms": 700 }
        });
        let config = migrate_value(raw).unwrap().config;
        assert_eq!(config.version, 2);
        assert_eq!(config.capture.interval_ms, 700);
    }

    #[test]
    fn newer_version_is_rejected() {
        let raw = serde_json::json!({ "version": 3 });
        let err = migrate_value(raw).unwrap_err();
        assert!(matches!(err, ConfigError::UnsupportedVersion(3)));
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
        let raw = serde_json::json!({ "version": 2 });
        let migrated = migrate_value(raw).unwrap();
        assert!(!migrated.migrated);
        assert_eq!(migrated.from_version, 2);
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
    fn v0_config_is_migrated_through_v2() {
        let raw = serde_json::json!({ "version": 0 });
        let migrated = migrate_value(raw).unwrap();
        assert_eq!(migrated.from_version, 0);
        assert_eq!(migrated.config.version, CURRENT_CONFIG_VERSION);
        assert!((migrated.config.result_window.opacity - 0.95).abs() < f64::EPSILON);
        assert_eq!(migrated.config.result_window.font_size_px, 14);
        assert!(!migrated.config.floating_ball.enabled);
    }
}
