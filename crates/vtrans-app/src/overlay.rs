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
use tracing::debug;
use vtrans_core::{PipelineMode, ScreenRegion};

use crate::events::{emit_overlay_hidden, emit_overlay_region};

/// Label of the persistent region overlay window declared in `tauri.conf.json`.
pub(crate) const OVERLAY_WINDOW_LABEL: &str = "overlay";

/// Intended visibility change for the persistent region overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayIntent {
    /// Make the region marker visible.
    Show,
    /// Hide the region marker.
    Hide,
    /// Leave the region marker exactly as it is.
    Keep,
}

/// Session events that drive an overlay visibility decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayEvent {
    /// A region selection was confirmed for the given session mode.
    RegionConfirmed(PipelineMode),
    /// A live session started.
    LiveStarted,
    /// A single capture finished (successfully or with an error).
    SingleCaptureCompleted,
}

/// Whether a stopped session is a pause or a real stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopKind {
    /// The session is paused and may resume; the marker stays visible.
    Pause,
    /// The session is really over; the marker is hidden.
    Stop,
}

/// Resolves the overlay intent for a session event.
///
/// The mode-aware rule is the single source of truth for the overlay
/// lifecycle contract:
///
/// - a region confirmed in live mode (or a live session start) **shows** the
///   persistent marker;
/// - a region confirmed in single mode never shows it, and finishing a
///   single capture **hides** it;
/// - a paused session keeps the marker (see
///   [`overlay_intent_for_stop`](Self::overlay_intent_for_stop)).
pub(crate) fn overlay_intent(event: OverlayEvent) -> OverlayIntent {
    match event {
        OverlayEvent::RegionConfirmed(PipelineMode::LiveRegion) | OverlayEvent::LiveStarted => {
            OverlayIntent::Show
        }
        OverlayEvent::RegionConfirmed(PipelineMode::SingleCapture)
        | OverlayEvent::SingleCaptureCompleted => OverlayIntent::Hide,
    }
}

/// Resolves the overlay intent when a live session is stopped.
///
/// Pausing keeps the marker so the user can resume without re-selecting;
/// a real stop hides it. The caller chooses the kind because the backend
/// cannot distinguish a UI pause from a UI stop by itself.
pub(crate) const fn overlay_intent_for_stop(stop: StopKind) -> OverlayIntent {
    match stop {
        StopKind::Pause => OverlayIntent::Keep,
        StopKind::Stop => OverlayIntent::Hide,
    }
}

/// Applies an overlay intent to the application.
///
/// [`OverlayIntent::Show`] requires the region that should be drawn; hide
/// and keep do not. Failures inside `show_region_overlay` /
/// `hide_region_overlay` are logged there and never propagated, so the
/// overlay can never fail a session command.
pub(crate) fn apply_overlay<R: Runtime>(
    app: &AppHandle<R>,
    intent: OverlayIntent,
    region: Option<&ScreenRegion>,
) {
    match intent {
        OverlayIntent::Show => {
            let Some(region) = region else {
                debug!("overlay show intent without a region; keeping overlay unchanged");
                return;
            };
            show_region_overlay(app, region);
        }
        OverlayIntent::Hide => hide_region_overlay(app),
        OverlayIntent::Keep => debug!("overlay kept unchanged"),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_mode_confirmation_never_shows_the_marker() {
        assert_eq!(
            overlay_intent(OverlayEvent::RegionConfirmed(PipelineMode::SingleCapture)),
            OverlayIntent::Hide
        );
    }

    #[test]
    fn live_mode_confirmation_shows_the_marker() {
        assert_eq!(
            overlay_intent(OverlayEvent::RegionConfirmed(PipelineMode::LiveRegion)),
            OverlayIntent::Show
        );
    }

    #[test]
    fn live_start_shows_the_marker() {
        assert_eq!(
            overlay_intent(OverlayEvent::LiveStarted),
            OverlayIntent::Show
        );
    }

    #[test]
    fn single_capture_finish_hides_the_marker() {
        assert_eq!(
            overlay_intent(OverlayEvent::SingleCaptureCompleted),
            OverlayIntent::Hide
        );
    }

    #[test]
    fn real_stop_hides_the_marker() {
        assert_eq!(overlay_intent_for_stop(StopKind::Stop), OverlayIntent::Hide);
    }

    #[test]
    fn pause_keeps_the_marker() {
        assert_eq!(
            overlay_intent_for_stop(StopKind::Pause),
            OverlayIntent::Keep
        );
    }
}
