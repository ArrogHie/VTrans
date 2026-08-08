//! Translation engine providers for `VTrans`.
//!
//! This crate implements [`vtrans_core::TranslationProvider`] with:
//!
//! * [`ApiTranslationProvider`] — HTTP/JSON API translation with timeouts,
//!   cancellation, and bounded retries.
//! * [`NativeTranslationProvider`] — local dual-engine translation
//!   (`Bergamot` en→zh + `CTranslate2` INT8 ja→zh) through the C++ bridge in
//!   `native/translation_bridge/`, loaded dynamically via [`ffi`].
//!
//! The local ONNX path (`LocalTranslationProvider`) was removed in v0.3.0
//! (decision A3); see `docs/modules/07-translation.md` for the full module
//! specification.

pub mod api;
pub mod ffi;
pub mod native;
pub mod prompt;
pub mod retry;
pub mod validate;

pub use api::{parse_response, ApiTranslationProvider};
pub use ffi::NativeTranslator;
pub use native::{NativeTranslationProvider, TranslationQuality, NATIVE_PROVIDER_ID};
pub use prompt::{build_system_prompt, build_translation_prompt};
pub use retry::RetryPolicy;
pub use validate::validate_language_pair;
