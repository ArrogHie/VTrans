//! Image preprocessing for the OCR pipeline.
//!
//! Converts captured frames into RGB, optionally crops the requested screen
//! region, resizes images to the model input size, and normalizes pixel
//! values into the NCHW tensors consumed by ONNX Runtime.
//!
//! PP-OCR models are trained on OpenCV-style BGR frames: the manifest
//! `mean` / `std` arrays are indexed B,G,R and the tensor channels must be
//! written B,G,R (guide §6.3). The Python baseline reproduces this exactly
//! (`cv2.imread` + channel-wise normalization), and the Rust tensors match
//! the baseline byte-for-byte only when channels are swapped to BGR.

// Image dimensions are bounded by capture sizes; numeric conversions stay
// within reasonable ranges.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless
)]

use image::{imageops, RgbImage};
use ndarray::Array4;

use vtrans_core::error::OcrError;
use vtrans_core::types::{CapturedImage, PixelFormat, ScreenRegion};
use vtrans_models::PreprocessParams;

/// Minimum detection image side length, rounded up to a multiple of 32.
///
/// `PaddleOCR` detection models expect input dimensions that are multiples of
/// 32 so that the feature maps stay aligned.
pub const DET_MIN_SIDE: u32 = 32;
/// Detection dimension stride used by PP-OCR feature maps.
pub const DET_STRIDE: u32 = 32;

/// Legacy recognition input height retained for API compatibility.
///
/// PP-OCRv4 used a 32-pixel input height; PP-OCRv6 uses 48, and the
/// authoritative value comes from the manifest `rec_input_height` field.
/// This constant only backs unit tests and fallback code paths.
pub const REC_HEIGHT: u32 = 32;
/// Default recognition input width for PP-OCRv6 Small (guide §8 / §10.1).
///
/// The authoritative value comes from the manifest `rec_input_width` field;
/// this constant only backs unit tests and fallback code paths.
pub const REC_MAX_WIDTH: u32 = 320;
/// PP-OCR recognition normalization mean (BGR order).
pub const REC_MEAN: [f32; 3] = [0.5; 3];
/// PP-OCR recognition normalization standard deviation (BGR order).
pub const REC_STD: [f32; 3] = [0.5; 3];

/// Output of detection preprocessing.
///
/// Contains the normalized NCHW tensor plus scale factors used to map
/// detected boxes back into the original image coordinate space.
#[derive(Debug, Clone)]
pub struct DetInput {
    /// Normalized tensor with shape `(1, 3, height, width)`.
    pub tensor: Array4<f32>,
    /// Resized width divided by original width.
    pub ratio_x: f32,
    /// Resized height divided by original height.
    pub ratio_y: f32,
    /// Resized image width.
    pub width: u32,
    /// Resized image height.
    pub height: u32,
}

/// Convert a captured frame to an RGB image.
///
/// Both `Rgba8` and `Bgra8` formats are supported; the alpha channel is
/// discarded.
///
/// # Errors
///
/// Returns [`OcrError::Preprocess`] if the captured image is invalid or the
/// pixel buffer cannot be converted.
///
/// # Example
///
/// ```
/// use vtrans_core::types::{CapturedImage, PixelFormat};
/// use vtrans_ocr::preprocess::to_rgb;
///
/// let image = CapturedImage::new(1, 1, PixelFormat::Rgba8, vec![255, 0, 0, 255]).unwrap();
/// let rgb = to_rgb(&image).unwrap();
/// assert_eq!(rgb.get_pixel(0, 0).0, [255, 0, 0]);
/// ```
pub fn to_rgb(image: &CapturedImage) -> Result<RgbImage, OcrError> {
    image
        .validate()
        .map_err(|e| OcrError::Preprocess(e.to_string()))?;

    let mut rgb = Vec::with_capacity(image.width as usize * image.height as usize * 3);
    match image.format {
        PixelFormat::Rgba8 => {
            for pixel in image.data.chunks_exact(4) {
                rgb.extend_from_slice(&pixel[..3]);
            }
        }
        PixelFormat::Bgra8 => {
            for pixel in image.data.chunks_exact(4) {
                rgb.push(pixel[2]);
                rgb.push(pixel[1]);
                rgb.push(pixel[0]);
            }
        }
    }

    RgbImage::from_raw(image.width, image.height, rgb)
        .ok_or_else(|| OcrError::Preprocess("failed to build RGB image".to_string()))
}

/// Return the RGB image for the screen region to recognize.
///
/// When `image` is already exactly `region` sized (the capture crate crops
/// frames before delivery), the whole image is used. Otherwise the region is
/// cropped from the image and clamped to its bounds.
///
/// # Errors
///
/// Returns [`OcrError::Preprocess`] if the region lies entirely outside the
/// image or the image is invalid.
///
/// # Example
///
/// ```
/// use vtrans_core::types::{CapturedImage, PixelFormat, ScreenRegion};
/// use vtrans_ocr::preprocess::rgb_region;
///
/// let image = CapturedImage::new(4, 2, PixelFormat::Rgba8, vec![0; 32]).unwrap();
/// let region = ScreenRegion::new("m", 0, 0, 4, 2);
/// assert_eq!(rgb_region(&image, &region).unwrap().dimensions(), (4, 2));
/// ```
pub fn rgb_region(image: &CapturedImage, region: &ScreenRegion) -> Result<RgbImage, OcrError> {
    let rgb = to_rgb(image)?;
    if image.width == region.width
        && image.height == region.height
        && region.x == 0
        && region.y == 0
    {
        return Ok(rgb);
    }

    let x = u32::try_from(region.x)
        .map_err(|_| OcrError::Preprocess("region x is negative".to_string()))?;
    let y = u32::try_from(region.y)
        .map_err(|_| OcrError::Preprocess("region y is negative".to_string()))?;
    if x >= image.width || y >= image.height {
        return Err(OcrError::Preprocess(
            "region is entirely outside the image".to_string(),
        ));
    }

    let width = region.width.min(image.width - x);
    let height = region.height.min(image.height - y);
    if width == 0 || height == 0 {
        return Err(OcrError::Preprocess(
            "region has zero visible size".to_string(),
        ));
    }
    Ok(imageops::crop_imm(&rgb, x, y, width, height).to_image())
}

/// Resize and normalize an image for the detection model.
///
/// The image is scaled so that its longest side does not exceed the manifest
/// `image_size` limit, with each side independently rounded to a multiple of
/// 32 (matching the Python baseline `DetResizeForTest`), then normalized to
/// `(1, 3, H, W)` with channels written in BGR order (guide §6.3).
///
/// # Errors
///
/// Returns [`OcrError::Preprocess`] for empty images or invalid manifest
/// parameters.
///
/// # Example
///
/// ```
/// use image::RgbImage;
/// use vtrans_models::PreprocessParams;
/// use vtrans_ocr::preprocess::det_preprocess;
///
/// let params = PreprocessParams {
///     image_size: (64, 64),
///     mean: [0.5; 3],
///     std: [0.5; 3],
///     det_threshold: 0.3,
///     unclip_ratio: 1.5,
///     box_threshold: 0.45,
///     max_candidates: 3000,
///     min_box_size: 3.0,
///     rec_input_height: 48,
///     rec_input_width: 320,
///     rec_append_space: true,
///     rec_blank_index: 0,
/// };
/// let image = RgbImage::from_pixel(64, 32, image::Rgb([128, 128, 128]));
/// let input = det_preprocess(&image, &params).unwrap();
/// assert_eq!(input.tensor.shape(), &[1, 3, 32, 64]);
/// ```
pub fn det_preprocess(rgb: &RgbImage, params: &PreprocessParams) -> Result<DetInput, OcrError> {
    let width = rgb.width();
    let height = rgb.height();
    if width == 0 || height == 0 {
        return Err(OcrError::Preprocess(
            "cannot preprocess an empty image".to_string(),
        ));
    }

    let limit_width = params.image_size.0.max(1);
    let limit_height = params.image_size.1.max(1);
    let scale = f32::min(
        limit_width as f32 / width as f32,
        limit_height as f32 / height as f32,
    )
    .min(1.0);
    if scale <= 0.0 {
        return Err(OcrError::Preprocess(
            "invalid detection scale factor".to_string(),
        ));
    }

    // PP-OCR `DetResizeForTest`: each side is scaled by the same ratio and
    // rounded to the nearest multiple of 32 (never below the 32-pixel
    // minimum). The Python baseline computes
    // `max(32, round(side * ratio / 32) * 32)` — see guide §6.2 / §10.1.
    let det_width = det_side(width as f32 * scale);
    let det_height = det_side(height as f32 * scale);
    let resized = resize_bilinear_cv2(rgb, det_width, det_height);
    let tensor = normalize_bgr_to_tensor(&resized, &params.mean, &params.std);

    Ok(DetInput {
        tensor,
        ratio_x: det_width as f32 / width as f32,
        ratio_y: det_height as f32 / height as f32,
        width: det_width,
        height: det_height,
    })
}

/// Round a detection side length to the nearest multiple of 32.
///
/// PP-OCR `DetResizeForTest` rounds each side to a multiple of 32 (the
/// stride of the detection feature maps) with a 32-pixel minimum. The
/// half-away-from-zero `round` matches the Python baseline for the ratios
/// produced by image dimension scaling.
///
/// # Example
///
/// ```
/// use vtrans_ocr::preprocess::det_side;
/// assert_eq!(det_side(100.0), 96);
/// assert_eq!(det_side(10.0), 32);
/// ```
#[must_use]
pub fn det_side(value: f32) -> u32 {
    let stepped = (value / DET_STRIDE as f32).round() * DET_STRIDE as f32;
    stepped.max(DET_MIN_SIDE as f32) as u32
}

/// Normalize an RGB image into a CHW tensor with channels in RGB order.
///
/// Values are converted to `[0, 1]` and then standardized with the provided
/// per-channel mean and standard deviation. PP-OCR models expect BGR order;
/// use [`normalize_bgr_to_tensor`] for model inputs and this function only
/// when an RGB-ordered tensor is explicitly required.
///
/// # Panics
///
/// Panics if the tensor shape cannot represent the RGB image, which cannot
/// happen because the output length is derived from the image dimensions.
///
/// # Example
///
/// ```
/// use image::RgbImage;
/// use vtrans_ocr::preprocess::normalize_rgb_to_tensor;
///
/// let image = RgbImage::from_pixel(1, 1, image::Rgb([0, 255, 128]));
/// let tensor = normalize_rgb_to_tensor(&image, &[0.5; 3], &[0.5; 3]);
/// assert_eq!(tensor.shape(), &[1, 3, 1, 1]);
/// assert!((tensor[[0, 0, 0, 0]] + 1.0).abs() < 1e-6);
/// ```
#[must_use]
pub fn normalize_rgb_to_tensor(rgb: &RgbImage, mean: &[f32; 3], std: &[f32; 3]) -> Array4<f32> {
    normalize_to_tensor(rgb, mean, std, [0, 1, 2])
}

/// Normalize an RGB image into a CHW tensor with channels in BGR order.
///
/// PP-OCR models are trained on OpenCV-style BGR frames (guide §6.3). The
/// manifest `mean` / `std` arrays are indexed B,G,R; writing the tensor in
/// BGR order reproduces the Python baseline exactly.
///
/// # Example
///
/// ```
/// use image::RgbImage;
/// use vtrans_ocr::preprocess::normalize_bgr_to_tensor;
///
/// let image = RgbImage::from_pixel(1, 1, image::Rgb([255, 0, 0]));
/// let tensor = normalize_bgr_to_tensor(&image, &[0.0; 3], &[1.0; 3]);
/// // Red pixel: B channel (index 0) is 0, R channel (index 2) is 1.
/// assert!((tensor[[0, 0, 0, 0]]).abs() < 1e-6);
/// assert!((tensor[[0, 1, 0, 0]]).abs() < 1e-6);
/// assert!((tensor[[0, 2, 0, 0]] - 1.0).abs() < 1e-6);
/// ```
#[must_use]
pub fn normalize_bgr_to_tensor(rgb: &RgbImage, mean: &[f32; 3], std: &[f32; 3]) -> Array4<f32> {
    // Channel 0 = B = pixel[2], channel 1 = G = pixel[1], channel 2 = R =
    // pixel[0]. The BGR↔RGB swap only reorders the source bytes; the
    // per-channel mean/std are applied by destination channel index.
    normalize_to_tensor(rgb, mean, std, [2, 1, 0])
}

/// Shared CHW tensor construction with a caller-chosen channel mapping.
fn normalize_to_tensor(
    rgb: &RgbImage,
    mean: &[f32; 3],
    std: &[f32; 3],
    channel_map: [usize; 3],
) -> Array4<f32> {
    let width = rgb.width() as usize;
    let height = rgb.height() as usize;
    let plane = width * height;
    let mut data = vec![0.0_f32; plane * 3];

    for (index, pixel) in rgb.pixels().enumerate() {
        for channel in 0..3 {
            let source = channel_map[channel];
            let normalized = (f32::from(pixel[source]) / 255.0 - mean[channel]) / std[channel];
            data[channel * plane + index] = normalized;
        }
    }

    Array4::from_shape_vec((1, 3, height, width), data)
        .expect("normalized tensor length matches the requested shape")
}

/// Resize an RGB crop for the recognition model.
///
/// The image is scaled to `height` rows while preserving aspect ratio and
/// capped at `max_width` columns.
///
/// # Example
///
/// ```
/// use image::RgbImage;
/// use vtrans_ocr::preprocess::resize_rec_image;
///
/// let image = RgbImage::from_pixel(64, 16, image::Rgb([255, 255, 255]));
/// let resized = resize_rec_image(&image, 32, 320);
/// assert_eq!(resized.dimensions(), (128, 32));
/// ```
#[must_use]
pub fn resize_rec_image(rgb: &RgbImage, height: u32, max_width: u32) -> RgbImage {
    let src_width = rgb.width().max(1);
    let src_height = rgb.height().max(1);
    let mut scale = height as f32 / src_height as f32;
    let mut new_width = ((src_width as f32 * scale).round().max(1.0)) as u32;
    if new_width > max_width {
        scale = max_width as f32 / src_width as f32;
        new_width = max_width;
    }
    let new_height = ((src_height as f32 * scale).round().max(1.0)) as u32;
    resize_bilinear_cv2(rgb, new_width, new_height)
}

/// Bilinear resize with the `OpenCV` `INTER_LINEAR` sampling convention.
///
/// The Python baseline resizes with `cv2.resize(..., cv2.INTER_LINEAR)`,
/// whose source coordinate for destination pixel `d` is
/// `(d + 0.5) * src / dst - 0.5`, clamped to the image edges. The
/// `image` crate's `Triangle` filter instead low-pass averages on
/// downscaling, which visibly blurs glyph edges; replicating the cv2
/// convention keeps the detection tensor byte-comparable with the baseline
/// artifacts produced by `scripts/ppocrv6/baseline_ocr.py` (guide §14).
#[must_use]
fn resize_bilinear_cv2(rgb: &RgbImage, new_width: u32, new_height: u32) -> RgbImage {
    let src_width = rgb.width().max(1) as f32;
    let src_height = rgb.height().max(1) as f32;
    let scale_x = src_width / new_width.max(1) as f32;
    let scale_y = src_height / new_height.max(1) as f32;
    let mut output = RgbImage::new(new_width, new_height);

    for (dst_x, dst_y, pixel) in output.enumerate_pixels_mut() {
        let src_y = ((dst_y as f32 + 0.5) * scale_y - 0.5).clamp(0.0, src_height - 1.0);
        let src_x = ((dst_x as f32 + 0.5) * scale_x - 0.5).clamp(0.0, src_width - 1.0);
        let y0 = src_y.floor() as u32;
        let y1 = (y0 + 1).min(rgb.height() - 1);
        let x0 = src_x.floor() as u32;
        let x1 = (x0 + 1).min(rgb.width() - 1);
        let fy = src_y - src_y.floor();
        let fx = src_x - src_x.floor();

        let top_left = rgb.get_pixel(x0, y0).0;
        let top_right = rgb.get_pixel(x1, y0).0;
        let bottom_left = rgb.get_pixel(x0, y1).0;
        let bottom_right = rgb.get_pixel(x1, y1).0;
        for channel in 0..3 {
            let top = top_left[channel] as f32 * (1.0 - fx) + top_right[channel] as f32 * fx;
            let bottom =
                bottom_left[channel] as f32 * (1.0 - fx) + bottom_right[channel] as f32 * fx;
            let value = (top * (1.0 - fy) + bottom * fy).round() as u8;
            pixel.0[channel] = value;
        }
    }
    output
}

/// Prepare an RGB crop for recognition inference.
///
/// Resizes to `height` rows, right-pads with zeros to `width` columns
/// (PP-OCRv6 rec input is `[N, 3, 48, W]` with dynamic W; the Python
/// baseline always pads to the fixed width — guide §8.2 / §8.3), and
/// normalizes with `mean = std = 0.5`.
///
/// # Example
///
/// ```
/// use image::RgbImage;
/// use vtrans_ocr::preprocess::prepare_rec_input;
///
/// let image = RgbImage::from_pixel(64, 48, image::Rgb([0, 0, 0]));
/// let tensor = prepare_rec_input(&image, 48, 320);
/// assert_eq!(tensor.shape(), &[1, 3, 48, 320]);
/// ```
#[must_use]
pub fn prepare_rec_input(rgb: &RgbImage, height: u32, width: u32) -> Array4<f32> {
    let resized = resize_rec_image(rgb, height, width);
    let padded = pad_to_width(&resized, width);
    normalize_bgr_to_tensor(&padded, &REC_MEAN, &REC_STD)
}

/// Right-pad an image with black pixels to exactly `width` columns.
///
/// The recognition model input is `[N, 3, 48, W]` with a dynamic width; the
/// pipeline pads every crop to the manifest `rec_input_width` so the batch
/// stays regular. Padding pixels normalize to `-1.0`, matching the Python
/// baseline's zero-filled canvas (guide §8.2).
///
/// # Example
///
/// ```
/// use image::RgbImage;
/// use vtrans_ocr::preprocess::pad_to_width;
///
/// let image = RgbImage::from_pixel(100, 48, image::Rgb([255, 255, 255]));
/// let padded = pad_to_width(&image, 320);
/// assert_eq!(padded.dimensions(), (320, 48));
/// ```
#[must_use]
pub fn pad_to_width(rgb: &RgbImage, width: u32) -> RgbImage {
    if rgb.width() >= width {
        return rgb.clone();
    }
    let mut padded = RgbImage::new(width, rgb.height());
    image::imageops::replace(&mut padded, rgb, 0, 0);
    padded
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtrans_core::types::PixelFormat;

    fn captured(width: u32, height: u32, format: PixelFormat, fill: u8) -> CapturedImage {
        let len = (width * height * 4) as usize;
        CapturedImage::new(width, height, format, vec![fill; len]).unwrap()
    }

    #[test]
    fn to_rgb_rgba_keeps_rgb_order() {
        let image = CapturedImage::new(1, 1, PixelFormat::Rgba8, vec![10, 20, 30, 255]).unwrap();
        assert_eq!(to_rgb(&image).unwrap().get_pixel(0, 0).0, [10, 20, 30]);
    }

    #[test]
    fn to_rgb_bgra_swaps_channels() {
        let image = CapturedImage::new(1, 1, PixelFormat::Bgra8, vec![10, 20, 30, 255]).unwrap();
        assert_eq!(to_rgb(&image).unwrap().get_pixel(0, 0).0, [30, 20, 10]);
    }

    #[test]
    fn rgb_region_uses_full_image_when_same_size() {
        let image = captured(3, 2, PixelFormat::Bgra8, 128);
        let region = ScreenRegion::new("m", 0, 0, 3, 2);
        assert_eq!(rgb_region(&image, &region).unwrap().dimensions(), (3, 2));
    }

    #[test]
    fn rgb_region_crops_sub_region() {
        let data = vec![0_u8; 4 * 4 * 4];
        let image = CapturedImage::new(4, 4, PixelFormat::Rgba8, data).unwrap();
        let region = ScreenRegion::new("m", 1, 1, 2, 2);
        assert_eq!(rgb_region(&image, &region).unwrap().dimensions(), (2, 2));
    }

    #[test]
    fn rgb_region_rejects_outside_region() {
        let image = captured(2, 2, PixelFormat::Rgba8, 0);
        let region = ScreenRegion::new("m", 5, 5, 2, 2);
        assert!(matches!(
            rgb_region(&image, &region),
            Err(OcrError::Preprocess(_))
        ));
    }

    #[test]
    fn det_preprocess_uses_manifest_params_and_stride() {
        let params = PreprocessParams {
            image_size: (64, 64),
            mean: [0.0, 0.0, 0.0],
            std: [1.0, 1.0, 1.0],
            det_threshold: 0.3,
            unclip_ratio: 1.5,
            box_threshold: 0.45,
            max_candidates: 3000,
            min_box_size: 3.0,
            rec_input_height: 48,
            rec_input_width: 320,
            rec_append_space: true,
            rec_blank_index: 0,
        };
        let image = RgbImage::from_pixel(40, 20, image::Rgb([255, 128, 0]));
        let input = det_preprocess(&image, &params).unwrap();
        assert_eq!(input.tensor.shape(), &[1, 3, 32, 32]);
        assert_eq!(input.width, 32);
        assert_eq!(input.height, 32);
        assert!((input.ratio_x - 0.8).abs() < 1e-6);
        assert!((input.ratio_y - 1.6).abs() < 1e-6);
    }

    #[test]
    fn det_preprocess_rounds_sides_to_nearest_32_like_python_baseline() {
        // 1150x910 with a 640 limit: ratio = 640/1150 = 0.55652;
        // nw = round(1150*ratio/32)*32 = 640; nh = round(910*ratio/32)*32 = 512.
        let params = PreprocessParams {
            image_size: (640, 640),
            mean: [0.0, 0.0, 0.0],
            std: [1.0, 1.0, 1.0],
            det_threshold: 0.2,
            unclip_ratio: 1.4,
            box_threshold: 0.45,
            max_candidates: 3000,
            min_box_size: 3.0,
            rec_input_height: 48,
            rec_input_width: 320,
            rec_append_space: true,
            rec_blank_index: 0,
        };
        let image = RgbImage::from_pixel(1150, 910, image::Rgb([128, 128, 128]));
        let input = det_preprocess(&image, &params).unwrap();
        assert_eq!(input.width, 640);
        assert_eq!(input.height, 512);
        assert_eq!(input.tensor.shape(), &[1, 3, 512, 640]);
    }

    #[test]
    fn det_side_rounds_nearest_stride() {
        assert_eq!(det_side(100.0), 96);
        assert_eq!(det_side(112.0), 128);
        assert_eq!(det_side(10.0), 32);
        assert_eq!(det_side(640.0), 640);
    }

    #[test]
    fn normalize_uses_chw_layout() {
        let image = RgbImage::from_pixel(1, 1, image::Rgb([255, 0, 0]));
        let tensor = normalize_rgb_to_tensor(&image, &[0.0; 3], &[1.0; 3]);
        assert!((tensor[[0, 0, 0, 0]] - 1.0).abs() < 1e-6);
        assert!((tensor[[0, 1, 0, 0]]).abs() < 1e-6);
        assert!((tensor[[0, 2, 0, 0]]).abs() < 1e-6);
    }

    #[test]
    fn normalize_uses_bgr_channel_order_for_models() {
        let image = RgbImage::from_pixel(1, 1, image::Rgb([255, 0, 128]));
        let tensor = normalize_bgr_to_tensor(&image, &[0.0; 3], &[1.0; 3]);
        // BGR order: B = 128/255, G = 0, R = 255/255.
        assert!((tensor[[0, 0, 0, 0]] - 128.0 / 255.0).abs() < 1e-6);
        assert!((tensor[[0, 1, 0, 0]]).abs() < 1e-6);
        assert!((tensor[[0, 2, 0, 0]] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn resize_rec_respects_width_cap() {
        let image = RgbImage::from_pixel(640, 32, image::Rgb([0, 0, 0]));
        let resized = resize_rec_image(&image, 48, 320);
        // 640x32 -> 48-high would be 960 wide; capped to 320 keeps 16 rows.
        assert_eq!(resized.dimensions(), (320, 16));
    }

    #[test]
    fn prepare_rec_input_pads_to_fixed_width() {
        let image = RgbImage::from_pixel(100, 48, image::Rgb([0, 0, 0]));
        let tensor = prepare_rec_input(&image, 48, 320);
        assert_eq!(tensor.shape(), &[1, 3, 48, 320]);
    }

    #[test]
    fn pad_to_width_fills_right_side_with_black() {
        let image = RgbImage::from_pixel(2, 1, image::Rgb([255, 255, 255]));
        let padded = pad_to_width(&image, 4);
        assert_eq!(padded.dimensions(), (4, 1));
        assert_eq!(padded.get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(padded.get_pixel(3, 0).0, [0, 0, 0]);
    }
}
