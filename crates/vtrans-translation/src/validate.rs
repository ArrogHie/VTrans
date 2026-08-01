//! Language pair validation shared by translation providers.

use vtrans_core::types::Language;
use vtrans_core::TranslationError;

/// Check that a requested `(source, target)` pair is present in `supported`.
///
/// A target of [`Language::Auto`] is always rejected because translation
/// must produce text in a concrete target language. A source of
/// [`Language::Auto`] is accepted when the provider supports that target
/// with an explicit auto source or with any concrete source.
///
/// # Arguments
///
/// * `source` - Requested source language.
/// * `target` - Requested target language.
/// * `supported` - Pairs advertised by a provider.
///
/// # Errors
///
/// Returns [`TranslationError::UnsupportedPair`] when the pair is not
/// supported or the target is [`Language::Auto`].
///
/// # Example
///
/// ```
/// use vtrans_core::types::Language;
/// use vtrans_translation::validate_language_pair;
///
/// let supported = [(Language::English, Language::Japanese)];
/// assert!(validate_language_pair(
///     Language::English,
///     Language::Japanese,
///     &supported,
/// )
/// .is_ok());
/// assert!(validate_language_pair(
///     Language::Japanese,
///     Language::English,
///     &supported,
/// )
/// .is_err());
/// ```
pub fn validate_language_pair(
    source: Language,
    target: Language,
    supported: &[(Language, Language)],
) -> Result<(), TranslationError> {
    if target.is_auto() {
        return Err(TranslationError::UnsupportedPair {
            src: source,
            target,
        });
    }

    let is_supported = if source.is_auto() {
        supported
            .iter()
            .any(|&(_, candidate_target)| candidate_target == target)
    } else {
        supported.contains(&(source, target))
    };

    if is_supported {
        Ok(())
    } else {
        Err(TranslationError::UnsupportedPair {
            src: source,
            target,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_pair_is_supported() {
        let supported = [(Language::English, Language::Japanese)];
        assert!(validate_language_pair(Language::English, Language::Japanese, &supported,).is_ok());
    }

    #[test]
    fn auto_source_is_supported_when_target_is_available() {
        let supported = [(Language::English, Language::Japanese)];
        assert!(validate_language_pair(Language::Auto, Language::Japanese, &supported,).is_ok());
    }

    #[test]
    fn unsupported_pair_returns_error() {
        let supported = [(Language::English, Language::Japanese)];
        let err =
            validate_language_pair(Language::Japanese, Language::English, &supported).unwrap_err();
        assert!(matches!(
            err,
            TranslationError::UnsupportedPair {
                src: Language::Japanese,
                target: Language::English,
            }
        ));
    }

    #[test]
    fn auto_target_is_rejected() {
        let supported = [(Language::English, Language::Auto)];
        assert!(validate_language_pair(Language::English, Language::Auto, &supported,).is_err());
    }
}
