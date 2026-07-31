//! Logging initialization and sensitive-data utilities.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialize tracing with console + rolling file output.
///
/// Sets up a console layer (ANSI colors) and a non-blocking file layer
/// (hourly rotation, max 5 files). The returned [`WorkerGuard`] must be
/// kept alive for the application lifetime.
///
/// # Arguments
/// * `log_dir` - Directory for log files (created if missing).
/// * `level` - Log level filter, e.g. `"info"`, `"debug"`.
///
/// # Errors
/// Returns `io::Error` if the directory cannot be created or the file
/// appender cannot be initialized.
#[tracing::instrument(skip(log_dir))]
pub fn init_logging(log_dir: &Path, level: &str) -> Result<WorkerGuard, std::io::Error> {
    std::fs::create_dir_all(log_dir)?;

    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::HOURLY)
        .filename_prefix("vtrans")
        .filename_suffix("log")
        .max_log_files(5)
        .build(log_dir)
        .map_err(std::io::Error::other)?;

    let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_writer(std::io::stdout),
        )
        .with(fmt::layer().with_ansi(false).with_writer(non_blocking_file))
        .init();

    Ok(guard)
}

/// Mask a sensitive string for safe logging.
///
/// Returns `"sk-****1234"` format for keys (prefix + `****` + suffix),
/// or `"****"` for strings of 8 characters or fewer.
///
/// # Example
/// ```
/// # use vtrans_core::mask_sensitive;
/// assert_eq!(mask_sensitive("abc"), "****");
/// assert_eq!(mask_sensitive("sk-1234567890abcdef"), "sk-1****cdef");
/// ```
#[must_use]
pub fn mask_sensitive(s: &str) -> String {
    if s.len() <= 8 {
        "****".to_string()
    } else {
        let prefix = &s[..s.len().min(4)];
        let suffix = &s[s.len().saturating_sub(4)..];
        format!("{prefix}****{suffix}")
    }
}

/// Truncate text for safe logging (max 20 chars + `"..."`).
///
/// # Example
/// ```
/// # use vtrans_core::truncate_for_log;
/// assert_eq!(truncate_for_log("hello"), "hello");
/// assert_eq!(truncate_for_log(&"a".repeat(100)), "aaaaaaaaaaaaaaaaaaaa...");
/// ```
#[must_use]
pub fn truncate_for_log(s: &str) -> String {
    const MAX_LEN: usize = 20;
    if s.chars().count() <= MAX_LEN {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(MAX_LEN).collect();
        format!("{truncated}...")
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_short_string() {
        assert_eq!(mask_sensitive("abc"), "****");
    }

    #[test]
    fn mask_long_string() {
        let masked = mask_sensitive("sk-1234567890abcdef");
        assert!(masked.starts_with("sk-1"));
        assert!(masked.ends_with("cdef"));
        assert!(masked.contains("****"));
    }

    #[test]
    fn mask_exact_boundary() {
        assert_eq!(mask_sensitive("12345678"), "****");
    }

    #[test]
    fn mask_just_over_boundary() {
        let masked = mask_sensitive("123456789");
        assert!(masked.contains("****"));
        assert!(masked.starts_with("1234"));
        assert!(masked.ends_with("6789"));
    }

    #[test]
    fn truncate_short_text() {
        assert_eq!(truncate_for_log("hello"), "hello");
    }

    #[test]
    fn truncate_exact_20_chars() {
        let s = "a".repeat(20);
        assert_eq!(truncate_for_log(&s), s);
    }

    #[test]
    fn truncate_long_text() {
        let long = "a".repeat(100);
        let truncated = truncate_for_log(&long);
        assert!(truncated.ends_with("..."));
        assert_eq!(truncated.chars().count(), 23);
    }

    #[test]
    fn truncate_unicode() {
        let s = "日本語テスト".repeat(10);
        let truncated = truncate_for_log(&s);
        assert!(truncated.ends_with("..."));
    }
}
