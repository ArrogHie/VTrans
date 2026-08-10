//! Azure Translator provider.

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

use super::language::{map_azure, provider_error};

/// Azure Translator provider.
///
/// Sends a JSON POST to Azure Translator's `/translate` endpoint with the
/// `Ocp-Apim-Subscription-Key` header. The stable runtime id is `"azure"`.
///
/// # Example
///
/// ```no_run
/// use std::time::Duration;
/// use vtrans_core::traits::TranslationProvider;
/// use vtrans_core::types::{Language, TranslationRequest};
/// use tokio_util::sync::CancellationToken;
/// use vtrans_translation::AzureTranslatorProvider;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let provider = AzureTranslatorProvider::new(
///     "https://api.cognitive.microsofttranslator.com/translate",
///     "eastasia",
///     "azure-key",
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
/// assert_eq!(result.provider_id, "azure");
/// # Ok(())
/// # }
/// ```
pub struct AzureTranslatorProvider {
    adapter: AzureAdapter,
    client: reqwest::Client,
    timeout: Duration,
    retry_policy: RetryPolicy,
    supported_pairs: Vec<(Language, Language)>,
}

impl fmt::Debug for AzureTranslatorProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AzureTranslatorProvider")
            .field("adapter", &self.adapter)
            .field("timeout", &self.timeout)
            .field("max_retries", &self.retry_policy.max_retries())
            .field("client", &self.client)
            .field("supported_pairs", &self.supported_pairs)
            .finish()
    }
}

/// Request/response/error logic for Azure Translator.
pub struct AzureAdapter {
    endpoint: String,
    region: String,
    api_key: String,
}

impl fmt::Debug for AzureAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AzureAdapter")
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("api_key", &"****")
            .finish()
    }
}

impl AzureTranslatorProvider {
    /// Create an Azure Translator provider.
    ///
    /// # Arguments
    /// * `endpoint` - Azure `/translate` endpoint URL.
    /// * `region` - Azure region (e.g. `eastasia`) sent as
    ///   `Ocp-Apim-Subscription-Region`.
    /// * `api_key` - Azure subscription key sent as
    ///   `Ocp-Apim-Subscription-Key`.
    /// * `timeout` - Per-attempt request timeout.
    /// * `max_retries` - Maximum number of retries after the first attempt.
    #[must_use]
    pub fn new(
        endpoint: &str,
        region: &str,
        api_key: &str,
        timeout: Duration,
        max_retries: u32,
    ) -> Self {
        Self {
            adapter: AzureAdapter {
                endpoint: endpoint.to_string(),
                region: region.to_string(),
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
impl TranslationProvider for AzureTranslatorProvider {
    /// Stable provider identifier reported in results and logs.
    fn id(&self) -> &'static str {
        "azure"
    }

    /// Pairs supported by the Azure provider.
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
impl TranslationProviderAdapter for AzureAdapter {
    fn id(&self) -> &'static str {
        "azure"
    }

    fn map_source_language(&self, language: Language) -> Option<String> {
        map_azure(language)
    }

    fn map_target_language(&self, language: Language) -> Option<String> {
        map_azure(language)
    }

    fn build_request(
        &self,
        request: &TranslationRequest,
    ) -> Result<OutgoingRequest, TranslationError> {
        let target = self
            .map_target_language(request.target)
            .ok_or_else(|| provider_error(request.source, request.target))?;
        let mut url = format!("{}?api-version=3.0&to={target}", self.endpoint);
        let body = json!([{"text": request.text}]);
        if let Some(source) = self.map_source_language(request.source) {
            url.push_str("&from=");
            url.push_str(&source);
        }
        let mut headers = BTreeMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        AuthStrategy::Header("Ocp-Apim-Subscription-Key").apply(&self.api_key, &mut headers);
        if !self.region.is_empty() {
            headers.insert(
                "Ocp-Apim-Subscription-Region".to_string(),
                self.region.clone(),
            );
        }
        Ok(OutgoingRequest::json("POST", url, headers, body))
    }

    fn parse_response(&self, body: &str) -> Result<ParsedTranslation, TranslationError> {
        parse_azure_response(body)
    }

    fn map_error(&self, status: StatusCode, body: &str) -> TranslationError {
        let _ = body;
        match status.as_u16() {
            401 | 403 => TranslationError::Unauthorized,
            429 => TranslationError::RateLimited,
            _ => TranslationError::ApiRequest(format!("HTTP status {}", status.as_u16())),
        }
    }

    fn retry_decision(
        &self,
        status: StatusCode,
        _body: &str,
        retry_after: Option<Duration>,
    ) -> RetryDecision {
        if matches!(status.as_u16(), 429 | 500 | 503) {
            RetryDecision::retry(retry_after)
        } else {
            RetryDecision::stop()
        }
    }
}

/// Parse an Azure Translator response into translated segments.
///
/// Azure returns an array of per-input objects, each with
/// `translations: [{ "text": "...", "to": "ja", "detectedSourceLanguage":
/// "en" }]`. Segments are flattened and joined with `\n`.
pub fn parse_azure_response(body: &str) -> Result<ParsedTranslation, TranslationError> {
    if body.trim().is_empty() {
        return Err(TranslationError::ParseResponse(
            "empty response body".to_string(),
        ));
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|error| TranslationError::ParseResponse(error.to_string()))?;
    let entries = value
        .as_array()
        .ok_or_else(|| TranslationError::ParseResponse("response is not an array".to_string()))?;
    let mut segments = Vec::new();
    let mut detected_source = None;
    for entry in entries {
        let translations = entry.get("translations").and_then(Value::as_array);
        let Some(translations) = translations else {
            continue;
        };
        for translation in translations {
            let Some(text) = translation.get("text").and_then(Value::as_str) else {
                continue;
            };
            segments.push(text.to_string());
            if detected_source.is_none() {
                if let Some(code) = translation
                    .get("detectedSourceLanguage")
                    .and_then(Value::as_str)
                {
                    detected_source = super::language::detected_from_code(code);
                }
            }
        }
    }
    if segments.is_empty() {
        return Err(TranslationError::ParseResponse(
            "no translation segments found".to_string(),
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

    fn adapter() -> AzureAdapter {
        AzureAdapter {
            endpoint: "https://api.cognitive.microsofttranslator.com/translate".to_string(),
            region: "eastasia".to_string(),
            api_key: "azure-key".to_string(),
        }
    }

    #[test]
    fn provider_id_is_azure() {
        let provider = AzureTranslatorProvider::new(
            "https://example.invalid",
            "eastasia",
            "key",
            Duration::from_secs(1),
            0,
        );
        assert_eq!(provider.id(), "azure");
    }

    #[test]
    fn build_request_uses_subscription_key_header() {
        let request = TranslationRequest::new("hello", Language::English, Language::Japanese);
        let outgoing = adapter().build_request(&request).unwrap();
        assert_eq!(outgoing.method, "POST");
        assert_eq!(
            outgoing.headers.get("Ocp-Apim-Subscription-Key").unwrap(),
            "azure-key"
        );
        assert_eq!(
            outgoing
                .headers
                .get("Ocp-Apim-Subscription-Region")
                .unwrap(),
            "eastasia"
        );
        assert!(outgoing.url.contains("to=ja"));
        assert!(outgoing.url.contains("from=en"));
        assert!(outgoing.url.contains("api-version=3.0"));
    }

    #[test]
    fn auto_source_omits_from() {
        let request = TranslationRequest::new("hello", Language::Auto, Language::Japanese);
        let outgoing = adapter().build_request(&request).unwrap();
        assert!(!outgoing.url.contains("from="));
        assert!(outgoing.url.contains("to=ja"));
    }

    #[test]
    fn parse_multi_segment_response() {
        let body = r#"[
            {"translations":[{"text":"こんにちは","to":"ja","detectedSourceLanguage":"en"}]},
            {"translations":[{"text":"世界","to":"ja","detectedSourceLanguage":"en"}]}
        ]"#;
        let parsed = parse_azure_response(body).unwrap();
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
            parse_azure_response(r"[]"),
            Err(TranslationError::ParseResponse(_))
        ));
    }

    #[test]
    fn azure_error_codes() {
        assert!(matches!(
            adapter().map_error(StatusCode::UNAUTHORIZED, ""),
            TranslationError::Unauthorized
        ));
        assert!(matches!(
            adapter().map_error(StatusCode::FORBIDDEN, ""),
            TranslationError::Unauthorized
        ));
        assert!(matches!(
            adapter().map_error(StatusCode::TOO_MANY_REQUESTS, ""),
            TranslationError::RateLimited
        ));
        assert!(
            adapter()
                .retry_decision(StatusCode::TOO_MANY_REQUESTS, "", None)
                .retry
        );
        assert!(
            !adapter()
                .retry_decision(StatusCode::UNAUTHORIZED, "", None)
                .retry
        );
    }
}
