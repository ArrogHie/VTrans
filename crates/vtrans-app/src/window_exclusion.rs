//! Windows Graphics Capture exclusion for `VTrans` windows (Bug-006).
#![allow(unsafe_code)] // The Win32 affinity call requires unsafe; the single block below has a SAFETY comment.
//!
//! `VTrans` captures whole monitors with Windows Graphics Capture
//! (`CreateForMonitor` in `vtrans-capture`), so every window on the display —
//! including `VTrans`' own windows — appears in the captured frames and can be
//! translated back into itself. `SetWindowDisplayAffinity` with
//! `WDA_EXCLUDEFROMCAPTURE` removes a window from every capture surface
//! (verified on Windows 11 on 2026-08-14: an excluded window disappears
//! entirely from WGC monitor frames and the background behind it shows
//! through).
//!
//! The decision which windows need exclusion is the pure
//! [`capture_exclusion_windows`] function; the per-window fault-tolerant
//! loop [`apply_capture_exclusions`] is generic over the handle type so the
//! "one failed window must not abort the remaining windows" contract is
//! unit-testable. Only the thin [`exclude_app_windows_from_capture`] entry
//! point touches the Tauri runtime and the Win32 call.

use tauri::{AppHandle, Manager, Runtime};
use tracing::{info, warn};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};

/// Window label of the main application window.
const MAIN_WINDOW_LABEL: &str = "main";
/// Window label of the result (translation popup) window.
const RESULT_WINDOW_LABEL: &str = "result";
/// Window label of the floating ball window.
const FLOATER_WINDOW_LABEL: &str = "floater";

/// Returns the window labels that must be excluded from screen capture.
///
/// `main`, `result`, and `floater` render translated text or application
/// chrome on top of the display and must never appear in captured frames.
/// `selector` and `overlay` are intentionally not listed: the selector is
/// only visible for the split second of a region drag (no capture runs in
/// that window) and the overlay draws its border outside the captured area,
/// so neither window needs `WDA_EXCLUDEFROMCAPTURE`.
#[must_use]
pub(crate) const fn capture_exclusion_windows() -> [&'static str; 3] {
    [MAIN_WINDOW_LABEL, RESULT_WINDOW_LABEL, FLOATER_WINDOW_LABEL]
}

/// Applies a best-effort per-window exclusion operation.
///
/// `entries` pairs each window label with its resolved native handle;
/// `set_affinity` performs the actual per-window operation. Every window is
/// processed independently: a failed call is logged as a warning with the
/// window label (never window content) and the loop continues with the next
/// entry, so one failure can never prevent the remaining windows from being
/// excluded. This function itself never fails.
///
/// The handle type is generic so the fault-tolerance contract is
/// unit-testable with plain integers instead of real window handles.
pub(crate) fn apply_capture_exclusions<H>(
    entries: impl IntoIterator<Item = (&'static str, H)>,
    mut set_affinity: impl FnMut(&'static str, H) -> Result<(), String>,
) {
    for (label, handle) in entries {
        match set_affinity(label, handle) {
            Ok(()) => info!(
                label,
                "window excluded from capture (WDA_EXCLUDEFROMCAPTURE)"
            ),
            Err(error) => warn!(
                label,
                error = %error,
                "failed to exclude window from capture; the window may appear in captured frames"
            ),
        }
    }
}

/// Excludes the `VTrans` application windows from screen capture at startup.
///
/// Must run once after the Tauri windows are created (the `setup` phase).
/// Without it, monitor-level WGC frames contain the main/result/floater
/// windows and the translator translates its own output. The exclusion is
/// best-effort: a missing window, a failed handle lookup, or a failed Win32
/// call is logged as a warning and never fails startup.
///
/// # Side effects
///
/// `WDA_EXCLUDEFROMCAPTURE` hides the marked windows from **every** capture
/// surface (screen sharing, third-party screenshots, recording tools, ...),
/// not just from `VTrans`. The user accepted this trade-off; it is documented
/// in the crate README.
#[tracing::instrument(skip(app))]
pub(crate) fn exclude_app_windows_from_capture<R: Runtime>(app: &AppHandle<R>) {
    let resolved = resolved_exclusion_windows(app);
    apply_capture_exclusions(resolved, exclude_hwnd_from_capture);
}

/// Resolves the exclusion decision to `(label, HWND)` pairs, skipping
/// windows that are missing or cannot yield a native handle.
///
/// Resolution failures are tolerated (the application still starts): a
/// missing window or a failed handle lookup is logged and simply leaves the
/// window in the capture.
fn resolved_exclusion_windows<R: Runtime>(app: &AppHandle<R>) -> Vec<(&'static str, HWND)> {
    let mut resolved = Vec::new();
    for label in capture_exclusion_windows() {
        let Some(window) = app.get_webview_window(label) else {
            warn!(
                label,
                "window is not configured; skipping capture exclusion"
            );
            continue;
        };
        match window.hwnd() {
            Ok(hwnd) => resolved.push((label, hwnd)),
            Err(error) => warn!(
                label,
                error = %error,
                "failed to resolve the native window handle; skipping capture exclusion"
            ),
        }
    }
    resolved
}

/// Sets `WDA_EXCLUDEFROMCAPTURE` on one native window handle.
///
/// `label` is unused here and only exists so the fault-tolerant loop can
/// log it. The error is returned as a displayable string instead of a
/// structured type because the loop treats every failure uniformly.
fn exclude_hwnd_from_capture(_label: &str, hwnd: HWND) -> Result<(), String> {
    // SAFETY: `hwnd` comes from the Tauri runtime (`WebviewWindow::hwnd`)
    // for a window declared in `tauri.conf.json` that is never destroyed
    // (every close request is converted into a hide), so the handle stays
    // valid for the whole process lifetime. The affinity value is the
    // documented `WDA_EXCLUDEFROMCAPTURE` constant and the call only
    // changes the capture visibility of that one window.
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }
        .map_err(|error| format!("SetWindowDisplayAffinity failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusion_set_contains_exactly_main_result_and_floater() {
        // Bug-006: only the three windows that render translated content or
        // application chrome need WDA. The selector is visible only while a
        // region drag is in progress (no capture runs then) and the overlay
        // draws its border outside the captured area, so neither may enter
        // the set.
        let set = capture_exclusion_windows();
        assert_eq!(set, ["main", "result", "floater"]);
        assert!(!set.contains(&"selector"));
        assert!(!set.contains(&"overlay"));
    }

    #[test]
    fn one_failed_window_does_not_abort_the_remaining_exclusions() {
        // The "result" window fails its affinity call; the loop must still
        // attempt "floater" and process every entry exactly once, in order.
        let entries = [("main", 1u32), ("result", 2u32), ("floater", 3u32)];
        let mut attempts = Vec::new();
        apply_capture_exclusions(entries, |label, handle| {
            attempts.push((label, handle));
            if label == "result" {
                Err("mock SetWindowDisplayAffinity failure".to_string())
            } else {
                Ok(())
            }
        });
        assert_eq!(attempts, [("main", 1), ("result", 2), ("floater", 3)]);
    }

    #[test]
    fn exclusion_loop_tolerates_multiple_failures() {
        // Even when every window fails, the loop reports every attempt and
        // never panics or returns an error.
        let entries = [("main", 1u32), ("result", 2u32), ("floater", 3u32)];
        let mut attempts = Vec::new();
        apply_capture_exclusions(entries, |label, _handle| {
            attempts.push(label);
            Err(format!("mock failure for {label}"))
        });
        assert_eq!(attempts, ["main", "result", "floater"]);
    }

    #[test]
    fn exclusion_loop_tolerates_an_empty_resolution() {
        // No resolved windows (for example because none of the labels is
        // configured) must not fail or invoke the setter.
        let mut calls = 0u32;
        apply_capture_exclusions(Vec::<(&str, u32)>::new(), |_label, _handle| {
            calls += 1;
            Ok(())
        });
        assert_eq!(calls, 0);
    }
}
