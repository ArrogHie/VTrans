//! OpenAI-compatible chat completion translation provider.

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
use crate::prompt::build_system_prompt;
use crate::retry::RetryPolicy;
use crate::validate::validate_language_pair;

use super::language::{map_source_auto, map_target_generic};

/// OpenAI-compatible chat completion provider.
///
/// Sends a chat-completion request to `endpoint` with `Authorization: Bearer
/// <key>`. The endpoint may be any OpenAI-compatible server. The stable
/// runtime id is `"openai"`.
///
/// # Example
///
/// ```no_run
/// use std::time::Duration;
/// use vtrans_core::traits::TranslationProvider;
/// use vtrans_core::types::{Language, TranslationRequest};
/// use tokio_util::sync::CancellationToken;
/// use vtrans_translation::OpenAiProvider;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let provider = OpenAiProvider::new(
///     "https://api.openai.com/v1/chat/completions",
///     "gpt-4o-mini",
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
/// assert_eq!(result.provider_id, "openai");
/// # Ok(())
/// # }
/// ```
pub struct OpenAiProvider {
    adapter: OpenAiAdapter,
    client: reqwest::Client,
    timeout: Duration,
    retry_policy: RetryPolicy,
    supported_pairs: Vec<(Language, Language)>,
}

impl fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiProvider")
            .field("adapter", &self.adapter)
            .field("timeout", &self.timeout)
            .field("max_retries", &self.retry_policy.max_retries())
            .field("client", &self.client)
            .field("supported_pairs", &self.supported_pairs)
            .finish()
    }
}

/// Request/response/error logic for OpenAI-compatible endpoints.
pub struct OpenAiAdapter {
    endpoint: String,
    model: String,
    api_key: String,
}

impl fmt::Debug for OpenAiAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiAdapter")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("api_key", &"****")
            .finish()
    }
}

impl OpenAiProvider {
    /// Create an `OpenAI` provider.
    ///
    /// # Arguments
    /// * `endpoint` - Full chat-completions URL.
    /// * `model` - Model identifier sent in the JSON request body.
    /// * `api_key` - API key sent as a `Bearer` token.
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
            adapter: OpenAiAdapter {
                endpoint: endpoint.to_string(),
                model: model.to_string(),
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
impl TranslationProvider for OpenAiProvider {
    /// Stable provider identifier reported in results and logs.
    fn id(&self) -> &'static str {
        "openai"
    }

    /// Pairs supported by the `OpenAI` provider.
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
impl TranslationProviderAdapter for OpenAiAdapter {
    fn id(&self) -> &'static str {
        "openai"
    }

    fn map_source_language(&self, language: Language) -> Option<String> {
        map_source_auto(language)
    }

    fn map_target_language(&self, language: Language) -> Option<String> {
        map_target_generic(language)
    }

    fn build_request(
        &self,
        request: &TranslationRequest,
    ) -> Result<OutgoingRequest, TranslationError> {
        let system_prompt = build_system_prompt(request.source, request.target);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": request.text}
            ],
            "temperature": 0.0
        });
        let mut headers = BTreeMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        AuthStrategy::Bearer.apply(&self.api_key, &mut headers);
        Ok(OutgoingRequest::json("POST", &self.endpoint, headers, body))
    }

    fn parse_response(&self, body: &str) -> Result<ParsedTranslation, TranslationError> {
        parse_openai_response(body)
    }

    fn map_error(&self, status: StatusCode, _body: &str) -> TranslationError {
        match status.as_u16() {
            401 => TranslationError::Unauthorized,
            429 => TranslationError::RateLimited,
            500 | 503 => TranslationError::ApiRequest(status_message(status)),
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

fn status_message(status: StatusCode) -> String {
    format!("HTTP status {}", status.as_u16())
}

/// Parse an OpenAI-compatible chat/completion response.
///
/// Supports `choices[0].message.content`, `choices[0].text`, common
/// translation fields, and a bare JSON string.
///
/// # Errors
/// Returns [`TranslationError::ParseResponse`] when the body is empty or no
/// translation field can be found.
pub fn parse_openai_response(body: &str) -> Result<ParsedTranslation, TranslationError> {
    if body.trim().is_empty() {
        return Err(TranslationError::ParseResponse(
            "empty response body".to_string(),
        ));
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|error| TranslationError::ParseResponse(error.to_string()))?;
    let text = extract_openai_text(&value)
        .ok_or_else(|| TranslationError::ParseResponse("no translation field found".to_string()))?;
    Ok(ParsedTranslation {
        segments: vec![text],
        detected_source: None,
    })
}

fn extract_openai_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(normalize_translation(text));
    }
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        for choice in choices {
            if let Some(text) = choice
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
            {
                return Some(normalize_translation(text));
            }
            if let Some(text) = choice.get("text").and_then(Value::as_str) {
                return Some(normalize_translation(text));
            }
        }
    }
    for key in ["translated_text", "translation", "result", "output", "text"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Some(normalize_translation(text));
        }
    }
    value.get("data").and_then(extract_openai_text)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_is_openai() {
        let provider = OpenAiProvider::new(
            "https://example.invalid",
            "model",
            "sk-key",
            Duration::from_secs(1),
            0,
        );
        assert_eq!(provider.id(), "openai");
    }

    #[test]
    fn debug_never_leaks_api_key() {
        let provider = OpenAiProvider::new(
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
    fn build_request_uses_bearer_and_prompt() {
        let adapter = OpenAiAdapter {
            endpoint: "https://example.invalid/v1/chat/completions".to_string(),
            model: "gpt-4o-mini".to_string(),
            api_key: "sk-secret".to_string(),
        };
        let request = TranslationRequest::new("hello", Language::English, Language::Japanese);
        let outgoing = adapter.build_request(&request).unwrap();
        assert_eq!(outgoing.method, "POST");
        assert_eq!(outgoing.url, "https://example.invalid/v1/chat/completions");
        assert_eq!(
            outgoing.headers.get("Authorization").unwrap(),
            "Bearer sk-secret"
        );
        let body = outgoing.body.unwrap();
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["messages"][1]["content"], "hello");
    }

    #[test]
    fn parse_chat_response() {
        let body = r#"{"choices":[{"message":{"content":"こんにちは"}}]}"#;
        let parsed = parse_openai_response(body).unwrap();
        assert_eq!(parsed.segments, vec!["こんにちは".to_string()]);
    }

    #[test]
    fn parse_completion_response() {
        let body = r#"{"choices":[{"text":"Bonjour"}]}"#;
        let parsed = parse_openai_response(body).unwrap();
        assert_eq!(parsed.into_text(), "Bonjour");
    }

    #[test]
    fn empty_body_is_parse_error() {
        assert!(matches!(
            parse_openai_response(""),
            Err(TranslationError::ParseResponse(_))
        ));
    }

    #[test]
    fn map_status_codes() {
        let adapter = OpenAiAdapter {
            endpoint: "x".to_string(),
            model: "m".to_string(),
            api_key: "k".to_string(),
        };
        assert!(matches!(
            adapter.map_error(StatusCode::UNAUTHORIZED, ""),
            TranslationError::Unauthorized
        ));
        assert!(matches!(
            adapter.map_error(StatusCode::TOO_MANY_REQUESTS, ""),
            TranslationError::RateLimited
        ));
        assert!(
            !adapter
                .retry_decision(StatusCode::UNAUTHORIZED, "", None)
                .retry
        );
        assert!(
            adapter
                .retry_decision(StatusCode::TOO_MANY_REQUESTS, "", None)
                .retry
        );
    }
}
