//! Cloud translation provider adapters.
//!
//! Each provider implements [`TranslationProviderAdapter`] plus a
//! [`vtrans_core::TranslationProvider`] wrapper that delegates to the shared
//! sender. The public, runtime-stable ids are `openai`, `deepl`, `google`,
//! `azure`, and `baidu`.

mod azure;
mod baidu;
mod deepl;
mod google;
mod language;
pub mod openai;

pub use azure::AzureTranslatorProvider;
pub use baidu::BaiduProvider;
pub use deepl::DeepLProvider;
pub use google::GoogleV2Provider;
pub use language::provider_error;
pub use openai::OpenAiProvider;
