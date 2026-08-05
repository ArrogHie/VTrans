//! Debug-mode capture-frame thumbnail pipeline.
//!
//! When Debug mode is enabled, frames that are about to enter OCR are
//! forwarded through a bounded channel with latest-value semantics, scaled
//! down to a thumbnail (longest edge ≤ [`MAX_THUMBNAIL_EDGE`]) and encoded
//! as JPEG, then emitted to the frontend debug panel as
//! `debug_frame_updated`. The pipeline is display-only: frames are never
//! written to disk, logged, or persisted. When Debug mode is off, no sink is
//! attached to the pipeline and none of this code runs.
//!
//! The forwarder counts **every** frame the pipeline hands over (each watch
//! wake corresponds to one captured frame accepted by frame-difference
//! detection), so the emitted `frame_index` advances even when the frame
//! itself is dropped by the ≤10 fps throttle or fails to encode. The
//! frontend can therefore detect dropped frames as gaps in the sequence.

use std::sync::Arc;
use std::time::{Duration, Instant};

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageBuffer};
use tauri::Manager;
use thiserror::Error;
use tokio::sync::watch;
use tracing::warn;
use vtrans_core::{CapturedImage, PixelFormat, ScreenRegion};
use vtrans_pipeline::FrameSink;

use crate::events::{emit_debug_frame, DebugFramePayload};
use crate::state::AppState;

/// Longest allowed edge of a debug thumbnail in pixels.
pub(crate) const MAX_THUMBNAIL_EDGE: u32 = 480;

/// Maximum debug frame rate. Older frames are dropped without being encoded
/// so a slow debug panel can never back-pressure the capture pipeline.
const MAX_FRAME_RATE: u32 = 10;

/// Minimum interval between two emitted debug frames (1000 / 10 fps).
const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(1000 / MAX_FRAME_RATE as u64);

/// How the region metadata attached to an emitted debug frame is resolved.
///
/// A single capture always originates from the region passed to the
/// `capture_once` command, which is not stored in `AppState`; a live session
/// instead follows `AppState::selected_region` so mid-session region updates
/// are reflected in the payload.
#[derive(Clone, Debug)]
pub(crate) enum RegionSource {
    /// The frame always originates from this exact region (single captures).
    Fixed(ScreenRegion),
    /// Follows the last region selected in `AppState` so live region updates
    /// are reflected; falls back to the region the session started with.
    FollowSelected(ScreenRegion),
}

impl RegionSource {
    /// Resolves the region metadata for one frame.
    ///
    /// `selected` is the current `AppState::selected_region` snapshot; it is
    /// ignored by [`RegionSource::Fixed`] because a single capture's region
    /// must not be confused with a previously selected live region.
    fn resolve(&self, selected: Option<ScreenRegion>) -> ScreenRegion {
        match self {
            Self::Fixed(region) => region.clone(),
            Self::FollowSelected(initial) => selected.unwrap_or_else(|| initial.clone()),
        }
    }
}

/// Returns `true` when a frame may be emitted now, enforcing the maximum
/// debug frame rate.
///
/// The first frame is always allowed; subsequent frames must be at least
/// [`MIN_FRAME_INTERVAL`] apart (the boundary itself is allowed).
fn throttle_allows(now: Instant, last_emit: Option<Instant>) -> bool {
    last_emit.map_or(true, |previous| {
        now.duration_since(previous) >= MIN_FRAME_INTERVAL
    })
}

/// Errors produced while encoding a debug thumbnail.
#[derive(Debug, Error)]
pub(crate) enum DebugFrameError {
    /// The frame dimensions or data buffer are inconsistent.
    #[error("invalid frame for debug thumbnail: {0}")]
    InvalidFrame(String),

    /// JPEG encoding failed.
    #[error("debug thumbnail encoding failed: {0}")]
    Encode(String),
}

/// Encodes a captured frame as a JPEG thumbnail (longest edge ≤
/// [`MAX_THUMBNAIL_EDGE`], quality 80).
///
/// This is a pure function without any Tauri dependency so the scaling and
/// encoding contract is unit-testable. The caller is expected to run it on
/// the blocking pool for large frames.
///
/// # Errors
///
/// Returns [`DebugFrameError::InvalidFrame`] for inconsistent dimensions or
/// buffers, and [`DebugFrameError::Encode`] when the JPEG encoder fails.
pub(crate) fn encode_debug_thumbnail(image: &CapturedImage) -> Result<Vec<u8>, DebugFrameError> {
    if image.width == 0 || image.height == 0 {
        return Err(DebugFrameError::InvalidFrame("zero dimension".to_string()));
    }
    let expected = CapturedImage::expected_data_len(image.width, image.height, image.format);
    if image.data.len() != expected {
        return Err(DebugFrameError::InvalidFrame(format!(
            "data length mismatch: expected {expected}, got {}",
            image.data.len()
        )));
    }
    let (dst_width, dst_height) = thumbnail_size(image.width, image.height);
    let rgba_data = match image.format {
        PixelFormat::Bgra8 => bgra_to_rgba(&image.data),
        PixelFormat::Rgba8 => image.data.clone(),
    };
    let dynamic = match image.format {
        PixelFormat::Bgra8 | PixelFormat::Rgba8 => DynamicImage::ImageRgba8(
            ImageBuffer::from_raw(image.width, image.height, rgba_data).ok_or_else(|| {
                DebugFrameError::InvalidFrame("buffer does not match dimensions".to_string())
            })?,
        ),
    };
    let rgb = dynamic
        .resize(dst_width, dst_height, image::imageops::FilterType::Triangle)
        .to_rgb8();
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, 80)
        .encode_image(&rgb)
        .map_err(|error| DebugFrameError::Encode(error.to_string()))?;
    Ok(output)
}

/// Converts a BGRA8 buffer to RGBA8 by swapping the red and blue channels.
///
/// `DynamicImage` has no BGRA variant, so BGRA capture frames are converted
/// before scaling/encoding.
fn bgra_to_rgba(data: &[u8]) -> Vec<u8> {
    let mut rgba = data.to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    rgba
}

/// Computes the thumbnail size that keeps the longest edge within
/// [`MAX_THUMBNAIL_EDGE`] while preserving the aspect ratio. Smaller images
/// are never upscaled.
fn thumbnail_size(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= MAX_THUMBNAIL_EDGE {
        return (width, height);
    }
    // Integer proportional scaling avoids float casts; the products fit in
    // u64 because `src * MAX` ≤ `u32::MAX * 480`.
    let dst_width =
        u32::try_from(u64::from(width) * u64::from(MAX_THUMBNAIL_EDGE) / u64::from(longest))
            .unwrap_or(MAX_THUMBNAIL_EDGE)
            .max(1);
    let dst_height =
        u32::try_from(u64::from(height) * u64::from(MAX_THUMBNAIL_EDGE) / u64::from(longest))
            .unwrap_or(MAX_THUMBNAIL_EDGE)
            .max(1);
    (dst_width, dst_height)
}

/// Frame sink attached to a pipeline while Debug mode is enabled.
///
/// The synchronous `on_frame` callback only clones the frame into a bounded
/// channel; encoding and emission happen on a background task so the capture
/// loop is never blocked by JPEG encoding.
struct ChannelFrameSink {
    tx: watch::Sender<CapturedImage>,
}

impl FrameSink for ChannelFrameSink {
    fn on_frame(&self, frame: &CapturedImage) {
        // A watch channel keeps exactly one value: sending is non-blocking
        // and overwrites the previous frame (latest-value semantics). The
        // only failure mode is a dropped receiver, which simply stops the
        // preview.
        let _ = self.tx.send(frame.clone());
    }
}

/// Spawns the debug frame forwarding task and returns a pipeline frame sink.
///
/// Received frames are throttled to [`MAX_FRAME_RATE`], encoded on the
/// blocking pool, and emitted as `debug_frame_updated`. The task exits when
/// the pipeline drops the sink (channel closed).
pub(crate) fn spawn_debug_frame_forwarder(
    app: tauri::AppHandle,
    region_source: RegionSource,
) -> Arc<dyn FrameSink> {
    let placeholder = CapturedImage::new(1, 1, PixelFormat::Rgba8, vec![0; 4])
        .expect("1x1 placeholder image is valid");
    let (tx, mut rx) = watch::channel(placeholder);
    tauri::async_runtime::spawn(async move {
        let mut last_emit: Option<Instant> = None;
        let mut frame_index: u64 = 0;
        while rx.changed().await.is_ok() {
            let image = rx.borrow_and_update().clone();
            // Every watch wake is one captured frame accepted by the
            // pipeline; advance the sequence even when the frame is dropped
            // below so the frontend can see the gap.
            frame_index = frame_index.wrapping_add(1);
            let now = Instant::now();
            if !throttle_allows(now, last_emit) {
                continue;
            }
            last_emit = Some(now);
            let selected = match &region_source {
                RegionSource::Fixed(_) => None,
                RegionSource::FollowSelected(_) => app.state::<AppState>().selected_region(),
            };
            let region = region_source.resolve(selected);
            let encoded = tokio::task::spawn_blocking(move || encode_debug_thumbnail(&image)).await;
            let bytes = match encoded {
                Ok(Ok(bytes)) => bytes,
                Ok(Err(error)) => {
                    warn!(error = %error, "skipping debug frame: thumbnail encoding failed");
                    continue;
                }
                Err(error) => {
                    warn!(error = %error, "skipping debug frame: encoder task failed");
                    continue;
                }
            };
            let payload = DebugFramePayload {
                image: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes),
                region,
                frame_index,
                timestamp_ms: unix_timestamp_ms(),
            };
            emit_debug_frame(&app, payload);
        }
    });
    Arc::new(ChannelFrameSink { tx })
}

fn unix_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().try_into().unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtrans_core::CapturedImage;

    fn bgra_image(width: u32, height: u32) -> CapturedImage {
        let data = (0..width * height)
            .flat_map(|pixel| {
                let value = u8::try_from(pixel % 256).expect("mod 256 fits u8");
                [value, value.wrapping_mul(2), value.wrapping_mul(3), 255]
            })
            .collect();
        CapturedImage::new(width, height, PixelFormat::Bgra8, data).unwrap()
    }

    #[test]
    fn thumbnail_size_keeps_longest_edge_within_limit() {
        assert_eq!(thumbnail_size(100, 50), (100, 50));
        assert_eq!(thumbnail_size(480, 270), (480, 270));
        assert_eq!(thumbnail_size(2000, 1000), (480, 240));
        assert_eq!(thumbnail_size(640, 360), (480, 270));
        assert_eq!(thumbnail_size(100, 2000), (24, 480));
    }

    #[test]
    fn thumbnail_encodes_to_valid_jpeg_within_limit() {
        let image = bgra_image(1920, 1080);
        let bytes = encode_debug_thumbnail(&image).unwrap();
        assert!(bytes.starts_with(&[0xFF, 0xD8, 0xFF]), "JPEG magic missing");

        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), 480);
        assert_eq!(decoded.height(), 270);
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let image = bgra_image(100, 50);
        let bytes = encode_debug_thumbnail(&image).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), 100);
        assert_eq!(decoded.height(), 50);
    }

    #[test]
    fn rgba_images_are_encoded_like_bgra() {
        let data = (0..100_u32 * 50)
            .flat_map(|pixel| {
                let value = u8::try_from(pixel % 256).expect("mod 256 fits u8");
                [value, value.wrapping_mul(2), value.wrapping_mul(3), 255]
            })
            .collect();
        let image = CapturedImage::new(100, 50, PixelFormat::Rgba8, data).unwrap();
        let bytes = encode_debug_thumbnail(&image).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (100, 50));
    }

    #[test]
    fn inconsistent_buffer_is_rejected() {
        let image = CapturedImage {
            width: 100,
            height: 50,
            format: PixelFormat::Bgra8,
            data: vec![0; 17],
        };
        assert!(matches!(
            encode_debug_thumbnail(&image),
            Err(DebugFrameError::InvalidFrame(_))
        ));
    }

    #[test]
    fn zero_dimension_is_rejected() {
        let image = CapturedImage {
            width: 0,
            height: 50,
            format: PixelFormat::Bgra8,
            data: Vec::new(),
        };
        assert!(matches!(
            encode_debug_thumbnail(&image),
            Err(DebugFrameError::InvalidFrame(_))
        ));
    }

    #[test]
    fn throttle_allows_the_first_frame_immediately() {
        assert!(throttle_allows(Instant::now(), None));
    }

    #[test]
    fn throttle_denies_frames_within_the_interval() {
        let now = Instant::now();
        let within = MIN_FRAME_INTERVAL
            .checked_sub(Duration::from_millis(1))
            .expect("the throttle interval is longer than one millisecond");
        let previous = now
            .checked_sub(within)
            .expect("instant supports the subtraction");
        assert!(!throttle_allows(now, Some(previous)));
    }

    #[test]
    fn throttle_allows_frames_at_or_after_the_interval_boundary() {
        let now = Instant::now();
        let at_boundary = now
            .checked_sub(MIN_FRAME_INTERVAL)
            .expect("instant supports the subtraction");
        assert!(throttle_allows(now, Some(at_boundary)));
        let after_boundary = now
            .checked_sub(MIN_FRAME_INTERVAL + Duration::from_millis(1))
            .expect("instant supports the subtraction");
        assert!(throttle_allows(now, Some(after_boundary)));
    }

    #[test]
    fn fixed_region_source_ignores_the_selected_region() {
        let source = RegionSource::Fixed(ScreenRegion::new("m0", 1, 2, 3, 4));
        let selected = Some(ScreenRegion::new("m9", 9, 9, 9, 9));
        assert_eq!(region_fields(&source.resolve(selected)), ("m0", 1, 2, 3, 4));
    }

    #[test]
    fn follow_selected_region_source_uses_the_selected_region() {
        let source = RegionSource::FollowSelected(ScreenRegion::new("m0", 1, 2, 3, 4));
        let selected = Some(ScreenRegion::new("m1", 10, 20, 30, 40));
        assert_eq!(
            region_fields(&source.resolve(selected)),
            ("m1", 10, 20, 30, 40)
        );
    }

    #[test]
    fn follow_selected_region_source_falls_back_to_the_starting_region() {
        let source = RegionSource::FollowSelected(ScreenRegion::new("m0", 1, 2, 3, 4));
        assert_eq!(region_fields(&source.resolve(None)), ("m0", 1, 2, 3, 4));
    }

    fn region_fields(region: &ScreenRegion) -> (&str, i32, i32, u32, u32) {
        (
            region.monitor_id.as_str(),
            region.x,
            region.y,
            region.width,
            region.height,
        )
    }
}
