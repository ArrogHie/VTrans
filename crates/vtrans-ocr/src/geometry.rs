//! Geometry helpers for OCR: polygon ordering, minimum-area rectangles,
//! polygon dilation, perspective warping, and rotation.
//!
//! Pixel coordinates use the standard image convention: `x` grows to the
//! right and `y` grows downward.

// Image math intentionally uses f32 coordinates; the conversions in this
// module are bounded by image dimensions and are not user-controlled sizes.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless
)]

use image::{Rgb, RgbImage};

/// A 2D point in image coordinates.
pub type Point = [f32; 2];

/// Compute the signed area of a polygon.
///
/// The result is positive for counter-clockwise vertex order in a
/// mathematical y-up coordinate system. Callers use the sign only to decide
/// polygon orientation.
fn signed_area(polygon: &[Point]) -> f32 {
    let mut sum = 0.0;
    for index in 0..polygon.len() {
        let current = polygon[index];
        let next = polygon[(index + 1) % polygon.len()];
        sum += current[0] * next[1] - next[0] * current[1];
    }
    sum * 0.5
}

/// Compute the absolute area of a polygon.
///
/// # Example
///
/// ```
/// use vtrans_ocr::geometry::{polygon_area, Point};
///
/// let square = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
/// assert!((polygon_area(&square) - 16.0).abs() < 1e-6);
/// ```
#[must_use]
pub fn polygon_area(polygon: &[Point]) -> f32 {
    signed_area(polygon).abs()
}

/// Compute the arithmetic mean of polygon vertices.
///
/// # Example
///
/// ```
/// use vtrans_ocr::geometry::{polygon_center, Point};
///
/// let square = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
/// let center = polygon_center(&square);
/// assert!((center[0] - 2.0).abs() < 1e-6);
/// assert!((center[1] - 2.0).abs() < 1e-6);
/// ```
#[must_use]
pub fn polygon_center(polygon: &[Point]) -> Point {
    let count = polygon.len().max(1);
    let sum_x = polygon.iter().map(|p| p[0]).sum::<f32>();
    let sum_y = polygon.iter().map(|p| p[1]).sum::<f32>();
    [sum_x / count as f32, sum_y / count as f32]
}

/// Compute the perimeter of a polygon.
///
/// # Example
///
/// ```
/// use vtrans_ocr::geometry::{polygon_perimeter, Point};
///
/// let square = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
/// assert!((polygon_perimeter(&square) - 16.0).abs() < 1e-6);
/// ```
#[must_use]
pub fn polygon_perimeter(polygon: &[Point]) -> f32 {
    polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
        .map(|(a, b)| ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt())
        .sum()
}

/// Order four polygon vertices clockwise starting from the top-left vertex.
///
/// The returned order is `[top_left, top_right, bottom_right, bottom_left]`
/// for a typical rectangle.
///
/// # Example
///
/// ```
/// use vtrans_ocr::geometry::{order_polygon, Point};
///
/// let shuffled = [[4.0, 4.0], [0.0, 0.0], [4.0, 0.0], [0.0, 4.0]];
/// let ordered = order_polygon(shuffled);
/// assert_eq!(ordered[0], [0.0, 0.0]);
/// assert_eq!(ordered[1], [4.0, 0.0]);
/// ```
#[must_use]
pub fn order_polygon(polygon: [Point; 4]) -> [Point; 4] {
    let center = polygon_center(&polygon);
    let mut order = [0_usize, 1, 2, 3];
    order.sort_by(|&a, &b| {
        let angle_a = (polygon[a][1] - center[1]).atan2(polygon[a][0] - center[0]);
        let angle_b = (polygon[b][1] - center[1]).atan2(polygon[b][0] - center[0]);
        angle_a
            .partial_cmp(&angle_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut result = order.map(|index| polygon[index]);
    let start = (0..4)
        .min_by(|&a, &b| {
            let key_a = result[a][0] + result[a][1];
            let key_b = result[b][0] + result[b][1];
            key_a
                .partial_cmp(&key_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);
    result.rotate_left(start);

    // In y-down image coordinates the clockwise ring has a positive shoelace
    // area. When the angular sort produced the opposite orientation, reverse
    // the ring and rotate it back to the top-left start.
    if signed_area(&result) < 0.0 {
        result.reverse();
        let new_start = (0..4)
            .min_by(|&a, &b| {
                let key_a = result[a][0] + result[a][1];
                let key_b = result[b][0] + result[b][1];
                key_a
                    .partial_cmp(&key_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(0);
        result.rotate_left(new_start);
    }
    result
}

/// Cross product of vectors `a - o` and `b - o`.
fn cross(o: Point, a: Point, b: Point) -> f32 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Build the convex hull of a point set using Andrew's monotone chain.
fn convex_hull(points: &[Point]) -> Vec<Point> {
    if points.len() <= 1 {
        return points.to_vec();
    }
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| {
        a[0].partial_cmp(&b[0])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut lower = Vec::new();
    for &point in &sorted {
        while lower.len() >= 2
            && cross(lower[lower.len() - 2], lower[lower.len() - 1], point) <= 0.0
        {
            lower.pop();
        }
        lower.push(point);
    }

    let mut upper = Vec::new();
    for &point in sorted.iter().rev() {
        while upper.len() >= 2
            && cross(upper[upper.len() - 2], upper[upper.len() - 1], point) <= 0.0
        {
            upper.pop();
        }
        upper.push(point);
    }

    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Compute the minimum-area enclosing rectangle of a point set.
///
/// This is the `OpenCV` `minAreaRect` equivalent used for text boxes. The
/// result is ordered clockwise from the top-left corner.
///
/// # Example
///
/// ```
/// use vtrans_ocr::geometry::{min_area_rect, polygon_area, Point};
///
/// let points = [[1.0, 1.0], [5.0, 1.0], [5.0, 3.0], [1.0, 3.0]];
/// let rect = min_area_rect(&points);
/// assert!((polygon_area(&rect) - 8.0).abs() < 1e-3);
/// ```
#[must_use]
pub fn min_area_rect(points: &[Point]) -> [Point; 4] {
    let hull = convex_hull(points);
    if hull.len() < 3 {
        let min_x = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        let min_y = points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let max_x = points
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let max_y = points
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max);
        let rect = [
            [min_x, min_y],
            [max_x, min_y],
            [max_x, max_y],
            [min_x, max_y],
        ];
        return order_polygon(rect);
    }

    let mut best = ([[0.0_f32; 2]; 4], f32::INFINITY);
    for edge in 0..hull.len() {
        let start = hull[edge];
        let end = hull[(edge + 1) % hull.len()];
        let dx = end[0] - start[0];
        let dy = end[1] - start[1];
        let length = (dx * dx + dy * dy).sqrt();
        if length < 1e-6 {
            continue;
        }
        let ux = dx / length;
        let uy = dy / length;
        let vx = -uy;
        let vy = ux;

        let mut min_u = f32::INFINITY;
        let mut max_u = f32::NEG_INFINITY;
        let mut min_v = f32::INFINITY;
        let mut max_v = f32::NEG_INFINITY;
        for point in &hull {
            let u = point[0] * ux + point[1] * uy;
            let v = point[0] * vx + point[1] * vy;
            min_u = min_u.min(u);
            max_u = max_u.max(u);
            min_v = min_v.min(v);
            max_v = max_v.max(v);
        }
        let area = (max_u - min_u) * (max_v - min_v);
        if area < best.1 {
            best.1 = area;
            let corner = |u: f32, v: f32| [u * ux + v * vx, u * uy + v * vy];
            best.0 = [
                corner(min_u, min_v),
                corner(max_u, min_v),
                corner(max_u, max_v),
                corner(min_u, max_v),
            ];
        }
    }

    order_polygon(best.0)
}

/// Dilation distance for a polygon, computed like PP-OCR's unclip step.
///
/// The distance is `area * ratio / perimeter`, with a minimum of zero.
///
/// # Example
///
/// ```
/// use vtrans_ocr::geometry::{dilation_distance, Point};
///
/// let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
/// assert!((dilation_distance(&square, 2.0) - 5.0).abs() < 1e-6);
/// ```
#[must_use]
pub fn dilation_distance(polygon: &[Point], ratio: f32) -> f32 {
    let area = polygon_area(polygon);
    let perimeter = polygon_perimeter(polygon);
    if perimeter <= f32::EPSILON {
        0.0
    } else {
        (area * ratio / perimeter).max(0.0)
    }
}

/// Expand a convex polygon outward by a fixed distance.
///
/// Each edge is moved along its outward normal and adjacent offset edges are
/// intersected. Degenerate or parallel cases fall back to the midpoint of
/// the two offset vertices.
///
/// # Panics
///
/// Panics if the polygon has fewer than three vertices.
///
/// # Example
///
/// ```
/// use vtrans_ocr::geometry::{offset_polygon, polygon_area, Point};
///
/// let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
/// let expanded = offset_polygon(square, 1.0);
/// assert!(polygon_area(&expanded) > 100.0);
/// ```
#[must_use]
pub fn offset_polygon(polygon: [Point; 4], distance: f32) -> [Point; 4] {
    assert!(polygon.len() >= 3, "polygon must have at least 3 vertices");
    if distance <= 0.0 {
        return polygon;
    }

    let mut ring = polygon;
    if signed_area(&ring) < 0.0 {
        ring.reverse();
    }

    let mut result = [[0.0_f32, 0.0]; 4];
    for index in 0..4 {
        let previous = ring[(index + 3) % 4];
        let current = ring[index];
        let next = ring[(index + 1) % 4];

        let edge1 = [current[0] - previous[0], current[1] - previous[1]];
        let edge2 = [next[0] - current[0], next[1] - current[1]];
        let normal1 = outward_normal(edge1);
        let normal2 = outward_normal(edge2);
        let offset1 = [
            previous[0] + normal1[0] * distance,
            previous[1] + normal1[1] * distance,
        ];
        let offset2 = [
            current[0] + normal2[0] * distance,
            current[1] + normal2[1] * distance,
        ];

        let denom = edge1[0] * edge2[1] - edge1[1] * edge2[0];
        if denom.abs() < 1e-6 {
            result[index] = [
                (offset1[0] + offset2[0]) * 0.5,
                (offset1[1] + offset2[1]) * 0.5,
            ];
        } else {
            let delta = [offset2[0] - offset1[0], offset2[1] - offset1[1]];
            let t = (delta[0] * edge2[1] - delta[1] * edge2[0]) / denom;
            result[index] = [offset1[0] + edge1[0] * t, offset1[1] + edge1[1] * t];
        }
    }
    order_polygon(result)
}

/// Return the outward unit normal of a clockwise (y-down) polygon edge.
fn outward_normal(edge: [f32; 2]) -> [f32; 2] {
    let length = (edge[0] * edge[0] + edge[1] * edge[1]).sqrt().max(1e-6);
    [edge[1] / length, -edge[0] / length]
}

/// Compute the 3x3 homography that maps `src` to `dst`.
fn compute_homography(src: &[Point; 4], dst: &[Point; 4]) -> Result<[f64; 9], &'static str> {
    let mut matrix = [[0.0_f64; 8]; 8];
    let mut rhs = [0.0_f64; 8];
    for index in 0..4 {
        let (x, y) = (f64::from(src[index][0]), f64::from(src[index][1]));
        let (u, v) = (f64::from(dst[index][0]), f64::from(dst[index][1]));
        matrix[index * 2] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y];
        matrix[index * 2 + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y];
        rhs[index * 2] = u;
        rhs[index * 2 + 1] = v;
    }
    gaussian_solve(&mut matrix, &mut rhs)
        .map(|h| [h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.0])
}

/// Solve an 8x8 linear system in place with partial pivoting.
fn gaussian_solve(
    matrix: &mut [[f64; 8]; 8],
    rhs: &mut [f64; 8],
) -> Result<[f64; 8], &'static str> {
    for column in 0..8 {
        let pivot = (column..8)
            .max_by(|&a, &b| {
                matrix[a][column]
                    .abs()
                    .partial_cmp(&matrix[b][column].abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .ok_or("no pivot row")?;
        if matrix[pivot][column].abs() < 1e-9 {
            return Err("homography matrix is singular");
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);

        let pivot_value = matrix[column][column];
        let pivot_row = matrix[column];
        let factors: Vec<f64> = (0..8)
            .map(|row| {
                if row == column {
                    0.0
                } else {
                    matrix[row][column] / pivot_value
                }
            })
            .collect();
        for row in 0..8 {
            let factor = factors[row];
            if factor == 0.0 {
                continue;
            }
            for (inner, value) in matrix[row].iter_mut().enumerate().skip(column) {
                *value -= factor * pivot_row[inner];
            }
            rhs[row] -= factor * rhs[column];
        }
    }

    let mut solution = [0.0_f64; 8];
    for row in 0..8 {
        solution[row] = rhs[row] / matrix[row][row];
    }
    Ok(solution)
}

/// Invert a 3x3 matrix represented in row-major order.
fn invert_homography(homography: [f64; 9]) -> Result<[f64; 9], &'static str> {
    let [h00, h01, h02, h10, h11, h12, h20, h21, h22] = homography;
    let determinant = h00 * (h11 * h22 - h12 * h21) - h01 * (h10 * h22 - h12 * h20)
        + h02 * (h10 * h21 - h11 * h20);
    if determinant.abs() < 1e-12 {
        return Err("homography is singular");
    }
    let inv = 1.0 / determinant;
    Ok([
        (h11 * h22 - h12 * h21) * inv,
        (h02 * h21 - h01 * h22) * inv,
        (h01 * h12 - h02 * h11) * inv,
        (h12 * h20 - h10 * h22) * inv,
        (h00 * h22 - h02 * h20) * inv,
        (h02 * h10 - h00 * h12) * inv,
        (h10 * h21 - h11 * h20) * inv,
        (h01 * h20 - h00 * h21) * inv,
        (h00 * h11 - h01 * h10) * inv,
    ])
}

/// Sample a source pixel with bilinear interpolation and edge clamping.
fn sample_bilinear(rgb: &RgbImage, x: f32, y: f32) -> Rgb<u8> {
    let width = rgb.width().saturating_sub(1);
    let height = rgb.height().saturating_sub(1);
    let x = x.clamp(0.0, width as f32);
    let y = y.clamp(0.0, height as f32);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(width);
    let y1 = (y0 + 1).min(height);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let p00 = rgb.get_pixel(x0, y0);
    let p10 = rgb.get_pixel(x1, y0);
    let p01 = rgb.get_pixel(x0, y1);
    let p11 = rgb.get_pixel(x1, y1);

    let mut out = [0_u8; 3];
    for channel in 0..3 {
        let top = f32::from(p00[channel]) * (1.0 - fx) + f32::from(p10[channel]) * fx;
        let bottom = f32::from(p01[channel]) * (1.0 - fx) + f32::from(p11[channel]) * fx;
        out[channel] = (top * (1.0 - fy) + bottom * fy).round() as u8;
    }
    Rgb(out)
}

/// Warp the source quad into an axis-aligned destination rectangle.
///
/// The destination rectangle has size `(dst_width, dst_height)`. Source
/// corners are mapped to destination corners in the order returned by
/// [`order_polygon`].
///
/// # Panics
///
/// Panics if `dst_width` or `dst_height` is zero.
///
/// # Example
///
/// ```
/// use image::RgbImage;
/// use vtrans_ocr::geometry::warp_perspective;
///
/// let image = RgbImage::from_pixel(4, 4, image::Rgb([10, 20, 30]));
/// let src = [[0.0, 0.0], [3.0, 0.0], [3.0, 3.0], [0.0, 3.0]];
/// let warped = warp_perspective(&image, src, 4, 4);
/// assert_eq!(warped.dimensions(), (4, 4));
/// ```
#[must_use]
pub fn warp_perspective(
    rgb: &RgbImage,
    src: [Point; 4],
    dst_width: u32,
    dst_height: u32,
) -> RgbImage {
    assert!(
        dst_width > 0 && dst_height > 0,
        "destination must be non-empty"
    );
    let mut output = RgbImage::new(dst_width, dst_height);
    let src = order_polygon(src);
    if polygon_area(&src) < 0.5 {
        return warp_fallback_crop(rgb, &src, dst_width, dst_height);
    }

    let dst = [
        [0.0, 0.0],
        [(dst_width - 1) as f32, 0.0],
        [(dst_width - 1) as f32, (dst_height - 1) as f32],
        [0.0, (dst_height - 1) as f32],
    ];
    let Ok(homography) = compute_homography(&src, &dst) else {
        return image::imageops::resize(
            rgb,
            dst_width,
            dst_height,
            image::imageops::FilterType::Triangle,
        );
    };
    let Ok(inverse) = invert_homography(homography) else {
        return image::imageops::resize(
            rgb,
            dst_width,
            dst_height,
            image::imageops::FilterType::Triangle,
        );
    };

    for y in 0..dst_height {
        for x in 0..dst_width {
            let u = f64::from(x) + 0.5;
            let v = f64::from(y) + 0.5;
            let z = inverse[6] * u + inverse[7] * v + inverse[8];
            if z.abs() < 1e-12 {
                output.put_pixel(x, y, Rgb([0, 0, 0]));
                continue;
            }
            let sx = ((inverse[0] * u + inverse[1] * v + inverse[2]) / z) as f32;
            let sy = ((inverse[3] * u + inverse[4] * v + inverse[5]) / z) as f32;
            output.put_pixel(x, y, sample_bilinear(rgb, sx, sy));
        }
    }
    output
}

/// Crop the source quad's bounding box and resize it for degenerate quads.
fn warp_fallback_crop(
    rgb: &RgbImage,
    src: &[Point; 4],
    dst_width: u32,
    dst_height: u32,
) -> RgbImage {
    let max_x = rgb.width().saturating_sub(1) as f32;
    let max_y = rgb.height().saturating_sub(1) as f32;
    let min_x = src
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .clamp(0.0, max_x);
    let min_y = src
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .clamp(0.0, max_y);
    let crop_x = min_x as u32;
    let crop_y = min_y as u32;
    let width = ((max_x.min(
        src.iter()
            .map(|point| point[0])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil(),
    ) - min_x
        + 1.0)
        .max(1.0)) as u32;
    let height = ((max_y.min(
        src.iter()
            .map(|point| point[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil(),
    ) - min_y
        + 1.0)
        .max(1.0)) as u32;
    let crop = image::imageops::crop_imm(rgb, crop_x, crop_y, width, height).to_image();
    image::imageops::resize(
        &crop,
        dst_width,
        dst_height,
        image::imageops::FilterType::Triangle,
    )
}

/// Rotate an image 90 degrees clockwise.
///
/// # Example
///
/// ```
/// use image::{Rgb, RgbImage};
/// use vtrans_ocr::geometry::rotate_90_cw;
///
/// let image = RgbImage::from_pixel(2, 3, Rgb([0, 0, 0]));
/// let rotated = rotate_90_cw(&image);
/// assert_eq!(rotated.dimensions(), (3, 2));
/// ```
#[must_use]
pub fn rotate_90_cw(rgb: &RgbImage) -> RgbImage {
    let (width, height) = rgb.dimensions();
    let mut output = RgbImage::new(height, width);
    for y in 0..height {
        for x in 0..width {
            output.put_pixel(height - 1 - y, x, *rgb.get_pixel(x, y));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_and_perimeter_of_square() {
        let square = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        assert!((polygon_area(&square) - 16.0).abs() < 1e-6);
        assert!((polygon_perimeter(&square) - 16.0).abs() < 1e-6);
    }

    #[test]
    fn order_polygon_starts_top_left_clockwise() {
        let shuffled = [[4.0, 4.0], [0.0, 0.0], [4.0, 0.0], [0.0, 4.0]];
        let ordered = order_polygon(shuffled);
        assert_eq!(ordered, [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]);
    }

    #[test]
    fn min_area_rect_of_rotated_rectangle() {
        let points = [[2.0, 1.0], [6.0, 3.0], [5.0, 5.0], [1.0, 3.0]];
        let rect = min_area_rect(&points);
        assert!((polygon_area(&rect) - 10.0).abs() < 0.2);
    }

    #[test]
    fn dilation_distance_scales_area_perimeter() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert!((dilation_distance(&square, 2.0) - 5.0).abs() < 1e-6);
        assert!(dilation_distance(&square, 0.0).abs() < 1e-6);
    }

    #[test]
    fn offset_expands_polygon() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let expanded = offset_polygon(square, 2.0);
        assert!(polygon_area(&expanded) > 196.0 - 1e-3);
    }

    #[test]
    fn warp_identity_preserves_pixels() {
        let image = RgbImage::from_pixel(4, 4, image::Rgb([12, 34, 56]));
        let src = [[0.0, 0.0], [3.0, 0.0], [3.0, 3.0], [0.0, 3.0]];
        let warped = warp_perspective(&image, src, 4, 4);
        assert_eq!(warped.get_pixel(0, 0).0, [12, 34, 56]);
    }

    #[test]
    fn warp_degenerate_quad_falls_back_to_crop() {
        let mut image = RgbImage::new(8, 8);
        for x in 0..8 {
            for y in 0..8 {
                image.put_pixel(x, y, image::Rgb([255, 255, 255]));
            }
        }
        image.put_pixel(1, 1, image::Rgb([0, 0, 0]));
        image.put_pixel(2, 1, image::Rgb([0, 0, 0]));
        image.put_pixel(3, 1, image::Rgb([0, 0, 0]));
        let degenerate = [[1.0, 1.0], [3.0, 1.0], [3.0, 1.0], [1.0, 1.0]];
        let warped = warp_perspective(&image, degenerate, 4, 4);
        assert_eq!(warped.dimensions(), (4, 4));
        // The crop path keeps black pixels instead of stretching the full
        // white image, which would produce an all-white output.
        assert!(warped.pixels().any(|pixel| pixel.0 == [0, 0, 0]));
    }

    #[test]
    fn rotate_90_cw_swaps_dimensions() {
        let mut image = RgbImage::new(2, 3);
        image.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        let rotated = rotate_90_cw(&image);
        assert_eq!(rotated.dimensions(), (3, 2));
        assert_eq!(rotated.get_pixel(2, 0).0, [255, 0, 0]);
    }
}
