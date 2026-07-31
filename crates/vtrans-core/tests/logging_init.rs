//! Integration tests for logging initialization.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use vtrans_core::init_logging;

/// Returns a unique temporary directory for this test process.
fn unique_log_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("vtrans-core-log-{nanos}"))
}

#[test]
fn init_logging_installs_subscriber_and_writes_log_file() {
    let log_dir = unique_log_dir();

    let guard = init_logging(&log_dir, "info").expect("init_logging should succeed");

    tracing::info!("integration test log line");

    // A second initialization must return an error instead of panicking on
    // the already-installed global subscriber.
    let second = init_logging(&unique_log_dir(), "info");
    assert!(second.is_err(), "second init_logging call should fail");

    // Dropping the guard flushes pending writes through the non-blocking
    // worker, ensuring the rolling appender has created its log file.
    drop(guard);

    assert!(log_dir.is_dir(), "log directory should exist");

    let files: Vec<_> = std::fs::read_dir(&log_dir)
        .expect("log directory should be readable")
        .filter_map(Result::ok)
        .collect();
    assert!(
        !files.is_empty(),
        "at least one rolling log file should exist"
    );
}
