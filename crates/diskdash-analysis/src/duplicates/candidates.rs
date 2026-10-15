//! Stage 1: candidate files from the scan snapshots (no file is touched).

use std::collections::HashMap;

use rusqlite::Connection;

use crate::eval::protected::Protected;

const REPARSE: i64 = 0x400;
/// OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS: opening would fetch the file from the cloud.
const CLOUD_ATTRS: i64 = 0x1000 | 0x40000 | 0x400000;

/// One snapshot taking part in a run, with how its disk should be read.
pub struct ScopeSnapshot<'a> {
    pub snapshot_id: i64,
    pub root: String,
    pub volume_id: Option<String>,
    pub volume_serial: u32,
    /// Physical disk key: files on one disk are read by one scheduler queue.
    pub disk: String,
    /// Spinning disk: read one file at a time.
    pub hdd: bool,
    pub conn: &'a Connection,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub snapshot_id: i64,
    pub entry_id: i64,
    pub path: String,
    pub size: u64,
    pub alloc: u64,
    /// Last-modified time recorded by the scan (unix seconds).
    pub mtime: i64,
    pub volume_serial: u32,
    pub root: String,
    pub volume_id: Option<String>,
    pub disk: String,
    pub hdd: bool,
    pub category: Option<i64>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExcludedCounts {
    /// Online-only cloud files: never opened.
    pub cloud_placeholder: u64,
    pub reparse_point: u64,
    /// Other names of a file the scan recorded as hard-linked.
    pub hard_link: u64,
    /// Inside protected system locations.
    pub protected: u64,
}

#[derive(Debug, Default)]
pub struct Selection {
    pub candidates: Vec<Candidate>,
    pub excluded: ExcludedCounts,
}

/// Files of at least `min_size` bytes whose size occurs more than once across
/// all `scopes`. Zero-byte files are always left out.
pub fn select(scopes: &[ScopeSnapshot<'_>], min_size: u64, protected: &Protected) -> rusqlite::Result<Selection> {
    let min = min_size.max(1) as i64;
    let mut out = Selection::default();

    // Pass 1: how often does each size occur, over every snapshot?
    let mut sizes: HashMap<i64, u32> = HashMap::new();
    for s in scopes {
        let mut stmt = s.conn.prepare(
            "SELECT size, count(*) FROM entries
             WHERE kind = 0 AND size >= ?1 AND (flags & 2) = 0 AND (attrs & ?2) = 0 AND hardlink_of IS NULL
             GROUP BY size",
        )?;
        let rows = stmt.query_map(rusqlite::params![min, REPARSE | CLOUD_ATTRS], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, u32>(1)?))
        })?;
        for row in rows {
            let (size, n) = row?;
            *sizes.entry(size).or_insert(0) += n;
        }
    }
    sizes.retain(|_, n| *n > 1);

    for s in scopes {
        // Exclusion counts (informational; they are not hashed).
        let (cloud, reparse, links): (i64, i64, i64) = s.conn.query_row(
            "SELECT
               coalesce(sum(CASE WHEN (flags & 2) != 0 OR (attrs & ?2) != 0 THEN 1 ELSE 0 END), 0),
               coalesce(sum(CASE WHEN (flags & 2) = 0 AND (attrs & ?2) = 0 AND (attrs & ?3) != 0 THEN 1 ELSE 0 END), 0),
               coalesce(sum(CASE WHEN (flags & 2) = 0 AND (attrs & ?4) = 0 AND hardlink_of IS NOT NULL THEN 1 ELSE 0 END), 0)
             FROM entries WHERE kind = 0 AND size >= ?1",
            rusqlite::params![min, CLOUD_ATTRS, REPARSE, REPARSE | CLOUD_ATTRS],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        out.excluded.cloud_placeholder += cloud.max(0) as u64;
        out.excluded.reparse_point += reparse.max(0) as u64;
        out.excluded.hard_link += links.max(0) as u64;

        // Pass 2: the rows of the sizes that collide.
        let mut stmt = s.conn.prepare(
            "SELECT e.id, e.size, e.alloc, coalesce(e.mtime, 0), e.name, p.path, e.category
             FROM entries e LEFT JOIN entries p ON p.id = e.parent_id
             WHERE e.kind = 0 AND e.size >= ?1 AND (e.flags & 2) = 0 AND (e.attrs & ?2) = 0 AND e.hardlink_of IS NULL
             ORDER BY e.id",
        )?;
        let rows = stmt.query_map(rusqlite::params![min, REPARSE | CLOUD_ATTRS], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<i64>>(6)?,
            ))
        })?;
        for row in rows {
            let (id, size, alloc, mtime, name, dir, category) = row?;
            if !sizes.contains_key(&size) {
                continue;
            }
            let dir = dir.unwrap_or_default();
            let path = if dir.ends_with('\\') { format!("{dir}{name}") } else { format!("{dir}\\{name}") };
            if protected.is_protected(&path) || is_system_path(&path) {
                out.excluded.protected += 1;
                continue;
            }
            out.candidates.push(Candidate {
                snapshot_id: s.snapshot_id,
                entry_id: id,
                path,
                size: size.max(0) as u64,
                alloc: alloc.max(0) as u64,
                mtime,
                volume_serial: s.volume_serial,
                root: s.root.clone(),
                volume_id: s.volume_id.clone(),
                disk: s.disk.clone(),
                hdd: s.hdd,
                category,
            });
        }
    }
    Ok(out)
}

/// Never suggest copies inside these (copies there are part of Windows or an app install).
fn is_system_path(path: &str) -> bool {
    let p = path.to_lowercase();
    let rel = p.get(2..).unwrap_or("");
    ["\\windows\\", "\\$recycle.bin\\", "\\system volume information\\", "\\program files\\windowsapps\\"]
        .iter()
        .any(|s| rel.starts_with(s))
}
