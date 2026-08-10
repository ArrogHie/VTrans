//! Backward-compatible API provider re-export.
//!
//! The single cloud provider was renamed to [`OpenAiProvider`] with a stable
//! runtime id of `"openai"`. This module keeps the old `ApiTranslationProvider`
//! and `parse_response` names available for existing callers without
//! requiring changes outside this crate.

use vtrans_core::TranslationError;

pub use crate::providers::openai::OpenAiProvider as ApiTranslationProvider;

/// Extract translated text from an OpenAI-compatible JSON response.
///
/// This is kept as a compatibility entry point. New code should prefer the
/// per-provider parsers behind [`crate::TranslationProviderAdapter`].
///
/// # Arguments
/// * `body` - Response body returned by the API.
///
/// # Errors
/// Returns [`TranslationError::ParseResponse`] when the body is empty or no
/// translation field can be found.
///
/// # Example
///
/// ```
/// use vtrans_translation::parse_response;
///
/// let body = r#"{"choices":[{"message":{"content":"こんにちは"}}]}"#;
/// assert_eq!(parse_response(body).unwrap(), "こんにちは");
/// ```
pub fn parse_response(body: &str) -> Result<String, TranslationError> {
    crate::providers::openai::parse_openai_response(body)
        .map(super::adapter::ParsedTranslation::into_text)
}
