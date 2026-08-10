//! `DeepL` translation provider.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use vtrans_core::error::TranslationError;
use vtrans_core::traits::TranslationProvider;
use vtrans_core::types::{Language, TranslationRequest, TranslationResult};

use crate::adapter::{
    send_with_adapter, OutgoingRequest, ParsedTranslation, RetryDecision,
    TranslationProviderAdapter,
};
use crate::auth::AuthStrategy;
use crate::retry::RetryPolicy;
use crate::validate::validate_language_pair;

use super::language::{map_deepl, provider_error};

/// `DeepL` translation provider.
///
/// Sends a form-encoded POST to `DeepL`'s v2 `/translate` endpoint with the
/// `DeepL-Auth-Key` header. The stable runtime id is `"deepl"`.
///
/// # Example
///
/// ```no_run
/// use std::time::Duration;
/// use vtrans_core::traits::TranslationProvider;
/// use vtrans_core::types::{Language, TranslationRequest};
/// use tokio_util::sync::CancellationToken;
/// use vtrans_translation::DeepLProvider;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let provider = DeepLProvider::new(
///     "https://api-free.deepl.com/v2/translate",
///     "deepl-secret",
///     Duration::from_secs(30),
///     2,
/// );
/// let request = TranslationRequest::new(
///     "hello",
///     Language::English,
///     Language::Japanese,
/// );
/// let result = provider
///     .translate(&request, CancellationToken::new())
///     .await?;
/// assert_eq!(result.provider_id, "deepl");
/// # Ok(())
/// # }
/// ```
pub struct DeepLProvider {
    adapter: DeepLAdapter,
    client: reqwest::Client,
    timeout: Duration,
    retry_policy: RetryPolicy,
    supported_pairs: Vec<(Language, Language)>,
}

impl fmt::Debug for DeepLProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeepLProvider")
            .field("adapter", &self.adapter)
            .field("timeout", &self.timeout)
            .field("max_retries", &self.retry_policy.max_retries())
            .field("client", &self.client)
            .field("supported_pairs", &self.supported_pairs)
            .finish()
    }
}

/// Request/response/error logic for `DeepL`.
pub struct DeepLAdapter {
    endpoint: String,
    api_key: String,
}

impl fmt::Debug for DeepLAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeepLAdapter")
            .field("endpoint", &self.endpoint)
            .field("api_key", &"****")
            .finish()
    }
}

impl DeepLProvider {
    /// Create a `DeepL` provider.
    ///
    /// # Arguments
    /// * `endpoint` - `DeepL` v2 `/translate` endpoint (Free or Pro).
    /// * `api_key` - `DeepL` API key sent as `DeepL-Auth-Key`.
    /// * `timeout` - Per-attempt request timeout.
    /// * `max_retries` - Maximum number of retries after the first attempt.
    #[must_use]
    pub fn new(endpoint: &str, api_key: &str, timeout: Duration, max_retries: u32) -> Self {
        Self {
            adapter: DeepLAdapter {
                endpoint: endpoint.to_string(),
                api_key: api_key.to_string(),
            },
            client: reqwest::Client::new(),
            timeout,
            retry_policy: RetryPolicy::new(max_retries),
            supported_pairs: crate::adapter::all_pairs(),
        }
    }

    /// Replace the retry policy used by this provider.
    ///
    /// # Arguments
    /// * `retry_policy` - New retry policy.
    #[must_use]
    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }
}

#[async_trait]
impl TranslationProvider for DeepLProvider {
    /// Stable provider identifier reported in results and logs.
    fn id(&self) -> &'static str {
        "deepl"
    }

    /// Pairs supported by the `DeepL` provider.
    fn supported_pairs(&self) -> &[(Language, Language)] {
        &self.supported_pairs
    }

    #[tracing::instrument(
        skip(self, request, cancel),
        fields(
            source = %request.source.code(),
            target = %request.target.code(),
            text_len = request.text.chars().count()
        )
    )]
    async fn translate(
        &self,
        request: &TranslationRequest,
        cancel: CancellationToken,
    ) -> Result<TranslationResult, TranslationError> {
        validate_language_pair(request.source, request.target, &self.supported_pairs)?;
        if cancel.is_cancelled() {
            return Err(TranslationError::Cancelled);
        }
        let started = Instant::now();
        let translated = send_with_adapter(
            &self.client,
            &self.adapter,
            request,
            self.timeout,
            self.retry_policy,
            cancel,
        )
        .await?;
        let elapsed_ms = crate::adapter::elapsed_millis(started);
        tracing::info!(
            provider_id = self.id(),
            source = %request.source.code(),
            target = %request.target.code(),
            elapsed_ms,
            text_len = translated.chars().count(),
            "translation completed"
        );
        Ok(TranslationResult::new(translated, self.id(), elapsed_ms))
    }
}

#[async_trait]
impl TranslationProviderAdapter for DeepLAdapter {
    fn id(&self) -> &'static str {
        "deepl"
    }

    fn map_source_language(&self, language: Language) -> Option<String> {
        map_deepl(language)
    }

    fn map_target_language(&self, language: Language) -> Option<String> {
        map_deepl(language)
    }

    fn build_request(
        &self,
        request: &TranslationRequest,
    ) -> Result<OutgoingRequest, TranslationError> {
        let target = self
            .map_target_language(request.target)
            .ok_or_else(|| provider_error(request.source, request.target))?;
        let mut form = format!(
            "target_lang={target}&text={}",
            crate::auth::urlencode(&request.text)
        );
        if let Some(source) = self.map_source_language(request.source) {
            form.push_str("&source_lang=");
            form.push_str(&source);
        }
        let mut headers = BTreeMap::new();
        headers.insert(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        AuthStrategy::AuthorizationScheme("DeepL-Auth-Key").apply(&self.api_key, &mut headers);
        Ok(OutgoingRequest::form("POST", &self.endpoint, headers, form))
    }

    fn parse_response(&self, body: &str) -> Result<ParsedTranslation, TranslationError> {
        parse_deepl_response(body)
    }

    fn map_error(&self, status: StatusCode, _body: &str) -> TranslationError {
        match status.as_u16() {
            403 => TranslationError::Unauthorized,
            _ => TranslationError::ApiRequest(format!("HTTP status {}", status.as_u16())),
        }
    }

    fn retry_decision(
        &self,
        status: StatusCode,
        _body: &str,
        retry_after: Option<Duration>,
    ) -> RetryDecision {
        if matches!(status.as_u16(), 429 | 500 | 529) {
            RetryDecision::retry(retry_after)
        } else {
            RetryDecision::stop()
        }
    }
}

/// Parse a `DeepL` v2 response into translated segments.
///
/// `DeepL` returns `{"translations":[{"detected_source_language":"EN",
/// "text":"...", ...}, ...]}`. Each `translations[]` entry is preserved as
/// its own segment so multi-segment responses never lose text.
pub fn parse_deepl_response(body: &str) -> Result<ParsedTranslation, TranslationError> {
    if body.trim().is_empty() {
        return Err(TranslationError::ParseResponse(
            "empty response body".to_string(),
        ));
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|error| TranslationError::ParseResponse(error.to_string()))?;
    let translations = value
        .get("translations")
        .and_then(Value::as_array)
        .ok_or_else(|| TranslationError::ParseResponse("missing translations array".to_string()))?;
    let mut segments = Vec::with_capacity(translations.len());
    let mut detected_source = None;
    for entry in translations {
        let Some(text) = entry.get("text").and_then(Value::as_str) else {
            return Err(TranslationError::ParseResponse(
                "translation entry missing text".to_string(),
            ));
        };
        segments.push(text.to_string());
        if detected_source.is_none() {
            if let Some(code) = entry
                .get("detected_source_language")
                .and_then(Value::as_str)
            {
                detected_source = super::language::detected_from_code(code);
            }
        }
    }
    if segments.is_empty() {
        return Err(TranslationError::ParseResponse(
            "empty translations array".to_string(),
        ));
    }
    Ok(ParsedTranslation {
        segments,
        detected_source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter() -> DeepLAdapter {
        DeepLAdapter {
            endpoint: "https://api-free.deepl.com/v2/translate".to_string(),
            api_key: "deepl-secret".to_string(),
        }
    }

    #[test]
    fn provider_id_is_deepl() {
        let provider = DeepLProvider::new(
            "https://api-free.deepl.com/v2/translate",
            "key",
            Duration::from_secs(1),
            0,
        );
        assert_eq!(provider.id(), "deepl");
    }

    #[test]
    fn build_request_uses_scheme_header_and_form() {
        let request = TranslationRequest::new("hello world", Language::English, Language::Japanese);
        let outgoing = adapter().build_request(&request).unwrap();
        assert_eq!(outgoing.method, "POST");
        assert_eq!(
            outgoing.headers.get("DeepL-Auth-Key").unwrap(),
            "deepl-secret"
        );
        assert!(!outgoing.headers.contains_key("Authorization"));
        let form = outgoing.form.unwrap();
        assert!(form.contains("target_lang=JA"));
        assert!(form.contains("source_lang=EN-US"));
        assert!(form.contains("text=hello+world"));
    }

    #[test]
    fn auto_source_is_omitted() {
        let request = TranslationRequest::new("hello", Language::Auto, Language::Japanese);
        let outgoing = adapter().build_request(&request).unwrap();
        let form = outgoing.form.unwrap();
        assert!(form.contains("target_lang=JA"));
        assert!(!form.contains("source_lang"));
    }

    #[test]
    fn parse_multi_segment_response() {
        let body = r#"{
            "translations": [
                {"detected_source_language":"EN","text":"こんにちは"},
                {"detected_source_language":"EN","text":"世界"}
            ]
        }"#;
        let parsed = parse_deepl_response(body).unwrap();
        assert_eq!(
            parsed.segments,
            vec!["こんにちは".to_string(), "世界".to_string()]
        );
        assert_eq!(parsed.detected_source, Some(Language::English));
        assert_eq!(parsed.into_text(), "こんにちは\n世界");
    }

    #[test]
    fn missing_translations_is_parse_error() {
        assert!(matches!(
            parse_deepl_response(r#"{"ok":true}"#),
            Err(TranslationError::ParseResponse(_))
        ));
    }

    #[test]
    fn deepl_error_codes() {
        assert!(matches!(
            adapter().map_error(StatusCode::FORBIDDEN, ""),
            TranslationError::Unauthorized
        ));
        assert!(
            adapter()
                .retry_decision(StatusCode::TOO_MANY_REQUESTS, "", None)
                .retry
        );
        assert!(
            adapter()
                .retry_decision(StatusCode::INTERNAL_SERVER_ERROR, "", None)
                .retry
        );
        assert!(
            adapter()
                .retry_decision(StatusCode::from_u16(529).unwrap(), "", None)
                .retry
        );
        assert!(
            !adapter()
                .retry_decision(StatusCode::FORBIDDEN, "", None)
                .retry
        );
        assert!(
            !adapter()
                .retry_decision(StatusCode::from_u16(456).unwrap(), "", None)
                .retry
        );
    }
}
