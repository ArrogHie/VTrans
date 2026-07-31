// VTrans pipeline orchestration. See docs/modules/09-pipeline.md.

pub mod cancel;
pub mod dedup;
pub mod live;
pub mod single;

// TODO(feat/09-pipeline): re-export Pipeline and run_single_capture once implemented.
