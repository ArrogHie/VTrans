//! System tray icon and window lifecycle control.
//!
//! Closing the main window hides it to the tray instead of exiting, so a
//! live translation session and its global shortcuts keep running in the
//! background. The tray menu restores the main window or quits the
//! application; quitting is the only intentional exit path and releases all
//! registered global shortcuts with the process.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};
use tracing::info;

use crate::error::AppError;

/// Label of the main application window.
pub(crate) const MAIN_WINDOW_LABEL: &str = "main";

/// Creates the tray icon with a show/quit menu.
///
/// The tray icon reuses the bundled application icon. Left-click restores
/// the main window; the right-click menu offers the same action plus a
/// definite quit entry.
///
/// # Errors
///
/// Returns an application error when the icon resource, menu, or tray icon
/// cannot be created.
#[tracing::instrument(skip(app))]
pub(crate) fn setup_tray<R: Runtime>(app: &AppHandle<R>) -> Result<(), AppError> {
    let show_item = MenuItem::with_id(app, SHOW_MAIN_ITEM_ID, "显示主窗口", true, None::<&str>)
        .map_err(|error| AppError::Tauri(error.to_string()))?;
    let quit_item = MenuItem::with_id(app, QUIT_ITEM_ID, "退出", true, None::<&str>)
        .map_err(|error| AppError::Tauri(error.to_string()))?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])
        .map_err(|error| AppError::Tauri(error.to_string()))?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| AppError::Tauri("default window icon is missing".to_string()))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("VTrans 屏幕翻译")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id().0.as_str() {
            SHOW_MAIN_ITEM_ID => show_main_window(app),
            QUIT_ITEM_ID => {
                info!("quit requested from tray menu");
                app.exit(0);
            }
            _ => {}
        })
        .build(app)
        .map_err(|error| AppError::Tauri(error.to_string()))?;
    info!("tray icon created");
    Ok(())
}

/// Shows and focuses the main window.
///
/// Used by the tray menu, a left-click on the tray icon, and the
/// single-instance plugin when a second process tries to start.
pub(crate) fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        tracing::warn!("main window is not configured");
        return;
    };
    if let Err(error) = window.show().and_then(|()| window.set_focus()) {
        tracing::warn!(error = %error, "failed to restore main window");
    }
}

const TRAY_ID: &str = "vtrans-tray";
const SHOW_MAIN_ITEM_ID: &str = "show-main";
const QUIT_ITEM_ID: &str = "quit";
