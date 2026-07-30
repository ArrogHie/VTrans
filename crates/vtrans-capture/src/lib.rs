// VTrans screen capture module. See docs/modules/04-capture.md.

pub mod coordinates;
pub mod graphics_capture;
pub mod monitor;
pub mod session;
pub mod source;

pub use monitor::MonitorInfo;
pub use source::WindowsCaptureSource;
