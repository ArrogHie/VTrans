//! Integration tests for the public coordinate helpers.
//!
//! These tests are platform-independent and cover DPI conversion, negative
//! virtual-desktop coordinates, and region boundary handling.

use vtrans_capture::coordinates::{
    clip_region_to_bounds, is_region_in_bounds, logical_to_physical, region_to_physical,
    to_monitor_relative,
};
use vtrans_core::types::ScreenRegion;

#[test]
fn logical_to_physical_at_150_percent() {
    assert_eq!(logical_to_physical(100.0, 1.5), 150);
}

#[test]
fn physical_to_logical_round_trips() {
    let scale = 1.5;
    assert_eq!(logical_to_physical(100.0, scale), 150);

    let logical = vtrans_capture::coordinates::physical_to_logical(150.0, scale);
    assert!((logical - 100.0).abs() < f32::EPSILON);
}

#[test]
fn negative_monitor_origin_is_normalized() {
    // A monitor to the left of the primary has origin (-1920, 0).
    let region = ScreenRegion::new("left", -1920, 0, 1920, 1080);
    let relative = to_monitor_relative(&region, -1920, 0);

    assert_eq!(relative.x, 0);
    assert_eq!(relative.y, 0);
    assert_eq!(relative.width, 1920);
    assert_eq!(relative.height, 1080);
}

#[test]
fn virtual_desktop_offset_is_preserved() {
    let region = ScreenRegion::new("left", -100, 50, 200, 100);
    let relative = to_monitor_relative(&region, -1920, 0);

    assert_eq!(relative.x, 1820);
    assert_eq!(relative.y, 50);
}

#[test]
fn multi_monitor_region_crossing_edge_clips() {
    // 20 pixels of the requested 100x100 region remain inside the monitor.
    let region = ScreenRegion::new("primary", 1900, 0, 100, 100);
    let clipped = clip_region_to_bounds(&region, 1920, 1080);

    assert_eq!(clipped.x, 1900);
    assert_eq!(clipped.width, 20);
    assert_eq!(clipped.height, 100);
}

#[test]
fn region_out_of_bounds_is_detected() {
    let in_bounds = ScreenRegion::new("m0", 0, 0, 1920, 1080);
    let out_x = ScreenRegion::new("m0", 1900, 0, 100, 100);
    let out_y = ScreenRegion::new("m0", 0, 1000, 100, 100);
    let negative = ScreenRegion::new("m0", -1, 0, 100, 100);

    assert!(is_region_in_bounds(&in_bounds, 1920, 1080));
    assert!(!is_region_in_bounds(&out_x, 1920, 1080));
    assert!(!is_region_in_bounds(&out_y, 1920, 1080));
    assert!(!is_region_in_bounds(&negative, 1920, 1080));
}

#[test]
fn region_to_physical_scales_position_and_size() {
    let logical = ScreenRegion::new("m1", 100, 50, 200, 100);
    let physical = region_to_physical(&logical, 2.0);

    assert_eq!(physical.monitor_id, "m1");
    assert_eq!(physical.x, 200);
    assert_eq!(physical.y, 100);
    assert_eq!(physical.width, 400);
    assert_eq!(physical.height, 200);
}
