//! `VTrans` application layer.
//!
//! This crate is the Rust/ frontend boundary: it assembles production
//! providers, exposes Tauri commands, forwards pipeline events, and owns
//! global shortcut registration.

pub mod commands;
pub mod error;
pub mod events;
pub mod hotkeys;
pub mod setup;
pub mod state;

pub use commands::LiveTranslationConfig;
pub use error::AppError;
pub use events::{emit_model_loading_progress, emit_pipeline_event};
pub use hotkeys::register_hotkeys;
pub use setup::{app_handle, builder, init_app};
pub use state::{AppState, AppStatus};
