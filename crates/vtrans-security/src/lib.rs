//! VTrans credential security module.
//! See docs/modules/03-security.md for full specification.

pub mod manager;
pub mod mask;

pub use manager::CredentialManager;
pub use mask::mask_key;
