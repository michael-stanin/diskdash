//! Stale-data analysis: last-access reliability and snapshot queries.

pub mod lastaccess;
pub mod query;

pub use lastaccess::{decide, parse_fsutil, query_setting, BasisReason, Decision, Setting, TimeBasis};
pub use query::{invalid_time_count, old_large, stale, StalePage, StaleQuery, StaleRow};
