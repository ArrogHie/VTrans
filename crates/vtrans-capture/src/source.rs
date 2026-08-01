#![allow(unsafe_code)] // Windows interop requires unsafe; each block below has a SAFETY comment.

//! `WindowsCaptureSource` — implements [`CaptureSource`] for Windows.
//!
//! Enumerates monitors at construction time and provides one-shot and
//! continuous capture via the Windows Graphics Capture API.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use vtrans_core::traits::{CaptureSession, CaptureSource};
use vtrans_core::types::{CapturedImage, ScreenRegion};
use vtrans_core::CaptureError;

use crate::coordinates::is_region_in_bounds;
use crate::graphics_capture::{crop_image, D3D11Context, FrameGrabber};
use crate::monitor::{enumerate_monitors, MonitorEntry, MonitorInfo};
use crate::session::WindowsCaptureSession;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

/// Enables per-monitor DPI awareness V2 for this process.
///
/// The call is intentionally non-fatal for callers: the Tauri shell may
/// already have set DPI awareness before this crate is initialized.
fn set_process_dpi_awareness_v2() -> Result<(), CaptureError> {
    // SAFETY: The context handle is a process-global constant supplied by
    // the Win32 API; no external state is required.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            .map_err(|e| CaptureError::DpiAwarenessFailed(e.to_string()))
    }
}

/// Windows screen capture source.
///
/// Implements the [`CaptureSource`] trait using the Windows Graphics
/// Capture API. Monitor information is enumerated at construction time;
/// call [`list_monitors`](Self::list_monitors) to inspect available displays.
///
/// # Example
///
/// ```no_run
/// use vtrans_capture::WindowsCaptureSource;
/// use vtrans_core::traits::CaptureSource;
/// use vtrans_core::types::ScreenRegion;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let source = WindowsCaptureSource::new()?;
/// for m in source.list_monitors() {
///     println!("{}: {}x{} at ({},{})", m.id, m.width, m.height, m.x, m.y);
/// }
/// let region = ScreenRegion::new(r"\\.\DISPLAY1", 0, 0, 800, 600);
/// let image = source.capture_once(&region).await?;
/// println!("captured {}x{} image", image.width, image.height);
/// # Ok(())
/// # }
/// ```
pub struct WindowsCaptureSource {
    monitors: Vec<MonitorEntry>,
    d3d: D3D11Context,
}

impl WindowsCaptureSource {
    /// Creates a new capture source, enumerating all monitors and
    /// initializing the D3D 11 device.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::InitFailed`] if the D3D 11 device or
    /// graphics capture infrastructure cannot be initialized.
    /// Returns [`CaptureError::MonitorNotFound`] if no monitors are found.
    #[tracing::instrument]
    pub fn new() -> Result<Self, CaptureError> {
        if let Err(e) = set_process_dpi_awareness_v2() {
            tracing::warn!(
                error = %e,
                "failed to set per-monitor DPI awareness; continuing with system DPI"
            );
        }
        let monitors = enumerate_monitors()?;
        if monitors.is_empty() {
            return Err(CaptureError::MonitorNotFound(
                "no monitors found".to_string(),
            ));
        }
        let d3d = D3D11Context::new()?;
        tracing::info!(count = monitors.len(), "WindowsCaptureSource initialized");
        Ok(Self { monitors, d3d })
    }

    /// Returns information about all connected monitors.
    #[must_use]
    #[tracing::instrument(skip(self))]
    pub fn list_monitors(&self) -> Vec<MonitorInfo> {
        self.monitors.iter().map(|e| e.info.clone()).collect()
    }

    /// Finds a monitor entry by ID.
    fn find_monitor(&self, id: &str) -> Result<&MonitorEntry, CaptureError> {
        self.monitors
            .iter()
            .find(|e| e.info.id == id)
            .ok_or_else(|| {
                tracing::warn!(monitor_id = id, "monitor not found");
                CaptureError::MonitorNotFound(id.to_string())
            })
    }

    /// Validates that a region is within the monitor's bounds.
    fn validate_region(region: &ScreenRegion, monitor: &MonitorInfo) -> Result<(), CaptureError> {
        if region.width == 0 || region.height == 0 {
            tracing::warn!(
                monitor_id = %region.monitor_id,
                w = region.width,
                h = region.height,
                "region has zero dimensions"
            );
            return Err(CaptureError::OutOfBounds {
                region: region.clone(),
            });
        }
        if !is_region_in_bounds(region, monitor.width, monitor.height) {
            tracing::warn!(
                monitor_id = %region.monitor_id,
                x = region.x,
                y = region.y,
                w = region.width,
                h = region.height,
                monitor_w = monitor.width,
                monitor_h = monitor.height,
                "region out of monitor bounds"
            );
            return Err(CaptureError::OutOfBounds {
                region: region.clone(),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl CaptureSource for WindowsCaptureSource {
    #[tracing::instrument(skip(self))]
    async fn capture_once(&self, region: &ScreenRegion) -> Result<CapturedImage, CaptureError> {
        let entry = self.find_monitor(&region.monitor_id)?;
        let monitor = &entry.info;
        Self::validate_region(region, monitor)?;

        tracing::debug!(
            monitor_id = %region.monitor_id,
            x = region.x,
            y = region.y,
            w = region.width,
            h = region.height,
            "starting single capture"
        );

        // SAFETY: entry.handle is a valid HMONITOR from enumerate_monitors.
        let mut grabber =
            unsafe { FrameGrabber::new(&self.d3d, entry.handle, monitor.width, monitor.height) }?;

        // Poll for the first frame with a timeout.
        let timeout = Duration::from_millis(500);
        let poll_interval = Duration::from_millis(2);
        let deadline = Instant::now() + timeout;

        let full_frame = loop {
            if let Some(frame) = grabber.try_get_next_frame()? {
                break frame;
            }
            if Instant::now() >= deadline {
                grabber.close()?;
                tracing::warn!(
                    monitor_id = %region.monitor_id,
                    "timed out waiting for first capture frame"
                );
                return Err(CaptureError::FrameGrabFailed(
                    "timed out waiting for first frame".to_string(),
                ));
            }
            tokio::time::sleep(poll_interval).await;
        };

        grabber.close()?;

        // Crop to the requested region (coordinates are monitor-relative).
        let crop_x = u32::try_from(region.x).map_err(|_| CaptureError::OutOfBounds {
            region: region.clone(),
        })?;
        let crop_y = u32::try_from(region.y).map_err(|_| CaptureError::OutOfBounds {
            region: region.clone(),
        })?;
        let cropped = crop_image(&full_frame, crop_x, crop_y, region.width, region.height)
            .ok_or_else(|| {
                tracing::warn!(region = ?region, "crop returned None");
                CaptureError::OutOfBounds {
                    region: region.clone(),
                }
            })?;

        tracing::debug!(
            full_w = full_frame.width,
            full_h = full_frame.height,
            crop_w = cropped.width,
            crop_h = cropped.height,
            "capture and crop complete"
        );

        Ok(cropped)
    }

    #[tracing::instrument(skip(self))]
    async fn start_session(
        &self,
        region: &ScreenRegion,
    ) -> Result<Box<dyn CaptureSession>, CaptureError> {
        let entry = self.find_monitor(&region.monitor_id)?;
        let monitor = &entry.info;
        Self::validate_region(region, monitor)?;

        tracing::info!(
            monitor_id = %region.monitor_id,
            x = region.x,
            y = region.y,
            w = region.width,
            h = region.height,
            "starting capture session"
        );

        // SAFETY: entry.handle is a valid HMONITOR from enumerate_monitors.
        let grabber =
            unsafe { FrameGrabber::new(&self.d3d, entry.handle, monitor.width, monitor.height) }?;

        let session = WindowsCaptureSession::new(grabber, region.clone());
        Ok(Box::new(session))
    }
}
