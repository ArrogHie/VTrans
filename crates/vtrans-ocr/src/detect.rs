//! Text detection model inference.
//!
//! Wraps the PP-OCR detection ONNX session and converts its probability-map
//! output into a 2D array for post-processing.

// ONNX shapes are trusted model metadata; conversions to `usize` are bounded
// by the model's declared dimensions.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use std::sync::Mutex;

use ndarray::{Array2, Array4, ArrayViewD};

use ort::session::{RunOptions, Session};
use ort::value::Tensor;

use vtrans_core::error::OcrError;

/// PP-OCR text detection model.
///
/// The ONNX `Session::run` API requires exclusive access, so the session is
/// guarded by a mutex. Detection and recognition can still run concurrently
/// with other CPU work because the provider executes inference on blocking
/// threads.
#[derive(Debug)]
pub struct Detector {
    session: Mutex<Session>,
    input_name: String,
    output_name: String,
}

impl Detector {
    /// Wrap a committed ONNX session as a text detector.
    ///
    /// The first session input is used as the image tensor and the first
    /// session output is used as the probability map.
    ///
    /// # Errors
    ///
    /// Returns [`OcrError::InvalidManifest`] if the model exposes no inputs
    /// or outputs.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ort::session::Session;
    /// use vtrans_ocr::detect::Detector;
    ///
    /// let session = Session::builder()?.commit_from_file("det.onnx")?;
    /// let detector = Detector::new(session)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(session: Session) -> Result<Self, OcrError> {
        let input_name = session
            .inputs()
            .first()
            .map(|input| input.name().to_string())
            .ok_or_else(|| {
                OcrError::InvalidManifest("detection model has no inputs".to_string())
            })?;
        let output_name = session
            .outputs()
            .first()
            .map(|output| output.name().to_string())
            .ok_or_else(|| {
                OcrError::InvalidManifest("detection model has no outputs".to_string())
            })?;
        tracing::debug!(
            input = %input_name,
            output = %output_name,
            "detection session io names"
        );
        Ok(Self {
            session: Mutex::new(session),
            input_name,
            output_name,
        })
    }

    /// Run detection inference and return the probability map.
    ///
    /// The input tensor must have shape `(1, 3, height, width)`.
    ///
    /// # Errors
    ///
    /// Returns [`OcrError::OrtRuntime`] when inference fails and
    /// [`OcrError::Postprocess`] when the output shape is unexpected.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ndarray::Array4;
    /// use ort::session::{RunOptions, Session};
    /// use vtrans_ocr::detect::Detector;
    ///
    /// let session = Session::builder()?.commit_from_file("det.onnx")?;
    /// let detector = Detector::new(session)?;
    /// let tensor = Array4::<f32>::zeros((1, 3, 64, 64));
    /// let run_options = RunOptions::new()?;
    /// let probability = detector.run(&tensor, &run_options)?;
    /// assert_eq!(probability.ndim(), 2);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn run(
        &self,
        input: &Array4<f32>,
        run_options: &RunOptions,
    ) -> Result<Array2<f32>, OcrError> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| OcrError::Inference("detection session mutex poisoned".to_string()))?;
        let shape = input.shape().to_vec();
        let data: Vec<f32> = input.iter().copied().collect();
        let tensor = Tensor::from_array((shape, data))
            .map_err(|e| OcrError::OrtRuntime(format!("create detection input: {e}")))?;
        let outputs = session
            .run_with_options(
                ort::inputs![self.input_name.as_str() => tensor],
                run_options,
            )
            .map_err(|e| OcrError::OrtRuntime(format!("detection inference failed: {e}")))?;

        let value = outputs.get(self.output_name.as_str()).ok_or_else(|| {
            OcrError::Inference("detection model returned no outputs".to_string())
        })?;
        let (shape, data) = value
            .try_extract_tensor::<f32>()
            .map_err(|e| OcrError::OrtRuntime(format!("extract detection output: {e}")))?;
        let shape: Vec<usize> = shape.iter().map(|dimension| *dimension as usize).collect();
        let array = ndarray::ArrayD::from_shape_vec(shape, data.to_vec())
            .map_err(|e| OcrError::OrtRuntime(format!("build detection output array: {e}")))?;
        extract_probability_map(&array.view())
    }
}

/// Normalize a detection model output into a `(height, width)` map.
///
/// Accepts common PP-OCR output shapes: `[H, W]`, `[1, H, W]`,
/// `[1, 1, H, W]`, and `[1, H, W, 1]`.
///
/// # Errors
///
/// Returns [`OcrError::Postprocess`] if the shape is not supported or the
/// array cannot be reshaped.
///
/// # Example
///
/// ```
/// use ndarray::ArrayD;
/// use vtrans_ocr::detect::extract_probability_map;
///
/// let array = ArrayD::from_shape_vec(vec![1, 2, 3], vec![0.0; 6]).unwrap();
/// let map = extract_probability_map(&array.view()).unwrap();
/// assert_eq!(map.dim(), (2, 3));
/// ```
pub fn extract_probability_map(array: &ArrayViewD<f32>) -> Result<Array2<f32>, OcrError> {
    let (height, width) = match array.shape() {
        [height, width]
        | [1, height, width]
        | [1, 1, height, width]
        | [height, width, 1]
        | [1, height, width, 1] => (*height, *width),
        shape => {
            return Err(OcrError::Postprocess(format!(
                "unsupported detection output shape: {shape:?}"
            )));
        }
    };
    array
        .to_owned()
        .into_shape_with_order((height, width))
        .map_err(|e| OcrError::Postprocess(format!("reshape detection output: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::ArrayD;

    fn array_from(shape: &[usize], value: f32) -> ArrayD<f32> {
        let len = shape.iter().product();
        ArrayD::from_shape_vec(shape.to_vec(), vec![value; len]).unwrap()
    }

    #[test]
    fn extracts_two_dimensional_map() {
        let map = extract_probability_map(&array_from(&[4, 5], 0.5).view()).unwrap();
        assert_eq!(map.dim(), (4, 5));
    }

    #[test]
    fn extracts_batched_map() {
        let map = extract_probability_map(&array_from(&[1, 4, 5], 0.5).view()).unwrap();
        assert_eq!(map.dim(), (4, 5));
    }

    #[test]
    fn extracts_batched_single_channel_map() {
        let map = extract_probability_map(&array_from(&[1, 1, 4, 5], 0.5).view()).unwrap();
        assert_eq!(map.dim(), (4, 5));
    }

    #[test]
    fn extracts_last_channel_map() {
        let map = extract_probability_map(&array_from(&[1, 4, 5, 1], 0.5).view()).unwrap();
        assert_eq!(map.dim(), (4, 5));
    }

    #[test]
    fn extracts_unbatched_last_channel_map() {
        let map = extract_probability_map(&array_from(&[4, 5, 1], 0.5).view()).unwrap();
        assert_eq!(map.dim(), (4, 5));
    }

    #[test]
    fn rejects_unsupported_shape() {
        assert!(matches!(
            extract_probability_map(&array_from(&[2, 3, 4], 0.5).view()),
            Err(OcrError::Postprocess(_))
        ));
    }
}
