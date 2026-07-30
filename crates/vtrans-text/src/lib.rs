//! VTrans text normalization module.
//! See docs/modules/06-text.md for full specification.

pub mod fingerprint;
pub mod japanese;
pub mod normalizer;
pub mod paragraph;

pub use normalizer::{is_duplicate, TextNormalizer};
