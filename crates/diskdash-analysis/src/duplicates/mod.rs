//! Duplicate detection: size, then partial hash, then full BLAKE3 hash.
//!
//! This is the one place that reads file contents. Files are only ever opened
//! for reading (through the [`engine::Source`] trait, whose Windows
//! implementation uses a plain read-only open with full sharing). Nothing here
//! modifies, moves, links or deletes a file or its timestamps.

pub mod candidates;
pub mod engine;
pub mod near;
pub mod rules;

pub use candidates::{select, Candidate, ExcludedCounts, ScopeSnapshot, Selection};
pub use engine::{
    find_duplicates, CacheKey, CachedHash, Cancelled, EngineEnv, FileId, Group, HashCache, MemoryCache, OpenError,
    OpenFile, Outcome, Skip, SkipReason, Source,
};
pub use near::{near_duplicates, NearPair, NearResult};
pub use rules::{choose_keep, KeepRule, Member};

/// Hash algorithm recorded with every result.
pub const ALGO: &str = "blake3";
/// Bytes read from each end of a file for the partial hash.
pub const PARTIAL_BYTES: u64 = 64 * 1024;
/// Streaming chunk for the full hash; cancellation is checked between chunks.
pub const CHUNK: usize = 1024 * 1024;
