//! Cleanup plans: instruction templates, CSV/TXT export and verification logic.
//!
//! Plans are checklists the user carries out by hand. Nothing here (or
//! anywhere) deletes, moves or runs anything; exports contain paths and
//! instructions only, never a script.

pub mod export;
pub mod templates;
pub mod verify;

pub use export::{file_name, to_csv, to_txt, ExportItem};
pub use templates::{TemplateCtx, Templates};
pub use verify::{classify, lookup_alloc, lookup_entry, within_root, EntryRef, Outcome, Verdict};
