//! Provider-agnostic translation adapter layer.
//!
//! Cloud translation providers differ in request shape, response shape,
//! authentication, language codes, and retry semantics. This module
//! captures those differences behind a single [`TranslationProviderAdapter`]
//! trait so the shared HTTP sender ([`send_with_adapter`]) stays provider
//! agnostic. Adding a provider means implementing the trait and one
//! [`vtrans_core::TranslationProvider`] wrapper; the transport never gains
//! `if provider == ...` branches.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use vtrans_core::error::TranslationError;
use vtrans_core::types::{Language, TranslationRequest};

use crate::retry::RetryPolicy;

/// A fully prepared outgoing HTTP request.
///
/// Produced by [`TranslationProviderAdapter::build_request`] and consumed
/// exclusively by the shared sender. The body is a JSON [`Value`] so the
/// sender does not need to know each provider's concrete request type.
#[derive(Debug, Clone)]
pub struct OutgoingRequest {
    /// HTTP method, e.g. `"POST"`.
    pub method: String,
    /// Absolute URL including any query-string credentials and Baidu's
    /// signature parameters.
    pub url: String,
    /// Headers to attach. The provider has already applied its
    /// [`AuthStrategy`](crate::auth::AuthStrategy), so the sender never
    /// sees the raw credential.
    pub headers: BTreeMap<String, String>,
    /// JSON body, or `None` for form-encoded bodies.
    pub body: Option<Value>,
    /// URL-encoded form body, mutually exclusive with `body`.
    pub form: Option<String>,
}

impl OutgoingRequest {
    /// Create a request with a JSON body.
    #[must_use]
    pub fn json(
        method: impl Into<String>,
        url: impl Into<String>,
        headers: BTreeMap<String, String>,
        body: Value,
    ) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers,
            body: Some(body),
            form: None,
        }
    }

    /// Create a request with a URL-encoded form body.
    #[must_use]
    pub fn form(
        method: impl Into<String>,
        url: impl Into<String>,
        headers: BTreeMap<String, String>,
        form: String,
    ) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers,
            body: None,
            form: Some(form),
        }
    }
}

/// A parsed translation response.
///
/// `segments` preserves the individual translated segments returned by an
/// API (e.g. `DeepL`'s `translations[]` or Google's `data.translations[]`).
/// The caller joins them with `\n` so multi-segment responses never lose a
/// segment. `detected_source` is filled when the API reports a detected
/// source language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTranslation {
    /// Individual translated segments, in API order.
    pub segments: Vec<String>,
    /// Detected source language, when reported by the API.
    pub detected_source: Option<Language>,
}

impl ParsedTranslation {
    /// Join all non-empty segments with a newline.
    #[must_use]
    pub fn into_text(self) -> String {
        self.segments.join("\n")
    }
}

/// Whether an error is retryable, and how long the server asked us to wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryDecision {
    /// Whether the attempt should be retried.
    pub retry: bool,
    /// Server-provided `Retry-After`, if any.
    pub retry_after: Option<Duration>,
}

impl RetryDecision {
    /// A non-retryable decision.
    #[must_use]
    pub const fn stop() -> Self {
        Self {
            retry: false,
            retry_after: None,
        }
    }

    /// A retryable decision with an optional server-requested delay.
    #[must_use]
    pub const fn retry(retry_after: Option<Duration>) -> Self {
        Self {
            retry: true,
            retry_after,
        }
    }
}

/// Provider-specific translation adapter.
///
/// Implementations are pure and synchronous: they map a language pair to
/// provider codes, build an [`OutgoingRequest`], parse a response body into
/// [`ParsedTranslation`], and classify HTTP statuses. The shared sender
/// orchestrates transport, timeout, cancellation, and retry.
#[async_trait]
pub trait TranslationProviderAdapter: Send + Sync {
    /// Stable provider identifier reported in results and logs.
    fn id(&self) -> &'static str;

    /// Map a `VTrans` source language to the provider's source code.
    ///
    /// [`Language::Auto`] is typically omitted from the request; providers
    /// that support auto-detection return `None` for it.
    fn map_source_language(&self, language: Language) -> Option<String>;

    /// Map a `VTrans` target language to the provider's target code.
    fn map_target_language(&self, language: Language) -> Option<String>;

    /// Build a fully-authenticated outgoing request.
    ///
    /// # Errors
    /// Returns [`TranslationError`] when the request cannot be built (e.g.
    /// an unmappable language).
    fn build_request(
        &self,
        request: &TranslationRequest,
    ) -> Result<OutgoingRequest, TranslationError>;

    /// Parse a successful response body into translated segments.
    ///
    /// # Errors
    /// Returns [`TranslationError::ParseResponse`] when the body is empty or
    /// no translated text can be extracted.
    fn parse_response(&self, body: &str) -> Result<ParsedTranslation, TranslationError>;

    /// Classify an HTTP status into a typed error.
    ///
    /// The adapter may inspect the response body through `body` to
    /// disambiguate (e.g. Google's 403 rate-limit body). The returned
    /// [`TranslationError`] has already been mapped; the sender uses its
    /// retry decision to decide whether to retry.
    ///
    /// The default implementation maps `401` to [`TranslationError::Unauthorized`],
    /// `429` to [`TranslationError::RateLimited`], and everything else to
    /// [`TranslationError::ApiRequest`], which matches OpenAI-compatible
    /// endpoints.
    fn map_error(&self, status: StatusCode, body: &str) -> TranslationError {
        let _ = body;
        match status.as_u16() {
            401 => TranslationError::Unauthorized,
            429 => TranslationError::RateLimited,
            _ => TranslationError::ApiRequest(format!("HTTP status {}", status.as_u16())),
        }
    }

    /// Whether a failed HTTP status should be retried, plus any server
    /// backoff hint.
    ///
    /// The default implementation retries transient statuses (`429`, `500`,
    /// `502`, `503`, `504`) and honors a server `Retry-After` if present.
    fn retry_decision(
        &self,
        status: StatusCode,
        body: &str,
        retry_after: Option<Duration>,
    ) -> RetryDecision {
        let _ = body;
        let transient = matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504);
        if transient {
            RetryDecision::retry(retry_after)
        } else {
            RetryDecision::stop()
        }
    }
}

/// Shared HTTP sender with timeout, cancellation, and bounded retry.
///
/// This is the single place that touches `reqwest`. It is provider agnostic:
/// authentication and request/response shaping happen in the adapter.
///
/// # Arguments
/// * `client` - Shared HTTP client.
/// * `adapter` - Provider adapter implementing request/response/error logic.
/// * `request` - The translation request.
/// * `timeout` - Per-attempt timeout.
/// * `retry_policy` - Retry bounds and backoff.
/// * `cancel` - Cancellation token.
///
/// # Errors
/// Returns the provider-mapped [`TranslationError`] after exhausting retries,
/// or [`TranslationError::Cancelled`] when cancelled.
pub async fn send_with_adapter(
    client: &Client,
    adapter: &dyn TranslationProviderAdapter,
    request: &TranslationRequest,
    timeout: Duration,
    retry_policy: RetryPolicy,
    cancel: CancellationToken,
) -> Result<String, TranslationError> {
    for attempt in 0..=retry_policy.max_retries() {
        if cancel.is_cancelled() {
            return Err(TranslationError::Cancelled);
        }
        let outgoing = match adapter.build_request(request) {
            Ok(outgoing) => outgoing,
            Err(error) => {
                warn!(provider = adapter.id(), error = %error, "failed to build request");
                return Err(error);
            }
        };

        let outcome = tokio::select! {
            () = cancel.cancelled() => Err(AttemptError {
                error: TranslationError::Cancelled,
                decision: RetryDecision::stop(),
            }),
            result = tokio::time::timeout(timeout, execute_one(client, adapter, &outgoing)) => {
                match result {
                    Err(_) => Err(AttemptError {
                        error: TranslationError::Timeout(timeout),
                        decision: RetryDecision::retry(None),
                    }),
                    Ok(result) => result,
                }
            }
        };

        match outcome {
            Ok(text) => return Ok(text),
            Err(AttemptError { error, decision }) => {
                warn!(
                    provider = adapter.id(),
                    attempt,
                    error = %error,
                    retry_after_ms = decision.retry_after.map_or(0, |d| d.as_millis()),
                    "translation attempt failed"
                );
                if !decision.retry || attempt >= retry_policy.max_retries() {
                    return Err(error);
                }
                let local_backoff = retry_policy.backoff_duration(attempt);
                let delay = decision
                    .retry_after
                    .map_or(local_backoff, |server| server.max(local_backoff));
                debug!(
                    provider = adapter.id(),
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

/// Outcome of one request attempt.
struct AttemptError {
    error: TranslationError,
    decision: RetryDecision,
}

/// Execute a single HTTP attempt and return the translated text.
async fn execute_one(
    client: &Client,
    adapter: &dyn TranslationProviderAdapter,
    outgoing: &OutgoingRequest,
) -> Result<String, AttemptError> {
    let started = Instant::now();
    let builder = build_request_builder(client, outgoing).map_err(|error| AttemptError {
        error,
        decision: RetryDecision::stop(),
    })?;
    let response = builder.send().await.map_err(|error| {
        if error.is_timeout() {
            AttemptError {
                error: TranslationError::Timeout(Duration::ZERO),
                decision: RetryDecision::retry(None),
            }
        } else {
            AttemptError {
                error: TranslationError::ApiRequest(error.to_string()),
                decision: RetryDecision::retry(None),
            }
        }
    })?;
    let status = response.status();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_retry_after);
    let body = response.text().await.map_err(|error| AttemptError {
        error: TranslationError::ApiRequest(format!("read response body: {error}")),
        decision: RetryDecision::retry(retry_after),
    })?;
    let elapsed_ms = elapsed_millis(started);

    if status.is_success() {
        let parsed = adapter.parse_response(&body).map_err(|error| {
            // A successful HTTP status can still carry a provider-level
            // error (e.g. Baidu 200 + error_code 54003). Let the adapter
            // decide whether the parsed error is retryable.
            let decision = adapter.retry_decision(status, &body, retry_after);
            AttemptError { error, decision }
        })?;
        if parsed.segments.iter().all(std::string::String::is_empty) {
            warn!(
                provider = adapter.id(),
                "translation response contained no text"
            );
            return Err(AttemptError {
                error: TranslationError::ParseResponse("empty translation segments".to_string()),
                decision: RetryDecision::stop(),
            });
        }
        info!(
            provider = adapter.id(),
            status = status.as_u16(),
            elapsed_ms,
            segments = parsed.segments.len(),
            "translation response parsed"
        );
        return Ok(parsed.into_text());
    }

    warn!(
        provider = adapter.id(),
        status = status.as_u16(),
        retry_after_ms = retry_after.map_or(0, |d| d.as_millis()),
        "translation API returned non-success status"
    );
    let decision = adapter.retry_decision(status, &body, retry_after);
    let error = adapter.map_error(status, &body);
    Err(AttemptError { error, decision })
}

/// Build a `reqwest` request from an [`OutgoingRequest`].
fn build_request_builder(
    client: &Client,
    outgoing: &OutgoingRequest,
) -> Result<RequestBuilder, TranslationError> {
    let method = Method::from_bytes(outgoing.method.as_bytes())
        .map_err(|error| TranslationError::ApiRequest(format!("invalid HTTP method: {error}")))?;
    let mut builder = client.request(method, &outgoing.url);
    for (name, value) in &outgoing.headers {
        builder = builder.header(name, value);
    }
    if let Some(body) = &outgoing.body {
        builder = builder.json(body);
    }
    if let Some(form) = &outgoing.form {
        builder = builder
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form.clone());
    }
    Ok(builder)
}

/// Parse a `Retry-After` header value expressed in seconds.
///
/// HTTP-date values are not parsed; the header is ignored in that case and
/// the local exponential backoff applies.
fn parse_retry_after(value: &str) -> Option<Duration> {
    let seconds = value.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds))
}

/// Convert an `Instant` delta to milliseconds, saturating at `u64::MAX`.
#[must_use]
pub fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Build the list of pairs shared by broad cloud providers.
///
/// Advertises `(Auto|concrete source, concrete target)` for every concrete
/// target. Providers with narrower support override this.
#[must_use]
pub fn all_pairs() -> Vec<(Language, Language)> {
    let mut pairs = Vec::with_capacity(12);
    for &target in Language::all_concrete() {
        pairs.push((Language::Auto, target));
        for &source in Language::all_concrete() {
            pairs.push((source, target));
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_pairs_cover_every_concrete_target() {
        let pairs = all_pairs();
        assert_eq!(pairs.len(), 12);
        for &target in Language::all_concrete() {
            assert!(pairs.contains(&(Language::Auto, target)));
            for &source in Language::all_concrete() {
                assert!(pairs.contains(&(source, target)));
            }
        }
    }

    #[test]
    fn parsed_translation_joins_segments() {
        let parsed = ParsedTranslation {
            segments: vec!["a".to_string(), "b".to_string()],
            detected_source: Some(Language::English),
        };
        assert_eq!(parsed.into_text(), "a\nb");
    }

    #[test]
    fn retry_decision_helpers() {
        assert!(!RetryDecision::stop().retry);
        let retry = RetryDecision::retry(Some(Duration::from_secs(3)));
        assert!(retry.retry);
        assert_eq!(retry.retry_after, Some(Duration::from_secs(3)));
    }

    #[test]
    fn outgoing_request_json_and_form() {
        let json = OutgoingRequest::json(
            "POST",
            "https://example.invalid",
            BTreeMap::new(),
            serde_json::json!({"a": 1}),
        );
        assert!(json.body.is_some());
        assert!(json.form.is_none());

        let form = OutgoingRequest::form(
            "POST",
            "https://example.invalid",
            BTreeMap::new(),
            "a=1".to_string(),
        );
        assert!(form.body.is_none());
        assert!(form.form.is_some());
    }

    #[test]
    fn parse_retry_after_seconds() {
        assert_eq!(parse_retry_after("5"), Some(Duration::from_secs(5)));
        assert_eq!(parse_retry_after("abc"), None);
    }

    /// Adapter that relies entirely on the trait's default `map_error` and
    /// `retry_decision` implementations.
    struct DefaultAdapter;

    #[async_trait]
    impl TranslationProviderAdapter for DefaultAdapter {
        fn id(&self) -> &'static str {
            "default"
        }

        fn map_source_language(&self, language: Language) -> Option<String> {
            if language.is_auto() {
                None
            } else {
                Some(language.code().to_string())
            }
        }

        fn map_target_language(&self, language: Language) -> Option<String> {
            if language.is_auto() {
                None
            } else {
                Some(language.code().to_string())
            }
        }

        fn build_request(
            &self,
            _request: &TranslationRequest,
        ) -> Result<OutgoingRequest, TranslationError> {
            Ok(OutgoingRequest::json(
                "POST",
                "http://example.invalid",
                BTreeMap::new(),
                serde_json::json!({}),
            ))
        }

        fn parse_response(&self, body: &str) -> Result<ParsedTranslation, TranslationError> {
            Ok(ParsedTranslation {
                segments: vec![body.to_string()],
                detected_source: None,
            })
        }
    }

    #[test]
    fn default_map_error_and_retry_decision() {
        let adapter = DefaultAdapter;
        assert!(matches!(
            adapter.map_error(StatusCode::UNAUTHORIZED, ""),
            TranslationError::Unauthorized
        ));
        assert!(matches!(
            adapter.map_error(StatusCode::TOO_MANY_REQUESTS, ""),
            TranslationError::RateLimited
        ));
        assert!(
            adapter
                .retry_decision(StatusCode::TOO_MANY_REQUESTS, "", None)
                .retry
        );
        assert!(
            adapter
                .retry_decision(StatusCode::INTERNAL_SERVER_ERROR, "", None)
                .retry
        );
        assert!(
            !adapter
                .retry_decision(StatusCode::UNAUTHORIZED, "", None)
                .retry
        );
    }
}
