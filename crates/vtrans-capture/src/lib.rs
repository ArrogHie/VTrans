//! `VTrans` screen capture module.
//!
//! Provides [`WindowsCaptureSource`], which implements the
//! [`CaptureSource`](vtrans_core::traits::CaptureSource) trait using the
//! Windows Graphics Capture API. Supports multi-monitor enumeration,
//! per-monitor DPI scaling, single-shot capture, and continuous capture
//! sessions.
//!
//! See `docs/modules/04-capture.md` for the full module specification.

pub mod coordinates;
pub mod graphics_capture;
pub mod monitor;
pub mod session;
pub mod source;

pub use monitor::MonitorInfo;
pub use source::WindowsCaptureSource;
