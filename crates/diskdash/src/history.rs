//! History and trends: snapshot comparison, free-space trends with a forecast,
//! and keeping the catalog small by shrinking old scans to folder totals.
//! Everything here reads our own databases; nothing touches user files.

use std::collections::HashMap;

use diskdash_analysis::history::{
    categories_delta, diff_dirs, forecast, load_dirs, load_summary_dirs, DirDelta, Forecast, ForecastLimits, Sample,
};
use diskdash_analysis::plans::verify::lookup_entry;
use diskdash_core::category::Category;
use diskdash_store::{Db, OwnFiles};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ApiError;
use crate::settings::Settings;
use crate::snapshots::{self, Snapshot, SnapshotStatus};

/// Files at least this large that are new since the older scan are listed on their own.
pub const NEW_LARGE_BYTES: i64 = 100 << 20;
const DEFAULT_LIST: u32 = 50;
const MAX_LIST: u32 = 500;
/// Samples older than this are thinned to one per day.
const FULL_RES_DAYS: i64 = 90;

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DiffQuery {
    pub old: i64,
    pub new: i64,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DirChange {
    pub path: String,
    #[ts(type = "number")]
    pub depth: i64,
    #[ts(type = "number")]
    pub old_alloc: u64,
    #[ts(type = "number")]
    pub new_alloc: u64,
    #[ts(type = "number")]
    pub delta: i64,
    #[ts(type = "number")]
    pub old_files: u64,
    #[ts(type = "number")]
    pub new_files: u64,
}

impl From<DirDelta> for DirChange {
    fn from(d: DirDelta) -> Self {
        Self {
            path: d.path,
            depth: d.depth,
            old_alloc: d.old_alloc,
            new_alloc: d.new_alloc,
            delta: d.delta,
            old_files: d.old_files,
            new_files: d.new_files,
        }
    }
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CategoryChange {
    pub category: String,
    #[ts(type = "number")]
    pub old_alloc: u64,
    #[ts(type = "number")]
    pub new_alloc: u64,
    #[ts(type = "number")]
    pub delta: i64,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct NewLargeFile {
    pub path: String,
    #[ts(type = "number")]
    pub alloc: u64,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DiffResponse {
    pub old: Snapshot,
    pub new: Snapshot,
    /// Change in total allocated bytes between the two scans.
    #[ts(type = "number")]
    pub total_delta: i64,
    pub growing: Vec<DirChange>,
    pub shrinking: Vec<DirChange>,
    pub appeared: Vec<DirChange>,
    pub disappeared: Vec<DirChange>,
    pub categories: Vec<CategoryChange>,
    /// Large files that are not in the older scan. Empty and `new_large_available = false`
    /// when the older scan was reduced to folder totals.
    pub new_large_files: Vec<NewLargeFile>,
    pub new_large_available: bool,
    /// Set when the older scan keeps folders only down to this depth, so the lists stop there too.
    #[ts(type = "number | null")]
    pub depth_limit: Option<u32>,
}

fn get_complete(conn: &Connection, id: i64) -> Result<Snapshot, ApiError> {
    let snap = snapshots::get(conn, id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::NotFound(format!("snapshot {id} not found")))?;
    if snap.status != SnapshotStatus::Complete {
        return Err(ApiError::Conflict(format!(
            "snapshot {id} is {}; only complete scans can be compared",
            snap.status.as_str()
        )));
    }
    Ok(snap)
}

fn category_totals(conn: &Connection, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<(i64, u64)>, ApiError> {
    let mut stmt = conn.prepare(sql).map_err(ApiError::internal)?;
    let rows = stmt
        .query_map(args, |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?.max(0) as u64)))
        .map_err(ApiError::internal)?;
    rows.collect::<Result<_, _>>().map_err(ApiError::internal)
}

pub fn diff(db: &Db, own: &OwnFiles, q: &DiffQuery) -> Result<DiffResponse, ApiError> {
    let catalog = db.connect_read_only().map_err(ApiError::internal)?;
    let (old, new) = (get_complete(&catalog, q.old)?, get_complete(&catalog, q.new)?);
    if !old.root.eq_ignore_ascii_case(&new.root) {
        return Err(ApiError::BadRequest("snapshots must share the same root".into()));
    }
    if new.summarised {
        return Err(ApiError::BadRequest(
            "the newer snapshot was reduced to folder totals; pick a newer scan as 'new'".into(),
        ));
    }
    if old.id >= new.id {
        return Err(ApiError::BadRequest("'old' must be an earlier scan than 'new'".into()));
    }
    let limit = q.limit.unwrap_or(DEFAULT_LIST).clamp(1, MAX_LIST) as usize;
    let new_conn = snapshots::open_snapshot(own, &new)?;

    let depth_limit = old.summarised.then_some(old.summary_depth).flatten().map(|d| d as i64);
    let (old_dirs, old_cats, old_conn) = if old.summarised {
        let dirs = load_summary_dirs(&catalog, old.id, None).map_err(ApiError::internal)?;
        let cats = category_totals(
            &catalog,
            "SELECT category, alloc FROM snapshot_summary_categories WHERE snapshot_id = ?1",
            &[&old.id],
        )?;
        (dirs, cats, None)
    } else {
        let conn = snapshots::open_snapshot(own, &old)?;
        let dirs = load_dirs(&conn, None).map_err(ApiError::internal)?;
        let cats = category_totals(&conn, "SELECT category, alloc FROM category_totals", &[])?;
        (dirs, cats, Some(conn))
    };
    let new_dirs = load_dirs(&new_conn, depth_limit).map_err(ApiError::internal)?;
    // Same depth on both sides, or deep folders would all look "new".
    let old_dirs: HashMap<_, _> = match depth_limit {
        Some(d) => old_dirs.into_iter().filter(|(_, r)| r.depth <= d).collect(),
        None => old_dirs,
    };
    let new_cats = category_totals(&new_conn, "SELECT category, alloc FROM category_totals", &[])?;

    let d = diff_dirs(&old_dirs, &new_dirs);
    let cut = |v: Vec<DirDelta>| v.into_iter().take(limit).map(DirChange::from).collect::<Vec<_>>();

    let (new_large_files, new_large_available) = match &old_conn {
        Some(old_conn) => (new_large_files(&new_conn, old_conn, &new.root, limit).map_err(ApiError::internal)?, true),
        None => (Vec::new(), false),
    };
    Ok(DiffResponse {
        total_delta: new.total_alloc as i64 - old.total_alloc as i64,
        growing: cut(d.growing),
        shrinking: cut(d.shrinking),
        appeared: cut(d.appeared),
        disappeared: cut(d.disappeared),
        categories: categories_delta(&old_cats, &new_cats)
            .into_iter()
            .map(|c| CategoryChange {
                category: Category::from_code(c.category as u8).key().to_string(),
                old_alloc: c.old_alloc,
                new_alloc: c.new_alloc,
                delta: c.delta,
            })
            .collect(),
        new_large_files,
        new_large_available,
        depth_limit: depth_limit.map(|d| d as u32),
        old,
        new,
    })
}

fn new_large_files(
    new_conn: &Connection,
    old_conn: &Connection,
    root: &str,
    limit: usize,
) -> rusqlite::Result<Vec<NewLargeFile>> {
    let mut stmt = new_conn.prepare(
        "SELECT p.path, f.name, f.alloc FROM entries f JOIN entries p ON p.id = f.parent_id
         WHERE f.kind = 0 AND f.alloc >= ?1 AND p.path IS NOT NULL ORDER BY f.alloc DESC LIMIT 1000",
    )?;
    let rows = stmt
        .query_map([NEW_LARGE_BYTES], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (dir, name, alloc) = row?;
        let path = format!("{}\\{}", dir.trim_end_matches('\\'), name);
        if lookup_entry(old_conn, root, &path)?.is_none() {
            out.push(NewLargeFile { path, alloc: alloc.max(0) as u64 });
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Free-space trends
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SamplePoint {
    #[ts(type = "number")]
    pub ts: i64,
    #[ts(type = "number")]
    pub free: u64,
    /// `inventory` (periodic) or `scan` (taken when a scan finished).
    pub source: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct VolumeTrend {
    pub volume_id: String,
    pub label: Option<String>,
    pub letters: Vec<String>,
    #[ts(type = "number")]
    pub capacity: u64,
    pub samples: Vec<SamplePoint>,
    pub forecast: Forecast,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct VolumesTrends {
    pub volumes: Vec<VolumeTrend>,
}

pub fn volume_trends(db: &Db, settings: &Settings, now: i64) -> Result<VolumesTrends, ApiError> {
    let conn = db.connect_read_only().map_err(ApiError::internal)?;
    let limits = ForecastLimits {
        low_percent: settings.low_space_percent,
        low_floor: u64::from(settings.low_space_floor_gib) << 30,
    };
    let mut vstmt = conn
        .prepare("SELECT volume_id, label, letters, capacity FROM volumes ORDER BY letters, volume_id")
        .map_err(ApiError::internal)?;
    let vols: Vec<(String, Option<String>, String, i64)> = vstmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(ApiError::internal)?
        .collect::<Result<_, _>>()
        .map_err(ApiError::internal)?;
    let mut sstmt = conn
        .prepare("SELECT bucket * 900, free, source FROM volume_samples WHERE volume_id = ?1 AND bucket >= ?2 / 900 ORDER BY bucket")
        .map_err(ApiError::internal)?;
    let mut out = Vec::new();
    for (volume_id, label, letters, capacity) in vols {
        let samples: Vec<SamplePoint> = sstmt
            .query_map(params![volume_id, now - 365 * 86_400], |r| {
                Ok(SamplePoint { ts: r.get(0)?, free: r.get::<_, i64>(1)?.max(0) as u64, source: r.get(2)? })
            })
            .map_err(ApiError::internal)?
            .collect::<Result<_, _>>()
            .map_err(ApiError::internal)?;
        let points: Vec<Sample> = samples.iter().map(|s| Sample { ts: s.ts, free: s.free }).collect();
        let capacity = capacity.max(0) as u64;
        out.push(VolumeTrend {
            forecast: forecast(&points, capacity, limits),
            volume_id,
            label,
            letters: letters.split(',').filter(|l| !l.is_empty()).map(str::to_string).collect(),
            capacity,
            samples,
        });
    }
    Ok(VolumesTrends { volumes: out })
}

// ---------------------------------------------------------------------------
// Pinning and retention
// ---------------------------------------------------------------------------

pub fn set_pinned(db: &Db, id: i64, pinned: bool) -> Result<Snapshot, ApiError> {
    let conn = db.connect().map_err(ApiError::internal)?;
    let n = conn
        .execute("UPDATE snapshots SET pinned = ?2 WHERE id = ?1", params![id, i64::from(pinned)])
        .map_err(ApiError::internal)?;
    if n == 0 {
        return Err(ApiError::NotFound(format!("snapshot {id} not found")));
    }
    snapshots::get(&conn, id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::NotFound(format!("snapshot {id} not found")))
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct RetentionReport {
    pub summarised: u32,
    pub samples_thinned: u32,
}

/// Where a test can stop the clean-up to prove that an interruption is harmless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Never,
    /// After the summary rows are written but before the catalog row is switched.
    BeforeSwitch,
    /// After the catalog row says `summary` but before the file is removed.
    BeforeFileRemoval,
}

/// Shrink scans beyond the newest `keep` (per root, unpinned) to folder totals down to
/// `depth`, then thin old free-space samples. Safe to repeat and safe to interrupt:
/// the catalog switches in one transaction and the file is removed afterwards;
/// a leftover file is removed on the next start or run.
pub fn retain(db: &Db, own: &OwnFiles, keep: u32, depth: u32, now: i64, stop: Stop) -> Result<RetentionReport, String> {
    let mut report = RetentionReport::default();
    let mut conn = db.connect().map_err(|e| e.to_string())?;
    let rows: Vec<(i64, String, i64, String)> = conn
        .prepare(
            "SELECT id, lower(root), pinned, db_file FROM snapshots
             WHERE status = 'complete' AND detail_level = 'full' ORDER BY id DESC",
        )
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect())
        .map_err(|e| e.to_string())?;
    let mut seen: HashMap<String, u32> = HashMap::new();
    for (id, root, pinned, db_file) in rows {
        if pinned != 0 {
            continue;
        }
        let n = seen.entry(root).or_default();
        *n += 1;
        if *n <= keep {
            continue;
        }
        let busy: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM cleanup_runs WHERE snapshot_id = ?1 AND status IN ('queued', 'running'))",
                [id],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if busy {
            continue;
        }
        let base = own.root().join(&db_file);
        let Ok(snap_conn) = Connection::open_with_flags(&base, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else {
            continue;
        };
        let dirs = load_dirs(&snap_conn, Some(i64::from(depth))).map_err(|e| e.to_string())?;
        let cats = category_totals_raw(&snap_conn)?;
        drop(snap_conn);
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM snapshot_summaries WHERE snapshot_id = ?1", [id]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM snapshot_summary_categories WHERE snapshot_id = ?1", [id])
            .map_err(|e| e.to_string())?;
        {
            let mut ins = tx
                .prepare(
                    "INSERT INTO snapshot_summaries(snapshot_id, path_key, path, depth, logical, allocated, file_count)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(|e| e.to_string())?;
            for (k, d) in &dirs {
                ins.execute(params![id, k, d.path, d.depth, d.size as i64, d.alloc as i64, d.files as i64])
                    .map_err(|e| e.to_string())?;
            }
            let mut cins = tx
                .prepare("INSERT INTO snapshot_summary_categories(snapshot_id, category, file_count, size, alloc) VALUES (?1, ?2, ?3, ?4, ?5)")
                .map_err(|e| e.to_string())?;
            for (c, files, size, alloc) in &cats {
                cins.execute(params![id, c, files, size, alloc]).map_err(|e| e.to_string())?;
            }
        }
        if stop == Stop::BeforeSwitch {
            return Ok(report); // dropping the transaction rolls everything back
        }
        tx.execute(
            "UPDATE snapshots SET detail_level = 'summary', summary_depth = ?2 WHERE id = ?1",
            params![id, depth],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        if stop == Stop::BeforeFileRemoval {
            return Ok(report);
        }
        snapshots::remove_snapshot_files(own, &base).map_err(|e| e.to_string())?;
        report.summarised += 1;
    }
    // Leftover files of snapshots that are already summarised (an earlier interruption).
    let left: Vec<String> = conn
        .prepare("SELECT db_file FROM snapshots WHERE detail_level = 'summary'")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .map_err(|e| e.to_string())?;
    for f in left {
        snapshots::remove_snapshot_files(own, &own.root().join(f)).map_err(|e| e.to_string())?;
    }
    report.samples_thinned = thin_samples(&conn, now - FULL_RES_DAYS * 86_400)?;
    Ok(report)
}

fn category_totals_raw(conn: &Connection) -> Result<Vec<(i64, i64, i64, i64)>, String> {
    conn.prepare("SELECT category, file_count, size, alloc FROM category_totals")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect())
        .map_err(|e| e.to_string())
}

/// Keep one sample per volume per day (the one with the least free space) before `before`.
fn thin_samples(conn: &Connection, before: i64) -> Result<u32, String> {
    let n = conn
        .execute(
            "DELETE FROM volume_samples WHERE bucket * 900 < ?1 AND (volume_id, bucket) NOT IN (
               SELECT volume_id, bucket FROM (
                 SELECT volume_id, bucket,
                        row_number() OVER (PARTITION BY volume_id, bucket * 900 / 86400 ORDER BY free ASC, bucket) AS rn
                 FROM volume_samples WHERE bucket * 900 < ?1) WHERE rn = 1)",
            [before],
        )
        .map_err(|e| e.to_string())?;
    Ok(n as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_samples_keep_one_per_day() {
        let tmp = tempfile::tempdir().unwrap();
        let dd = diskdash_store::DataDir::at(tmp.path().join("dd"));
        let own = OwnFiles::new(&dd).unwrap();
        let db = diskdash_store::open_catalog(&dd, &own).unwrap();
        let conn = db.connect().unwrap();
        conn.execute(
            "INSERT INTO volumes(volume_id, volume_path, letters, label, filesystem, capacity, media, first_seen, last_seen)
             VALUES ('v', 'p', 'C:', 'C', 'NTFS', 1000, 'Ssd', 'a', 'a')",
            [],
        )
        .unwrap();
        // Day 1: four samples, day 2: one sample, both long ago; plus a recent one.
        for (bucket, ts, free) in
            [(1, 100, 50), (2, 200, 30), (3, 300, 40), (4, 400, 60), (100, 86_500, 70), (20_000, 9_000_000, 80)]
        {
            conn.execute(
                "INSERT INTO volume_samples(volume_id, bucket, ts, capacity, free) VALUES ('v', ?1, ?2, 1000, ?3)",
                params![bucket, ts, free],
            )
            .unwrap();
        }
        let removed = thin_samples(&conn, 1_000_000).unwrap();
        assert_eq!(removed, 3);
        let left: Vec<i64> = conn
            .prepare("SELECT free FROM volume_samples ORDER BY bucket")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(left, [30, 70, 80], "the day's minimum, the other day, and the recent sample");
    }

    /// Two synthetic snapshots of 1M entries each (100k folders): the comparison must stay fast
    /// and bounded. Run with `cargo test -p diskdash --release -- --ignored diff_of_two_million`.
    #[test]
    #[ignore = "benchmark: run in release mode"]
    fn diff_of_two_million_entry_snapshots_is_fast() {
        const DIRS: i64 = 100_000;
        const FILES_PER_DIR: i64 = 9;
        fn build(grow: i64) -> Connection {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch(diskdash_core::store::SCHEMA).unwrap();
            let tx = conn.unchecked_transaction().unwrap();
            {
                let mut ins = tx
                    .prepare(
                        "INSERT INTO entries(id, parent_id, name, name_lower, kind, depth, size, alloc, attrs, path, file_count)
                         VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, ?6, 0, ?7, ?8)",
                    )
                    .unwrap();
                let mut id = 1i64;
                for d in 0..DIRS {
                    let dir_id = id;
                    let path = format!(r"D:\top{}\sub{}", d % 1000, d);
                    ins.execute(params![
                        dir_id,
                        0,
                        format!("sub{d}"),
                        1,
                        2,
                        4096 * FILES_PER_DIR + grow * (d % 7),
                        path,
                        FILES_PER_DIR
                    ])
                    .unwrap();
                    id += 1;
                    for f in 0..FILES_PER_DIR {
                        ins.execute(params![id, dir_id, format!("f{f}.bin"), 0, 3, 4096, Option::<String>::None, 0])
                            .unwrap();
                        id += 1;
                    }
                }
            }
            tx.commit().unwrap();
            conn
        }
        let (old, new) = (build(0), build(8192));
        let start = std::time::Instant::now();
        let (a, b) = (load_dirs(&old, None).unwrap(), load_dirs(&new, None).unwrap());
        let d = diff_dirs(&a, &b);
        let took = start.elapsed();
        assert!(!d.growing.is_empty());
        assert!(took.as_secs() < 10, "diff took {took:?}");
    }
}
