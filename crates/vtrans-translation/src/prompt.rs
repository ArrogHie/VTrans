//! Prompt templates for API translation providers.

use vtrans_core::types::Language;

/// Build the system instruction for a translation request.
///
/// The prompt asks for translated text only and explicitly forbids
/// explanations, commentary, notes, and quotation marks.
///
/// # Arguments
///
/// * `source` - Source language of the input text.
/// * `target` - Target language of the translation.
///
/// # Example
///
/// ```
/// use vtrans_core::types::Language;
/// use vtrans_translation::build_system_prompt;
///
/// let prompt = build_system_prompt(Language::English, Language::Japanese);
/// assert!(prompt.contains("English"));
/// assert!(prompt.contains("Japanese"));
/// assert!(prompt.contains("only the translated text"));
/// ```
#[must_use]
pub fn build_system_prompt(source: Language, target: Language) -> String {
    let source_name = if source.is_auto() {
        "the detected source language"
    } else {
        source.display_name()
    };
    let target_name = target.display_name();
    format!(
        "You are a professional translator. Translate the user-provided text \
         from {source_name} to {target_name}. Output only the translated text. \
         Do not add notes, greetings, or quotation marks."
    )
}

/// Build a full prompt containing the system instruction and original text.
///
/// This is useful for text-completion style endpoints. Chat-completion
/// endpoints should send [`build_system_prompt`] as the system message and
/// the original text as the user message.
///
/// # Arguments
///
/// * `text` - Original text to translate.
/// * `source` - Source language of `text`.
/// * `target` - Target language of the translation.
///
/// # Example
///
/// ```
/// use vtrans_core::types::Language;
/// use vtrans_translation::build_translation_prompt;
///
/// let prompt = build_translation_prompt("hello", Language::English, Language::Japanese);
/// assert!(prompt.ends_with("hello"));
/// ```
#[must_use]
pub fn build_translation_prompt(text: &str, source: Language, target: Language) -> String {
    format!("{}\n\n{text}", build_system_prompt(source, target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_contains_language_names() {
        let prompt = build_system_prompt(Language::English, Language::Japanese);
        assert!(prompt.contains("English"));
        assert!(prompt.contains("Japanese"));
    }

    #[test]
    fn system_prompt_demands_translation_only() {
        let prompt = build_system_prompt(Language::Auto, Language::ChineseSimplified);
        assert!(prompt.contains("only the translated text"));
        assert!(!prompt.contains("explain"));
        assert!(!prompt.contains("commentary"));
    }

    #[test]
    fn auto_source_is_phrased_as_detected_language() {
        let prompt = build_system_prompt(Language::Auto, Language::English);
        assert!(prompt.contains("detected source language"));
    }

    #[test]
    fn full_prompt_includes_original_text() {
        let prompt = build_translation_prompt("hello", Language::English, Language::Japanese);
        assert!(prompt.ends_with("hello"));
        assert!(prompt.contains("professional translator"));
    }
}
