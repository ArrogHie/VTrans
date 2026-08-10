//! Canonical credential targets for cloud translation providers.
//!
//! [`CredentialTarget`] is the typed, single source of truth for the logical
//! credential targets used by [`crate::CredentialManager`]. Each variant maps
//! to a fixed logical name stored under the `VTrans:` namespace (for example
//! `VTrans:openai`), so callers cannot mistype a provider name and every
//! consumer shares the same spelling.
//!
//! The enum deliberately covers only *credential* targets. The legacy
//! `translation` target used by older consumers is not a provider and stays
//! reachable through the generic
//! [`store`](crate::CredentialManager::store) /
//! [`load`](crate::CredentialManager::load) /
//! [`delete`](crate::CredentialManager::delete) methods for backward
//! compatibility.

use std::fmt;

/// Logical credential target for a cloud translation provider.
///
/// Each variant maps to a fixed logical name (see [`as_str`](Self::as_str))
/// that is prefixed with `VTrans:` by the credential manager before it
/// reaches the backend store.
///
/// # Example
///
/// ```
/// use vtrans_security::CredentialTarget;
///
/// assert_eq!(CredentialTarget::OpenAI.as_str(), "openai");
/// assert_eq!(CredentialTarget::BaiduAppId.as_str(), "baidu_app_id");
/// assert!(CredentialTarget::ALL.contains(&CredentialTarget::Tencent));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CredentialTarget {
    /// `OpenAI` API key, stored under the `openai` target.
    OpenAI,
    /// `DeepL` API key, stored under the `deepl` target.
    DeepL,
    /// `Google` Cloud Translation API key, stored under the `google` target.
    Google,
    /// `Azure` Translator subscription key, stored under the `azure` target.
    Azure,
    /// `Baidu` Translate APP ID, stored under the `baidu_app_id` target.
    ///
    /// The APP ID is not a secret, but it is routed through the credential
    /// vault anyway so that all provider credentials live in one place and
    /// follow the same lifecycle.
    BaiduAppId,
    /// `Baidu` Translate Secret Key, stored under the `baidu_secret` target.
    ///
    /// Kept independent from [`BaiduAppId`](Self::BaiduAppId): the
    /// application layer reads both targets separately and lets the
    /// `BaiduProvider` assemble the request signature.
    BaiduSecret,
    /// `Tencent` translator credential, stored under the `tencent` target.
    ///
    /// Reserved for a future integration; no provider consumes this target
    /// yet, but the target is fully functional.
    Tencent,
}

impl CredentialTarget {
    /// All known credential targets in a stable declaration order.
    ///
    /// Useful for iterating the full target set (validation, UI enumeration,
    /// tests).
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_security::CredentialTarget;
    ///
    /// let names: Vec<&str> = CredentialTarget::ALL
    ///     .iter()
    ///     .map(|t| t.as_str())
    ///     .collect();
    /// assert_eq!(
    ///     names,
    ///     ["openai", "deepl", "google", "azure", "baidu_app_id", "baidu_secret", "tencent"]
    /// );
    /// ```
    pub const ALL: [Self; 7] = [
        Self::OpenAI,
        Self::DeepL,
        Self::Google,
        Self::Azure,
        Self::BaiduAppId,
        Self::BaiduSecret,
        Self::Tencent,
    ];

    /// Returns the logical target name stored under the `VTrans:` namespace.
    ///
    /// The returned name is stable: changing it would orphan previously
    /// stored credentials, so it must only change through a migration.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_security::CredentialTarget;
    ///
    /// assert_eq!(CredentialTarget::Azure.as_str(), "azure");
    /// assert_eq!(CredentialTarget::BaiduSecret.as_str(), "baidu_secret");
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::DeepL => "deepl",
            Self::Google => "google",
            Self::Azure => "azure",
            Self::BaiduAppId => "baidu_app_id",
            Self::BaiduSecret => "baidu_secret",
            Self::Tencent => "tencent",
        }
    }
}

impl fmt::Display for CredentialTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::CredentialTarget;

    #[test]
    fn target_names_are_stable_and_documented() {
        let expected = [
            ("openai", CredentialTarget::OpenAI),
            ("deepl", CredentialTarget::DeepL),
            ("google", CredentialTarget::Google),
            ("azure", CredentialTarget::Azure),
            ("baidu_app_id", CredentialTarget::BaiduAppId),
            ("baidu_secret", CredentialTarget::BaiduSecret),
            ("tencent", CredentialTarget::Tencent),
        ];
        for (name, target) in expected {
            assert_eq!(target.as_str(), name);
            assert_eq!(target.to_string(), name);
        }
    }

    #[test]
    fn all_contains_every_variant_exactly_once() {
        let mut seen = Vec::new();
        for target in CredentialTarget::ALL {
            assert!(!seen.contains(&target), "duplicate target: {target:?}");
            seen.push(target);
        }
        assert_eq!(seen.len(), 7);
        // Sorted by declaration order, matching the canonical list.
        assert_eq!(
            seen,
            vec![
                CredentialTarget::OpenAI,
                CredentialTarget::DeepL,
                CredentialTarget::Google,
                CredentialTarget::Azure,
                CredentialTarget::BaiduAppId,
                CredentialTarget::BaiduSecret,
                CredentialTarget::Tencent,
            ]
        );
    }

    #[test]
    fn baidu_app_id_and_secret_are_distinct_targets() {
        assert_ne!(
            CredentialTarget::BaiduAppId.as_str(),
            CredentialTarget::BaiduSecret.as_str()
        );
        assert_ne!(CredentialTarget::BaiduAppId, CredentialTarget::BaiduSecret);
    }

    #[test]
    fn display_round_trips_through_as_str() {
        for target in CredentialTarget::ALL {
            assert_eq!(target.as_str(), target.to_string());
        }
    }
}
