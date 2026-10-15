//! Verification of plan items against a later scan. Pure snapshot data: the
//! functions here take a database connection and numbers, never a file path to open.

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Gone from the scan.
    Removed,
    /// Same size as before.
    Present,
    /// Still there but smaller.
    Partial,
    /// Bigger than before.
    Grown,
    /// Gone from the source, present at the destination (manual-move items).
    Moved,
}

impl Outcome {
    pub fn key(self) -> &'static str {
        match self {
            Outcome::Removed => "removed",
            Outcome::Present => "present",
            Outcome::Partial => "partial",
            Outcome::Grown => "grown",
            Outcome::Moved => "moved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub outcome: Outcome,
    /// Bytes freed (allocated). Negative when the item grew.
    pub freed: i64,
}

/// Classify one item. `current` is its allocated size in the new scan (`None` =
/// not there). `destination` is `Some(alloc)` when a manual-move item's destination exists.
pub fn classify(baseline: u64, current: Option<u64>, destination: Option<u64>) -> Verdict {
    let b = baseline as i64;
    match current {
        None => match destination {
            Some(_) => Verdict { outcome: Outcome::Moved, freed: b },
            None => Verdict { outcome: Outcome::Removed, freed: b },
        },
        Some(c) => {
            let c = c as i64;
            let tolerance = (b / 100).max(4096);
            if (c - b).abs() <= tolerance {
                Verdict { outcome: Outcome::Present, freed: 0 }
            } else if c < b {
                Verdict { outcome: Outcome::Partial, freed: b - c }
            } else {
                Verdict { outcome: Outcome::Grown, freed: b - c }
            }
        }
    }
}

/// Is `path` inside the scan root (case-insensitive, component boundaries)?
pub fn within_root(root: &str, path: &str) -> bool {
    let (r, p) = (root.trim_end_matches('\\').to_lowercase(), path.to_lowercase());
    p == r || p.strip_prefix(&r).is_some_and(|rest| rest.starts_with('\\'))
}

/// Allocated size of `path` in a snapshot, walking names from the root; `None` when it is not there.
pub fn lookup_alloc(conn: &Connection, root: &str, path: &str) -> rusqlite::Result<Option<u64>> {
    if !within_root(root, path) {
        return Ok(None);
    }
    let root_trim = root.trim_end_matches('\\');
    let rest = &path[root_trim.len()..];
    let mut id = 0i64;
    let mut alloc: i64 = match conn.query_row("SELECT alloc FROM entries WHERE id = 0", [], |r| r.get(0)).optional()? {
        Some(a) => a,
        None => return Ok(None),
    };
    for seg in rest.split('\\').filter(|s| !s.is_empty()) {
        let next: Option<(i64, i64)> = conn
            .query_row(
                "SELECT id, alloc FROM entries WHERE parent_id = ?1 AND name_lower = ?2 AND id != 0 LIMIT 1",
                params![id, seg.to_lowercase()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match next {
            Some((i, a)) => (id, alloc) = (i, a),
            None => return Ok(None),
        }
    }
    Ok(Some(alloc.max(0) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    #[test]
    fn classification() {
        assert_eq!(classify(3 * GIB, None, None), Verdict { outcome: Outcome::Removed, freed: 3 * GIB as i64 });
        assert_eq!(classify(3 * GIB, Some(3 * GIB), None), Verdict { outcome: Outcome::Present, freed: 0 });
        assert_eq!(classify(3 * GIB, Some(3 * GIB + 1000), None).outcome, Outcome::Present, "within tolerance");
        assert_eq!(classify(3 * GIB, Some(GIB), None), Verdict { outcome: Outcome::Partial, freed: 2 * GIB as i64 });
        assert_eq!(classify(GIB, Some(2 * GIB), None), Verdict { outcome: Outcome::Grown, freed: -(GIB as i64) });
        assert_eq!(classify(3 * GIB, None, Some(3 * GIB)), Verdict { outcome: Outcome::Moved, freed: 3 * GIB as i64 });
        assert_eq!(classify(0, Some(0), None).outcome, Outcome::Present);
    }

    #[test]
    fn root_coverage() {
        assert!(within_root(r"D:\", r"d:\Games\x"));
        assert!(within_root(r"D:\Games", r"D:\games"));
        assert!(!within_root(r"D:\", r"C:\x"));
        assert!(!within_root(r"D:\Games", r"D:\GamesExtra\x"));
    }
}
