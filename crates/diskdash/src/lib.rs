//! diskdash server: a local, strictly read-only disk space dashboard.
//!
//! The binary (`main.rs`) wires these modules together; tests build the same
//! router with [`app::build_router`].

pub mod app;
pub mod assets;
pub mod config;
pub mod error;
pub mod events;
pub mod inventory;
pub mod jobs;
pub mod logging;
pub mod scans;
pub mod security;
pub mod settings;
pub mod snapshots;
pub mod system;
pub mod time;
