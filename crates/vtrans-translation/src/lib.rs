//! Translation engine providers for `VTrans`.
//!
//! This crate implements [`vtrans_core::TranslationProvider`] with an
//! HTTP/JSON API provider and a local ONNX provider. Both providers support
//! cooperative cancellation, and the API provider adds configurable timeouts
//! and bounded retries with exponential backoff.
//!
//! See `docs/modules/07-translation.md` for the full module specification.

pub mod api;
pub mod local_onnx;
pub mod prompt;
pub mod retry;
pub mod validate;

pub use api::{parse_response, ApiTranslationProvider};
pub use local_onnx::LocalTranslationProvider;
pub use prompt::{build_system_prompt, build_translation_prompt};
pub use retry::RetryPolicy;
pub use validate::validate_language_pair;
