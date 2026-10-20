//! Reading the NTFS Master File Table.
//!
//! Read-only end to end: the volume handle is opened with `GENERIC_READ`, the
//! parser works on byte slices with explicit bounds checks and contains no
//! `unsafe`, and the only thing written is the snapshot database, by the caller.
//!
//! - [`record`]: one FILE record (fixups, attributes) → [`record::RawRecord`]
//! - [`assemble`]: base and extension records → [`model::MftEntry`]
//! - [`import`]: entries → a snapshot database, in two passes with compact memory
//! - [`incremental`]: apply USN changes to a copy of a snapshot
//! - [`usn`]: USN journal records and the rules for when a quick rescan is possible
//! - [`volume`]: Windows volume access (used inside the elevated helper)

pub mod assemble;
pub mod import;
pub mod incremental;
pub mod model;
pub mod record;
pub mod runs;
pub mod usn;
#[cfg(windows)]
pub mod volume;

pub use model::{EntryName, MftEntry, MftInfo};
