//! VTrans configuration management.
//! See docs/modules/02-config.md for full specification.

pub mod defaults;
pub mod manager;
pub mod migration;
pub mod schema;
pub mod validation;

pub use manager::ConfigManager;
pub use schema::AppConfig;
