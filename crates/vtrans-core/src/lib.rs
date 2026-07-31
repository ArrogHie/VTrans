//! `VTrans` core types and traits.
//!
//! This crate defines the shared data structures, provider traits, error
//! types, and logging utilities used across all `VTrans` crates. Every other
//! `vtrans-*` crate depends on this one and must import types from here
//! rather than redefining them.
//!
//! See `docs/modules/01-core.md` for the full module specification.

pub mod error;
pub mod logging;
pub mod traits;
pub mod types;

pub use error::{CaptureError, CoreError, OcrError, TranslationError};
pub use logging::{init_logging, mask_sensitive, truncate_for_log};
pub use traits::{CaptureSession, CaptureSource, OcrProvider, TranslationProvider};
pub use types::*;
