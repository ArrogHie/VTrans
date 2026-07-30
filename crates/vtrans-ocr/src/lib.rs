// VTrans OCR recognition module. See docs/modules/05-ocr.md.

pub mod detect;
pub mod geometry;
pub mod postprocess;
pub mod preprocess;
pub mod provider;
pub mod recognize;

pub use provider::PaddleOcrProvider;
