//! Text recognition model inference.
//!
//! Wraps a PP-OCR recognition ONNX session, prepares fixed-height crops,
//! runs inference, and decodes the CTC logits into text.

// ONNX shapes are trusted model metadata; conversions to `usize` are bounded
// by the model's declared dimensions.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::sync::Mutex;

use image::RgbImage;
use ndarray::{Array2, Array4, ArrayViewD};

use ort::session::{RunOptions, Session};
use ort::value::Tensor;

use vtrans_core::error::OcrError;

use crate::postprocess::{ctc_greedy_decode, RecognizedLine};
use crate::preprocess::{
    normalize_rgb_to_tensor, resize_rec_image, REC_HEIGHT, REC_MAX_WIDTH, REC_MEAN, REC_STD,
};

/// Maximum width of a single recognition chunk in pixels.
///
/// Long text lines are recognized in chunks no wider than this value so the
/// input stays within the model's conventional dynamic-width limit while the
/// character height stays at [`REC_HEIGHT`].
const REC_CHUNK_WIDTH: u32 = REC_MAX_WIDTH;

/// Pixel overlap between adjacent recognition chunks.
///
/// A character that happens to sit on a chunk boundary is fully visible in
/// the next chunk; the overlapping decoded text is removed again by
/// [`trim_overlap_text`].
const REC_CHUNK_OVERLAP: u32 = 16;

/// PP-OCR text recognition model.
///
/// Each recognizer owns its ONNX session, input/output names, and character
/// dictionary. Sessions are initialized once when the provider is created.
#[derive(Debug)]
pub struct Recognizer {
    session: Mutex<Session>,
    input_name: String,
    output_name: String,
    dict: Vec<String>,
}

impl Recognizer {
    /// Wrap a committed ONNX session as a recognizer.
    ///
    /// The dictionary must contain the CTC blank at index `0`, either as an
    /// explicit empty first line or added automatically during provider
    /// construction.
    ///
    /// # Errors
    ///
    /// Returns [`OcrError::InvalidManifest`] if the model has no inputs or
    /// outputs, or the dictionary is empty.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ort::session::Session;
    /// use vtrans_ocr::recognize::Recognizer;
    ///
    /// let session = Session::builder()?.commit_from_file("rec.onnx")?;
    /// let recognizer = Recognizer::new(session, vec![String::new(), "a".to_string()])?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(session: Session, dict: Vec<String>) -> Result<Self, OcrError> {
        if dict.is_empty() {
            return Err(OcrError::InvalidManifest(
                "recognition dictionary is empty".to_string(),
            ));
        }
        let input_name = session
            .inputs()
            .first()
            .map(|input| input.name().to_string())
            .ok_or_else(|| {
                OcrError::InvalidManifest("recognition model has no inputs".to_string())
            })?;
        let output_name = session
            .outputs()
            .first()
            .map(|output| output.name().to_string())
            .ok_or_else(|| {
                OcrError::InvalidManifest("recognition model has no outputs".to_string())
            })?;
        tracing::debug!(
            input = %input_name,
            output = %output_name,
            dict_size = dict.len(),
            "recognition session io names"
        );
        Ok(Self {
            session: Mutex::new(session),
            input_name,
            output_name,
            dict,
        })
    }

    /// Recognize a single text-line crop.
    ///
    /// # Errors
    ///
    /// Returns [`OcrError::OrtRuntime`] when inference fails and
    /// [`OcrError::Postprocess`] when the output shape or dictionary is
    /// incompatible.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use image::RgbImage;
    /// use ort::session::{RunOptions, Session};
    /// use vtrans_ocr::recognize::Recognizer;
    ///
    /// let session = Session::builder()?.commit_from_file("rec.onnx")?;
    /// let recognizer = Recognizer::new(session, vec![String::new(), "a".to_string()])?;
    /// let crop = RgbImage::from_pixel(32, 32, image::Rgb([0, 0, 0]));
    /// let run_options = RunOptions::new()?;
    /// let line = recognizer.run(&crop, &run_options)?;
    /// assert!(!line.text.is_empty() || line.confidence == 0.0);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn run(
        &self,
        rgb: &RgbImage,
        run_options: &RunOptions,
    ) -> Result<RecognizedLine, OcrError> {
        let resized = resize_rec_image(rgb, REC_HEIGHT, u32::MAX);
        if resized.width() <= REC_CHUNK_WIDTH {
            let tensor = normalize_rgb_to_tensor(&resized, &REC_MEAN, &REC_STD);
            return self.run_tensor(&tensor, run_options);
        }
        self.run_split(&resized, run_options)
    }

    /// Run a single inference pass over a normalized tensor.
    fn run_tensor(
        &self,
        tensor: &Array4<f32>,
        run_options: &RunOptions,
    ) -> Result<RecognizedLine, OcrError> {
        let shape = tensor.shape().to_vec();
        let data: Vec<f32> = tensor.iter().copied().collect();
        let mut session = self
            .session
            .lock()
            .map_err(|_| OcrError::Inference("recognition session mutex poisoned".to_string()))?;
        let input = Tensor::from_array((shape, data))
            .map_err(|e| OcrError::OrtRuntime(format!("create recognition input: {e}")))?;
        let outputs = session
            .run_with_options(ort::inputs![self.input_name.as_str() => input], run_options)
            .map_err(|e| OcrError::OrtRuntime(format!("recognition inference failed: {e}")))?;
        let value = outputs.get(self.output_name.as_str()).ok_or_else(|| {
            OcrError::Inference("recognition model returned no outputs".to_string())
        })?;
        let (shape, data) = value
            .try_extract_tensor::<f32>()
            .map_err(|e| OcrError::OrtRuntime(format!("extract recognition output: {e}")))?;
        let shape: Vec<usize> = shape.iter().map(|dimension| *dimension as usize).collect();
        let logits = ndarray::ArrayD::from_shape_vec(shape, data.to_vec())
            .map_err(|e| OcrError::OrtRuntime(format!("build recognition output array: {e}")))?;
        let (text, confidence) = decode_logits(&logits.view(), &self.dict)?;
        Ok(RecognizedLine { text, confidence })
    }

    /// Recognize a wide line by splitting it into overlapping chunks and
    /// concatenating the decoded texts in order.
    fn run_split(
        &self,
        resized: &RgbImage,
        run_options: &RunOptions,
    ) -> Result<RecognizedLine, OcrError> {
        let chunks = split_rec_chunks(resized, REC_CHUNK_WIDTH, REC_CHUNK_OVERLAP);
        tracing::debug!(
            image_width = resized.width(),
            chunks = chunks.len(),
            "wide line split into recognition chunks"
        );
        let mut text = String::new();
        let mut confidence_sum = 0.0_f32;
        for chunk in &chunks {
            let tensor = normalize_rgb_to_tensor(chunk, &REC_MEAN, &REC_STD);
            let line = self.run_tensor(&tensor, run_options)?;
            text = trim_overlap_text(&text, &line.text);
            confidence_sum += line.confidence;
        }
        Ok(RecognizedLine {
            text,
            confidence: confidence_sum / chunks.len() as f32,
        })
    }
}

/// Split a resized recognition image into overlapping horizontal chunks.
///
/// Each chunk is at most `chunk_width` pixels wide; later chunks start
/// `chunk_width - overlap` pixels after the previous chunk so that a
/// character cut by a boundary is captured completely by the next chunk.
///
/// # Example
///
/// ```
/// use image::RgbImage;
/// use vtrans_ocr::recognize::split_rec_chunks;
///
/// let image = RgbImage::from_pixel(700, 32, image::Rgb([0, 0, 0]));
/// let chunks = split_rec_chunks(&image, 320, 16);
/// assert_eq!(chunks.len(), 3);
/// assert!(chunks.iter().all(|chunk| chunk.width() <= 320));
/// ```
#[must_use]
pub fn split_rec_chunks(rgb: &RgbImage, chunk_width: u32, overlap: u32) -> Vec<RgbImage> {
    let width = rgb.width();
    if width == 0 {
        return Vec::new();
    }
    let step = chunk_width.saturating_sub(overlap).max(1);
    let mut chunks = Vec::new();
    let mut x = 0_u32;
    while x < width {
        let end = (x + chunk_width).min(width);
        chunks.push(image::imageops::crop_imm(rgb, x, 0, end - x, rgb.height()).to_image());
        if end == width {
            break;
        }
        x += step;
    }
    chunks
}

/// Append `next` to `prev`, removing the longest suffix of `prev` that also
/// starts `next`. The removed overlap corresponds to the pixel region shared
/// by adjacent chunks, which is decoded by both.
///
/// # Example
///
/// ```
/// use vtrans_ocr::recognize::trim_overlap_text;
///
/// assert_eq!(trim_overlap_text("a replica of an", "of an Olmec"), "a replica of an Olmec");
/// assert_eq!(trim_overlap_text("first", "second"), "firstsecond");
/// ```
#[must_use]
pub fn trim_overlap_text(prev: &str, next: &str) -> String {
    let prev_chars: Vec<char> = prev.chars().collect();
    let next_chars: Vec<char> = next.chars().collect();
    let max = prev_chars.len().min(next_chars.len());
    for length in (2..=max).rev() {
        let suffix = &prev_chars[prev_chars.len() - length..];
        let prefix = &next_chars[..length];
        if suffix == prefix {
            let mut merged = prev_chars[..prev_chars.len() - length].to_vec();
            merged.extend_from_slice(&next_chars);
            return merged.into_iter().collect();
        }
    }
    format!("{prev}{next}")
}

/// Decode a recognition logits array with greedy CTC.
///
/// Accepts `[T, C]` and `[1, T, C]` layouts.
///
/// # Errors
///
/// Returns [`OcrError::Postprocess`] if the array shape is unsupported or
/// the class count does not match the dictionary. A mismatch usually means
/// the dictionary and the recognition model come from different PP-OCR
/// releases; use the dictionary shipped with the same model version
/// (`num_classes = dictionary line count + 1`, blank at index 0).
///
/// # Example
///
/// ```
/// use ndarray::ArrayD;
/// use vtrans_ocr::recognize::decode_logits;
///
/// let dict = ["", "a", "b"].map(String::from).to_vec();
/// // t0: a, t1: blank, t2: b
/// let logits = ArrayD::from_shape_vec(
///     vec![1, 3, 3],
///     vec![0.1, 0.9, 0.0, 0.9, 0.1, 0.0, 0.1, 0.0, 0.9],
/// ).unwrap();
/// let (text, _) = decode_logits(&logits.view(), &dict).unwrap();
/// assert_eq!(text, "ab");
/// ```
pub fn decode_logits(logits: &ArrayViewD<f32>, dict: &[String]) -> Result<(String, f32), OcrError> {
    let (width, classes) = match logits.shape() {
        [width, classes] | [1, width, classes] => (*width, *classes),
        shape => {
            return Err(OcrError::Postprocess(format!(
                "unsupported recognition output shape: {shape:?}"
            )));
        }
    };
    if classes != dict.len() {
        return Err(OcrError::Postprocess(format!(
            "recognition classes {classes} do not match dictionary size {}",
            dict.len()
        )));
    }
    let array: Array2<f32> = logits
        .to_owned()
        .into_shape_with_order((width, classes))
        .map_err(|e| OcrError::Postprocess(format!("reshape recognition output: {e}")))?;
    ctc_greedy_decode(array.as_slice().unwrap_or_default(), width, dict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::ArrayD;

    #[test]
    fn decode_two_dimensional_logits() {
        let dict = ["", "a"].map(String::from).to_vec();
        let logits = ArrayD::from_shape_vec(vec![2, 2], vec![0.1, 0.9, 0.9, 0.1]).unwrap();
        let (text, _) = decode_logits(&logits.view(), &dict).unwrap();
        assert_eq!(text, "a");
    }

    #[test]
    fn decode_batched_logits() {
        let dict = ["", "a", "b"].map(String::from).to_vec();
        let logits = ArrayD::from_shape_vec(
            vec![1, 3, 3],
            vec![0.1, 0.9, 0.0, 0.1, 0.0, 0.9, 0.9, 0.1, 0.0],
        )
        .unwrap();
        let (text, _) = decode_logits(&logits.view(), &dict).unwrap();
        assert_eq!(text, "ab");
    }

    #[test]
    fn decode_rejects_unsupported_rank() {
        let dict = ["", "a"].map(String::from).to_vec();
        let logits = ArrayD::from_shape_vec(vec![2, 2, 2, 1], vec![0.0; 8]).unwrap();
        assert!(matches!(
            decode_logits(&logits.view(), &dict),
            Err(OcrError::Postprocess(_))
        ));
    }

    #[test]
    fn decode_rejects_class_mismatch() {
        let dict = ["", "a"].map(String::from).to_vec();
        let logits = ArrayD::from_shape_vec(vec![2, 3], vec![0.0; 6]).unwrap();
        assert!(matches!(
            decode_logits(&logits.view(), &dict),
            Err(OcrError::Postprocess(_))
        ));
    }

    #[test]
    fn split_narrow_image_returns_single_chunk() {
        let image = RgbImage::from_pixel(319, 32, image::Rgb([0, 0, 0]));
        let chunks = split_rec_chunks(&image, REC_CHUNK_WIDTH, REC_CHUNK_OVERLAP);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].dimensions(), (319, 32));
    }

    #[test]
    fn split_wide_image_chunks_with_overlap() {
        let image = RgbImage::from_pixel(700, 32, image::Rgb([0, 0, 0]));
        let chunks = split_rec_chunks(&image, 320, 16);
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|chunk| chunk.width() <= 320));
        assert_eq!(chunks[0].width(), 320);
        assert_eq!(chunks[1].width(), 320);
        assert_eq!(chunks[2].width(), 92);
        // Each seam contributes exactly `overlap` extra pixels.
        let actual_total: u32 = chunks.iter().map(image::RgbImage::width).sum();
        let expected_total = image.width() + (chunks.len() as u32 - 1) * 16;
        assert_eq!(actual_total, expected_total);
    }

    #[test]
    fn split_exact_multiple_stops_without_tail() {
        let image = RgbImage::from_pixel(640, 32, image::Rgb([0, 0, 0]));
        let chunks = split_rec_chunks(&image, 320, 0);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].width(), 320);
        assert_eq!(chunks[1].width(), 320);
    }

    #[test]
    fn split_empty_image_returns_no_chunks() {
        let image = RgbImage::new(0, 0);
        assert!(split_rec_chunks(&image, 320, 16).is_empty());
    }

    #[test]
    fn trim_overlap_removes_shared_region() {
        assert_eq!(
            trim_overlap_text("a replica of an", "of an Olmec"),
            "a replica of an Olmec"
        );
    }

    #[test]
    fn trim_overlap_no_match_concatenates() {
        assert_eq!(trim_overlap_text("first", "second"), "firstsecond");
        assert_eq!(trim_overlap_text("", "second"), "second");
    }

    #[test]
    fn trim_overlap_uses_longest_match() {
        // "ab" is shared but "cab" is longer and wins.
        assert_eq!(trim_overlap_text("xcab", "cabine"), "xcabine");
    }
}
