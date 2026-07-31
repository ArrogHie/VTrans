//! Log-safe masking of API keys.
//!
//! [`mask_key`] is a thin wrapper around [`vtrans_core::mask_sensitive`] so
//! the whole workspace shares a single masking implementation and format
//! (4-character prefix + `****` + 4-character suffix for keys longer than 8
//! characters, otherwise `****`).

use vtrans_core::mask_sensitive;

/// Mask an API key so it is safe to include in logs.
///
/// Keys of 8 characters or fewer are fully masked (`"****"`). Longer keys are
/// shown as `<first 4 chars>****<last 4 chars>`, e.g. `sk-1****cdef`. The
/// middle of the key is never recoverable from the output.
///
/// # Example
///
/// ```
/// use vtrans_security::mask_key;
///
/// assert_eq!(mask_key("short"), "****");
/// let masked = mask_key("sk-123456789012");
/// assert!(masked.starts_with("sk-1"));
/// assert!(masked.ends_with("9012"));
/// assert!(masked.contains("****"));
/// ```
#[must_use]
pub fn mask_key(key: &str) -> String {
    mask_sensitive(key)
}

#[cfg(test)]
mod tests {
    use super::mask_key;

    /// A 12-character key must keep only its first and last 4 characters
    /// visible. The spec's `sk-****1234` shape maps to `sk-1****1234` under
    /// the shared 4+4 masking format of `vtrans_core::mask_sensitive`.
    #[test]
    fn mask_key_12_char_key_shows_prefix_and_suffix() {
        let masked = mask_key("sk-1abcd1234");
        assert_eq!(masked, "sk-1****1234");
    }

    #[test]
    fn mask_key_short_key_is_fully_masked() {
        assert_eq!(mask_key("1234567"), "****");
        assert_eq!(mask_key("12345678"), "****");
    }

    #[test]
    fn mask_key_empty_key_is_fully_masked() {
        assert_eq!(mask_key(""), "****");
    }

    #[test]
    fn mask_key_never_leaks_middle_of_key() {
        let key = "sk-verysecretmiddle-abcdef";
        let masked = mask_key(key);
        assert!(!masked.contains("verysecret"));
        assert_eq!(masked, "sk-v****cdef");
    }

    #[test]
    fn mask_key_never_contains_the_full_key() {
        let key = "sk-0123456789abcdef";
        let masked = mask_key(key);
        assert!(!masked.contains(key));
        assert_ne!(masked, key);
    }

    #[test]
    fn mask_key_handles_unicode() {
        let key = "秘密の鍵123456789";
        let masked = mask_key(key);
        assert!(masked.starts_with("秘密の鍵"));
        assert!(masked.ends_with("6789"));
        assert!(masked.contains("****"));
        assert!(!masked.contains("1234567"));
    }

    /// For any input length the masked output must never contain the full
    /// key, and the middle section must always be masked.
    #[test]
    fn mask_key_never_reveals_full_key_for_various_lengths() {
        for len in 0..=20 {
            let key = "x".repeat(len);
            let masked = mask_key(&key);
            assert!(
                !masked.contains(&key) || key.is_empty(),
                "masked output for {len}-char key must not contain the full key"
            );
            assert!(
                masked.contains("****"),
                "masked output must always contain the mask"
            );
        }
    }
}
