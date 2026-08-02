//! Tauri application bootstrap helpers.

use std::error::Error;

use tauri::{App, AppHandle, Builder, Manager, Runtime};
use tauri_plugin_global_shortcut::Builder as GlobalShortcutBuilder;

use crate::commands::invoke_handler;
use crate::error::AppError;
use crate::hotkeys::register_hotkeys;
use crate::state::AppState;

/// Initializes application state for an already-created Tauri application.
///
/// The global-shortcut plugin must be installed before this function is
/// called. Tauri runtime setup is kept here so the desktop entry point can
/// remain a thin wrapper.
///
/// # Errors
///
/// Returns an application error when paths, providers, or shortcuts fail to initialize.
pub fn init_app(app: &mut App<tauri::Wry>) -> Result<(), AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Tauri(error.to_string()))?;
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
