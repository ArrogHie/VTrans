//! Text recognition model inference.
//!
//! Wraps a PP-OCR recognition ONNX session, prepares fixed-height crops,
//! runs inference, and decodes the CTC logits into text.

// ONNX shapes are trusted model metadata; conversions to `usize` are bounded
// by the model's declared dimensions.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use std::sync::Mutex;

use image::RgbImage;
use ndarray::{Array2, ArrayViewD};

use ort::session::{RunOptions, Session};
use ort::value::Tensor;

use vtrans_core::error::OcrError;

use crate::postprocess::{ctc_greedy_decode, RecognizedLine};
use crate::preprocess::prepare_rec_input;

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
        let tensor = prepare_rec_input(rgb);
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
}

/// Decode a recognition logits array with greedy CTC.
///
/// Accepts `[T, C]` and `[1, T, C]` layouts.
///
/// # Errors
///
/// Returns [`OcrError::Postprocess`] if the array shape is unsupported or
/// the class count does not match the dictionary.
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
}
