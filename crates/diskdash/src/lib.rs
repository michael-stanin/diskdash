//! diskdash server: a local, strictly read-only disk space dashboard.
//!
//! The binary (`main.rs`) wires these modules together; tests build the same
//! router with [`app::build_router`].

pub mod alerts;
pub mod app;
pub mod apps;
pub mod assets;
pub mod cleanup;
pub mod cleanup_sys;
pub mod config;
pub mod dup_sys;
pub mod duplicates;
pub mod error;
pub mod events;
pub mod helper;
pub mod history;
pub mod inventory;
pub mod jobs;
pub mod logging;
pub mod plans;
pub mod protection;
pub mod reports;
pub mod scans;
pub mod security;
pub mod settings;
pub mod shell;
pub mod snapshots;
pub mod stale;
pub mod system;
pub mod tiering;
pub mod time;
