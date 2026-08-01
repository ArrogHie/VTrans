//! HTTP/JSON translation provider.

use std::fmt;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use vtrans_core::error::TranslationError;
use vtrans_core::traits::TranslationProvider;
use vtrans_core::types::{Language, TranslationRequest, TranslationResult};

use crate::prompt::build_system_prompt;
use crate::retry::RetryPolicy;
use crate::validate::validate_language_pair;

/// A generic HTTP/JSON translation provider.
///
/// The provider sends an OpenAI-compatible chat-completion request to
/// `endpoint`, using `model` as the model name and `api_key` as a bearer
/// token. The API key is never written to logs or serialized by this crate;
/// application code should load it from
/// `vtrans_security::CredentialManager` before constructing the provider.
///
/// Retryable failures are retried with the configured [`RetryPolicy`].
/// Timeouts and cancellation are applied to every request attempt.
///
/// # Example
///
/// ```no_run
/// use std::time::Duration;
/// use vtrans_core::traits::TranslationProvider;
/// use vtrans_core::types::{Language, TranslationRequest};
/// use tokio_util::sync::CancellationToken;
/// use vtrans_translation::ApiTranslationProvider;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let provider = ApiTranslationProvider::new(
///     "https://api.example.com/v1/chat/completions",
///     "translator-model",
///     "sk-example",
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
/// assert_eq!(result.provider_id, "api");
/// # Ok(())
/// # }
/// ```
pub struct ApiTranslationProvider {
    endpoint: String,
    model: String,
    api_key: String,
    client: Client,
    timeout: Duration,
    retry_policy: RetryPolicy,
    supported_pairs: Vec<(Language, Language)>,
}

impl ApiTranslationProvider {
    /// Create an API provider.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - Full HTTP(S) URL for the translation API.
    /// * `model` - Model identifier sent in the JSON request body.
    /// * `api_key` - API key sent as a `Bearer` token. It is stored in
    ///   process memory only and never logged.
    /// * `timeout` - Per-attempt request timeout.
    /// * `max_retries` - Maximum number of retries after the first attempt.
    #[must_use]
    pub fn new(
        endpoint: &str,
        model: &str,
        api_key: &str,
        timeout: Duration,
        max_retries: u32,
    ) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            model: model.to_string(),
            api_key: api_key.to_string(),
            client: Client::new(),
            timeout,
            retry_policy: RetryPolicy::new(max_retries),
            supported_pairs: api_supported_pairs(),
        }
    }

    /// Replace the retry policy used by this provider.
    ///
    /// This is primarily useful for tests that need a zero-delay policy.
    ///
    /// # Arguments
    ///
    /// * `retry_policy` - New retry policy.
    #[must_use]
    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// Execute one request attempt with timeout and cancellation.
    async fn execute_once(
        &self,
        request: &TranslationRequest,
        cancel: CancellationToken,
    ) -> Result<String, TranslationError> {
        let system_prompt = build_system_prompt(request.source, request.target);
        let body = build_request_body(&self.model, &system_prompt, &request.text);
        let response = tokio::select! {
            () = cancel.cancelled() => return Err(TranslationError::Cancelled),
            outcome = tokio::time::timeout(
                self.timeout,
                self.client
                    .post(&self.endpoint)
                    .bearer_auth(&self.api_key)
                    .json(&body)
                    .send(),
            ) => match outcome {
                Err(_) => Err(TranslationError::Timeout(self.timeout)),
                Ok(Err(error)) if error.is_timeout() => {
                    Err(TranslationError::Timeout(self.timeout))
                }
                Ok(Err(error)) => Err(TranslationError::ApiRequest(error.to_string())),
                Ok(Ok(response)) => Ok(response),
            },
        };
        let response = response?;
        let status = response.status();

        if let Some(error) = status_error(status) {
            return Err(error);
        }
        if !status.is_success() {
            warn!(
                status = status.as_u16(),
                "translation API returned non-success status"
            );
            return Err(TranslationError::ApiRequest(format!(
                "HTTP status {}",
                status.as_u16()
            )));
        }

        let body = response.text().await.map_err(|error| {
            TranslationError::ApiRequest(format!("read response body: {error}"))
        })?;
        parse_response(&body)
    }

    /// Run attempts with the configured retry policy.
    async fn translate_with_retry(
        &self,
        request: &TranslationRequest,
        cancel: CancellationToken,
    ) -> Result<String, TranslationError> {
        for attempt in 0..=self.retry_policy.max_retries() {
            if cancel.is_cancelled() {
                return Err(TranslationError::Cancelled);
            }
            match self.execute_once(request, cancel.clone()).await {
                Ok(text) => return Ok(text),
                Err(error) => {
                    warn!(attempt, error = %error, "translation request attempt failed");
                    if !self.retry_policy.should_retry(&error, attempt) {
                        return Err(error);
                    }
                    let delay = self.retry_policy.backoff_duration(attempt);
                    debug!(
                        attempt,
                        delay_ms = delay.as_millis(),
                        "retrying translation request"
                    );
                    tokio::select! {
                        () = cancel.cancelled() => return Err(TranslationError::Cancelled),
                        () = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }
        Err(TranslationError::ApiRequest(
            "translation request made no attempts".to_string(),
        ))
    }
}

impl fmt::Debug for ApiTranslationProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiTranslationProvider")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("api_key", &"****")
            .field("timeout", &self.timeout)
            .field("max_retries", &self.retry_policy.max_retries())
            .field("client", &self.client)
            .field("supported_pairs", &self.supported_pairs)
            .finish()
    }
}

#[async_trait]
impl TranslationProvider for ApiTranslationProvider {
    /// Stable provider identifier used in logs and results.
    fn id(&self) -> &'static str {
        "api"
    }

    /// Pairs supported by the generic API provider.
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
        let started = Instant::now();
        validate_language_pair(request.source, request.target, &self.supported_pairs)?;
        if cancel.is_cancelled() {
            return Err(TranslationError::Cancelled);
        }

        let translated = self.translate_with_retry(request, cancel).await?;
        let elapsed_ms = elapsed_millis(started);
        info!(
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

/// Build the list of pairs advertised by the generic API provider.
fn api_supported_pairs() -> Vec<(Language, Language)> {
    let mut pairs = Vec::with_capacity(12);
    for &target in Language::all_concrete() {
        pairs.push((Language::Auto, target));
        for &source in Language::all_concrete() {
            pairs.push((source, target));
        }
    }
    pairs
}

/// Convert an HTTP status to a typed translation error, if known.
fn status_error(status: StatusCode) -> Option<TranslationError> {
    match status.as_u16() {
        401 => Some(TranslationError::Unauthorized),
        429 => Some(TranslationError::RateLimited),
        _ => None,
    }
}

/// OpenAI-compatible chat completion request body.
#[derive(Debug, Serialize)]
struct ApiRequestBody<'a> {
    model: &'a str,
    messages: Vec<ApiMessage<'a>>,
    temperature: f32,
}

/// One chat message in an API request body.
#[derive(Debug, Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// Build the JSON body sent to an OpenAI-compatible endpoint.
fn build_request_body<'a>(
    model: &'a str,
    system_prompt: &'a str,
    text: &'a str,
) -> ApiRequestBody<'a> {
    ApiRequestBody {
        model,
        messages: vec![
            ApiMessage {
                role: "system",
                content: system_prompt,
            },
            ApiMessage {
                role: "user",
                content: text,
            },
        ],
        temperature: 0.0,
    }
}

/// Extract translated text from a JSON API response.
///
/// Supported shapes include `OpenAI` chat completions (`choices[0].message.
/// content`), completion-style responses (`choices[0].text`), common
/// translation fields (`translated_text`, `translation`, `result`, `output`,
/// `text`), and a bare JSON string.
///
/// # Arguments
///
/// * `body` - Response body returned by the API.
///
/// # Errors
///
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
    if body.trim().is_empty() {
        return Err(TranslationError::ParseResponse(
            "empty response body".to_string(),
        ));
    }
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| TranslationError::ParseResponse(error.to_string()))?;
    extract_translation(&value)
        .ok_or_else(|| TranslationError::ParseResponse("no translation field found".to_string()))
}

/// Recursively look for a translation string in a parsed JSON value.
fn extract_translation(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(normalize_translation(text));
    }

    if let Some(choices) = value.get("choices").and_then(serde_json::Value::as_array) {
        for choice in choices {
            if let Some(text) = choice
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(serde_json::Value::as_str)
            {
                return Some(normalize_translation(text));
            }
            if let Some(text) = choice.get("text").and_then(serde_json::Value::as_str) {
                return Some(normalize_translation(text));
            }
        }
    }

    for key in ["translated_text", "translation", "result", "output", "text"] {
        if let Some(text) = value.get(key).and_then(serde_json::Value::as_str) {
            return Some(normalize_translation(text));
        }
    }

    value.get("data").and_then(extract_translation)
}

/// Strip common wrappers that language models add around the translation.
fn normalize_translation(text: &str) -> String {
    let trimmed = text.trim();
    let stripped = trimmed
        .strip_prefix("```")
        .and_then(|rest| rest.strip_suffix("```"))
        .unwrap_or(trimmed);
    stripped.trim().trim_matches('"').trim().to_string()
}

/// Convert an `Instant` delta to milliseconds, saturating at `u64::MAX`.
fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_pairs_cover_every_concrete_target() {
        let provider = ApiTranslationProvider::new(
            "https://example.invalid",
            "model",
            "secret",
            Duration::from_secs(1),
            0,
        );
        for &target in Language::all_concrete() {
            assert!(provider.supported_pairs.contains(&(Language::Auto, target)));
            for &source in Language::all_concrete() {
                assert!(provider.supported_pairs.contains(&(source, target)));
            }
        }
    }

    #[test]
    fn debug_output_never_contains_api_key() {
        let provider = ApiTranslationProvider::new(
            "https://example.invalid",
            "model",
            "sk-very-secret-key",
            Duration::from_secs(1),
            0,
        );
        let debug = format!("{provider:?}");
        assert!(!debug.contains("sk-very-secret-key"));
        assert!(debug.contains("****"));
    }

    #[test]
    fn parse_openai_chat_response() {
        let body = r#"{"choices":[{"message":{"content":"こんにちは"}}]}"#;
        assert_eq!(parse_response(body).unwrap(), "こんにちは");
    }

    #[test]
    fn parse_completion_style_response() {
        let body = r#"{"choices":[{"text":"Bonjour"}]}"#;
        assert_eq!(parse_response(body).unwrap(), "Bonjour");
    }

    #[test]
    fn parse_translation_field_response() {
        let body = r#"{"translated_text":"你好"}"#;
        assert_eq!(parse_response(body).unwrap(), "你好");
    }

    #[test]
    fn parse_bare_json_string() {
        assert_eq!(parse_response(r#""hola""#).unwrap(), "hola");
    }

    #[test]
    fn parse_nested_data_response() {
        let body = r#"{"data":{"output":"Hallo"}}"#;
        assert_eq!(parse_response(body).unwrap(), "Hallo");
    }

    #[test]
    fn normalize_strips_markdown_fence_and_quotes() {
        assert_eq!(normalize_translation("```\n\"Bonjour\"\n```"), "Bonjour");
    }

    #[test]
    fn empty_response_is_parse_error() {
        assert!(matches!(
            parse_response(""),
            Err(TranslationError::ParseResponse(_))
        ));
    }

    #[test]
    fn missing_translation_field_is_parse_error() {
        assert!(matches!(
            parse_response(r#"{"ok":true}"#),
            Err(TranslationError::ParseResponse(_))
        ));
    }

    #[test]
    fn status_mapping_recognizes_auth_and_rate_limit() {
        assert!(matches!(
            status_error(StatusCode::UNAUTHORIZED),
            Some(TranslationError::Unauthorized)
        ));
        assert!(matches!(
            status_error(StatusCode::TOO_MANY_REQUESTS),
            Some(TranslationError::RateLimited)
        ));
        assert!(status_error(StatusCode::OK).is_none());
    }

    #[tokio::test]
    async fn pre_cancelled_request_returns_cancelled() {
        let provider = ApiTranslationProvider::new(
            "https://example.invalid",
            "model",
            "",
            Duration::from_secs(30),
            0,
        );
        let cancel = CancellationToken::new();
        cancel.cancel();
        let request = TranslationRequest::new("hello", Language::English, Language::Japanese);
        let result = provider.translate(&request, cancel).await;
        assert!(matches!(result, Err(TranslationError::Cancelled)));
    }
}
