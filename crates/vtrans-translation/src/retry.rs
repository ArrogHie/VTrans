//! Bounded retry policy with exponential backoff.

use std::time::Duration;

use vtrans_core::TranslationError;

/// Default initial backoff used by [`RetryPolicy::new`].
pub const DEFAULT_INITIAL_BACKOFF: Duration = Duration::from_secs(1);
/// Default maximum backoff used by [`RetryPolicy::new`].
pub const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(8);

/// Policy for retrying transient API translation failures.
///
/// Backoff follows the sequence `initial * 2^n` and is capped at
/// `max_backoff`. Rate limits, timeouts, and generic API request failures
/// are retryable; authentication and parsing errors are not.
///
/// # Example
///
/// ```
/// use std::time::Duration;
/// use vtrans_core::TranslationError;
/// use vtrans_translation::RetryPolicy;
///
/// let policy = RetryPolicy::new(3);
/// assert_eq!(policy.backoff_duration(0), Duration::from_secs(1));
/// assert!(policy.should_retry(&TranslationError::RateLimited, 0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    max_retries: u32,
    initial_backoff: Duration,
    max_backoff: Duration,
}

impl RetryPolicy {
    /// Create a policy with the default 1s/2s/4s/8s exponential backoff.
    ///
    /// # Arguments
    ///
    /// * `max_retries` - Maximum number of retries after the first attempt.
    #[must_use]
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            initial_backoff: DEFAULT_INITIAL_BACKOFF,
            max_backoff: DEFAULT_MAX_BACKOFF,
        }
    }

    /// Configure custom backoff bounds for this policy.
    ///
    /// # Arguments
    ///
    /// * `initial_backoff` - Delay before the first retry.
    /// * `max_backoff` - Upper bound applied to every retry delay.
    #[must_use]
    pub fn with_limits(self, initial_backoff: Duration, max_backoff: Duration) -> Self {
        Self {
            initial_backoff,
            max_backoff,
            ..self
        }
    }

    /// Return the maximum number of retries.
    #[must_use]
    pub const fn max_retries(self) -> u32 {
        self.max_retries
    }

    /// Return the delay before retry `retry_index`.
    ///
    /// `retry_index` is zero-based: `0` is the delay before the first retry.
    ///
    /// # Arguments
    ///
    /// * `retry_index` - Number of retries already attempted.
    #[must_use]
    pub fn backoff_duration(self, retry_index: u32) -> Duration {
        let multiplier = 1_u32.checked_shl(retry_index).unwrap_or(u32::MAX);
        self.initial_backoff
            .saturating_mul(multiplier)
            .min(self.max_backoff)
    }

    /// Return whether `error` should be retried after `retry_index` attempts.
    ///
    /// # Arguments
    ///
    /// * `error` - Error returned by the previous attempt.
    /// * `retry_index` - Number of retries already attempted.
    #[must_use]
    pub fn should_retry(self, error: &TranslationError, retry_index: u32) -> bool {
        retry_index < self.max_retries && is_retryable(error)
    }
}

/// Return `true` for transient errors that can be retried safely.
#[must_use]
pub fn is_retryable(error: &TranslationError) -> bool {
    matches!(
        error,
        TranslationError::ApiRequest(_)
            | TranslationError::Timeout(_)
            | TranslationError::RateLimited
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_backoff_is_exponential() {
        let policy = RetryPolicy::new(3);
        assert_eq!(policy.backoff_duration(0), Duration::from_secs(1));
        assert_eq!(policy.backoff_duration(1), Duration::from_secs(2));
        assert_eq!(policy.backoff_duration(2), Duration::from_secs(4));
        assert_eq!(policy.backoff_duration(3), Duration::from_secs(8));
    }

    #[test]
    fn custom_backoff_is_capped() {
        let policy =
            RetryPolicy::new(5).with_limits(Duration::from_millis(10), Duration::from_millis(30));
        assert_eq!(policy.backoff_duration(0), Duration::from_millis(10));
        assert_eq!(policy.backoff_duration(1), Duration::from_millis(20));
        assert_eq!(policy.backoff_duration(2), Duration::from_millis(30));
        assert_eq!(policy.backoff_duration(10), Duration::from_millis(30));
    }

    #[test]
    fn retries_are_bounded_by_max_retries() {
        let policy = RetryPolicy::new(2);
        assert!(policy.should_retry(&TranslationError::RateLimited, 0));
        assert!(policy.should_retry(&TranslationError::RateLimited, 1));
        assert!(!policy.should_retry(&TranslationError::RateLimited, 2));
    }

    #[test]
    fn retryable_error_classes() {
        assert!(is_retryable(&TranslationError::ApiRequest("boom".into())));
        assert!(is_retryable(&TranslationError::Timeout(
            Duration::from_secs(1)
        )));
        assert!(is_retryable(&TranslationError::RateLimited));
    }

    #[test]
    fn non_retryable_error_classes() {
        assert!(!is_retryable(&TranslationError::Unauthorized));
        assert!(!is_retryable(&TranslationError::Cancelled));
        assert!(!is_retryable(&TranslationError::ParseResponse(
            "bad".into()
        )));
        assert!(!is_retryable(&TranslationError::ModelLoad("bad".into())));
        assert!(!is_retryable(&TranslationError::Inference("bad".into())));
    }
}
