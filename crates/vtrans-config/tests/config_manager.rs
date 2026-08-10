//! End-to-end tests for the `vtrans-config` public API.
//!
//! These tests exercise the `ConfigManager` through its public surface
//! against real temporary directories, covering first-run default creation,
//! round trips, migration of a legacy fixture, validation errors, and
//! concurrent updates.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Barrier};

use tempfile::tempdir;
use vtrans_config::{AppConfig, ConfigError, ConfigManager, CURRENT_CONFIG_VERSION};
use vtrans_core::Language;

/// Directory containing test fixture files.
const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// Writes `contents` to `config.json` inside `dir`.
fn write_config(dir: &Path, contents: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("config.json"), contents).unwrap();
}

/// Reads the config file back as raw JSON.
fn read_raw(dir: &Path) -> serde_json::Value {
    let contents = fs::read_to_string(dir.join("config.json")).unwrap();
    serde_json::from_str(&contents).unwrap()
}

#[test]
fn first_load_returns_default_and_writes_file() {
    let dir = tempdir().unwrap();
    let manager = ConfigManager::new(dir.path()).unwrap();

    let config = manager.load().unwrap();
    assert_eq!(config, AppConfig::default());
    assert!(manager.config_path().is_file());

    let on_disk: AppConfig =
        serde_json::from_str(&fs::read_to_string(manager.config_path()).unwrap()).unwrap();
    assert_eq!(on_disk, AppConfig::default());
    assert_eq!(on_disk.version, CURRENT_CONFIG_VERSION);
}

#[test]
fn save_and_load_round_trip_preserves_all_sections() {
    let dir = tempdir().unwrap();
    let manager = ConfigManager::new(dir.path()).unwrap();

    let mut config = AppConfig::default();
    config.capture.interval_ms = 1200;
    config.capture.difference_threshold = 0.10;
    config.ocr.language = Language::Japanese;
    config.ocr.min_confidence = 0.70;
    config.translation.provider = "local".to_string();
    config.translation.source_language = Language::Japanese;
    config.translation.target_language = Language::English;
    config.translation.region = Some("eastasia".to_string());
    config.translation.app_id = Some("2026081000000000".to_string());
    config.translation.timeout_seconds = 45;
    config.result_window.always_on_top = false;
    config.result_window.opacity = 0.8;
    config.result_window.font_size_px = 18;
    config.floating_ball.enabled = true;
    config.floating_ball.opacity = 0.9;
    config.floating_ball.size_px = 56;
    config.hotkeys.live_translate = "Ctrl+Shift+L".to_string();
    config.log_level = "debug".to_string();
    config.model_dir = Some(dir.path().join("models"));

    manager.save(&config).unwrap();
    assert_eq!(manager.load().unwrap(), config);
}

#[test]
fn update_applies_and_persists_mutation() {
    let dir = tempdir().unwrap();
    let manager = ConfigManager::new(dir.path()).unwrap();
    manager.save(&AppConfig::default()).unwrap();

    manager
        .update(|c| {
            c.capture.interval_ms = 900;
            c.hotkeys.stop_live = "Ctrl+Shift+X".to_string();
        })
        .unwrap();

    let loaded = manager.load().unwrap();
    assert_eq!(loaded.capture.interval_ms, 900);
    assert_eq!(loaded.hotkeys.stop_live, "Ctrl+Shift+X");
}

#[test]
fn update_on_missing_file_returns_not_found() {
    let dir = tempdir().unwrap();
    let manager = ConfigManager::new(dir.path()).unwrap();

    let err = manager
        .update(|c| c.log_level = "debug".to_string())
        .unwrap_err();
    assert!(matches!(err, ConfigError::NotFound(_)));
}

#[test]
fn invalid_range_is_rejected_on_load() {
    let dir = tempdir().unwrap();
    write_config(dir.path(), r#"{"capture":{"interval_ms":10},"version":5}"#);
    let manager = ConfigManager::new(dir.path()).unwrap();

    let err = manager.load().unwrap_err();
    match err {
        ConfigError::Validation(msg) => assert!(msg.contains("capture.interval_ms")),
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[test]
fn newer_version_is_rejected() {
    let dir = tempdir().unwrap();
    write_config(dir.path(), r#"{"version":99}"#);
    let manager = ConfigManager::new(dir.path()).unwrap();

    assert!(matches!(
        manager.load(),
        Err(ConfigError::UnsupportedVersion(99))
    ));
}

#[test]
fn malformed_json_is_rejected() {
    let dir = tempdir().unwrap();
    write_config(dir.path(), "{ not json !!");
    let manager = ConfigManager::new(dir.path()).unwrap();

    assert!(matches!(manager.load(), Err(ConfigError::Parse(_))));
}

#[test]
fn versionless_fixture_is_migrated_and_persisted() {
    let dir = tempdir().unwrap();
    let fixture = fs::read_to_string(Path::new(FIXTURES_DIR).join("config_v0.json")).unwrap();
    write_config(dir.path(), &fixture);
    let manager = ConfigManager::new(dir.path()).unwrap();

    let config = manager.load().unwrap();

    // Fields present in the fixture survive the migration.
    assert_eq!(config.capture.interval_ms, 1000);
    assert_eq!(config.translation.provider, "local");
    assert_eq!(config.translation.target_language, Language::Japanese);

    // Missing fields are filled with defaults.
    assert_eq!(config.ocr.language, Language::Auto);
    assert_eq!(config.hotkeys.select_and_translate, "Alt+Shift+A");
    assert_eq!(config.log_level, "info");
    assert_eq!(config.version, CURRENT_CONFIG_VERSION);

    // The migrated file is persisted with the new version stamped in.
    let persisted = read_raw(dir.path());
    assert_eq!(
        persisted["version"].as_u64(),
        Some(u64::from(CURRENT_CONFIG_VERSION))
    );
}

#[test]
fn update_migrates_v0_file_before_applying_mutation() {
    let dir = tempdir().unwrap();
    let fixture = fs::read_to_string(Path::new(FIXTURES_DIR).join("config_v0.json")).unwrap();
    write_config(dir.path(), &fixture);
    let manager = ConfigManager::new(dir.path()).unwrap();

    manager.update(|c| c.capture.interval_ms = 1500).unwrap();

    let loaded = manager.load().unwrap();
    assert_eq!(loaded.capture.interval_ms, 1500);
    // Fields from the v0 fixture survive the migration + mutation cycle.
    assert_eq!(loaded.translation.provider, "local");
    assert_eq!(loaded.translation.target_language, Language::Japanese);
    assert_eq!(loaded.version, CURRENT_CONFIG_VERSION);
}

#[test]
fn load_does_not_rewrite_current_version_file() {
    let dir = tempdir().unwrap();
    // Compact v5 JSON: loading must not touch the file when it is already
    // at the current version (no pretty-printing, no field expansion).
    let raw = r#"{"version":5,"capture":{"interval_ms":600,"difference_threshold":0.05}}"#;
    write_config(dir.path(), raw);
    let manager = ConfigManager::new(dir.path()).unwrap();

    manager.load().unwrap();

    let on_disk = fs::read_to_string(dir.path().join("config.json")).unwrap();
    assert_eq!(on_disk, raw);
}

#[test]
fn v1_config_missing_new_fields_is_migrated_with_defaults() {
    let dir = tempdir().unwrap();
    write_config(
        dir.path(),
        r#"{"version":1,"result_window":{"always_on_top":false}}"#,
    );
    let manager = ConfigManager::new(dir.path()).unwrap();

    let config = manager.load().unwrap();

    assert_eq!(config.version, CURRENT_CONFIG_VERSION);
    assert!(!config.result_window.always_on_top);
    assert!((config.result_window.opacity - 0.95).abs() < f64::EPSILON);
    assert_eq!(config.result_window.font_size_px, 14);
    assert!(!config.floating_ball.enabled);

    // The migrated file is persisted with the new fields and version.
    let persisted = read_raw(dir.path());
    assert_eq!(
        persisted["version"].as_u64(),
        Some(u64::from(CURRENT_CONFIG_VERSION))
    );
    assert_eq!(persisted["result_window"]["opacity"].as_f64(), Some(0.95));
    assert_eq!(
        persisted["result_window"]["font_size_px"].as_u64(),
        Some(14)
    );
    assert_eq!(persisted["floating_ball"]["enabled"].as_bool(), Some(false));
    assert_eq!(persisted["floating_ball"]["opacity"].as_f64(), Some(1.0));
    assert_eq!(persisted["floating_ball"]["size_px"].as_u64(), Some(48));
    assert_eq!(persisted["translation"]["quality"].as_str(), Some("fast"));
}

#[test]
fn invalid_opacity_is_rejected_on_load() {
    let dir = tempdir().unwrap();
    write_config(
        dir.path(),
        r#"{"version":5,"result_window":{"opacity":0.2}}"#,
    );
    let manager = ConfigManager::new(dir.path()).unwrap();

    let err = manager.load().unwrap_err();
    match err {
        ConfigError::Validation(msg) => assert!(msg.contains("result_window.opacity")),
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[test]
fn invalid_font_size_is_rejected_on_load() {
    let dir = tempdir().unwrap();
    write_config(
        dir.path(),
        r#"{"version":5,"result_window":{"font_size_px":30}}"#,
    );
    let manager = ConfigManager::new(dir.path()).unwrap();

    let err = manager.load().unwrap_err();
    match err {
        ConfigError::Validation(msg) => assert!(msg.contains("result_window.font_size_px")),
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[test]
fn v2_config_missing_new_fields_is_migrated_with_defaults() {
    let dir = tempdir().unwrap();
    write_config(
        dir.path(),
        r#"{"version":2,"floating_ball":{"enabled":true}}"#,
    );
    let manager = ConfigManager::new(dir.path()).unwrap();

    let config = manager.load().unwrap();

    assert_eq!(config.version, CURRENT_CONFIG_VERSION);
    assert!(config.floating_ball.enabled);
    assert!((config.floating_ball.opacity - 1.0).abs() < f64::EPSILON);
    assert_eq!(config.floating_ball.size_px, 48);

    // The migrated file is persisted with the new fields and version.
    let persisted = read_raw(dir.path());
    assert_eq!(
        persisted["version"].as_u64(),
        Some(u64::from(CURRENT_CONFIG_VERSION))
    );
    assert_eq!(persisted["floating_ball"]["opacity"].as_f64(), Some(1.0));
    assert_eq!(persisted["floating_ball"]["size_px"].as_u64(), Some(48));
}

#[test]
fn v2_config_present_new_fields_are_preserved() {
    let dir = tempdir().unwrap();
    write_config(
        dir.path(),
        r#"{"version":2,"floating_ball":{"enabled":true,"opacity":0.85,"size_px":64}}"#,
    );
    let manager = ConfigManager::new(dir.path()).unwrap();

    let config = manager.load().unwrap();

    assert_eq!(config.version, CURRENT_CONFIG_VERSION);
    assert!(config.floating_ball.enabled);
    assert!((config.floating_ball.opacity - 0.85).abs() < f64::EPSILON);
    assert_eq!(config.floating_ball.size_px, 64);
}

#[test]
fn invalid_floating_ball_opacity_is_rejected_on_load() {
    let dir = tempdir().unwrap();
    write_config(
        dir.path(),
        r#"{"version":5,"floating_ball":{"opacity":0.2}}"#,
    );
    let manager = ConfigManager::new(dir.path()).unwrap();

    let err = manager.load().unwrap_err();
    match err {
        ConfigError::Validation(msg) => assert!(msg.contains("floating_ball.opacity")),
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[test]
fn invalid_floating_ball_size_is_rejected_on_load() {
    let dir = tempdir().unwrap();
    write_config(
        dir.path(),
        r#"{"version":5,"floating_ball":{"size_px":80}}"#,
    );
    let manager = ConfigManager::new(dir.path()).unwrap();

    let err = manager.load().unwrap_err();
    match err {
        ConfigError::Validation(msg) => assert!(msg.contains("floating_ball.size_px")),
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[test]
fn v3_config_with_inconsistent_languages_is_migrated_and_persisted() {
    let dir = tempdir().unwrap();
    let fixture = fs::read_to_string(Path::new(FIXTURES_DIR).join("config_v3.json")).unwrap();
    write_config(dir.path(), &fixture);
    let manager = ConfigManager::new(dir.path()).unwrap();

    let config = manager.load().unwrap();

    // The OCR language is authoritative: the drifted source language follows it.
    assert_eq!(config.ocr.language, Language::Japanese);
    assert_eq!(config.translation.source_language, Language::Japanese);
    assert_eq!(config.translation.quality, "fast");
    assert_eq!(config.version, CURRENT_CONFIG_VERSION);

    // The migration result is persisted, including the new quality field.
    let persisted = read_raw(dir.path());
    assert_eq!(
        persisted["version"].as_u64(),
        Some(u64::from(CURRENT_CONFIG_VERSION))
    );
    assert_eq!(persisted["ocr"]["language"].as_str(), Some("ja"));
    assert_eq!(
        persisted["translation"]["source_language"].as_str(),
        Some("ja")
    );
    assert_eq!(persisted["translation"]["quality"].as_str(), Some("fast"));
}

#[test]
fn v4_config_with_api_provider_is_migrated_and_persisted() {
    let dir = tempdir().unwrap();
    let fixture = fs::read_to_string(Path::new(FIXTURES_DIR).join("config_v4.json")).unwrap();
    write_config(dir.path(), &fixture);
    let manager = ConfigManager::new(dir.path()).unwrap();

    let config = manager.load().unwrap();

    // The legacy "api" id is renamed to "openai"; quality and language
    // linkage survive; the new fields default to None.
    assert_eq!(config.translation.provider, "openai");
    assert_eq!(config.translation.quality, "balanced");
    assert_eq!(config.translation.region, None);
    assert_eq!(config.translation.app_id, None);
    assert_eq!(config.version, CURRENT_CONFIG_VERSION);

    // The migration result is persisted.
    let persisted = read_raw(dir.path());
    assert_eq!(
        persisted["version"].as_u64(),
        Some(u64::from(CURRENT_CONFIG_VERSION))
    );
    assert_eq!(
        persisted["translation"]["provider"].as_str(),
        Some("openai")
    );
}

#[test]
fn cloud_provider_config_round_trips_through_manager() {
    let dir = tempdir().unwrap();
    let manager = ConfigManager::new(dir.path()).unwrap();

    let mut config = AppConfig::default();
    config.translation.provider = "azure".to_string();
    config.translation.region = Some("eastasia".to_string());
    config.translation.api_model = String::new();
    manager.save(&config).unwrap();

    let loaded = manager.load().unwrap();
    assert_eq!(loaded.translation.provider, "azure");
    assert_eq!(loaded.translation.region.as_deref(), Some("eastasia"));
    assert!(loaded.translation.api_model.is_empty());
}

#[test]
fn baidu_provider_without_app_id_is_rejected_on_save() {
    let dir = tempdir().unwrap();
    let manager = ConfigManager::new(dir.path()).unwrap();

    let mut config = AppConfig::default();
    config.translation.provider = "baidu".to_string();
    let err = manager.save(&config).unwrap_err();
    match err {
        ConfigError::Validation(msg) => assert!(msg.contains("translation.app_id")),
        other => panic!("expected Validation, got {other:?}"),
    }
    assert!(!manager.config_path().exists());
}

#[test]
fn legacy_api_provider_is_rejected_at_current_version() {
    let dir = tempdir().unwrap();
    let manager = ConfigManager::new(dir.path()).unwrap();

    let mut config = AppConfig::default();
    config.translation.provider = "api".to_string();
    let err = manager.save(&config).unwrap_err();
    match err {
        ConfigError::Validation(msg) => assert!(msg.contains("translation.provider")),
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[test]
fn deepl_provider_with_empty_model_is_accepted() {
    let dir = tempdir().unwrap();
    let manager = ConfigManager::new(dir.path()).unwrap();

    let mut config = AppConfig::default();
    config.translation.provider = "deepl".to_string();
    config.translation.api_model = String::new();
    manager.save(&config).unwrap();

    assert!(manager.load().unwrap().translation.api_model.is_empty());
}

#[test]
fn mismatched_languages_at_current_version_are_rejected_on_load() {
    let dir = tempdir().unwrap();
    write_config(
        dir.path(),
        r#"{"version":5,"ocr":{"language":"ja"},"translation":{"source_language":"en"}}"#,
    );
    let manager = ConfigManager::new(dir.path()).unwrap();

    let err = manager.load().unwrap_err();
    match err {
        ConfigError::Validation(msg) => {
            assert!(msg.contains("ocr.language"), "message: {msg}");
            assert!(msg.contains("source_language"), "message: {msg}");
        }
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[test]
fn quality_round_trip_through_manager() {
    let dir = tempdir().unwrap();
    let manager = ConfigManager::new(dir.path()).unwrap();

    let mut config = AppConfig::default();
    config.translation.quality = "balanced".to_string();
    manager.save(&config).unwrap();

    let loaded = manager.load().unwrap();
    assert_eq!(loaded.translation.quality, "balanced");
    let persisted = read_raw(dir.path());
    assert_eq!(
        persisted["translation"]["quality"].as_str(),
        Some("balanced")
    );
}

#[test]
fn invalid_quality_is_rejected_on_save() {
    let dir = tempdir().unwrap();
    let manager = ConfigManager::new(dir.path()).unwrap();

    let mut config = AppConfig::default();
    config.translation.quality = "ultra".to_string();
    let err = manager.save(&config).unwrap_err();
    match err {
        ConfigError::Validation(msg) => assert!(msg.contains("translation.quality")),
        other => panic!("expected Validation, got {other:?}"),
    }
    assert!(!manager.config_path().exists());
}

#[test]
fn concurrent_updates_do_not_lose_mutations() {
    const THREADS: usize = 8;

    let dir = tempdir().unwrap();
    let manager = Arc::new(ConfigManager::new(dir.path()).unwrap());
    manager.save(&AppConfig::default()).unwrap();

    let barrier = Arc::new(Barrier::new(THREADS));
    let handles: Vec<_> = (0..THREADS)
        .map(|_| {
            let manager = Arc::clone(&manager);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                manager.update(|c| c.capture.interval_ms += 1).unwrap();
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    let loaded = manager.load().unwrap();
    let expected = 500 + u32::try_from(THREADS).unwrap();
    assert_eq!(loaded.capture.interval_ms, expected);
}
