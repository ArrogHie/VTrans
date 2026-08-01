//! `VTrans` OCR recognition module.
//!
//! Implements the [`OcrProvider`](vtrans_core::traits::OcrProvider) trait
//! using PaddleOCR-style ONNX models. The pipeline runs text detection,
//! per-line perspective correction, recognition, CTC decoding, and
//! reading-order merging.
//!
//! See `docs/modules/05-ocr.md` for the full module specification.

pub mod detect;
pub mod geometry;
pub mod postprocess;
pub mod preprocess;
pub mod provider;
pub mod recognize;

pub use provider::PaddleOcrProvider;
