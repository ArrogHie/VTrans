//! VTrans model management module.
//! See docs/modules/08-models.md for full specification.

pub mod manager;
pub mod manifest;
pub mod path;
pub mod verify;

pub use manager::ModelManager;
pub use manifest::ModelManifest;
pub use verify::VerifyReport;
