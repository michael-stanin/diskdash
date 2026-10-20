//! Filesystem scanner core for diskdash.
//!
//! Strictly read-only: this crate enumerates directories and reads metadata
//! (`FILE_READ_ATTRIBUTES` handles only). It never opens a file for its
//! contents or for writing. The only file it writes is the snapshot database
//! whose path the caller has already validated as the app's own.

pub mod alloc;
pub mod category;
pub mod ntfs;
pub mod paths;
pub mod reparse;
pub mod scan;
pub mod store;
pub mod win;
