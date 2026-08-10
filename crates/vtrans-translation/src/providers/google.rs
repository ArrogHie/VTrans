//! Google Cloud Translation v2 provider.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::{json, Value};
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

use super::language::{map_google, provider_error};

/// Google Cloud Translation v2 provider.
///
/// Sends a JSON POST to the v2 `translate` method with the API key as a
/// `key` query parameter. The stable runtime id is `"google"`.
///
/// # Example
///
/// ```no_run
/// use std::time::Duration;
/// use vtrans_core::traits::TranslationProvider;
/// use vtrans_core::types::{Language, TranslationRequest};
/// use tokio_util::sync::CancellationToken;
/// use vtrans_translation::GoogleV2Provider;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let provider = GoogleV2Provider::new(
///     "https://translation.googleapis.com/language/translate/v2",
///     "google-api-key",
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
/// assert_eq!(result.provider_id, "google");
/// # Ok(())
/// # }
/// ```
pub struct GoogleV2Provider {
    adapter: GoogleV2Adapter,
    client: reqwest::Client,
    timeout: Duration,
    retry_policy: RetryPolicy,
    supported_pairs: Vec<(Language, Language)>,
}

impl fmt::Debug for GoogleV2Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleV2Provider")
            .field("adapter", &self.adapter)
            .field("timeout", &self.timeout)
            .field("max_retries", &self.retry_policy.max_retries())
            .field("client", &self.client)
            .field("supported_pairs", &self.supported_pairs)
            .finish()
    }
}

/// Request/response/error logic for Google v2.
pub struct GoogleV2Adapter {
    endpoint: String,
    api_key: String,
}

impl fmt::Debug for GoogleV2Adapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleV2Adapter")
            .field("endpoint", &self.endpoint)
            .field("api_key", &"****")
            .finish()
    }
}

impl GoogleV2Provider {
    /// Create a Google v2 provider.
    ///
    /// # Arguments
    /// * `endpoint` - Google v2 `translate` URL.
    /// * `api_key` - Google API key sent as a `key` query parameter.
    /// * `timeout` - Per-attempt request timeout.
    /// * `max_retries` - Maximum number of retries after the first attempt.
    #[must_use]
    pub fn new(endpoint: &str, api_key: &str, timeout: Duration, max_retries: u32) -> Self {
        Self {
            adapter: GoogleV2Adapter {
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
impl TranslationProvider for GoogleV2Provider {
    /// Stable provider identifier reported in results and logs.
    fn id(&self) -> &'static str {
        "google"
    }

    /// Pairs supported by the Google provider.
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
impl TranslationProviderAdapter for GoogleV2Adapter {
    fn id(&self) -> &'static str {
        "google"
    }

    fn map_source_language(&self, language: Language) -> Option<String> {
        map_google(language)
    }

    fn map_target_language(&self, language: Language) -> Option<String> {
        map_google(language)
    }

    fn build_request(
        &self,
        request: &TranslationRequest,
    ) -> Result<OutgoingRequest, TranslationError> {
        let target = self
            .map_target_language(request.target)
            .ok_or_else(|| provider_error(request.source, request.target))?;
        let mut body = json!({
            "q": [request.text],
            "target": target,
        });
        let mut query = format!("target={target}");
        if let Some(source) = self.map_source_language(request.source) {
            body["source"] = Value::String(source.clone());
            query.push_str("&source=");
            query.push_str(&source);
        } else {
            query.push_str("&format=text");
        }
        // The API key travels as a query parameter via the Query strategy.
        let mut strategy_query = String::new();
        AuthStrategy::Query("key").apply_query(&self.api_key, &mut strategy_query);
        query.push_str(&strategy_query);
        let url = format!("{}?{query}", self.endpoint);
        let mut headers = BTreeMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        Ok(OutgoingRequest::json("POST", url, headers, body))
    }

    fn parse_response(&self, body: &str) -> Result<ParsedTranslation, TranslationError> {
        parse_google_response(body)
    }

    fn map_error(&self, status: StatusCode, body: &str) -> TranslationError {
        match status.as_u16() {
            401 | 403 => {
                // 403 may be a rate-limit (quota exceeded) reported in the
                // body, or an auth failure. Prefer the body's error code.
                if body.contains("RATE_LIMIT") || body.contains("dailyLimitExceeded") {
                    TranslationError::RateLimited
                } else {
                    TranslationError::Unauthorized
                }
            }
            429 => TranslationError::RateLimited,
            _ => TranslationError::ApiRequest(format!("HTTP status {}", status.as_u16())),
        }
    }

    fn retry_decision(
        &self,
        status: StatusCode,
        body: &str,
        retry_after: Option<Duration>,
    ) -> RetryDecision {
        if (status.as_u16() == 403
            && (body.contains("RATE_LIMIT") || body.contains("dailyLimitExceeded")))
            || matches!(status.as_u16(), 429 | 500 | 503)
        {
            RetryDecision::retry(retry_after)
        } else {
            RetryDecision::stop()
        }
    }
}

/// Parse a Google v2 response into translated segments.
///
/// Google returns `{"data":{"translations":[{"translatedText":"...",
/// "detectedSourceLanguage":"en"}, ...]}}`. Each `translations[]` entry is
/// preserved as its own segment.
pub fn parse_google_response(body: &str) -> Result<ParsedTranslation, TranslationError> {
    if body.trim().is_empty() {
        return Err(TranslationError::ParseResponse(
            "empty response body".to_string(),
        ));
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|error| TranslationError::ParseResponse(error.to_string()))?;
    let translations = value
        .get("data")
        .and_then(|data| data.get("translations"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            TranslationError::ParseResponse("missing data.translations array".to_string())
        })?;
    let mut segments = Vec::with_capacity(translations.len());
    let mut detected_source = None;
    for entry in translations {
        let Some(text) = entry.get("translatedText").and_then(Value::as_str) else {
            return Err(TranslationError::ParseResponse(
                "translation entry missing translatedText".to_string(),
            ));
        };
        segments.push(text.to_string());
        if detected_source.is_none() {
            if let Some(code) = entry.get("detectedSourceLanguage").and_then(Value::as_str) {
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

    fn adapter() -> GoogleV2Adapter {
        GoogleV2Adapter {
            endpoint: "https://translation.googleapis.com/language/translate/v2".to_string(),
            api_key: "google-key".to_string(),
        }
    }

    #[test]
    fn provider_id_is_google() {
        let provider =
            GoogleV2Provider::new("https://example.invalid", "key", Duration::from_secs(1), 0);
        assert_eq!(provider.id(), "google");
    }

    #[test]
    fn build_request_adds_key_query_and_target() {
        let request = TranslationRequest::new("hello", Language::English, Language::Japanese);
        let outgoing = adapter().build_request(&request).unwrap();
        assert_eq!(outgoing.method, "POST");
        assert!(outgoing.url.contains("target=ja"));
        assert!(outgoing.url.contains("source=en"));
        assert!(outgoing.url.contains("key=google-key"));
        let body = outgoing.body.unwrap();
        assert_eq!(body["source"], "en");
        assert_eq!(body["target"], "ja");
    }

    #[test]
    fn auto_source_omits_source_field() {
        let request = TranslationRequest::new("hello", Language::Auto, Language::English);
        let outgoing = adapter().build_request(&request).unwrap();
        let body = outgoing.body.unwrap();
        assert!(body.get("source").is_none());
        assert!(outgoing.url.contains("format=text"));
    }

    #[test]
    fn parse_multi_segment_response() {
        let body = r#"{
            "data": {
                "translations": [
                    {"translatedText":"こんにちは","detectedSourceLanguage":"en"},
                    {"translatedText":"世界","detectedSourceLanguage":"en"}
                ]
            }
        }"#;
        let parsed = parse_google_response(body).unwrap();
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
            parse_google_response(r#"{"data":{}}"#),
            Err(TranslationError::ParseResponse(_))
        ));
    }

    #[test]
    fn google_error_codes() {
        assert!(matches!(
            adapter().map_error(StatusCode::UNAUTHORIZED, ""),
            TranslationError::Unauthorized
        ));
        assert!(matches!(
            adapter().map_error(
                StatusCode::FORBIDDEN,
                r#"{"error":{"message":"RATE_LIMIT"}}"#
            ),
            TranslationError::RateLimited
        ));
        assert!(
            adapter()
                .retry_decision(
                    StatusCode::FORBIDDEN,
                    r#"{"error":{"message":"dailyLimitExceeded"}}"#,
                    None
                )
                .retry
        );
        assert!(
            !adapter()
                .retry_decision(StatusCode::FORBIDDEN, "", None)
                .retry
        );
    }
}
