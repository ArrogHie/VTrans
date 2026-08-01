//! Platform integration tests for the Windows capture implementation.
//!
//! These tests require an interactive Windows desktop session. The capture
//! workflow is kept in one test so concurrent Graphics Capture sessions do
//! not interfere with each other during parallel test runs.

use std::time::Duration;

use vtrans_capture::WindowsCaptureSource;
use vtrans_core::traits::CaptureSource;
use vtrans_core::types::{PixelFormat, ScreenRegion};
use vtrans_core::CaptureError;

/// Builds a small valid region on the first monitor.
fn small_region(source: &WindowsCaptureSource) -> ScreenRegion {
    let monitor = source
        .list_monitors()
        .into_iter()
        .find(|m| m.is_primary)
        .or_else(|| source.list_monitors().into_iter().next())
        .expect("at least one monitor must be available");

    let width = monitor.width.min(64);
    let height = monitor.height.min(64);
    ScreenRegion::new(monitor.id, 0, 0, width, height)
}

#[test]
fn list_monitors_returns_physical_layout() {
    let source = WindowsCaptureSource::new().expect("capture source should initialize");
    let monitors = source.list_monitors();

    assert!(!monitors.is_empty(), "no monitors were enumerated");
    for monitor in monitors {
        assert!(!monitor.id.is_empty());
        assert!(monitor.width > 0);
        assert!(monitor.height > 0);
        assert!(monitor.scale_factor > 0.0);
    }
}

#[tokio::test]
async fn capture_once_and_session_round_trip() {
    let source = WindowsCaptureSource::new().expect("capture source should initialize");
    let region = small_region(&source);

    let image = source
        .capture_once(&region)
        .await
        .expect("single capture should produce an image");
    assert_eq!(image.width, region.width);
    assert_eq!(image.height, region.height);
    assert_eq!(image.format, PixelFormat::Bgra8);
    assert_eq!(image.data.len(), (image.width * image.height * 4) as usize);

    let mut session = source
        .start_session(&region)
        .await
        .expect("session should start");

    let first = tokio::time::timeout(Duration::from_secs(5), session.next_frame())
        .await
        .expect("session should produce the first frame in time")
        .expect("session should not end before the first frame")
        .expect("first frame should be an image");
    assert_eq!(first.width, region.width);
    assert_eq!(first.height, region.height);

    // When the screen is static, the session replays the last frame instead
    // of ending the stream.
    let second = tokio::time::timeout(Duration::from_secs(5), session.next_frame())
        .await
        .expect("session should return the cached frame in time")
        .expect("session should not end while a cached frame exists")
        .expect("cached frame should be an image");
    assert_eq!(second.width, region.width);
    assert_eq!(second.height, region.height);

    session.stop().await.expect("stop should release resources");
    assert!(matches!(
        session.next_frame().await,
        Err(CaptureError::SessionStopped)
    ));
}

#[tokio::test]
async fn out_of_bounds_region_is_rejected() {
    let source = WindowsCaptureSource::new().expect("capture source should initialize");
    let monitors = source.list_monitors();
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary)
        .or_else(|| monitors.first())
        .expect("at least one monitor must be available");

    let region = ScreenRegion::new(monitor.id.clone(), 0, 0, monitor.width.saturating_add(1), 1);

    let result = source.capture_once(&region).await;
    assert!(matches!(result, Err(CaptureError::OutOfBounds { .. })));
}
