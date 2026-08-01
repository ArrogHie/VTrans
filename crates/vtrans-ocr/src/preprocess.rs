//! Image preprocessing for the OCR pipeline.
//!
//! Converts captured frames into RGB, optionally crops the requested screen
//! region, resizes images to the model input size, and normalizes pixel
//! values into the NCHW tensors consumed by ONNX Runtime.

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

/// Fixed recognition input height used by PP-OCR recognition models.
pub const REC_HEIGHT: u32 = 32;
/// Maximum recognition input width for the dynamic-width rec models.
pub const REC_MAX_WIDTH: u32 = 320;
/// PP-OCR recognition normalization mean (RGB order).
pub const REC_MEAN: [f32; 3] = [0.5; 3];
/// PP-OCR recognition normalization standard deviation (RGB order).
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

/// Resize and normalize an RGB image for the detection model.
///
/// The image is scaled so that its longest side does not exceed the manifest
/// `image_size` limit, rounded down to a multiple of 32, then normalized to
/// `(1, 3, H, W)`.
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
    let mut scale = f32::min(
        limit_width as f32 / width as f32,
        limit_height as f32 / height as f32,
    );
    scale = scale.min(1.0);
    if scale <= 0.0 {
        return Err(OcrError::Preprocess(
            "invalid detection scale factor".to_string(),
        ));
    }

    let det_width = det_side(width as f32 * scale);
    let det_height = det_side(height as f32 * scale);
    let resized = imageops::resize(rgb, det_width, det_height, imageops::FilterType::Triangle);
    let tensor = normalize_rgb_to_tensor(&resized, &params.mean, &params.std);

    Ok(DetInput {
        tensor,
        ratio_x: det_width as f32 / width as f32,
        ratio_y: det_height as f32 / height as f32,
        width: det_width,
        height: det_height,
    })
}

/// Round a detection side length down to a multiple of 32.
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
    let stepped = (value / DET_STRIDE as f32).floor() * DET_STRIDE as f32;
    stepped.max(DET_MIN_SIDE as f32) as u32
}

/// Normalize an RGB image into a CHW tensor.
///
/// Values are converted to `[0, 1]` and then standardized with the provided
/// per-channel mean and standard deviation.
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
    let width = rgb.width() as usize;
    let height = rgb.height() as usize;
    let plane = width * height;
    let mut data = vec![0.0_f32; plane * 3];

    for (index, pixel) in rgb.pixels().enumerate() {
        for channel in 0..3 {
            let normalized = (f32::from(pixel[channel]) / 255.0 - mean[channel]) / std[channel];
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
    imageops::resize(rgb, new_width, new_height, imageops::FilterType::Triangle)
}

/// Prepare an RGB crop for recognition inference.
///
/// Uses the PP-OCR standard 32-pixel height, 320-pixel maximum width, and
/// `mean = std = 0.5` normalization.
///
/// # Example
///
/// ```
/// use image::RgbImage;
/// use vtrans_ocr::preprocess::prepare_rec_input;
///
/// let image = RgbImage::from_pixel(64, 32, image::Rgb([0, 0, 0]));
/// let tensor = prepare_rec_input(&image);
/// assert_eq!(tensor.shape(), &[1, 3, 32, 64]);
/// ```
#[must_use]
pub fn prepare_rec_input(rgb: &RgbImage) -> Array4<f32> {
    let resized = resize_rec_image(rgb, REC_HEIGHT, REC_MAX_WIDTH);
    normalize_rgb_to_tensor(&resized, &REC_MEAN, &REC_STD)
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
    fn normalize_uses_chw_layout() {
        let image = RgbImage::from_pixel(1, 1, image::Rgb([255, 0, 0]));
        let tensor = normalize_rgb_to_tensor(&image, &[0.0; 3], &[1.0; 3]);
        assert!((tensor[[0, 0, 0, 0]] - 1.0).abs() < 1e-6);
        assert!((tensor[[0, 1, 0, 0]]).abs() < 1e-6);
        assert!((tensor[[0, 2, 0, 0]]).abs() < 1e-6);
    }

    #[test]
    fn resize_rec_respects_width_cap() {
        let image = RgbImage::from_pixel(640, 32, image::Rgb([0, 0, 0]));
        let resized = resize_rec_image(&image, 32, 320);
        assert_eq!(resized.dimensions(), (320, 16));
    }

    #[test]
    fn prepare_rec_input_uses_fixed_height() {
        let image = RgbImage::from_pixel(100, 16, image::Rgb([0, 0, 0]));
        let tensor = prepare_rec_input(&image);
        assert_eq!(tensor.shape(), &[1, 3, 32, 200]);
    }
}
