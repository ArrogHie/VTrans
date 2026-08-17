//! Shared test helpers for asserting on tracing output.
//!
//! Compiled only for tests (`#[cfg(test)]`) and never part of the public API.
//!
//! A single process-global capturing subscriber is installed on first use
//! (`OnceLock`), so multiple test modules (e.g. `manager` and `dpapi`) can
//! assert on log output without racing to install the global default
//! subscriber.
//!
//! A thread-local `with_default` subscriber is deliberately not used:
//! `tracing` caches callsite interest globally, so while one thread's capture
//! is active another test thread may register the same callsite against the
//! no-op dispatcher and permanently cache `Interest::never()` for it,
//! silently dropping the events under test. A global default is shared by
//! every thread, so callsite interest is always computed against the
//! capturing subscriber and assertions stay deterministic.

use std::sync::{Arc, Mutex, OnceLock};

use tracing_subscriber::fmt;

/// A `MakeWriter` that records everything written to it so tests can assert
/// on log output.
#[derive(Clone, Default)]
struct CapturingWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CapturingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("capture lock should not be poisoned")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl fmt::MakeWriter<'_> for CapturingWriter {
    type Writer = Self;

    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

/// Process-wide buffer that receives every test log event.
static TEST_LOG_BUFFER: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();

/// Installs the process-global capturing subscriber exactly once and returns
/// the shared log buffer.
pub(crate) fn install_test_log_subscriber() -> &'static Arc<Mutex<Vec<u8>>> {
    TEST_LOG_BUFFER.get_or_init(|| {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = fmt()
            .with_writer(CapturingWriter(Arc::clone(&buffer)))
            .with_max_level(tracing::Level::DEBUG)
            .without_time()
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("test subscriber should be installed exactly once");
        buffer
    })
}

/// Clears the shared capture buffer so the next assertions only see events
/// produced afterwards.
pub(crate) fn clear_captured_log() {
    install_test_log_subscriber()
        .lock()
        .expect("capture lock should not be poisoned")
        .clear();
}

/// Returns the captured log as a UTF-8 string.
pub(crate) fn captured_log() -> String {
    String::from_utf8(
        install_test_log_subscriber()
            .lock()
            .expect("capture lock should not be poisoned")
            .clone(),
    )
    .expect("captured log should be valid UTF-8")
}
