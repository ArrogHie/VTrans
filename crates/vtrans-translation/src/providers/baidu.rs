//! Baidu Translate (百度通用翻译) provider.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

use super::language::{map_baidu, provider_error};

/// Baidu Translate provider.
///
/// Sends a form-encoded POST to Baidu's `/trans/api` endpoint with an
/// `appid`, random `salt`, and an MD5 signature over
/// `appid + q + salt + secret`. The stable runtime id is `"baidu"`.
///
/// # Example
///
/// ```no_run
/// use std::time::Duration;
/// use vtrans_core::traits::TranslationProvider;
/// use vtrans_core::types::{Language, TranslationRequest};
/// use tokio_util::sync::CancellationToken;
/// use vtrans_translation::BaiduProvider;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let provider = BaiduProvider::new(
///     "https://fanyi-api.baidu.com/api/trans/vip/translate",
///     "app-id",
///     "secret-key",
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
/// assert_eq!(result.provider_id, "baidu");
/// # Ok(())
/// # }
/// ```
pub struct BaiduProvider {
    adapter: BaiduAdapter,
    client: reqwest::Client,
    timeout: Duration,
    retry_policy: RetryPolicy,
    supported_pairs: Vec<(Language, Language)>,
}

impl fmt::Debug for BaiduProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BaiduProvider")
            .field("adapter", &self.adapter)
            .field("timeout", &self.timeout)
            .field("max_retries", &self.retry_policy.max_retries())
            .field("client", &self.client)
            .field("supported_pairs", &self.supported_pairs)
            .finish()
    }
}

/// Request/response/error logic for Baidu.
pub struct BaiduAdapter {
    endpoint: String,
    app_id: String,
    secret: String,
}

impl fmt::Debug for BaiduAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BaiduAdapter")
            .field("endpoint", &self.endpoint)
            .field("app_id", &"****")
            .field("secret", &"****")
            .finish()
    }
}

impl BaiduProvider {
    /// Create a Baidu provider.
    ///
    /// # Arguments
    /// * `endpoint` - Baidu `/trans/vip/translate` endpoint URL.
    /// * `app_id` - Baidu APP ID.
    /// * `secret` - Baidu secret key used for the MD5 signature.
    /// * `timeout` - Per-attempt request timeout.
    /// * `max_retries` - Maximum number of retries after the first attempt.
    #[must_use]
    pub fn new(
        endpoint: &str,
        app_id: &str,
        secret: &str,
        timeout: Duration,
        max_retries: u32,
    ) -> Self {
        Self {
            adapter: BaiduAdapter {
                endpoint: endpoint.to_string(),
                app_id: app_id.to_string(),
                secret: secret.to_string(),
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
impl TranslationProvider for BaiduProvider {
    /// Stable provider identifier reported in results and logs.
    fn id(&self) -> &'static str {
        "baidu"
    }

    /// Pairs supported by the Baidu provider.
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
impl TranslationProviderAdapter for BaiduAdapter {
    fn id(&self) -> &'static str {
        "baidu"
    }

    fn map_source_language(&self, language: Language) -> Option<String> {
        Some(map_baidu(language))
    }

    fn map_target_language(&self, language: Language) -> Option<String> {
        Some(map_baidu(language))
    }

    fn build_request(
        &self,
        request: &TranslationRequest,
    ) -> Result<OutgoingRequest, TranslationError> {
        let source = self
            .map_source_language(request.source)
            .ok_or_else(|| provider_error(request.source, request.target))?;
        let target = self
            .map_target_language(request.target)
            .ok_or_else(|| provider_error(request.source, request.target))?;
        let salt = random_salt();
        let mut form = format!(
            "q={}&from={source}&to={target}",
            crate::auth::urlencode(&request.text)
        );
        // The MD5 signature is computed inside the auth strategy so the
        // secret never reaches the shared sender or logs.
        AuthStrategy::BaiduMd5.apply_baidu_form(
            &mut form,
            &self.app_id,
            &request.text,
            &salt,
            &self.secret,
        );
        let mut headers = BTreeMap::new();
        headers.insert(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        Ok(OutgoingRequest::form("POST", &self.endpoint, headers, form))
    }

    fn parse_response(&self, body: &str) -> Result<ParsedTranslation, TranslationError> {
        parse_baidu_response(body)
    }

    fn map_error(&self, status: StatusCode, body: &str) -> TranslationError {
        // Baidu reports errors in the JSON body with an `error_code`.
        if let Some(code) = extract_baidu_error_code(body) {
            return match code {
                52003 | 54001 => TranslationError::Unauthorized,
                54003 | 54005 => TranslationError::RateLimited,
                54004 => {
                    TranslationError::ApiRequest("baidu insufficient balance (54004)".to_string())
                }
                58001 => TranslationError::UnsupportedPair {
                    src: Language::Auto,
                    target: Language::Auto,
                },
                _ => TranslationError::ApiRequest(format!("baidu error {code}")),
            };
        }
        match status.as_u16() {
            401 => TranslationError::Unauthorized,
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
        if let Some(code) = extract_baidu_error_code(body) {
            return if matches!(code, 54003 | 54005) {
                RetryDecision::retry(retry_after)
            } else {
                RetryDecision::stop()
            };
        }
        if matches!(status.as_u16(), 429 | 500 | 503) {
            RetryDecision::retry(retry_after)
        } else {
            RetryDecision::stop()
        }
    }
}

/// Generate a random salt string for Baidu request signing.
fn random_salt() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!("{millis}")
}

/// Extract a Baidu `error_code` from a response body, if present.
fn extract_baidu_error_code(body: &str) -> Option<i64> {
    let value: Value = serde_json::from_str(body).ok()?;
    value.get("error_code").and_then(Value::as_i64)
}

/// Parse a Baidu response into translated segments.
///
/// Baidu returns `{"trans_result":[{"src":"...","dst":"..."}, ...]}`. Each
/// `dst` is preserved as its own segment.
pub fn parse_baidu_response(body: &str) -> Result<ParsedTranslation, TranslationError> {
    if body.trim().is_empty() {
        return Err(TranslationError::ParseResponse(
            "empty response body".to_string(),
        ));
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|error| TranslationError::ParseResponse(error.to_string()))?;
    if let Some(code) = value.get("error_code").and_then(Value::as_i64) {
        return Err(TranslationError::ApiRequest(format!("baidu error {code}")));
    }
    let results = value
        .get("trans_result")
        .and_then(Value::as_array)
        .ok_or_else(|| TranslationError::ParseResponse("missing trans_result array".to_string()))?;
    let mut segments = Vec::with_capacity(results.len());
    for entry in results {
        let Some(text) = entry.get("dst").and_then(Value::as_str) else {
            return Err(TranslationError::ParseResponse(
                "trans_result entry missing dst".to_string(),
            ));
        };
        segments.push(text.to_string());
    }
    if segments.is_empty() {
        return Err(TranslationError::ParseResponse(
            "empty trans_result array".to_string(),
        ));
    }
    Ok(ParsedTranslation {
        segments,
        detected_source: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter() -> BaiduAdapter {
        BaiduAdapter {
            endpoint: "https://fanyi-api.baidu.com/api/trans/vip/translate".to_string(),
            app_id: "202400000000000".to_string(),
            secret: "secret-key".to_string(),
        }
    }

    #[test]
    fn provider_id_is_baidu() {
        let provider = BaiduProvider::new(
            "https://example.invalid",
            "app",
            "secret",
            Duration::from_secs(1),
            0,
        );
        assert_eq!(provider.id(), "baidu");
    }

    #[test]
    fn baidu_md5_signature_matches_spec() {
        // Official example: appid=2015063000000001, q=apple, salt=1435660288,
        // secret=12345678 -> sign=f89f9594663708c1605f3d736d01d2d4
        let mut form = String::new();
        AuthStrategy::BaiduMd5.apply_baidu_form(
            &mut form,
            "2015063000000001",
            "apple",
            "1435660288",
            "12345678",
        );
        assert!(form.contains("&sign=f89f9594663708c1605f3d736d01d2d4"));
        assert!(!form.contains("12345678"));
    }

    #[test]
    fn build_request_contains_signed_form() {
        let request = TranslationRequest::new("apple", Language::English, Language::Japanese);
        let outgoing = adapter().build_request(&request).unwrap();
        assert_eq!(outgoing.method, "POST");
        let form = outgoing.form.unwrap();
        assert!(form.contains("from=en"));
        assert!(form.contains("to=jp"));
        assert!(form.contains("appid=202400000000000"));
        assert!(form.contains("salt="));
        assert!(form.contains("sign="));
        assert!(!form.contains("secret-key"));
    }

    #[test]
    fn auto_source_maps_to_auto() {
        let request = TranslationRequest::new("hello", Language::Auto, Language::Japanese);
        let outgoing = adapter().build_request(&request).unwrap();
        let form = outgoing.form.unwrap();
        assert!(form.contains("from=auto"));
    }

    #[test]
    fn parse_multi_segment_response() {
        let body = r#"{
            "from":"en",
            "to":"zh",
            "trans_result":[
                {"src":"hello","dst":"你好"},
                {"src":"world","dst":"世界"}
            ]
        }"#;
        let parsed = parse_baidu_response(body).unwrap();
        assert_eq!(
            parsed.segments,
            vec!["你好".to_string(), "世界".to_string()]
        );
        assert_eq!(parsed.into_text(), "你好\n世界");
    }

    #[test]
    fn baidu_error_code_maps() {
        assert!(matches!(
            adapter().map_error(StatusCode::OK, r#"{"error_code":52003}"#),
            TranslationError::Unauthorized
        ));
        assert!(matches!(
            adapter().map_error(StatusCode::OK, r#"{"error_code":54005}"#),
            TranslationError::RateLimited
        ));
        assert!(matches!(
            adapter().map_error(StatusCode::OK, r#"{"error_code":58001}"#),
            TranslationError::UnsupportedPair { .. }
        ));
        assert!(matches!(
            adapter().map_error(StatusCode::OK, r#"{"error_code":54004}"#),
            TranslationError::ApiRequest(_)
        ));
        assert!(
            adapter()
                .retry_decision(StatusCode::OK, r#"{"error_code":54003}"#, None)
                .retry
        );
        assert!(
            !adapter()
                .retry_decision(StatusCode::OK, r#"{"error_code":52003}"#, None)
                .retry
        );
    }

    #[test]
    fn missing_trans_result_is_parse_error() {
        assert!(matches!(
            parse_baidu_response(r#"{"ok":true}"#),
            Err(TranslationError::ParseResponse(_))
        ));
    }
}
