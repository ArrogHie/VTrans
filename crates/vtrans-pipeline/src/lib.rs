// VTrans pipeline orchestration. See docs/modules/09-pipeline.md.

pub mod cancel;
pub mod dedup;
pub mod live;
pub mod single;

pub use live::Pipeline;
pub use single::run_single_capture;
