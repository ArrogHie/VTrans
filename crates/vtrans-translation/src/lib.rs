//! Translation engine providers for `VTrans`.
//!
//! This crate implements [`vtrans_core::TranslationProvider`] with a local
//! ONNX provider and a pluggable set of cloud providers (`OpenAI`, `DeepL`,
//! Google, Azure, and Baidu). All providers support cooperative
//! cancellation; cloud providers add configurable timeouts and bounded
//! retries with exponential backoff.
//!
//! See `docs/modules/07-translation.md` for the full module specification.

pub mod adapter;
pub mod api;
pub mod auth;
pub mod local_onnx;
pub mod prompt;
pub mod providers;
pub mod retry;
pub mod validate;

pub use adapter::{
    send_with_adapter, OutgoingRequest, ParsedTranslation, RetryDecision,
    TranslationProviderAdapter,
};
pub use api::{parse_response, ApiTranslationProvider};
pub use auth::AuthStrategy;
pub use local_onnx::LocalTranslationProvider;
pub use prompt::{build_system_prompt, build_translation_prompt};
pub use providers::{
    AzureTranslatorProvider, BaiduProvider, DeepLProvider, GoogleV2Provider, OpenAiProvider,
};
pub use retry::RetryPolicy;
pub use validate::validate_language_pair;
