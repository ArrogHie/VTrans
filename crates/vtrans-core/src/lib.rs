//! VTrans core types and traits.
//! See docs/modules/01-core.md for full specification.

pub mod error;
pub mod logging;
pub mod traits;
pub mod types;

pub use error::{CaptureError, CoreError, OcrError, TranslationError};
pub use logging::init_logging;
pub use traits::{CaptureSession, CaptureSource, OcrProvider, TranslationProvider};
pub use types::*;
