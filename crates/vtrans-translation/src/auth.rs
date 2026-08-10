//! Authentication strategies for cloud translation providers.
//!
//! Each provider selects exactly one strategy and applies it inside its own
//! [`build_request`](crate::adapter::TranslationProviderAdapter::build_request).
//! The shared HTTP sender never hard-codes a bearer token or any other
//! authentication scheme, so adding a provider never requires touching the
//! transport layer.

/// How a provider authenticates requests to its API.
///
/// The strategy is applied by the provider inside [`build_request`] and is
/// never inspected by the shared HTTP sender, so authentication stays
/// encapsulated per provider.
///
/// # Example
///
/// ```
/// use vtrans_translation::AuthStrategy;
///
/// let mut headers = std::collections::BTreeMap::new();
/// AuthStrategy::AuthorizationScheme("DeepL-Auth-Key")
///     .apply("fancy-secret", &mut headers);
/// assert_eq!(headers.get("DeepL-Auth-Key").unwrap(), "fancy-secret");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthStrategy {
    /// `Authorization: Bearer <key>`.
    Bearer,
    /// A dedicated authorization header whose name is the scheme, e.g.
    /// `DeepL-Auth-Key: <key>`.
    AuthorizationScheme(&'static str),
    /// A fixed header with the key as its value, e.g.
    /// `Ocp-Apim-Subscription-Key: <key>`.
    Header(&'static str),
    /// A query-string credential, e.g. `?key=<key>`.
    Query(&'static str),
    /// Baidu's `appid + q + salt + secret` MD5 signature. The signature
    /// uses the raw secret and is computed by the provider's own
    /// [`build_request`](crate::adapter::TranslationProviderAdapter::build_request)
    /// so the secret never reaches the shared sender.
    BaiduMd5,
}

impl AuthStrategy {
    /// Apply this strategy to a mutable header map.
    ///
    /// Query and Baidu strategies are handled by the provider's
    /// [`build_request`](crate::adapter::TranslationProviderAdapter::build_request)
    /// which owns the query construction; this method only covers the
    /// header-based schemes.
    ///
    /// # Arguments
    /// * `key` - The credential value.
    /// * `headers` - Request headers to mutate.
    pub fn apply(self, key: &str, headers: &mut std::collections::BTreeMap<String, String>) {
        match self {
            Self::Bearer => {
                headers.insert("Authorization".to_string(), format!("Bearer {key}"));
            }
            Self::AuthorizationScheme(scheme) => {
                headers.insert(scheme.to_string(), key.to_string());
            }
            Self::Header(header) => {
                headers.insert(header.to_string(), key.to_string());
            }
            Self::Query(_) | Self::BaiduMd5 => {
                // Query credentials and Baidu signatures are applied by the
                // provider's build_request, which owns the URL construction.
            }
        }
    }

    /// Append a query-string credential to an existing query string.
    ///
    /// Used by providers that authenticate with a query parameter (e.g.
    /// Google's `key`). The value is URL-encoded.
    ///
    /// # Arguments
    /// * `key` - The credential value.
    /// * `query` - Existing query string to extend (without leading `?`).
    pub fn apply_query(self, key: &str, query: &mut String) {
        if let Self::Query(param) = self {
            if !query.is_empty() {
                query.push('&');
            }
            query.push_str(param);
            query.push('=');
            query.push_str(&urlencode(key));
        }
    }

    /// Append Baidu's signed form fields (`appid`, `salt`, `sign`) for the
    /// `BaiduMd5` strategy.
    ///
    /// The signature is `MD5(appid + q + salt + secret)`. The secret is
    /// consumed here, so the shared HTTP sender never sees it.
    ///
    /// # Arguments
    /// * `form` - Existing form body to extend.
    /// * `app_id` - Baidu APP ID.
    /// * `query` - The raw source text being translated.
    /// * `salt` - Per-request random salt.
    /// * `key` - Baidu secret key.
    pub fn apply_baidu_form(
        self,
        form: &mut String,
        app_id: &str,
        query: &str,
        salt: &str,
        key: &str,
    ) {
        if self == Self::BaiduMd5 {
            let sign = md5_hex(&format!("{app_id}{query}{salt}{key}"));
            form.push_str("&appid=");
            form.push_str(app_id);
            form.push_str("&salt=");
            form.push_str(salt);
            form.push_str("&sign=");
            form.push_str(&sign);
        }
    }

    /// Return a short, log-safe description of this strategy.
    #[must_use]
    pub fn describe(self) -> String {
        match self {
            Self::Bearer => "bearer".to_string(),
            Self::AuthorizationScheme(scheme) => format!("scheme:{scheme}"),
            Self::Header(header) => format!("header:{header}"),
            Self::Query(param) => format!("query:{param}"),
            Self::BaiduMd5 => "baidu-md5".to_string(),
        }
    }
}

/// Percent-encode a string for use in a URL query value.
#[must_use]
pub fn urlencode(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                use std::fmt::Write as _;
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}

/// Compute the MD5 hex digest of a byte string.
#[must_use]
pub fn md5_hex(input: &str) -> String {
    use md5::{Digest, Md5};

    let digest = Md5::digest(input.as_bytes());
    hex::encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_sets_authorization_header() {
        let mut headers = std::collections::BTreeMap::new();
        AuthStrategy::Bearer.apply("sk-1234", &mut headers);
        assert_eq!(headers.get("Authorization").unwrap(), "Bearer sk-1234");
    }

    #[test]
    fn scheme_sets_dedicated_header() {
        let mut headers = std::collections::BTreeMap::new();
        AuthStrategy::AuthorizationScheme("DeepL-Auth-Key").apply("sk-1234", &mut headers);
        assert_eq!(headers.get("DeepL-Auth-Key").unwrap(), "sk-1234");
        assert!(!headers.contains_key("Authorization"));
    }

    #[test]
    fn header_sets_fixed_header() {
        let mut headers = std::collections::BTreeMap::new();
        AuthStrategy::Header("Ocp-Apim-Subscription-Key").apply("abc", &mut headers);
        assert_eq!(headers.get("Ocp-Apim-Subscription-Key").unwrap(), "abc");
    }

    #[test]
    fn query_and_baidu_do_not_touch_headers() {
        let mut headers = std::collections::BTreeMap::new();
        AuthStrategy::Query("key").apply("secret", &mut headers);
        AuthStrategy::BaiduMd5.apply("secret", &mut headers);
        assert!(headers.is_empty());
    }

    #[test]
    fn query_appends_encoded_value() {
        let mut query = String::from("target=ja");
        AuthStrategy::Query("key").apply_query("my key", &mut query);
        assert_eq!(query, "target=ja&key=my+key");
    }

    #[test]
    fn baidu_md5_does_not_append_query() {
        let mut query = String::from("target=zh");
        AuthStrategy::BaiduMd5.apply_query("secret", &mut query);
        assert_eq!(query, "target=zh");
    }

    #[test]
    fn baidu_form_appends_signature() {
        let mut form = String::from("q=apple");
        AuthStrategy::BaiduMd5.apply_baidu_form(&mut form, "app-id", "apple", "42", "secret");
        assert!(form.contains("&appid=app-id"));
        assert!(form.contains("&salt=42"));
        assert!(form.contains("&sign="));
        assert!(!form.contains("secret"));
    }

    #[test]
    fn describe_is_log_safe() {
        assert_eq!(AuthStrategy::Bearer.describe(), "bearer");
        assert_eq!(
            AuthStrategy::AuthorizationScheme("DeepL-Auth-Key").describe(),
            "scheme:DeepL-Auth-Key"
        );
        assert_eq!(
            AuthStrategy::Header("Ocp-Apim-Subscription-Key").describe(),
            "header:Ocp-Apim-Subscription-Key"
        );
        assert_eq!(AuthStrategy::Query("key").describe(), "query:key");
        assert_eq!(AuthStrategy::BaiduMd5.describe(), "baidu-md5");
    }
}
