//! Tauri application bootstrap helpers.

use std::error::Error;
use std::path::Path;

use tauri::{App, AppHandle, Builder, Manager, Runtime};
use tauri_plugin_global_shortcut::Builder as GlobalShortcutBuilder;
use tracing::{info, warn};
use tracing_appender::non_blocking::WorkerGuard;
use vtrans_config::ConfigManager;
use vtrans_core::init_logging;

use crate::commands::invoke_handler;
use crate::error::AppError;
use crate::hotkeys::register_hotkeys;
use crate::state::AppState;

/// Keeps the non-blocking tracing writer alive for the application lifetime.
///
/// Tauri manages this value as inert state; the guard's `Drop` implementation
/// flushes pending records and shuts down the writer thread at exit.
pub(crate) struct LoggingGuard(
    /// Never read; the guard is held solely so its `Drop` implementation runs
    /// when Tauri drops the managed state at application exit.
    #[allow(dead_code)]
    WorkerGuard,
);

/// Initializes the shared tracing subscriber for the application.
///
/// Log records go to the console and to `app_data_dir/logs` with hourly
/// rotation (five files retained; see [`vtrans_core::init_logging`]). The
/// configured `log_level` is used unless the `RUST_LOG` environment variable
/// overrides it.
///
/// Returns `None` when the subscriber is already initialized (for example by
/// an embedding host or a test harness); the application continues without
/// file logging in that case instead of failing to start.
fn init_app_logging(app_data_dir: &Path, level: &str) -> Option<WorkerGuard> {
    let log_dir = app_data_dir.join("logs");
    match init_logging(&log_dir, level) {
        Ok(guard) => {
            info!(log_dir = %log_dir.display(), level, "tracing initialized");
            Some(guard)
        }
        Err(error) => {
            warn!(
                error = %error,
                log_dir = %log_dir.display(),
                "tracing initialization failed; continuing without file logging"
            );
            None
        }
    }
}

/// Initializes application state for an already-created Tauri application.
///
/// The global-shortcut plugin must be installed before this function is
/// called. Tauri runtime setup is kept here so the desktop entry point can
/// remain a thin wrapper.
///
/// Logging is configured before any state is created so that startup
/// diagnostics are captured by the rolling file writer (see §5.1 of
/// `docs/ARCHITECTURE.md`).
///
/// # Errors
///
/// Returns an application error when paths, providers, or shortcuts fail to initialize.
#[tracing::instrument(skip_all)]
pub fn init_app(app: &mut App<tauri::Wry>) -> Result<(), AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Tauri(error.to_string()))?;
    let config = ConfigManager::new(&app_data_dir)
        .and_then(|manager| manager.load())
        .map_err(AppError::from)?;
    if let Some(guard) = init_app_logging(&app_data_dir, &config.log_level) {
        app.manage(LoggingGuard(guard));
    }
    let state = AppState::new(&app_data_dir)?;
    app.manage(state);
    app.state::<AppState>().attach_handle(app.handle().clone());
    register_hotkeys(app.handle())?;
    Ok(())
}

/// Builds the Tauri builder used by the desktop entry point.
///
/// The returned builder installs the global shortcut plugin, registers all
/// IPC commands, and initializes application state during setup.
pub fn builder() -> Builder<tauri::Wry> {
    Builder::default()
        .plugin(GlobalShortcutBuilder::new().build())
        .invoke_handler(invoke_handler())
        .setup(|app| init_app(app).map_err(|error| -> Box<dyn Error> { Box::new(error) }))
}

/// Returns the application handle from a Tauri setup callback.
#[must_use]
pub fn app_handle<R: Runtime>(app: &App<R>) -> AppHandle<R> {
    app.handle().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_initializes_once_and_tolerates_duplicate_setup() {
        let dir =
            std::env::temp_dir().join(format!("vtrans-app-logging-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let first = init_app_logging(&dir, "info");
        assert!(
            first.is_some(),
            "the first initialization must install a subscriber"
        );

        // A second initialization must not panic or fail startup: the core
        // helper rejects a duplicate global subscriber and setup degrades
        // gracefully to running without file logging.
        let second = init_app_logging(&dir, "debug");
        assert!(
            second.is_none(),
            "a duplicate initialization must be tolerated"
        );

        drop(first);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
