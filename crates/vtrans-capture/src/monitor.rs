#![allow(unsafe_code)] // Windows interop requires unsafe; each block below has a SAFETY comment.

//! Monitor enumeration and information.
//!
//! Uses the Win32 `EnumDisplayMonitors` API to discover all connected
//! displays, and `GetDpiForMonitor` to obtain per-monitor DPI scale factors.
//! The [`MonitorInfo`] struct is the public representation; the raw
//! `HMONITOR` handle is kept internally for graphics capture.

use std::mem::size_of;

use vtrans_core::CaptureError;
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HMONITOR, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

/// Information about a single physical display monitor.
///
/// All dimensions are in physical pixels. The `(x, y)` origin is in
/// virtual-desktop coordinates and may be negative for non-primary
/// monitors (e.g. a monitor to the left of the primary has `x = -1920`).
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    /// Unique identifier (Win32 device name, e.g. `\\.\DISPLAY1`).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
    /// X origin in virtual-desktop coordinates (may be negative).
    pub x: i32,
    /// Y origin in virtual-desktop coordinates (may be negative).
    pub y: i32,
    /// DPI scale factor (`dpi / 96.0`). `1.0` = 100%, `1.5` = 150%, etc.
    pub scale_factor: f32,
    /// Whether this is the primary monitor.
    pub is_primary: bool,
}

impl MonitorInfo {
    /// Creates a new `MonitorInfo` with the given fields.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        width: u32,
        height: u32,
        x: i32,
        y: i32,
        scale_factor: f32,
        is_primary: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            width,
            height,
            x,
            y,
            scale_factor,
            is_primary,
        }
    }
}

/// Internal entry pairing a [`MonitorInfo`] with its raw `HMONITOR` handle.
///
/// The handle is needed to create a `GraphicsCaptureItem` via the
/// `IGraphicsCaptureItemInterop` interface. It is not exposed publicly.
#[derive(Debug, Clone)]
pub(crate) struct MonitorEntry {
    /// Public monitor information.
    pub info: MonitorInfo,
    /// Raw Win32 monitor handle.
    pub handle: HMONITOR,
}

// SAFETY: HMONITOR is a handle to a monitor (isize wrapper). Monitor
// handles are process-global and safe to share across threads.
unsafe impl Send for MonitorEntry {}
unsafe impl Sync for MonitorEntry {}

/// Enumerates all display monitors using the Win32 API.
///
/// Returns one [`MonitorEntry`] per connected display, with DPI scale
/// factors resolved via `GetDpiForMonitor`. Monitors that fail DPI
/// query default to a scale of `1.0` (96 DPI) with a warning log.
///
/// # Errors
///
/// Returns [`CaptureError::InitFailed`] if the Win32 enumeration call
/// itself fails (extremely rare; usually indicates no display attached).
#[tracing::instrument]
pub(crate) fn enumerate_monitors() -> Result<Vec<MonitorEntry>, CaptureError> {
    let mut entries: Vec<MonitorEntry> = Vec::new();

    // SAFETY: `entries_ptr` is a valid pointer to a local `Vec`. The
    // callback only pushes into it and does not read stale data. We
    // hold an exclusive borrow for the duration of the call.
    let entries_ptr = std::ptr::addr_of_mut!(entries);
    let result = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(monitor_enum_callback),
            LPARAM(entries_ptr as isize),
        )
    };

    if result == BOOL(0) {
        let msg = "EnumDisplayMonitors returned FALSE";
        tracing::error!(msg);
        return Err(CaptureError::InitFailed(msg.to_string()));
    }

    if entries.is_empty() {
        tracing::warn!("no monitors found via EnumDisplayMonitors");
    }

    Ok(entries)
}

/// Callback for `EnumDisplayMonitors`.
///
/// # Safety
///
/// `lparam` must be a valid `*mut Vec<MonitorEntry>` pointing to a
/// live vector. `hmonitor` must be a valid monitor handle returned
/// by the enumeration API.
unsafe extern "system" fn monitor_enum_callback(
    hmonitor: HMONITOR,
    _hdc: windows::Win32::Graphics::Gdi::HDC,
    _lprc: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let entries = &mut *(lparam.0 as *mut Vec<MonitorEntry>);

    match read_monitor_info(hmonitor) {
        Ok(info) => {
            entries.push(MonitorEntry {
                info,
                handle: hmonitor,
            });
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to read monitor info, skipping");
        }
    }

    BOOL(1) // continue enumeration
}

/// Reads detailed information for a single monitor handle.
///
/// # Safety
///
/// `hmonitor` must be a valid `HMONITOR` returned by the enumeration API.
#[allow(clippy::cast_precision_loss)]
unsafe fn read_monitor_info(hmonitor: HMONITOR) -> Result<MonitorInfo, String> {
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = u32::try_from(size_of::<MONITORINFOEXW>()).unwrap_or(0);

    if GetMonitorInfoW(hmonitor, std::ptr::addr_of_mut!(mi).cast()) == BOOL(0) {
        return Err("GetMonitorInfoW failed".to_string());
    }

    let rc = mi.monitorInfo.rcMonitor;
    let width = u32::try_from(rc.right - rc.left).unwrap_or(0);
    let height = u32::try_from(rc.bottom - rc.top).unwrap_or(0);
    let x = rc.left;
    let y = rc.top;
    let is_primary = (mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;

    let device_name = utf16_to_string(&mi.szDevice);
    let id = device_name.clone();
    let name = device_name.clone();

    let scale_factor = match get_monitor_dpi(hmonitor) {
        Ok(dpi) => dpi as f32 / 96.0,
        Err(e) => {
            tracing::warn!(
                monitor = %id,
                error = %e,
                "failed to get DPI, defaulting to 1.0"
            );
            1.0
        }
    };

    Ok(MonitorInfo::new(
        id,
        name,
        width,
        height,
        x,
        y,
        scale_factor,
        is_primary,
    ))
}

/// Queries the effective DPI for a monitor.
///
/// # Safety
///
/// `hmonitor` must be a valid `HMONITOR`.
unsafe fn get_monitor_dpi(hmonitor: HMONITOR) -> Result<u32, String> {
    let mut dpi_x = 0u32;
    let mut dpi_y = 0u32;

    let result = GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    if let Err(e) = result {
        Err(format!("GetDpiForMonitor failed: {e}"))
    } else {
        Ok(dpi_x)
    }
}

/// Converts a null-terminated UTF-16 buffer to a Rust `String`.
fn utf16_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_info_new() {
        let m = MonitorInfo::new(r"\\.\DISPLAY1", "Display 1", 1920, 1080, 0, 0, 1.0, true);
        assert_eq!(m.id, r"\\.\DISPLAY1");
        assert_eq!(m.width, 1920);
        assert_eq!(m.height, 1080);
        assert!((m.scale_factor - 1.0).abs() < f32::EPSILON);
        assert!(m.is_primary);
    }

    #[test]
    fn monitor_info_clone() {
        let m = MonitorInfo::new("m0", "Mon", 100, 200, 10, 20, 1.5, false);
        let m2 = m.clone();
        assert_eq!(m.id, m2.id);
        assert_eq!(m.width, m2.width);
    }

    #[test]
    fn utf16_to_string_basic() {
        let buf = [u16::from(b'H'), u16::from(b'i'), 0];
        assert_eq!(utf16_to_string(&buf), "Hi");
    }

    #[test]
    fn utf16_to_string_no_null() {
        let buf = [u16::from(b'A'), u16::from(b'B')];
        assert_eq!(utf16_to_string(&buf), "AB");
    }

    #[test]
    fn utf16_to_string_empty() {
        let buf = [0u16];
        assert_eq!(utf16_to_string(&buf), "");
    }

    #[test]
    fn utf16_to_string_unicode() {
        let buf = [0x4E2D, 0x6587, 0];
        assert_eq!(utf16_to_string(&buf), "中文");
    }
}
