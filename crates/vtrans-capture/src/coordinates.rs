//! DPI and multi-monitor coordinate conversion utilities.
//!
//! Windows uses a virtual desktop coordinate system where each monitor has
//! an origin (`x`, `y`) that may be negative (e.g. a monitor to the left of
//! the primary has `x = -1920`). Coordinates from the frontend are in
//! device-independent pixels (DIPs); the capture API works in physical
//! pixels. This module provides the conversions and boundary checks needed
//! to translate between these coordinate spaces.

use vtrans_core::types::ScreenRegion;

/// Converts a logical coordinate (DIP) to a physical pixel coordinate.
///
/// Uses the monitor's scale factor (`dpi / 96.0`). The result is rounded
/// to the nearest integer pixel.
///
/// # Arguments
/// * `x` - Logical coordinate in DIPs.
/// * `scale` - Monitor scale factor (e.g. `1.0` for 96 DPI, `1.5` for 144 DPI).
///
/// # Example
///
/// ```
/// use vtrans_capture::coordinates::logical_to_physical;
///
/// // At 150% DPI, 100 DIP = 150 physical pixels.
/// assert_eq!(logical_to_physical(100.0, 1.5), 150);
/// // At 100% DPI, coordinates pass through unchanged.
/// assert_eq!(logical_to_physical(100.0, 1.0), 100);
/// ```
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn logical_to_physical(x: f32, scale: f32) -> i32 {
    (x * scale).round() as i32
}

/// Converts a physical pixel coordinate to a logical coordinate (DIP).
///
/// Returns the input unchanged if `scale` is zero (avoids division by zero).
///
/// # Arguments
/// * `x` - Physical pixel coordinate.
/// * `scale` - Monitor scale factor (`dpi / 96.0`).
///
/// # Example
///
/// ```
/// use vtrans_capture::coordinates::physical_to_logical;
///
/// assert!((physical_to_logical(150.0, 1.5) - 100.0).abs() < f32::EPSILON);
/// ```
#[must_use]
pub fn physical_to_logical(x: f32, scale: f32) -> f32 {
    if scale.abs() < f32::EPSILON {
        x
    } else {
        x / scale
    }
}

/// Converts a [`ScreenRegion`] from logical (DIP) to physical pixel coordinates.
///
/// The `monitor_id` is preserved; only the position and dimensions are scaled.
///
/// # Arguments
/// * `region` - Region in logical coordinates.
/// * `scale` - Monitor scale factor.
///
/// # Example
///
/// ```
/// use vtrans_capture::coordinates::region_to_physical;
/// use vtrans_core::types::ScreenRegion;
///
/// let logical = ScreenRegion::new("m0", 100, 50, 200, 100);
/// let physical = region_to_physical(&logical, 2.0);
/// assert_eq!(physical.x, 200);
/// assert_eq!(physical.y, 100);
/// assert_eq!(physical.width, 400);
/// assert_eq!(physical.height, 200);
/// ```
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn region_to_physical(region: &ScreenRegion, scale: f32) -> ScreenRegion {
    ScreenRegion {
        monitor_id: region.monitor_id.clone(),
        x: logical_to_physical(region.x as f32, scale),
        y: logical_to_physical(region.y as f32, scale),
        width: u32::try_from(logical_to_physical(region.width as f32, scale)).unwrap_or(0),
        height: u32::try_from(logical_to_physical(region.height as f32, scale)).unwrap_or(0),
    }
}

/// Checks whether a region fits entirely within the given monitor dimensions.
///
/// The region's `(x, y)` must be non-negative, and `x + width` / `y + height`
/// must not exceed the monitor's physical dimensions.
///
/// # Arguments
/// * `region` - Region in physical pixel coordinates relative to the monitor.
/// * `monitor_width` - Monitor width in physical pixels.
/// * `monitor_height` - Monitor height in physical pixels.
///
/// # Example
///
/// ```
/// use vtrans_capture::coordinates::is_region_in_bounds;
/// use vtrans_core::types::ScreenRegion;
///
/// let region = ScreenRegion::new("m0", 0, 0, 100, 100);
/// assert!(is_region_in_bounds(&region, 1920, 1080));
///
/// let oob = ScreenRegion::new("m0", 1900, 0, 100, 100);
/// assert!(!is_region_in_bounds(&oob, 1920, 1080));
/// ```
#[must_use]
pub fn is_region_in_bounds(region: &ScreenRegion, monitor_width: u32, monitor_height: u32) -> bool {
    if region.x < 0 || region.y < 0 {
        return false;
    }
    let x_end = i64::from(region.x).saturating_add(i64::from(region.width));
    let y_end = i64::from(region.y).saturating_add(i64::from(region.height));
    x_end <= i64::from(monitor_width) && y_end <= i64::from(monitor_height)
}

/// Clips a region to fit within the monitor's physical bounds.
///
/// Negative offsets are clamped to zero, and the width/height are reduced
/// so the region does not extend past the monitor edge. If the region is
/// entirely outside the monitor, the result has zero dimensions.
///
/// # Arguments
/// * `region` - Region in physical pixel coordinates.
/// * `monitor_width` - Monitor width in physical pixels.
/// * `monitor_height` - Monitor height in physical pixels.
///
/// # Example
///
/// ```
/// use vtrans_capture::coordinates::clip_region_to_bounds;
/// use vtrans_core::types::ScreenRegion;
///
/// let region = ScreenRegion::new("m0", -10, -10, 100, 100);
/// let clipped = clip_region_to_bounds(&region, 1920, 1080);
/// assert_eq!(clipped.x, 0);
/// assert_eq!(clipped.y, 0);
/// assert_eq!(clipped.width, 90);
/// assert_eq!(clipped.height, 90);
/// ```
#[must_use]
pub fn clip_region_to_bounds(
    region: &ScreenRegion,
    monitor_width: u32,
    monitor_height: u32,
) -> ScreenRegion {
    let clamped_x = region.x.max(0);
    let clamped_y = region.y.max(0);

    let x_end = i64::from(region.x)
        .saturating_add(i64::from(region.width))
        .clamp(0, i64::from(monitor_width));
    let y_end = i64::from(region.y)
        .saturating_add(i64::from(region.height))
        .clamp(0, i64::from(monitor_height));

    let new_width = u32::try_from((x_end - i64::from(clamped_x)).max(0)).unwrap_or(0);
    let new_height = u32::try_from((y_end - i64::from(clamped_y)).max(0)).unwrap_or(0);

    ScreenRegion {
        monitor_id: region.monitor_id.clone(),
        x: clamped_x,
        y: clamped_y,
        width: new_width,
        height: new_height,
    }
}

/// Converts a region from virtual-desktop coordinates to monitor-relative
/// physical pixel coordinates.
///
/// Each monitor has an origin `(monitor_x, monitor_y)` in the virtual
/// desktop. This function subtracts the monitor origin to produce
/// coordinates relative to the monitor's top-left corner.
///
/// # Arguments
/// * `region` - Region in virtual-desktop coordinates.
/// * `monitor_x` - Monitor's X origin in the virtual desktop.
/// * `monitor_y` - Monitor's Y origin in the virtual desktop.
///
/// # Example
///
/// ```
/// use vtrans_capture::coordinates::to_monitor_relative;
/// use vtrans_core::types::ScreenRegion;
///
/// // A region at virtual-desktop (100, 100) on a monitor at (-1920, 0).
/// let region = ScreenRegion::new("m0", 100, 100, 200, 200);
/// let relative = to_monitor_relative(&region, -1920, 0);
/// assert_eq!(relative.x, 2020);
/// assert_eq!(relative.y, 100);
/// ```
#[must_use]
pub fn to_monitor_relative(region: &ScreenRegion, monitor_x: i32, monitor_y: i32) -> ScreenRegion {
    ScreenRegion {
        monitor_id: region.monitor_id.clone(),
        x: region.x.saturating_sub(monitor_x),
        y: region.y.saturating_sub(monitor_y),
        width: region.width,
        height: region.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_to_physical_150_percent() {
        assert_eq!(logical_to_physical(100.0, 1.5), 150);
    }

    #[test]
    fn logical_to_physical_100_percent() {
        assert_eq!(logical_to_physical(100.0, 1.0), 100);
    }

    #[test]
    fn logical_to_physical_200_percent() {
        assert_eq!(logical_to_physical(100.0, 2.0), 200);
    }

    #[test]
    fn logical_to_physical_rounding() {
        assert_eq!(logical_to_physical(100.0, 1.25), 125);
        assert_eq!(logical_to_physical(1.0, 1.5), 2);
        assert_eq!(logical_to_physical(3.0, 1.5), 5);
    }

    #[test]
    fn logical_to_physical_negative() {
        assert_eq!(logical_to_physical(-100.0, 1.5), -150);
    }

    #[test]
    fn logical_to_physical_zero_scale() {
        assert_eq!(logical_to_physical(100.0, 0.0), 0);
    }

    #[test]
    fn physical_to_logical_150_percent() {
        let value = physical_to_logical(150.0, 1.5);
        assert!((value - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn physical_to_logical_100_percent() {
        let value = physical_to_logical(100.0, 1.0);
        assert!((value - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn physical_to_logical_zero_scale() {
        let value = physical_to_logical(100.0, 0.0);
        assert!((value - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn region_to_physical_200_percent() {
        let region = ScreenRegion::new("m0", 100, 50, 200, 100);
        let physical = region_to_physical(&region, 2.0);
        assert_eq!(physical.x, 200);
        assert_eq!(physical.y, 100);
        assert_eq!(physical.width, 400);
        assert_eq!(physical.height, 200);
        assert_eq!(physical.monitor_id, "m0");
    }

    #[test]
    fn region_to_physical_preserves_monitor_id() {
        let region = ScreenRegion::new("Display2", 10, 20, 30, 40);
        let physical = region_to_physical(&region, 1.0);
        assert_eq!(physical.monitor_id, "Display2");
    }

    #[test]
    fn in_bounds_exact_fit() {
        let region = ScreenRegion::new("m0", 0, 0, 1920, 1080);
        assert!(is_region_in_bounds(&region, 1920, 1080));
    }

    #[test]
    fn in_bounds_partial() {
        let region = ScreenRegion::new("m0", 100, 100, 100, 100);
        assert!(is_region_in_bounds(&region, 1920, 1080));
    }

    #[test]
    fn out_of_bounds_x() {
        let region = ScreenRegion::new("m0", 1900, 0, 100, 100);
        assert!(!is_region_in_bounds(&region, 1920, 1080));
    }

    #[test]
    fn out_of_bounds_y() {
        let region = ScreenRegion::new("m0", 0, 1000, 100, 100);
        assert!(!is_region_in_bounds(&region, 1920, 1080));
    }

    #[test]
    fn out_of_bounds_negative_x() {
        let region = ScreenRegion::new("m0", -1, 0, 100, 100);
        assert!(!is_region_in_bounds(&region, 1920, 1080));
    }

    #[test]
    fn out_of_bounds_negative_y() {
        let region = ScreenRegion::new("m0", 0, -1, 100, 100);
        assert!(!is_region_in_bounds(&region, 1920, 1080));
    }

    #[test]
    fn in_bounds_zero_size() {
        let region = ScreenRegion::new("m0", 0, 0, 0, 0);
        assert!(is_region_in_bounds(&region, 1920, 1080));
    }

    #[test]
    fn clip_already_in_bounds() {
        let region = ScreenRegion::new("m0", 100, 100, 200, 200);
        let clipped = clip_region_to_bounds(&region, 1920, 1080);
        assert_eq!(clipped.x, 100);
        assert_eq!(clipped.y, 100);
        assert_eq!(clipped.width, 200);
        assert_eq!(clipped.height, 200);
    }

    #[test]
    fn clip_negative_origin() {
        let region = ScreenRegion::new("m0", -10, -10, 100, 100);
        let clipped = clip_region_to_bounds(&region, 1920, 1080);
        assert_eq!(clipped.x, 0);
        assert_eq!(clipped.y, 0);
        assert_eq!(clipped.width, 90);
        assert_eq!(clipped.height, 90);
    }

    #[test]
    fn clip_exceeds_width() {
        let region = ScreenRegion::new("m0", 1900, 0, 100, 100);
        let clipped = clip_region_to_bounds(&region, 1920, 1080);
        assert_eq!(clipped.x, 1900);
        assert_eq!(clipped.width, 20);
    }

    #[test]
    fn clip_exceeds_height() {
        let region = ScreenRegion::new("m0", 0, 1000, 100, 100);
        let clipped = clip_region_to_bounds(&region, 1920, 1080);
        assert_eq!(clipped.y, 1000);
        assert_eq!(clipped.height, 80);
    }

    #[test]
    fn clip_entirely_outside() {
        let region = ScreenRegion::new("m0", 2000, 2000, 100, 100);
        let clipped = clip_region_to_bounds(&region, 1920, 1080);
        assert_eq!(clipped.width, 0);
        assert_eq!(clipped.height, 0);
    }

    #[test]
    fn to_monitor_relative_negative_origin() {
        let region = ScreenRegion::new("m1", -1920, 0, 100, 100);
        let relative = to_monitor_relative(&region, -1920, 0);
        assert_eq!(relative.x, 0);
        assert_eq!(relative.y, 0);
    }

    #[test]
    fn to_monitor_relative_positive_offset() {
        let region = ScreenRegion::new("m0", 2020, 100, 200, 200);
        let relative = to_monitor_relative(&region, -1920, 0);
        assert_eq!(relative.x, 3940);
        assert_eq!(relative.y, 100);
    }

    #[test]
    fn to_monitor_relative_preserves_dimensions() {
        let region = ScreenRegion::new("m0", 100, 100, 200, 200);
        let relative = to_monitor_relative(&region, 0, 0);
        assert_eq!(relative.width, 200);
        assert_eq!(relative.height, 200);
    }
}
