// VTrans OCR recognition module. See docs/modules/05-ocr.md.

pub mod detect;
pub mod geometry;
pub mod postprocess;
pub mod preprocess;
pub mod provider;
pub mod recognize;

// TODO(feat/05-ocr): re-export PaddleOcrProvider once implemented.
