//! Hot folders on a drive: folders with a lot of recent activity. Snapshot data only.

use rusqlite::{params, Connection};

use crate::eval::protected::Protected;

#[derive(Debug, Clone)]
pub struct HotParams {
    /// "Recent" means modified within this many days.
    pub days: u32,
    /// A folder is hot when at least this share (percent) of its bytes is recent...
    pub share_percent: f64,
    /// ...or at least this many of its files are.
    pub min_files: u64,
    /// Folders smaller than this are not considered.
    pub min_alloc: u64,
}

impl Default for HotParams {
    fn default() -> Self {
        Self { days: 30, share_percent: 20.0, min_files: 100, min_alloc: 1 << 30 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HotFolder {
    pub entry_id: i64,
    pub path: String,
    pub alloc: u64,
    pub recent_files: u64,
    pub recent_bytes: u64,
    /// Share of the folder's bytes that is recent, 0..=100.
    pub share: f64,
    /// Plain-language reason shown to the user.
    pub reason: String,
}

fn under(path: &str, base: &str) -> bool {
    let (p, b) = (path.to_lowercase(), base.to_lowercase());
    p == b || p.starts_with(&format!("{b}\\"))
}

/// The outermost folders (two levels below the root at most) that are hot.
pub fn hot_folders(
    conn: &Connection,
    now: i64,
    p: &HotParams,
    protected: &Protected,
) -> rusqlite::Result<Vec<HotFolder>> {
    let cutoff = now - i64::from(p.days) * 86_400;
    let mut stmt = conn.prepare(
        "SELECT id, path, alloc, coalesce(last_desc_id, id) FROM entries
         WHERE kind = 1 AND depth BETWEEN 1 AND 2 AND alloc >= ?1 AND path IS NOT NULL AND (flags & 2) = 0
         ORDER BY depth, alloc DESC, id",
    )?;
    let dirs: Vec<(i64, String, i64, i64)> = stmt
        .query_map([p.min_alloc as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let mut chosen: Vec<HotFolder> = Vec::new();
    for (id, path, alloc, last) in dirs {
        if chosen.iter().any(|c| under(&path, &c.path)) || protected.is_protected(&path) {
            continue;
        }
        let (files, bytes): (i64, i64) = conn.query_row(
            "SELECT count(*), coalesce(sum(alloc), 0) FROM entries WHERE kind = 0 AND id BETWEEN ?1 AND ?2 AND mtime >= ?3",
            params![id, last, cutoff],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let share = if alloc > 0 { bytes as f64 * 100.0 / alloc as f64 } else { 0.0 };
        let (files, bytes) = (files.max(0) as u64, bytes.max(0) as u64);
        let reason = if share >= p.share_percent {
            format!("{}% of it was modified in the last {} days", share.round() as u64, p.days)
        } else if files >= p.min_files {
            format!("{files} files in it were modified in the last {} days", p.days)
        } else {
            continue;
        };
        chosen.push(HotFolder {
            entry_id: id,
            path,
            alloc: alloc.max(0) as u64,
            recent_files: files,
            recent_bytes: bytes,
            share,
            reason,
        });
        if chosen.len() >= 200 {
            break;
        }
    }
    Ok(chosen)
}
