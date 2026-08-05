//! Persistent selection-region overlay window management.
//!
//! The overlay is a borderless, transparent, always-on-top, click-through
//! window that draws the currently selected capture region on the screen.
//! It stays visible after the selector window closes so the user always sees
//! which part of the screen is being translated. The webview draws the border
//! with pure CSS; only region coordinates cross the IPC boundary, never image
//! data. The window is declared as non-focusable (`focusable: false`) so
//! showing it never steals keyboard focus from the application being
//! translated.

use tauri::{AppHandle, Manager, Runtime};
use vtrans_core::ScreenRegion;

use crate::events::{emit_overlay_hidden, emit_overlay_region};

/// Label of the persistent region overlay window declared in `tauri.conf.json`.
pub(crate) const OVERLAY_WINDOW_LABEL: &str = "overlay";

/// Positions the overlay window on the monitor the region belongs to and
/// emits the region for the webview to draw.
///
/// The overlay is placed exactly over the target monitor and sized to it;
/// `ScreenRegion` coordinates are physical pixels relative to that monitor,
/// so the webview only needs to divide by its device pixel ratio.
///
/// When the monitor cannot be resolved the overlay is hidden and the failure
/// is logged; a stale marker is worse than no marker.
#[tracing::instrument(skip(app), fields(monitor_id = %region.monitor_id))]
pub(crate) fn show_region_overlay<R: Runtime>(app: &AppHandle<R>, region: &ScreenRegion) {
    let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
        tracing::warn!("overlay window is not configured");
        return;
    };
    let monitor = match app.available_monitors() {
        Ok(monitors) => monitors.into_iter().find(|monitor| {
            monitor
                .name()
                .is_some_and(|name| name == &region.monitor_id)
        }),
        Err(error) => {
            tracing::warn!(error = %error, "failed to enumerate monitors for overlay");
            None
        }
    };
    let Some(monitor) = monitor else {
        tracing::warn!(
            monitor_id = %region.monitor_id,
            "overlay monitor not found; hiding overlay"
        );
        hide_region_overlay(app);
        return;
    };
    let result = window
        .set_position(tauri::Position::Physical(*monitor.position()))
        .and_then(|()| window.set_size(tauri::Size::Physical(*monitor.size())))
        .and_then(|()| window.show());
    if let Err(error) = result {
        tracing::warn!(error = %error, "failed to position or show overlay window");
        return;
    }
    emit_overlay_region(app, region);
}

/// Hides the overlay window and tells its webview to clear the marker.
///
/// Idempotent: the window and event are optional, and hiding an already
/// hidden window is a no-op.
#[tracing::instrument(skip(app))]
pub(crate) fn hide_region_overlay<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        if let Err(error) = window.hide() {
            tracing::warn!(error = %error, "failed to hide overlay window");
        }
    }
    emit_overlay_hidden(app);
}
