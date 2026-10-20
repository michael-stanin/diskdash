//! SQLite storage for diskdash.
//!
//! - [`datadir`]: where the app keeps its own state.
//! - [`own_files`]: the ONLY code allowed to create, write or delete files. It
//!   refuses any path outside the app data directory.
//! - [`db`]: connection factory (WAL, foreign keys, busy timeout).
//! - [`migrate`]: embedded, numbered SQL migrations.

pub mod datadir;
pub mod db;
pub mod migrate;
pub mod own_files;

pub use datadir::DataDir;
pub use db::{open_catalog, Db};
pub use migrate::{migrate, Migration, MigrationError, MIGRATIONS};
pub use own_files::{write_report_file, OwnFiles, OwnFilesError, ReportWriteError, REPORT_EXTENSIONS};
