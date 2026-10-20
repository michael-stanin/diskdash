//! Snapshot catalog (main DB) and queries over per-snapshot databases.

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use diskdash_analysis::stale::{BasisReason, TimeBasis};
use diskdash_core::category::Category;
use diskdash_store::{Db, OwnFiles};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ApiError;

pub const MAX_PAGE: u32 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SnapshotStatus {
    Running,
    Partial,
    Complete,
    Failed,
}

impl SnapshotStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Partial => "partial",
            Self::Complete => "complete",
            Self::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "partial" => Self::Partial,
            "complete" => Self::Complete,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Snapshot {
    #[ts(type = "number")]
    pub id: i64,
    pub root: String,
    pub volume_id: Option<String>,
    pub status: SnapshotStatus,
    pub method: String,
    pub elevated: bool,
    /// False when hard links were not detected on this drive (sizes may over-count).
    pub hardlinks_deduplicated: bool,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub resumed: bool,
    #[ts(type = "number")]
    pub file_count: u64,
    #[ts(type = "number")]
    pub dir_count: u64,
    #[ts(type = "number")]
    pub link_count: u64,
    #[ts(type = "number")]
    pub total_size: u64,
    #[ts(type = "number")]
    pub total_alloc: u64,
    #[ts(type = "number")]
    pub error_count: u64,
    #[ts(type = "number")]
    pub access_denied_count: u64,
    #[ts(type = "number")]
    pub link_unknown_count: u64,
    #[ts(type = "number | null")]
    pub job_id: Option<i64>,
    pub error: Option<String>,
    /// Timestamp the stale-data analysis uses for this scan.
    pub time_basis: TimeBasis,
    /// Why: last-access updates on or off, or unknown.
    pub time_basis_reason: BasisReason,
    /// Kept from automatic clean-up of old scans.
    pub pinned: bool,
    /// True once the scan was reduced to folder totals; its files can no longer be browsed.
    pub summarised: bool,
    #[ts(type = "number | null")]
    pub summary_depth: Option<u32>,
    #[serde(skip)]
    pub db_file: String,
}

const SNAP_COLS: &str = "id, root, volume_id, status, method, elevated, hardlinks, started_at, finished_at, resumed,
    file_count, dir_count, link_count, total_size, total_alloc, error_count, access_denied_count, link_unknown_count,
    job_id, error, db_file, time_basis, time_basis_reason, pinned, detail_level, summary_depth";

fn snap_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Snapshot> {
    let u = |i: usize| r.get::<_, i64>(i).map(|v| v.max(0) as u64);
    Ok(Snapshot {
        id: r.get(0)?,
        root: r.get(1)?,
        volume_id: r.get(2)?,
        status: SnapshotStatus::parse(&r.get::<_, String>(3)?),
        method: r.get(4)?,
        elevated: r.get::<_, i64>(5)? != 0,
        hardlinks_deduplicated: r.get::<_, String>(6)? == "detected",
        started_at: r.get(7)?,
        finished_at: r.get(8)?,
        resumed: r.get::<_, i64>(9)? != 0,
        file_count: u(10)?,
        dir_count: u(11)?,
        link_count: u(12)?,
        total_size: u(13)?,
        total_alloc: u(14)?,
        error_count: u(15)?,
        access_denied_count: u(16)?,
        link_unknown_count: u(17)?,
        job_id: r.get(18)?,
        error: r.get(19)?,
        db_file: r.get(20)?,
        time_basis: TimeBasis::parse(&r.get::<_, String>(21)?),
        time_basis_reason: BasisReason::parse(&r.get::<_, String>(22)?),
        pinned: r.get::<_, i64>(23)? != 0,
        summarised: r.get::<_, String>(24)? == "summary",
        summary_depth: r.get::<_, Option<i64>>(25)?.map(|d| d.max(0) as u32),
    })
}

pub fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<Snapshot>> {
    conn.query_row(&format!("SELECT {SNAP_COLS} FROM snapshots WHERE id = ?1"), [id], snap_row).optional()
}

#[derive(Debug, Default, Deserialize)]
pub struct ListFilter {
    pub root: Option<String>,
    pub volume_id: Option<String>,
    pub limit: Option<u32>,
}

pub fn list(conn: &Connection, f: &ListFilter) -> rusqlite::Result<Vec<Snapshot>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SNAP_COLS} FROM snapshots
         WHERE (?1 IS NULL OR lower(root) = lower(?1)) AND (?2 IS NULL OR volume_id = ?2)
         ORDER BY id DESC LIMIT ?3"
    ))?;
    let rows = stmt.query_map(params![f.root, f.volume_id, f.limit.unwrap_or(100).min(500)], snap_row)?;
    rows.collect()
}

/// Latest complete snapshot per root (newest first).
pub fn latest(conn: &Connection) -> rusqlite::Result<Vec<Snapshot>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SNAP_COLS} FROM snapshots s WHERE status = 'complete' AND id = (
           SELECT max(id) FROM snapshots t WHERE lower(t.root) = lower(s.root) AND t.status = 'complete')
         ORDER BY finished_at DESC"
    ))?;
    let rows = stmt.query_map([], snap_row)?;
    rows.collect()
}

/// Startup: interrupted scans become `partial` (resumable); catalog rows whose
/// file vanished become `failed`; snapshot files without a row are deleted.
pub fn reconcile(db: &Db, own: &OwnFiles, snapshots_dir: &Path) -> Result<(), String> {
    let conn = db.connect().map_err(|e| e.to_string())?;
    conn.execute("UPDATE snapshots SET status = 'partial', error = 'interrupted' WHERE status = 'running'", [])
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, String)> = conn
        .prepare("SELECT id, db_file FROM snapshots WHERE detail_level = 'full'")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .map_err(|e| e.to_string())?;
    let data_root = own.root().to_path_buf();
    for (id, file) in &rows {
        if !data_root.join(file).exists() {
            conn.execute(
                "UPDATE snapshots SET status = 'failed', error = 'snapshot file missing' WHERE id = ?1 AND status != 'failed'",
                [id],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    // A summarised snapshot has no file; one that is left over from an interrupted clean-up is removed.
    let summarised: Vec<String> = conn
        .prepare("SELECT db_file FROM snapshots WHERE detail_level = 'summary'")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .map_err(|e| e.to_string())?;
    for file in &summarised {
        remove_snapshot_files(own, &data_root.join(file)).map_err(|e| e.to_string())?;
    }
    let known: std::collections::HashSet<String> =
        rows.iter().map(|(_, f)| f.rsplit(['\\', '/']).next().unwrap_or(f).to_lowercase()).collect();
    if let Ok(rd) = std::fs::read_dir(snapshots_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            let base = name.trim_end_matches("-wal").trim_end_matches("-shm").to_string();
            if base.ends_with(".db") && !known.contains(&base) {
                tracing::info!(file = %name, "removing orphan snapshot file");
                let _ = own.remove_file(&e.path());
            }
        }
    }
    Ok(())
}

/// Delete a snapshot row and its files (only our own files in the data dir).
pub fn delete(conn: &Connection, own: &OwnFiles, snap: &Snapshot) -> Result<(), ApiError> {
    if snap.status == SnapshotStatus::Running {
        return Err(ApiError::Conflict("this scan is still running; cancel it first".into()));
    }
    let busy: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM cleanup_runs WHERE snapshot_id = ?1 AND status IN ('queued', 'running'))",
            [snap.id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if busy {
        return Err(ApiError::Conflict(
            "this scan is still being checked for cleanup suggestions; try again in a moment".into(),
        ));
    }
    conn.execute("DELETE FROM snapshots WHERE id = ?1", [snap.id]).map_err(ApiError::internal)?;
    remove_snapshot_files(own, &own.root().join(&snap.db_file)).map_err(ApiError::internal)?;
    Ok(())
}

/// Remove a snapshot database and its `-wal` / `-shm` side files (our own files only).
pub fn remove_snapshot_files(own: &OwnFiles, base: &Path) -> Result<(), diskdash_store::OwnFilesError> {
    for suffix in ["", "-wal", "-shm"] {
        let mut p = base.to_path_buf().into_os_string();
        p.push(suffix);
        let p = PathBuf::from(p);
        if p.exists() {
            own.remove_file(&p)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Queries over one snapshot database
// ---------------------------------------------------------------------------

pub fn open_snapshot(own: &OwnFiles, snap: &Snapshot) -> Result<Connection, ApiError> {
    let path: PathBuf = own.root().join(&snap.db_file);
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| ApiError::Internal(format!("cannot open snapshot {}: {e}", snap.id)))?;
    let _ = conn.busy_timeout(Duration::from_secs(5));
    Ok(conn)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Measure {
    /// Size on disk (default).
    #[default]
    Alloc,
    /// Logical size.
    Size,
}

impl Measure {
    fn column(self) -> &'static str {
        match self {
            Measure::Alloc => "alloc",
            Measure::Size => "size",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum EntryKind {
    File,
    Dir,
    Link,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct EntryView {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub alloc: u64,
    pub modified: Option<String>,
    pub category: Option<String>,
    /// Directories: the category holding the most bytes below.
    pub top_category: Option<String>,
    pub ext: Option<String>,
    #[ts(type = "number | null")]
    pub file_count: Option<u64>,
    #[ts(type = "number | null")]
    pub dir_count: Option<u64>,
    pub newest_modified: Option<String>,
    pub access_denied: bool,
    pub cloud_placeholder: bool,
    pub hardlink_repeat: bool,
    pub link_target: Option<String>,
}

pub(crate) fn rfc3339(secs: Option<i64>) -> Option<String> {
    let s = secs.filter(|s| *s > 0)?;
    Some(humantime::format_rfc3339_seconds(UNIX_EPOCH + Duration::from_secs(s as u64)).to_string())
}

/// Column list for [`entry_row`]. Snapshots written before `top_category`
/// existed read it as NULL.
fn entry_cols(conn: &Connection) -> String {
    let top = if has_column(conn, "top_category") { "e.top_category" } else { "NULL" };
    format!(
        "e.id, e.name, e.kind, e.size, e.alloc, e.mtime, e.category, e.ext, e.file_count, e.dir_count,
         e.max_mtime, e.flags, e.hardlink_of, e.link_target, coalesce(e.path, p.path), {top}, e.parent_id, e.depth"
    )
}

fn has_column(conn: &Connection, col: &str) -> bool {
    conn.prepare("SELECT name FROM pragma_table_info('entries')")
        .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>())
        .map(|cols| cols.iter().any(|c| c == col))
        .unwrap_or(false)
}

pub(crate) fn cat_key(code: Option<i64>) -> Option<String> {
    code.map(|c| Category::from_code(c as u8).key().to_string())
}

fn entry_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<EntryView> {
    let kind = match r.get::<_, i64>(2)? {
        1 => EntryKind::Dir,
        2 => EntryKind::Link,
        _ => EntryKind::File,
    };
    let name: String = r.get(1)?;
    let own_or_parent: Option<String> = r.get(14)?;
    let path = match kind {
        EntryKind::Dir => own_or_parent.unwrap_or_default(),
        _ => diskdash_core::paths::join(&own_or_parent.unwrap_or_default(), &name),
    };
    let flags: i64 = r.get(11)?;
    Ok(EntryView {
        id: r.get(0)?,
        name,
        path,
        kind,
        size: r.get::<_, i64>(3)?.max(0) as u64,
        alloc: r.get::<_, i64>(4)?.max(0) as u64,
        modified: rfc3339(r.get(5)?),
        category: cat_key(r.get(6)?),
        top_category: cat_key(r.get(15)?),
        ext: r.get::<_, Option<String>>(7)?.filter(|e| !e.is_empty()),
        file_count: r.get::<_, Option<i64>>(8)?.map(|v| v.max(0) as u64),
        dir_count: r.get::<_, Option<i64>>(9)?.map(|v| v.max(0) as u64),
        newest_modified: rfc3339(r.get(10)?),
        access_denied: flags & 1 != 0,
        cloud_placeholder: flags & 2 != 0,
        hardlink_repeat: r.get::<_, Option<i64>>(12)?.is_some(),
        link_target: r.get(13)?,
    })
}

/// A directory of the snapshot by full path (or the root when `None`).
pub struct DirRef {
    pub id: i64,
    pub depth: i64,
    pub last_desc: i64,
}

pub fn find_dir(conn: &Connection, path: Option<&str>) -> Result<DirRef, ApiError> {
    type DirRow = (i64, i64, Option<i64>);
    let read = |r: &rusqlite::Row<'_>| -> rusqlite::Result<DirRow> { Ok((r.get(0)?, r.get(1)?, r.get(2)?)) };
    let row: Option<DirRow> = match path.map(|p| p.trim_end_matches('\\')).filter(|p| !p.is_empty()) {
        None => conn.query_row("SELECT id, depth, last_desc_id FROM entries WHERE id = 0", [], read).optional(),
        Some(p) => conn
            .query_row(
                "SELECT id, depth, last_desc_id FROM entries WHERE kind = 1 AND (path = ?1 OR path = ?2) LIMIT 1",
                params![p, format!("{p}\\")],
                read,
            )
            .optional(),
    }
    .map_err(ApiError::internal)?;
    match row {
        Some((id, depth, last_desc)) => Ok(DirRef { id, depth, last_desc: last_desc.unwrap_or(id) }),
        None => Err(ApiError::NotFound(format!("no directory {} in this snapshot", path.unwrap_or("(root)")))),
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Page<T: TS> {
    pub items: Vec<T>,
    /// Pass back as `cursor` for the next page; null on the last page.
    #[ts(type = "number | null")]
    pub next_cursor: Option<u32>,
}

pub fn children(
    conn: &Connection,
    dir: &DirRef,
    measure: Measure,
    limit: u32,
    cursor: u32,
) -> Result<Page<EntryView>, ApiError> {
    let limit = limit.clamp(1, MAX_PAGE);
    let col = measure.column();
    let cols = entry_cols(conn);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {cols} FROM entries e LEFT JOIN entries p ON p.id = e.parent_id
             WHERE e.parent_id = ?1 AND e.id != 0 ORDER BY e.{col} DESC, e.id LIMIT ?2 OFFSET ?3"
        ))
        .map_err(ApiError::internal)?;
    let items: Vec<EntryView> = stmt
        .query_map(params![dir.id, limit + 1, cursor], entry_row)
        .and_then(|rows| rows.collect())
        .map_err(ApiError::internal)?;
    page(items, limit, cursor)
}

fn page(mut items: Vec<EntryView>, limit: u32, cursor: u32) -> Result<Page<EntryView>, ApiError> {
    let more = items.len() > limit as usize;
    items.truncate(limit as usize);
    Ok(Page { items, next_cursor: more.then_some(cursor + limit) })
}

#[derive(Debug, Clone, Default)]
pub struct TopQuery {
    pub limit: u32,
    pub offset: u32,
    /// Category key filter (files only).
    pub category: Option<String>,
}

pub fn top_files(
    conn: &Connection,
    under: &DirRef,
    measure: Measure,
    q: &TopQuery,
) -> Result<Vec<EntryView>, ApiError> {
    let col = measure.column();
    let cols = entry_cols(conn);
    let category = match q.category.as_deref() {
        None => None,
        Some(k) => Some(
            diskdash_core::category::ALL
                .iter()
                .find(|c| c.key() == k)
                .map(|c| *c as i64)
                .ok_or_else(|| ApiError::BadRequest(format!("unknown category {k}")))?,
        ),
    };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {cols} FROM entries e LEFT JOIN entries p ON p.id = e.parent_id
             WHERE e.kind = 0 AND e.hardlink_of IS NULL AND e.id BETWEEN ?1 AND ?2 AND (?5 IS NULL OR e.category = ?5)
             ORDER BY e.{col} DESC LIMIT ?3 OFFSET ?4"
        ))
        .map_err(ApiError::internal)?;
    stmt.query_map(params![under.id, under.last_desc, q.limit.clamp(1, MAX_PAGE), q.offset, category], entry_row)
        .and_then(|rows| rows.collect())
        .map_err(ApiError::internal)
}

pub fn top_dirs(
    conn: &Connection,
    under: &DirRef,
    max_depth: Option<u32>,
    measure: Measure,
    q: &TopQuery,
) -> Result<Vec<EntryView>, ApiError> {
    let col = measure.column();
    let cols = entry_cols(conn);
    let max_depth = max_depth.map(|d| under.depth + i64::from(d.max(1))).unwrap_or(i64::MAX);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {cols} FROM entries e LEFT JOIN entries p ON p.id = e.parent_id
             WHERE e.kind = 1 AND e.id > ?1 AND e.id <= ?2 AND e.depth <= ?3
             ORDER BY e.{col} DESC LIMIT ?4 OFFSET ?5"
        ))
        .map_err(ApiError::internal)?;
    stmt.query_map(params![under.id, under.last_desc, max_depth, q.limit.clamp(1, MAX_PAGE), q.offset], entry_row)
        .and_then(|rows| rows.collect())
        .map_err(ApiError::internal)
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CategoryTotal {
    pub category: String,
    #[ts(type = "number")]
    pub file_count: u64,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub alloc: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ExtTotal {
    /// Empty string for files without an extension.
    pub ext: String,
    pub category: String,
    #[ts(type = "number")]
    pub file_count: u64,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub alloc: u64,
}

pub fn categories(conn: &Connection, under: &DirRef) -> Result<Vec<CategoryTotal>, ApiError> {
    let sql = if under.id == 0 {
        "SELECT category, file_count, size, alloc FROM category_totals ORDER BY alloc DESC"
    } else {
        "SELECT coalesce(category, 9), count(*), sum(size), sum(alloc) FROM entries
         WHERE kind = 0 AND hardlink_of IS NULL AND id BETWEEN ?1 AND ?2 GROUP BY coalesce(category, 9) ORDER BY 4 DESC"
    };
    let mut stmt = conn.prepare(sql).map_err(ApiError::internal)?;
    let map = |r: &rusqlite::Row<'_>| {
        Ok(CategoryTotal {
            category: Category::from_code(r.get::<_, i64>(0)? as u8).key().to_string(),
            file_count: r.get::<_, i64>(1)?.max(0) as u64,
            size: r.get::<_, i64>(2)?.max(0) as u64,
            alloc: r.get::<_, i64>(3)?.max(0) as u64,
        })
    };
    let rows =
        if under.id == 0 { stmt.query_map([], map) } else { stmt.query_map(params![under.id, under.last_desc], map) };
    rows.and_then(|r| r.collect()).map_err(ApiError::internal)
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ExtBreakdown {
    pub items: Vec<ExtTotal>,
    /// Everything beyond `limit`, summed.
    pub other: Option<ExtTotal>,
}

pub fn extensions(conn: &Connection, under: &DirRef, limit: u32) -> Result<ExtBreakdown, ApiError> {
    let sql = if under.id == 0 {
        "SELECT ext, category, file_count, size, alloc FROM ext_totals ORDER BY alloc DESC"
    } else {
        "SELECT coalesce(ext, ''), coalesce(max(category), 9), count(*), sum(size), sum(alloc) FROM entries
         WHERE kind = 0 AND hardlink_of IS NULL AND id BETWEEN ?1 AND ?2 GROUP BY coalesce(ext, '') ORDER BY 5 DESC"
    };
    let mut stmt = conn.prepare(sql).map_err(ApiError::internal)?;
    let map = |r: &rusqlite::Row<'_>| {
        Ok(ExtTotal {
            ext: r.get(0)?,
            category: Category::from_code(r.get::<_, i64>(1)? as u8).key().to_string(),
            file_count: r.get::<_, i64>(2)?.max(0) as u64,
            size: r.get::<_, i64>(3)?.max(0) as u64,
            alloc: r.get::<_, i64>(4)?.max(0) as u64,
        })
    };
    let all: Vec<ExtTotal> = if under.id == 0 {
        stmt.query_map([], map).and_then(|r| r.collect())
    } else {
        stmt.query_map(params![under.id, under.last_desc], map).and_then(|r| r.collect())
    }
    .map_err(ApiError::internal)?;
    let limit = limit.clamp(1, MAX_PAGE) as usize;
    let other = (all.len() > limit).then(|| {
        all[limit..].iter().fold(
            ExtTotal { ext: "other".into(), category: "other".into(), file_count: 0, size: 0, alloc: 0 },
            |mut acc, e| {
                acc.file_count += e.file_count;
                acc.size += e.size;
                acc.alloc += e.alloc;
                acc
            },
        )
    });
    Ok(ExtBreakdown { items: all.into_iter().take(limit).collect(), other })
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScanErrorRow {
    pub path: String,
    pub op: String,
    pub code: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ErrorSummary {
    pub code: u32,
    pub label: String,
    #[ts(type = "number")]
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ErrorsResponse {
    pub summary: Vec<ErrorSummary>,
    pub items: Vec<ScanErrorRow>,
    #[ts(type = "number | null")]
    pub next_cursor: Option<u32>,
}

fn error_label(code: u32) -> &'static str {
    match code {
        2 | 3 => "Deleted during the scan",
        5 => "Access denied",
        32 => "In use by another program",
        1920 => "Cannot access the file",
        _ => "Other error",
    }
}

pub fn errors(conn: &Connection, code: Option<u32>, limit: u32, cursor: u32) -> Result<ErrorsResponse, ApiError> {
    let summary = conn
        .prepare("SELECT code, count(*) FROM errors GROUP BY code ORDER BY 2 DESC")
        .and_then(|mut s| {
            s.query_map([], |r| {
                let code = r.get::<_, i64>(0)? as u32;
                Ok(ErrorSummary { code, label: error_label(code).into(), count: r.get::<_, i64>(1)? as u64 })
            })?
            .collect()
        })
        .map_err(ApiError::internal)?;
    let limit = limit.clamp(1, MAX_PAGE);
    let mut items: Vec<ScanErrorRow> = conn
        .prepare(
            "SELECT path, op, code, message FROM errors WHERE (?1 IS NULL OR code = ?1) ORDER BY id LIMIT ?2 OFFSET ?3",
        )
        .and_then(|mut s| {
            s.query_map(params![code, limit + 1, cursor], |r| {
                Ok(ScanErrorRow {
                    path: r.get(0)?,
                    op: r.get(1)?,
                    code: r.get::<_, i64>(2)? as u32,
                    message: r.get(3)?,
                })
            })?
            .collect()
        })
        .map_err(ApiError::internal)?;
    let more = items.len() > limit as usize;
    items.truncate(limit as usize);
    Ok(ErrorsResponse { summary, items, next_cursor: more.then_some(cursor + limit) })
}

/// A directory by entry id.
pub fn find_dir_by_id(conn: &Connection, id: i64) -> Result<DirRef, ApiError> {
    conn.query_row("SELECT id, depth, last_desc_id FROM entries WHERE id = ?1 AND kind = 1", [id], |r| {
        Ok(DirRef { id: r.get(0)?, depth: r.get(1)?, last_desc: r.get::<_, Option<i64>>(2)?.unwrap_or(id) })
    })
    .optional()
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::NotFound(format!("no directory with id {id} in this snapshot")))
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct OtherBucket {
    #[ts(type = "number")]
    pub count: u64,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub alloc: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TreeNode {
    pub entry: EntryView,
    /// Present when the level below was loaded.
    pub children: Option<Vec<TreeNode>>,
    /// Children beyond the per-level cap, folded together.
    pub other: Option<OtherBucket>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Crumb {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TreeResponse {
    pub node: TreeNode,
    /// Root first, ending with the node itself.
    pub ancestors: Vec<Crumb>,
    pub limit: u32,
}

pub(crate) fn entry_by_id(conn: &Connection, id: i64) -> Result<EntryView, ApiError> {
    let cols = entry_cols(conn);
    conn.query_row(
        &format!("SELECT {cols} FROM entries e LEFT JOIN entries p ON p.id = e.parent_id WHERE e.id = ?1"),
        [id],
        entry_row,
    )
    .optional()
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::NotFound(format!("no entry with id {id} in this snapshot")))
}

/// Up to `limit` largest children (links and hard-link repeats left out, as in
/// the aggregates) plus an "Other" bucket so the level always sums to the parent.
fn capped_children(
    conn: &Connection,
    parent: &EntryView,
    measure: Measure,
    limit: u32,
) -> Result<(Vec<EntryView>, Option<OtherBucket>), ApiError> {
    let cols = entry_cols(conn);
    let col = measure.column();
    let kids: Vec<EntryView> = conn
        .prepare(&format!(
            "SELECT {cols} FROM entries e LEFT JOIN entries p ON p.id = e.parent_id
             WHERE e.parent_id = ?1 AND e.id != 0 AND e.kind != 2 AND e.hardlink_of IS NULL
             ORDER BY e.{col} DESC, e.id LIMIT ?2"
        ))
        .and_then(|mut s| s.query_map(params![parent.id, limit], entry_row)?.collect())
        .map_err(ApiError::internal)?;
    let total: i64 = conn
        .query_row(
            "SELECT count(*) FROM entries WHERE parent_id = ?1 AND id != 0 AND kind != 2 AND hardlink_of IS NULL",
            [parent.id],
            |r| r.get(0),
        )
        .map_err(ApiError::internal)?;
    let count = (total.max(0) as u64).saturating_sub(kids.len() as u64);
    let other = (count > 0).then(|| OtherBucket {
        count,
        size: parent.size.saturating_sub(kids.iter().map(|k| k.size).sum()),
        alloc: parent.alloc.saturating_sub(kids.iter().map(|k| k.alloc).sum()),
    });
    Ok((kids, other))
}

/// The node, its children (and grandchildren when `depth` is 2), capped per level.
pub fn tree(
    conn: &Connection,
    node_id: i64,
    depth: u32,
    measure: Measure,
    limit: u32,
) -> Result<TreeResponse, ApiError> {
    // Owner decision: at most 20 per level (5 minimum) so tiles stay readable.
    let limit = limit.clamp(5, 20);
    let root = entry_by_id(conn, node_id)?;
    if root.kind != EntryKind::Dir {
        return Err(ApiError::BadRequest(format!("entry {node_id} is not a directory")));
    }
    let (kids, other) = capped_children(conn, &root, measure, limit)?;
    let mut children = Vec::with_capacity(kids.len());
    for k in kids {
        let child = if depth >= 2 && k.kind == EntryKind::Dir && !k.access_denied {
            let (grand, gother) = capped_children(conn, &k, measure, limit)?;
            TreeNode {
                children: Some(grand.into_iter().map(|g| TreeNode { entry: g, children: None, other: None }).collect()),
                other: gother,
                entry: k,
            }
        } else {
            TreeNode { entry: k, children: None, other: None }
        };
        children.push(child);
    }

    // Walk up for the breadcrumb (depth is small; primary-key lookups).
    let mut ancestors = Vec::new();
    let mut cur: Option<i64> = Some(node_id);
    while let Some(id) = cur {
        let (name, path, parent): (String, Option<String>, Option<i64>) = conn
            .query_row("SELECT name, path, parent_id FROM entries WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(ApiError::internal)?;
        ancestors.push(Crumb { id, name, path: path.unwrap_or_default() });
        cur = if id == 0 { None } else { parent };
    }
    ancestors.reverse();
    Ok(TreeResponse { node: TreeNode { entry: root, children: Some(children), other }, ancestors, limit })
}

/// Resolve an entry's full path (for Explorer).
pub fn entry_path(conn: &Connection, id: i64) -> Result<(String, EntryKind), ApiError> {
    let e = entry_by_id(conn, id)?;
    Ok((e.path, e.kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskdash_store::{open_catalog, DataDir};

    #[test]
    fn reconcile_marks_interrupted_missing_and_removes_orphans() {
        let tmp = tempfile::tempdir().unwrap();
        let dd = DataDir::at(tmp.path().join("dd"));
        let own = OwnFiles::new(&dd).unwrap();
        let db = open_catalog(&dd, &own).unwrap();
        let snaps = dd.snapshots_dir();
        own.create_dir_all(&snaps).unwrap();
        drop(own.open_append(&snaps.join("1.db")).unwrap());
        drop(own.open_append(&snaps.join("99.db")).unwrap()); // orphan
        let conn = db.connect().unwrap();
        for (id, status, file) in [(1, "running", "snapshots/1.db"), (2, "complete", "snapshots/2.db")] {
            conn.execute(
                "INSERT INTO snapshots (id, root, hardlinks, status, started_at, db_file) VALUES (?1, 'C:\\', 'detected', ?2, 'x', ?3)",
                params![id, status, file],
            )
            .unwrap();
        }
        reconcile(&db, &own, &snaps).unwrap();
        let s1 = get(&conn, 1).unwrap().unwrap();
        assert_eq!(s1.status, SnapshotStatus::Partial, "interrupted scans become resumable");
        let s2 = get(&conn, 2).unwrap().unwrap();
        assert_eq!(s2.status, SnapshotStatus::Failed, "missing file → failed");
        assert!(snaps.join("1.db").exists());
        assert!(!snaps.join("99.db").exists(), "orphan file removed");
    }
}

/// Response wrapper: every snapshot-derived answer says which snapshot and when.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SnapshotData<T: TS> {
    #[ts(type = "number")]
    pub snapshot_id: i64,
    pub as_of: Option<String>,
    pub data: T,
}

impl<T: TS> SnapshotData<T> {
    pub fn new(snap: &Snapshot, data: T) -> Self {
        Self { snapshot_id: snap.id, as_of: snap.finished_at.clone().or_else(|| Some(snap.started_at.clone())), data }
    }
}
