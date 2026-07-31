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
const MIGRATIONS: &[Migration] = &[Migration {
    from: 0,
    to: 1,
    apply: migrate_v0_to_v1,
}];

/// Reads the schema version stored in raw config JSON.
///
/// A missing `version` key is treated as version `0` (legacy file). A
/// malformed or out-of-range version is normalized to `0` here and is
/// rejected with a parse error by [`migrate_value`] via [`VersionProbe`].
pub(crate) fn raw_version(raw: &Value) -> u32 {
    raw.get("version")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or_default()
}

/// Migrates raw config JSON to the latest schema version and validates it.
///
/// Missing fields are filled with defaults during deserialization. Returns
/// [`ConfigError::UnsupportedVersion`] when the file is newer than this
/// build supports and [`ConfigError::Validation`] when the migrated config
/// violates a validation rule.
pub(crate) fn migrate_value(raw: Value) -> Result<AppConfig, ConfigError> {
    // Probe the version first: a malformed `version` field surfaces as a
    // clear parse error instead of being silently treated as legacy.
    let probe: VersionProbe = serde_json::from_value(raw.clone())?;
    if probe.version > CURRENT_CONFIG_VERSION {
        return Err(ConfigError::UnsupportedVersion(probe.version));
    }

    let mut config: AppConfig = serde_json::from_value(raw)?;
    if probe.version < CURRENT_CONFIG_VERSION {
        apply_migrations(&mut config, probe.version)?;
    }
    config.validate()?;
    Ok(config)
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
/// the current version. Future migrations will transform field values
/// here as the schema evolves.
fn migrate_v0_to_v1(config: &mut AppConfig) {
    config.version = CURRENT_CONFIG_VERSION;
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
        let config = migrate_value(raw).unwrap();
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
        let config = migrate_value(raw).unwrap();
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.translation.provider, "local");
        assert_eq!(config.translation.target_language, Language::Japanese);
        assert_eq!(config.capture.interval_ms, 500);
    }

    #[test]
    fn current_version_passes_through() {
        let raw = serde_json::json!({
            "version": 1,
            "capture": { "interval_ms": 700 }
        });
        let config = migrate_value(raw).unwrap();
        assert_eq!(config.version, 1);
        assert_eq!(config.capture.interval_ms, 700);
    }

    #[test]
    fn newer_version_is_rejected() {
        let raw = serde_json::json!({ "version": 2 });
        let err = migrate_value(raw).unwrap_err();
        assert!(matches!(err, ConfigError::UnsupportedVersion(2)));
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
    fn raw_version_missing_key_is_zero() {
        let raw = serde_json::json!({});
        assert_eq!(raw_version(&raw), 0);
    }

    #[test]
    fn raw_version_reads_numeric_key() {
        let raw = serde_json::json!({ "version": 1 });
        assert_eq!(raw_version(&raw), 1);
    }

    #[test]
    fn raw_version_tolerates_malformed_key() {
        let raw = serde_json::json!({ "version": "one" });
        assert_eq!(raw_version(&raw), 0);
    }
}
