//! Language code mapping between `VTrans` and cloud translation providers.

use vtrans_core::types::Language;
use vtrans_core::TranslationError;

/// Map a `VTrans` source language to a provider source code.
///
/// Returns `None` for [`Language::Auto`] so providers that support
/// auto-detection can omit the source entirely.
#[must_use]
pub fn map_source_auto(language: Language) -> Option<String> {
    match language {
        Language::Auto => None,
        Language::ChineseSimplified => Some("zh-CN".to_string()),
        Language::Japanese => Some("ja".to_string()),
        Language::English => Some("en".to_string()),
    }
}

/// Map a `VTrans` target language to a generic provider target code.
///
/// This is the default mapping used by `OpenAI` prompt-based providers where
/// concrete codes are not required.
#[must_use]
pub fn map_target_generic(language: Language) -> Option<String> {
    if language.is_auto() {
        None
    } else {
        Some(language.code().to_string())
    }
}

/// Map a `VTrans` language to a `DeepL` language code.
///
/// `DeepL` uses uppercase codes: `ZH` (Chinese), `JA`, `EN-US`. It does not
/// support auto-detection of the source in the target position.
#[must_use]
pub fn map_deepl(language: Language) -> Option<String> {
    match language {
        Language::Auto => None,
        Language::ChineseSimplified => Some("ZH".to_string()),
        Language::Japanese => Some("JA".to_string()),
        Language::English => Some("EN-US".to_string()),
    }
}

/// Map a `VTrans` language to a Google Translate v2 language code.
///
/// Google uses `zh-CN`, `ja`, `en`; `auto` is used for source detection.
#[must_use]
pub fn map_google(language: Language) -> Option<String> {
    match language {
        Language::Auto => None,
        Language::ChineseSimplified => Some("zh-CN".to_string()),
        Language::Japanese => Some("ja".to_string()),
        Language::English => Some("en".to_string()),
    }
}

/// Map a `VTrans` language to an Azure Translator language code.
///
/// Azure uses `zh-Hans`, `ja`, `en`; `auto` is not an accepted source code.
#[must_use]
pub fn map_azure(language: Language) -> Option<String> {
    match language {
        Language::Auto => None,
        Language::ChineseSimplified => Some("zh-Hans".to_string()),
        Language::Japanese => Some("ja".to_string()),
        Language::English => Some("en".to_string()),
    }
}

/// Map a `VTrans` language to a Baidu Translate language code.
///
/// Baidu uses `auto` (source detection), `zh`, `jp` (Japanese), and `en`.
/// Every `VTrans` language maps to a concrete Baidu code, so the result is
/// never `None`.
#[must_use]
pub fn map_baidu(language: Language) -> String {
    match language {
        Language::Auto => "auto".to_string(),
        Language::ChineseSimplified => "zh".to_string(),
        Language::Japanese => "jp".to_string(),
        Language::English => "en".to_string(),
    }
}

/// Parse a provider language code back into a `VTrans` [`Language`].
///
/// Used to fill `detected_source` when an API reports the detected language.
/// Returns `None` when the code is not recognized.
#[must_use]
pub fn detected_from_code(code: &str) -> Option<Language> {
    match code {
        "zh" | "zh-CN" | "zh-Hans" | "ZH" => Some(Language::ChineseSimplified),
        "ja" | "jp" | "JA" => Some(Language::Japanese),
        "en" | "EN-US" | "EN" => Some(Language::English),
        _ => None,
    }
}

/// Build a provider `TranslationError` for an unmappable language.
#[must_use]
pub fn provider_error(source: Language, target: Language) -> TranslationError {
    TranslationError::UnsupportedPair {
        src: source,
        target,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_auto_maps_to_none() {
        assert_eq!(map_source_auto(Language::Auto), None);
        assert_eq!(map_source_auto(Language::English), Some("en".to_string()));
    }

    #[test]
    fn deepl_codes() {
        assert_eq!(
            map_deepl(Language::ChineseSimplified),
            Some("ZH".to_string())
        );
        assert_eq!(map_deepl(Language::Japanese), Some("JA".to_string()));
        assert_eq!(map_deepl(Language::English), Some("EN-US".to_string()));
        assert_eq!(map_deepl(Language::Auto), None);
    }

    #[test]
    fn google_codes() {
        assert_eq!(
            map_google(Language::ChineseSimplified),
            Some("zh-CN".to_string())
        );
        assert_eq!(map_google(Language::Japanese), Some("ja".to_string()));
        assert_eq!(map_google(Language::English), Some("en".to_string()));
        assert_eq!(map_google(Language::Auto), None);
    }

    #[test]
    fn azure_codes() {
        assert_eq!(
            map_azure(Language::ChineseSimplified),
            Some("zh-Hans".to_string())
        );
        assert_eq!(map_azure(Language::Japanese), Some("ja".to_string()));
        assert_eq!(map_azure(Language::English), Some("en".to_string()));
        assert_eq!(map_azure(Language::Auto), None);
    }

    #[test]
    fn baidu_codes() {
        assert_eq!(map_baidu(Language::Auto), "auto".to_string());
        assert_eq!(map_baidu(Language::ChineseSimplified), "zh".to_string());
        assert_eq!(map_baidu(Language::Japanese), "jp".to_string());
        assert_eq!(map_baidu(Language::English), "en".to_string());
    }

    #[test]
    fn detected_from_provider_codes() {
        assert_eq!(
            detected_from_code("zh-Hans"),
            Some(Language::ChineseSimplified)
        );
        assert_eq!(detected_from_code("jp"), Some(Language::Japanese));
        assert_eq!(detected_from_code("EN-US"), Some(Language::English));
        assert_eq!(detected_from_code("xx"), None);
    }

    #[test]
    fn provider_error_uses_unsupported_pair() {
        let err = provider_error(Language::English, Language::Japanese);
        assert!(matches!(
            err,
            TranslationError::UnsupportedPair {
                src: Language::English,
                target: Language::Japanese,
            }
        ));
    }
}
