//! `WindowsCaptureSession` — implements [`CaptureSession`] for Windows.
//!
//! Wraps a [`FrameGrabber`] and crops each frame to the user-selected
//! [`ScreenRegion`]. The session is single-use: after [`stop`] is called,
//! [`next_frame`] returns [`CaptureError::SessionStopped`].

use std::time::{Duration, Instant};

use async_trait::async_trait;
use vtrans_core::traits::CaptureSession;
use vtrans_core::types::{CapturedImage, ScreenRegion};
use vtrans_core::CaptureError;

use crate::graphics_capture::{crop_image, FrameGrabber};

/// How long `next_frame` waits for a new frame before treating the session
/// as ended (e.g. monitor disconnected or display is idle).
const FRAME_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Poll interval used while waiting for the next graphics-capture frame.
const FRAME_POLL_INTERVAL: Duration = Duration::from_millis(16);

/// Continuous capture session for a specific screen region.
///
/// Created by [`WindowsCaptureSource::start_session`](crate::WindowsCaptureSource::start_session).
/// Each call to [`next_frame`](Self::next_frame) retrieves the latest
/// captured frame and crops it to the session's region.
pub(crate) struct WindowsCaptureSession {
    grabber: FrameGrabber,
    region: ScreenRegion,
    stopped: bool,
    last_frame: Option<CapturedImage>,
}

impl WindowsCaptureSession {
    /// Creates a new session from a frame grabber and target region.
    pub(crate) fn new(grabber: FrameGrabber, region: ScreenRegion) -> Self {
        Self {
            grabber,
            region,
            stopped: false,
            last_frame: None,
        }
    }
}

#[async_trait]
impl CaptureSession for WindowsCaptureSession {
    #[tracing::instrument(skip(self))]
    async fn next_frame(&mut self) -> Result<Option<CapturedImage>, CaptureError> {
        if self.stopped {
            return Err(CaptureError::SessionStopped);
        }

        let deadline = Instant::now() + FRAME_WAIT_TIMEOUT;
        loop {
            if self.stopped {
                return Err(CaptureError::SessionStopped);
            }

            let Some(full_frame) = self.grabber.try_get_next_frame()? else {
                // Replay the previous frame when the screen has not changed;
                // callers can decide whether it needs reprocessing.
                if let Some(last) = &self.last_frame {
                    return Ok(Some(last.clone()));
                }
                if Instant::now() >= deadline {
                    tracing::warn!(
                        monitor_id = %self.region.monitor_id,
                        "timed out waiting for next capture frame"
                    );
                    return Ok(None);
                }
                tokio::time::sleep(FRAME_POLL_INTERVAL).await;
                continue;
            };

            let crop_x = u32::try_from(self.region.x).map_err(|_| CaptureError::OutOfBounds {
                region: self.region.clone(),
            })?;
            let crop_y = u32::try_from(self.region.y).map_err(|_| CaptureError::OutOfBounds {
                region: self.region.clone(),
            })?;
            let cropped = crop_image(
                &full_frame,
                crop_x,
                crop_y,
                self.region.width,
                self.region.height,
            )
            .ok_or_else(|| {
                tracing::warn!(
                    monitor_id = %self.region.monitor_id,
                    "session frame crop out of bounds"
                );
                CaptureError::OutOfBounds {
                    region: self.region.clone(),
                }
            })?;
            self.last_frame = Some(cropped.clone());

            tracing::debug!(
                w = cropped.width,
                h = cropped.height,
                "session frame captured and cropped"
            );
            return Ok(Some(cropped));
        }
    }

    #[tracing::instrument(skip(self))]
    async fn stop(&mut self) -> Result<(), CaptureError> {
        if self.stopped {
            tracing::debug!("session already stopped");
            return Ok(());
        }
        self.stopped = true;
        self.grabber.close()?;
        tracing::info!("capture session stopped");
        Ok(())
    }
}
