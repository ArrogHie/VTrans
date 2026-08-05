//! Minimal runnable example for `vtrans-capture`.
//!
//! Requires an interactive Windows desktop session:
//!
//! ```powershell
//! cargo run -p vtrans-capture --example capture_demo
//! ```

use std::time::Duration;

use vtrans_capture::WindowsCaptureSource;
use vtrans_core::traits::CaptureSource;
use vtrans_core::types::ScreenRegion;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create the capture source: enumerate monitors and initialize D3D11/WinRT.
    let source = WindowsCaptureSource::new()?;

    // 2. Select a 640x480 region at the primary monitor's top-left corner
    //    (physical pixels, relative to the monitor).
    let primary = source
        .list_monitors()
        .into_iter()
        .find(|m| m.is_primary)
        .ok_or("no primary monitor")?;
    let region = ScreenRegion::new(primary.id, 0, 0, 640, 480);

    // 3. Single capture. OutOfBounds means the region needs adjustment;
    //    other errors may be retried or require rebuilding the source.
    let image = match source.capture_once(&region).await {
        Ok(image) => image,
        Err(vtrans_core::CaptureError::OutOfBounds { .. }) => {
            println!("region out of bounds, adjust and retry");
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };
    println!("single capture: {}x{}", image.width, image.height);

    // 4. The continuous session is owned by the caller; stop or drop
    //    releases the Graphics Capture resources.
    let mut session = source.start_session(&region).await?;
    let frame = tokio::time::timeout(Duration::from_secs(5), session.next_frame())
        .await
        .map_err(|_| "timed out waiting for first frame")??;
    let frame = frame.ok_or("capture session ended before first frame")?;
    println!("session frame: {}x{}", frame.width, frame.height);

    // 5. Stop explicitly; next_frame after stop returns CaptureError::SessionStopped.
    session.stop().await?;
    Ok(())
}
