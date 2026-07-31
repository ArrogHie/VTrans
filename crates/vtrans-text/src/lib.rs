//! `VTrans` text normalization module.
//! See docs/modules/06-text.md for full specification.

pub mod fingerprint;
pub mod japanese;
pub mod normalizer;
pub mod paragraph;

// TODO(feat/06-text): re-export TextNormalizer and is_duplicate once implemented.
