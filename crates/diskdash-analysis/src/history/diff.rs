//! Snapshot diff by directory path. Directories are matched on their
//! normalised (case-folded) full path, because entry ids differ between snapshot
//! files. Works on full snapshots (their database) and on summaries (catalog rows).

use std::collections::HashMap;

use rusqlite::{params, Connection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirRow {
    pub path: String,
    pub depth: i64,
    pub alloc: u64,
    pub size: u64,
    pub files: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirDelta {
    pub path: String,
    pub depth: i64,
    pub old_alloc: u64,
    pub new_alloc: u64,
    pub delta: i64,
    pub old_files: u64,
    pub new_files: u64,
}

#[derive(Debug, Default)]
pub struct DiffResult {
    /// Biggest growth first.
    pub growing: Vec<DirDelta>,
    /// Biggest shrink first.
    pub shrinking: Vec<DirDelta>,
    /// Folders only in the newer snapshot (the outermost of each new tree).
    pub appeared: Vec<DirDelta>,
    /// Folders only in the older snapshot (the outermost of each removed tree).
    pub disappeared: Vec<DirDelta>,
}

/// Directories of a full snapshot, up to `max_depth` when given, keyed by lower-case path.
pub fn load_dirs(conn: &Connection, max_depth: Option<i64>) -> rusqlite::Result<HashMap<String, DirRow>> {
    let mut stmt = conn.prepare(
        "SELECT path, depth, alloc, size, coalesce(file_count, 0) FROM entries
         WHERE kind = 1 AND path IS NOT NULL AND (?1 IS NULL OR depth <= ?1)",
    )?;
    let rows = stmt.query_map(params![max_depth], |r| {
        Ok(DirRow {
            path: r.get(0)?,
            depth: r.get(1)?,
            alloc: r.get::<_, i64>(2)?.max(0) as u64,
            size: r.get::<_, i64>(3)?.max(0) as u64,
            files: r.get::<_, i64>(4)?.max(0) as u64,
        })
    })?;
    let mut out = HashMap::new();
    for r in rows {
        let r = r?;
        out.insert(key(&r.path), r);
    }
    Ok(out)
}

/// Directories of a summarised snapshot, read from the catalog.
pub fn load_summary_dirs(
    catalog: &Connection,
    snapshot_id: i64,
    max_depth: Option<i64>,
) -> rusqlite::Result<HashMap<String, DirRow>> {
    let mut stmt = catalog.prepare(
        "SELECT path_key, path, depth, allocated, logical, file_count FROM snapshot_summaries
         WHERE snapshot_id = ?1 AND (?2 IS NULL OR depth <= ?2)",
    )?;
    let rows = stmt.query_map(params![snapshot_id, max_depth], |r| {
        Ok((
            r.get::<_, String>(0)?,
            DirRow {
                path: r.get(1)?,
                depth: r.get(2)?,
                alloc: r.get::<_, i64>(3)?.max(0) as u64,
                size: r.get::<_, i64>(4)?.max(0) as u64,
                files: r.get::<_, i64>(5)?.max(0) as u64,
            },
        ))
    })?;
    rows.collect::<Result<_, _>>()
}

pub fn key(path: &str) -> String {
    path.trim_end_matches('\\').to_lowercase()
}

fn parent_key(k: &str) -> Option<&str> {
    k.rsplit_once('\\').map(|(p, _)| p)
}

fn delta(old: Option<&DirRow>, new: Option<&DirRow>) -> DirDelta {
    let r = new.or(old).expect("one side exists");
    DirDelta {
        path: r.path.clone(),
        depth: r.depth,
        old_alloc: old.map_or(0, |o| o.alloc),
        new_alloc: new.map_or(0, |n| n.alloc),
        delta: new.map_or(0, |n| n.alloc as i64) - old.map_or(0, |o| o.alloc as i64),
        old_files: old.map_or(0, |o| o.files),
        new_files: new.map_or(0, |n| n.files),
    }
}

/// Compare two sets of directories.
///
/// A parent whose change is (almost) all explained by one child is left out of
/// the grow/shrink lists, so the lists name the folders where the change is.
pub fn diff_dirs(old: &HashMap<String, DirRow>, new: &HashMap<String, DirRow>) -> DiffResult {
    let mut out = DiffResult::default();
    let mut changed: HashMap<&str, DirDelta> = HashMap::new();
    for (k, n) in new {
        match old.get(k) {
            Some(o) if o.alloc != n.alloc => {
                changed.insert(k, delta(Some(o), Some(n)));
            }
            Some(_) => {}
            None => {
                // Outermost only: its parent existed before (or it is a top-level folder).
                if parent_key(k).is_none_or(|p| old.contains_key(p) || !new.contains_key(p)) {
                    out.appeared.push(delta(None, Some(n)));
                }
            }
        }
    }
    for (k, o) in old {
        if !new.contains_key(k) && parent_key(k).is_none_or(|p| new.contains_key(p) || !old.contains_key(p)) {
            out.disappeared.push(delta(Some(o), None));
        }
    }
    // The largest change among each parent's children.
    let mut biggest_child: HashMap<&str, i64> = HashMap::new();
    for (k, d) in &changed {
        if let Some(p) = parent_key(k) {
            let e = biggest_child.entry(p).or_insert(0);
            if d.delta.abs() > e.abs() && d.delta.signum() != 0 {
                *e = d.delta;
            }
        }
    }
    for (k, d) in &changed {
        let explained = biggest_child
            .get(k)
            .is_some_and(|c| c.signum() == d.delta.signum() && (c.abs() as f64) >= 0.9 * d.delta.abs() as f64);
        if explained {
            continue;
        }
        if d.delta > 0 {
            out.growing.push(d.clone());
        } else {
            out.shrinking.push(d.clone());
        }
    }
    out.growing.sort_by(|a, b| b.delta.cmp(&a.delta).then(a.path.cmp(&b.path)));
    out.shrinking.sort_by(|a, b| a.delta.cmp(&b.delta).then(a.path.cmp(&b.path)));
    out.appeared.sort_by(|a, b| b.new_alloc.cmp(&a.new_alloc).then(a.path.cmp(&b.path)));
    out.disappeared.sort_by(|a, b| b.old_alloc.cmp(&a.old_alloc).then(a.path.cmp(&b.path)));
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryDelta {
    pub category: i64,
    pub old_alloc: u64,
    pub new_alloc: u64,
    pub delta: i64,
}

/// Per-category change in allocated bytes, largest change first.
pub fn categories_delta(old: &[(i64, u64)], new: &[(i64, u64)]) -> Vec<CategoryDelta> {
    let mut m: HashMap<i64, (u64, u64)> = HashMap::new();
    for (c, a) in old {
        m.entry(*c).or_default().0 = *a;
    }
    for (c, a) in new {
        m.entry(*c).or_default().1 = *a;
    }
    let mut v: Vec<CategoryDelta> = m
        .into_iter()
        .map(|(category, (o, n))| CategoryDelta { category, old_alloc: o, new_alloc: n, delta: n as i64 - o as i64 })
        .filter(|d| d.delta != 0)
        .collect();
    v.sort_by(|a, b| b.delta.abs().cmp(&a.delta.abs()).then(a.category.cmp(&b.category)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str, gib: u64) -> (String, DirRow) {
        let depth = path.matches('\\').count() as i64 - 1;
        (key(path), DirRow { path: path.into(), depth, alloc: gib << 30, size: gib << 30, files: gib })
    }

    fn set(rows: &[(&str, u64)]) -> HashMap<String, DirRow> {
        rows.iter().map(|(p, g)| row(p, *g)).collect()
    }

    #[test]
    fn growth_shrink_appeared_and_disappeared() {
        let old = set(&[(r"D:\Downloads", 10), (r"D:\Games", 30), (r"D:\Old", 5), (r"D:\Old\a", 5)]);
        let new = set(&[(r"D:\Downloads", 25), (r"D:\Games", 20), (r"D:\New", 8), (r"D:\New\sub", 8)]);
        let d = diff_dirs(&old, &new);
        assert_eq!(d.growing[0].path, r"D:\Downloads");
        assert_eq!(d.growing[0].delta, 15 << 30);
        assert_eq!(d.shrinking.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), [r"D:\Games"]);
        assert_eq!(
            d.appeared.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(),
            [r"D:\New"],
            "only the outermost new folder"
        );
        assert_eq!(d.disappeared.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), [r"D:\Old"]);
    }

    #[test]
    fn a_parent_is_listed_when_no_single_child_explains_it() {
        let old = set(&[(r"C:\Users", 10), (r"C:\Users\a", 5), (r"C:\Users\b", 5)]);
        let new = set(&[(r"C:\Users", 20), (r"C:\Users\a", 10), (r"C:\Users\b", 10)]);
        let d = diff_dirs(&old, &new);
        let paths: Vec<&str> = d.growing.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(paths[0], r"C:\Users", "10 GiB of growth, 5 GiB in each child");
        assert_eq!(paths.len(), 3);
    }

    #[test]
    fn paths_match_regardless_of_case() {
        let old = set(&[(r"D:\Data", 5)]);
        let new: HashMap<String, DirRow> = [row(r"D:\DATA", 9)].into_iter().collect();
        let d = diff_dirs(&old, &new);
        assert_eq!(d.growing.len(), 1);
        assert!(d.appeared.is_empty() && d.disappeared.is_empty());
    }

    #[test]
    fn category_deltas_are_sorted_by_size_of_change() {
        let d = categories_delta(&[(1, 100), (2, 500), (3, 50)], &[(1, 400), (2, 200), (3, 50), (4, 10)]);
        assert_eq!(d.iter().map(|c| (c.category, c.delta)).collect::<Vec<_>>(), [(1, 300), (2, -300), (4, 10)]);
    }
}
