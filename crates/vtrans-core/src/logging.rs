use std::path::Path;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, EnvFilter, prelude::*};

/// Initialize tracing with console + rolling file output.
///
/// Uses hourly rotation with a maximum of 5 log files retained.
/// (tracing-appender does not support size-based rotation; hourly is the
/// closest practical alternative to the spec's original "10MB rotation".)
///
/// Returns a WorkerGuard that must be kept alive for the application lifetime.
/// Dropping the guard may cause async log writes to be lost.
///
/// # Arguments
/// * `log_dir` - Directory for log files
/// * `level` - Log level filter string, e.g. "info", "debug"
pub fn init_logging(log_dir: &Path, level: &str) -> Result<WorkerGuard, std::io::Error> {
    std::fs::create_dir_all(log_dir)?;

    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::Hourly)
        .filename_prefix("vtrans")
        .filename_suffix("log")
        .max_log_files(5)
        .build(log_dir)?;

    let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_writer(std::io::stdout),
        )
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(non_blocking_file),
        )
        .init();

    Ok(guard)
}

/// Mask a sensitive string for safe logging.
/// Returns "sk-****1234" format for keys, or "****" for short strings.
pub fn mask_sensitive(s: &str) -> String {
    if s.len() <= 8 {
        "****".to_string()
    } else {
        let prefix = &s[..s.len().min(4)];
        let suffix = &s[s.len().saturating_sub(4)..];
        format!("{prefix}****{suffix}")
    }
}

/// Truncate text for safe logging (max 20 chars + "...")
pub fn truncate_for_log(s: &str) -> String {
    const MAX_LEN: usize = 20;
    if s.chars().count() <= MAX_LEN {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(MAX_LEN).collect();
        format!("{truncated}...")
    }
}

[cfg(test)]
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
    fn truncate_short_text() {
        assert_eq!(truncate_for_log("hello"), "hello");
    }

    #[test]
    fn truncate_long_text() {
        let long = "a".repeat(100);
        let truncated = truncate_for_log(&long);
        assert!(truncated.ends_with("..."));
        assert_eq!(truncated.chars().count(), 23);
    }
}
