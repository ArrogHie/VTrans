//! Tauri application bootstrap helpers.

use std::error::Error;
use std::path::{Path, PathBuf};

use tauri::{App, AppHandle, Builder, Manager, Runtime};
use tauri_plugin_global_shortcut::Builder as GlobalShortcutBuilder;
use tracing::{info, warn};
use tracing_appender::non_blocking::WorkerGuard;
use vtrans_config::ConfigManager;
use vtrans_core::init_logging;

use crate::commands::invoke_handler;
use crate::error::AppError;
use crate::hotkeys::register_hotkeys;
use crate::overlay::OVERLAY_WINDOW_LABEL;
use crate::paths::{migrate_legacy_config_if_needed, resolve_data_root};
use crate::state::AppState;
use crate::tray::{setup_tray, show_main_window};
use crate::window_exclusion::exclude_app_windows_from_capture;

/// Environment variable that enables Debug mode when set to `1`/`true`.
const DEBUG_ENV_VAR: &str = "VTRANS_DEBUG";

/// Command-line flag that enables Debug mode.
const DEBUG_CLI_FLAG: &str = "--debug";

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
/// Log records go to the console and to `{data}/logs` with hourly rotation
/// (five files retained; see [`vtrans_core::init_logging`]). The configured
/// `log_level` is used unless the `RUST_LOG` environment variable overrides
/// it.
///
/// Returns `None` when the subscriber is already initialized (for example by
/// an embedding host or a test harness); the application continues without
/// file logging in that case instead of failing to start.
fn init_app_logging(data_root: &Path, level: &str) -> Option<WorkerGuard> {
    let log_dir = data_root.join("logs");
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
/// Returns an application error when the executable location, paths,
/// configuration, capture, or shortcuts fail to initialize. Model problems
/// never fail startup: provisioning runs inside [`AppState::new_with_debug`]
/// and degrades to a recorded status plus placeholder providers.
#[tracing::instrument(skip_all)]
pub fn init_app(app: &mut App<tauri::Wry>) -> Result<(), AppError> {
    // R1: all mutable state lives under a single portable `{exe}/data` root.
    // A legacy roaming `config.json` is migrated in once on first boot.
    let data_root = resolve_data_root()?;
    migrate_legacy_config_if_needed(&data_root);
    let config = ConfigManager::new(&data_root)
        .and_then(|manager| manager.load())
        .map_err(AppError::from)?;
    if let Some(guard) = init_app_logging(&data_root, &config.log_level) {
        app.manage(LoggingGuard(guard));
    }
    let debug_mode = parse_debug_mode();
    info!(debug_mode, data_root = %data_root.display(), "debug mode flag resolved");
    // R2: locate the read-only bundled model source (resource dir first,
    // source checkout as a dev fallback).
    let bundled_models = bundled_models_dir(app);
    if let Some(dir) = &bundled_models {
        info!(bundled_models = %dir.display(), "bundled model source located");
    } else {
        warn!("bundled model source unavailable; model provisioning is disabled for this run");
    }
    let state = AppState::new_with_debug(&data_root, bundled_models.as_deref(), debug_mode)?;
    app.manage(state);
    app.state::<AppState>().attach_handle(app.handle().clone());
    setup_tray(app.handle())?;
    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        // The overlay is a pure visual marker and must never intercept mouse
        // input; the v2 window configuration has no click-through field, so
        // it is enabled here once at startup.
        if let Err(error) = window.set_ignore_cursor_events(true) {
            tracing::warn!(error = %error, "failed to enable overlay click-through");
        }
    }
    // Bug-006: VTrans captures whole monitors with WGC, so its own windows
    // must be excluded from capture or the translator translates its own
    // UI. Runs once after all configured windows exist; best-effort only
    // (failures are logged inside and never fail startup).
    exclude_app_windows_from_capture(app.handle());
    register_hotkeys(app.handle())?;
    Ok(())
}

/// Locates the read-only bundled model source (`resource_dir()/resources/models`).
///
/// On Windows the Tauri resource directory is the executable directory in
/// both dev (tauri-build copies the resources there) and production (NSIS
/// places them next to the exe), so `{resource_dir}/resources/models` works
/// for both. A source-checkout fallback keeps `cargo run`-style flows alive
/// when the resource copy is stale.
fn bundled_models_dir(app: &App<tauri::Wry>) -> Option<PathBuf> {
    let resource_models = app
        .path()
        .resource_dir()
        .ok()?
        .join("resources")
        .join("models");
    if resource_models.join("manifest.json").exists() {
        return Some(resource_models);
    }
    let checkout_models = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("src-tauri")
        .join("resources")
        .join("models");
    if checkout_models.join("manifest.json").exists() {
        return Some(checkout_models);
    }
    None
}

/// Resolves whether Debug mode is enabled for this run.
///
/// Debug mode is enabled by the `--debug` command-line flag or the
/// `VTRANS_DEBUG=1` environment variable. It is never persisted. Malformed
/// values fall back to disabled with a warning instead of failing startup.
fn parse_debug_mode() -> bool {
    let from_cli = std::env::args().any(|argument| argument == DEBUG_CLI_FLAG);
    let from_env = std::env::var(DEBUG_ENV_VAR).is_ok_and(|value| parse_debug_env_value(&value));
    from_cli || from_env
}

/// Parses the `VTRANS_DEBUG` environment variable value.
///
/// `1`/`true` (case-insensitive) enable Debug mode; anything else disables
/// it. Kept as a pure function so the accepted values are unit-testable.
fn parse_debug_env_value(value: &str) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => true,
        other => {
            if !other.is_empty() {
                warn!(
                    value = other,
                    "invalid VTRANS_DEBUG value; debug mode disabled"
                );
            }
            false
        }
    }
}

/// Builds the Tauri builder used by the desktop entry point.
///
/// The returned builder installs the global shortcut plugin, registers all
/// IPC commands, and initializes application state during setup.
pub fn builder() -> Builder<tauri::Wry> {
    Builder::default()
        .plugin(GlobalShortcutBuilder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second process must not run: it would fight for the global
            // shortcuts. Restore the existing instance's main window instead.
            show_main_window(app);
        }))
        .invoke_handler(invoke_handler())
        .setup(|app| init_app(app).map_err(|error| -> Box<dyn Error> { Box::new(error) }))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Every VTrans window is hidden rather than destroyed: the
                // main window keeps the process alive (live sessions, global
                // shortcuts, tray restore) and the user can quit from the
                // tray menu; the auxiliary windows must stay alive so
                // `get_webview_window` can restore them later.
                api.prevent_close();
                if let Err(error) = window.hide() {
                    tracing::warn!(
                        label = window.label(),
                        error = %error,
                        "failed to hide window on close request"
                    );
                }
            }
        })
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

    #[test]
    fn debug_mode_resolution_accepts_flag_and_env_values() {
        let cases = [
            ("1", true),
            ("true", true),
            ("TRUE", true),
            ("0", false),
            ("false", false),
            ("", false),
            ("maybe", false),
        ];
        for (value, expected) in cases {
            assert_eq!(parse_debug_env_value(value), expected, "value: {value}");
        }
    }
}
