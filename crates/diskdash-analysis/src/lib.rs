//! Cleanup rules for diskdash: the YAML rule model, loading with user overrides,
//! and evaluation against scan snapshots.
//!
//! Everything here is read-only with respect to the user's files. Rules only
//! describe what to look for and how the user can clean it up by hand.

pub mod duplicates;
pub mod eval;
pub mod rules;
pub mod stale;
