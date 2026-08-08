//! Integration tests for [`NativeTranslationProvider`] against the real
//! native bridge and models.
//!
//! These tests are `#[ignore]`d by default because they require:
//!
//! 1. the translation models at `src-tauri/resources/models` (manifest v2,
//!    downloaded by `scripts/translation/setup_translation_models.ps1`),
//!    and
//! 2. `translation_bridge.dll` next to the models directory
//!    (`src-tauri/resources/native/`, built by
//!    `native/translation_bridge/build.ps1`).
//!
//! Run with:
//!
//! ```text
//! cargo test -p vtrans-translation --test native_provider -- --ignored
//! ```
//!
//! The models directory can be overridden with the `VTRANS_MODELS_DIR`
//! environment variable.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use vtrans_core::traits::TranslationProvider;
use vtrans_core::types::{Language, TranslationRequest};
use vtrans_models::ModelManager;
use vtrans_translation::{NativeTranslationProvider, TranslationQuality};

/// One fixed regression sample (integration guide section 27).
#[derive(Debug, Deserialize)]
struct RegressionSample {
    source_language: String,
    source_text: String,
    expected_contains: Vec<String>,
}

fn models_dir() -> PathBuf {
    std::env::var("VTRANS_MODELS_DIR").map_or_else(
        |_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources/models"),
        PathBuf::from,
    )
}

fn load_samples() -> Vec<RegressionSample> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/regression_samples.json");
    let content = std::fs::read_to_string(&path).expect("read regression_samples.json");
    serde_json::from_str(&content).expect("parse regression_samples.json")
}

fn source_language(code: &str) -> Language {
    Language::from_code(code)
        .unwrap_or_else(|| panic!("unsupported source language in fixture: {code}"))
}

/// Load the provider once per test binary run (models load is slow).
fn provider() -> NativeTranslationProvider {
    let dir = models_dir();
    let manager = ModelManager::from_manifest_dir(&dir)
        .unwrap_or_else(|error| panic!("load manifest from {}: {error}", dir.display()));
    let report = manager.verify_integrity().unwrap();
    assert!(
        report.is_ok(),
        "model integrity failed: {:#?}",
        report.failed
    );
    NativeTranslationProvider::from_manager(&manager)
        .expect("create native provider")
        .with_quality(TranslationQuality::Balanced)
        .expect("apply balanced quality")
}

#[tokio::test]
#[ignore = "requires real translation models and translation_bridge.dll"]
async fn en_zh_and_ja_zh_regression_samples_pass() {
    let provider = provider();
    assert_eq!(provider.id(), "local-native");

    for sample in load_samples() {
        let source = source_language(&sample.source_language);
        let request =
            TranslationRequest::new(&sample.source_text, source, Language::ChineseSimplified);
        let result = provider
            .translate(&request, CancellationToken::new())
            .await
            .unwrap_or_else(|error| {
                panic!("translation failed for {:?}: {error}", sample.source_text)
            });
        assert!(
            sample
                .expected_contains
                .iter()
                .any(|expected| result.translated_text.contains(expected)),
            "translation {:?} contains none of {:?}",
            result.translated_text,
            sample.expected_contains
        );
    }
}

#[tokio::test]
#[ignore = "requires real translation models and translation_bridge.dll"]
async fn unsupported_pair_and_auto_source_are_rejected_before_ffi() {
    let provider = provider();

    let request = TranslationRequest::new(
        "你好",
        Language::ChineseSimplified,
        Language::ChineseSimplified,
    );
    let error = provider
        .translate(&request, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        vtrans_core::TranslationError::UnsupportedPair { .. }
    ));

    let request = TranslationRequest::new("hello", Language::Auto, Language::ChineseSimplified);
    let error = provider
        .translate(&request, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        vtrans_core::TranslationError::UnsupportedPair { .. }
    ));
}
