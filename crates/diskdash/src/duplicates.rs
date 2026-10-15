//! Duplicate finder service: runs the hashing engine as a background job and
//! serves the groups. Suggestions only: nothing here (or anywhere) deletes,
//! moves, links or changes a file.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use diskdash_analysis::duplicates::{
    self, choose_keep, near_duplicates, CacheKey, CachedHash, EngineEnv, HashCache, KeepRule, Member, ScopeSnapshot,
    SkipReason, Source, ALGO,
};
use diskdash_analysis::rules::Env;
use diskdash_inventory::model::MediaClass;
use diskdash_store::{Db, OwnFiles};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ApiError;
use crate::inventory::InventoryService;
use crate::jobs::{Job, JobContext, JobError, JobRunner};
use crate::settings;
use crate::snapshots::{self, SnapshotStatus};
use crate::time::now_rfc3339;

pub const DEFAULT_MIN_MIB: u64 = 1;
pub const DEFAULT_NEAR_THRESHOLD: f64 = 0.9;
const NEAR_MIN_FILES: usize = 10;
const NEAR_MAX_PAIRS: usize = 10_000;
const MAX_SKIP_ROWS: usize = 5_000;
const KEEP_RUNS_IN_CACHE: i64 = 5;
pub const PAGE: u32 = 20;
const MAX_PAGE: u32 = 100;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DupRequest {
    /// Scans to look through; empty = the latest scan of every drive.
    #[serde(default)]
    pub snapshot_ids: Vec<i64>,
    /// Smallest file considered, in MiB (default 1).
    pub min_size_mib: Option<u64>,
    #[serde(default)]
    pub near_duplicates: bool,
    pub near_threshold: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum DupStatus {
    Queued,
    Running,
    Complete,
    Failed,
    Cancelled,
}

impl DupStatus {
    fn parse(s: &str) -> Self {
        match s {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "complete" => Self::Complete,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DupRun {
    #[ts(type = "number")]
    pub id: i64,
    pub status: DupStatus,
    pub algo: String,
    #[ts(type = "number")]
    pub min_size: u64,
    /// Roots that were searched.
    pub roots: Vec<String>,
    pub near_enabled: bool,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    #[ts(type = "number | null")]
    pub job_id: Option<i64>,
    #[ts(type = "number")]
    pub candidates: u64,
    #[ts(type = "number")]
    pub files_hashed: u64,
    #[ts(type = "number")]
    pub cache_hits: u64,
    #[ts(type = "number")]
    pub group_count: u64,
    /// (members - 1) x allocated size, summed over every group.
    #[ts(type = "number")]
    pub wasted: u64,
    /// Files left out, by reason (cloud placeholder, changed since scan, ...).
    #[ts(type = "Record<string, number>")]
    pub skips: BTreeMap<String, u64>,
    pub near_truncated: bool,
    /// Roots where reading files may update last-access times.
    pub atime_roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DupMember {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number")]
    pub snapshot_id: i64,
    #[ts(type = "number")]
    pub entry_id: i64,
    pub path: String,
    pub drive: String,
    pub modified: Option<String>,
    pub category: Option<String>,
    /// Suggested outcome: `keep`, `remove`, or none when no keep rule was applied.
    pub suggested: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DupGroup {
    #[ts(type = "number")]
    pub id: i64,
    pub hash: String,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub alloc: u64,
    #[ts(type = "number")]
    pub wasted: u64,
    pub rule: Option<String>,
    pub members: Vec<DupMember>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DupGroupsPage {
    pub groups: Vec<DupGroup>,
    #[ts(type = "number | null")]
    pub next_cursor: Option<u32>,
    /// Groups matching the filters.
    #[ts(type = "number")]
    pub total: u64,
    #[ts(type = "number")]
    pub total_wasted: u64,
    pub run: Option<DupRun>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NearView {
    pub dir_a: String,
    pub dir_b: String,
    pub similarity: f64,
    #[ts(type = "number")]
    pub shared_files: u64,
    #[ts(type = "number")]
    pub shared_bytes: u64,
    #[ts(type = "number")]
    pub differing_bytes: u64,
    #[ts(type = "number")]
    pub files_a: u64,
    #[ts(type = "number")]
    pub files_b: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SkipRow {
    pub reason: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GroupFilter {
    pub drive: Option<String>,
    pub folder: Option<String>,
    pub category: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SuggestRequest {
    /// newest | oldest | shortest-path | prefer-drive | prefer-folder
    pub rule: String,
    /// Drive letter or folder for the prefer rules.
    pub value: Option<String>,
    /// One group; omitted = every group matching the filter.
    pub group_id: Option<i64>,
    pub drive: Option<String>,
    pub folder: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SuggestResult {
    #[ts(type = "number")]
    pub applied: u64,
}

pub struct DupService {
    pub db: Db,
    pub own: OwnFiles,
    pub jobs: JobRunner,
    pub inventory: Arc<InventoryService>,
    pub env: Arc<dyn Env>,
    pub source: Arc<dyn Source>,
}

fn rule_from(req: &SuggestRequest) -> Result<KeepRule, ApiError> {
    let need = |what: &str| {
        req.value
            .clone()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| ApiError::BadRequest(format!("this rule needs a {what}")))
    };
    Ok(match req.rule.as_str() {
        "newest" => KeepRule::Newest,
        "oldest" => KeepRule::Oldest,
        "shortest-path" => KeepRule::ShortestPath,
        "prefer-drive" => KeepRule::PreferDrive(need("drive")?),
        "prefer-folder" => KeepRule::PreferFolder(need("folder")?),
        other => return Err(ApiError::BadRequest(format!("unknown keep rule {other}"))),
    })
}

/// Catalog-backed hash cache. Each hit or store marks the entry as used in this run.
struct SqlCache {
    conn: Mutex<Connection>,
    run: i64,
}

impl HashCache for SqlCache {
    fn get(&self, k: &CacheKey) -> Option<CachedHash> {
        let c = self.conn.lock().ok()?;
        let row = c
            .query_row(
                "SELECT partial, full FROM hash_cache WHERE volume_serial = ?1 AND file_id = ?2 AND size = ?3 AND mtime = ?4 AND algo = ?5",
                params![i64::from(k.serial), k.index as i64, k.size as i64, k.mtime, ALGO],
                |r| Ok(CachedHash { partial: r.get(0)?, full: r.get(1)? }),
            )
            .ok()?;
        let _ = c.execute(
            "UPDATE hash_cache SET last_run = ?6 WHERE volume_serial = ?1 AND file_id = ?2 AND size = ?3 AND mtime = ?4 AND algo = ?5",
            params![i64::from(k.serial), k.index as i64, k.size as i64, k.mtime, ALGO, self.run],
        );
        Some(row)
    }

    fn put(&self, k: &CacheKey, v: &CachedHash) {
        let Ok(c) = self.conn.lock() else { return };
        let _ = c.execute(
            "INSERT INTO hash_cache (volume_serial, file_id, size, mtime, algo, partial, full, last_run)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(volume_serial, file_id, size, mtime, algo) DO UPDATE SET
               partial = coalesce(excluded.partial, partial), full = coalesce(excluded.full, full), last_run = excluded.last_run",
            params![i64::from(k.serial), k.index as i64, k.size as i64, k.mtime, ALGO, v.partial, v.full, self.run],
        );
    }
}

fn run_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DupRun> {
    let json = |i: usize| -> rusqlite::Result<String> { r.get(i) };
    let scope: serde_json::Value = serde_json::from_str(&json(3)?).unwrap_or_default();
    Ok(DupRun {
        id: r.get(0)?,
        status: DupStatus::parse(&r.get::<_, String>(1)?),
        algo: r.get(2)?,
        roots: scope["roots"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        min_size: r.get::<_, i64>(4)?.max(0) as u64,
        near_enabled: r.get::<_, i64>(5)? != 0,
        started_at: r.get(6)?,
        finished_at: r.get(7)?,
        error: r.get(8)?,
        job_id: r.get(9)?,
        candidates: r.get::<_, i64>(10)?.max(0) as u64,
        files_hashed: r.get::<_, i64>(11)?.max(0) as u64,
        cache_hits: r.get::<_, i64>(12)?.max(0) as u64,
        group_count: r.get::<_, i64>(13)?.max(0) as u64,
        wasted: r.get::<_, i64>(14)?.max(0) as u64,
        skips: serde_json::from_str(&json(15)?).unwrap_or_default(),
        near_truncated: r.get::<_, i64>(16)? != 0,
        atime_roots: serde_json::from_str(&json(17)?).unwrap_or_default(),
    })
}

const RUN_COLS: &str =
    "id, status, algo, scope, min_size, near_enabled, started_at, finished_at, error, job_id, candidates,
    files_hashed, cache_hits, group_count, wasted, skips, near_truncated, atime_roots";

type GroupRow = (i64, String, i64, i64, i64, Option<String>, Option<i64>);

fn drive_letter(root: &str) -> String {
    root.chars().next().map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default()
}

fn sql_like(folder: &str) -> String {
    let f = folder.trim().trim_end_matches('\\').to_lowercase().replace(['%', '_'], "");
    format!("{f}\\%")
}

impl DupService {
    /// Startup: runs that were queued or running when the app stopped did not finish.
    pub fn reconcile(db: &Db) -> Result<(), String> {
        let conn = db.connect().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE dup_runs SET status = 'failed', error = 'interrupted' WHERE status IN ('queued', 'running')",
            [],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Start a run. Only one at a time.
    pub fn start(self: &Arc<Self>, req: &DupRequest) -> Result<u64, ApiError> {
        let conn = self.db.connect().map_err(ApiError::internal)?;
        let busy: bool = conn
            .query_row("SELECT EXISTS (SELECT 1 FROM dup_runs WHERE status IN ('queued', 'running'))", [], |r| r.get(0))
            .map_err(ApiError::internal)?;
        if busy {
            return Err(ApiError::Conflict("a duplicate search is already running".into()));
        }
        let min_mib = req.min_size_mib.unwrap_or(DEFAULT_MIN_MIB);
        if !(1..=1_000_000).contains(&min_mib) {
            return Err(ApiError::BadRequest("min_size_mib must be at least 1".into()));
        }
        let threshold = req.near_threshold.unwrap_or(DEFAULT_NEAR_THRESHOLD);
        if !(0.5..=1.0).contains(&threshold) {
            return Err(ApiError::BadRequest("near_threshold must be between 0.5 and 1".into()));
        }
        let ids: Vec<i64> = if req.snapshot_ids.is_empty() {
            snapshots::latest(&conn)
                .map_err(ApiError::internal)?
                .into_iter()
                .filter(|s| crate::cleanup::is_drive_root(&s.root))
                .map(|s| s.id)
                .collect()
        } else {
            req.snapshot_ids.clone()
        };
        let mut roots = Vec::new();
        let mut atime_roots = Vec::new();
        for id in &ids {
            let s = snapshots::get(&conn, *id)
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::NotFound(format!("snapshot {id} not found")))?;
            if s.status != SnapshotStatus::Complete {
                return Err(ApiError::Conflict(format!(
                    "snapshot {id} is {}; only complete scans can be searched",
                    s.status.as_str()
                )));
            }
            if s.time_basis == diskdash_analysis::stale::TimeBasis::Atime {
                atime_roots.push(s.root.clone());
            }
            roots.push(s.root);
        }
        if ids.is_empty() {
            return Err(ApiError::Conflict("scan a drive first: duplicates are found from a completed scan".into()));
        }
        conn.execute(
            "INSERT INTO dup_runs (status, algo, min_size, scope, near_enabled, near_threshold, started_at, atime_roots)
             VALUES ('queued', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                ALGO,
                (min_mib << 20) as i64,
                serde_json::json!({ "snapshots": ids, "roots": roots }).to_string(),
                req.near_duplicates,
                threshold,
                now_rfc3339(),
                serde_json::to_string(&atime_roots).unwrap_or_else(|_| "[]".into()),
            ],
        )
        .map_err(ApiError::internal)?;
        let run_id = conn.last_insert_rowid();
        let job = DupJob {
            svc: self.clone(),
            run_id,
            ids,
            roots,
            min_size: min_mib << 20,
            near: req.near_duplicates.then_some(threshold),
        };
        let job_id = self.jobs.submit(Box::new(job)).map_err(ApiError::from)?;
        conn.execute("UPDATE dup_runs SET job_id = ?2 WHERE id = ?1", params![run_id, job_id as i64])
            .map_err(ApiError::internal)?;
        Ok(job_id)
    }

    fn fail(&self, run: i64, status: &str, message: &str) {
        if let Ok(c) = self.db.connect() {
            let _ = c.execute(
                "UPDATE dup_runs SET status = ?2, error = ?3, finished_at = ?4 WHERE id = ?1",
                params![run, status, message, now_rfc3339()],
            );
        }
    }

    pub fn latest_run(&self, conn: &Connection) -> Option<DupRun> {
        conn.query_row(&format!("SELECT {RUN_COLS} FROM dup_runs ORDER BY id DESC LIMIT 1"), [], run_row).ok()
    }

    pub fn status(&self) -> Result<Option<DupRun>, ApiError> {
        let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
        Ok(self.latest_run(&conn))
    }

    fn filter_sql(f: &GroupFilter) -> (String, Vec<rusqlite::types::Value>) {
        let mut clauses = vec!["g.run_id = ?1".to_string()];
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        let mut member = Vec::new();
        if let Some(d) = f.drive.as_deref().filter(|d| !d.is_empty()) {
            args.push(drive_letter(d).into());
            member.push(format!("m.drive = ?{}", args.len() + 1));
        }
        if let Some(p) = f.folder.as_deref().filter(|d| !d.trim().is_empty()) {
            args.push(sql_like(p).into());
            member.push(format!("lower(m.path) LIKE ?{}", args.len() + 1));
        }
        if let Some(c) = f.category.as_deref().filter(|d| !d.is_empty()) {
            args.push(c.to_string().into());
            member.push(format!("m.category = ?{}", args.len() + 1));
        }
        if !member.is_empty() {
            clauses.push(format!(
                "EXISTS (SELECT 1 FROM dup_members m WHERE m.group_id = g.id AND {})",
                member.join(" AND ")
            ));
        }
        (clauses.join(" AND "), args)
    }

    fn load_members(conn: &Connection, group: i64, keep: Option<i64>) -> rusqlite::Result<Vec<DupMember>> {
        let mut stmt = conn.prepare(
            "SELECT id, snapshot_id, entry_id, path, drive, mtime, category FROM dup_members WHERE group_id = ?1 ORDER BY path",
        )?;
        let rows = stmt.query_map([group], |r| {
            let id: i64 = r.get(0)?;
            Ok(DupMember {
                id,
                snapshot_id: r.get(1)?,
                entry_id: r.get(2)?,
                path: r.get(3)?,
                drive: r.get(4)?,
                modified: snapshots::rfc3339(r.get(5)?),
                category: r.get(6)?,
                suggested: keep.map(|k| if k == id { "keep".to_string() } else { "remove".to_string() }),
            })
        })?;
        rows.collect()
    }

    pub fn groups(&self, f: &GroupFilter) -> Result<DupGroupsPage, ApiError> {
        let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
        let run = self.latest_run(&conn);
        let Some(r) = run.as_ref().filter(|r| r.status == DupStatus::Complete) else {
            // Show the last finished run while a new one is going.
            let done: Option<DupRun> = conn
                .query_row(
                    &format!("SELECT {RUN_COLS} FROM dup_runs WHERE status = 'complete' ORDER BY id DESC LIMIT 1"),
                    [],
                    run_row,
                )
                .ok();
            return match done {
                Some(d) => self.groups_of(&conn, &d, f, run),
                None => Ok(DupGroupsPage { groups: Vec::new(), next_cursor: None, total: 0, total_wasted: 0, run }),
            };
        };
        self.groups_of(&conn, r, f, run.clone())
    }

    fn groups_of(
        &self,
        conn: &Connection,
        shown: &DupRun,
        f: &GroupFilter,
        latest: Option<DupRun>,
    ) -> Result<DupGroupsPage, ApiError> {
        let limit = f.limit.unwrap_or(PAGE).clamp(1, MAX_PAGE);
        let cursor = f.cursor.unwrap_or(0);
        let (clause, args) = Self::filter_sql(f);
        let mut all: Vec<rusqlite::types::Value> = vec![shown.id.into()];
        all.extend(args);
        let (total, wasted): (i64, i64) = conn
            .query_row(
                &format!("SELECT count(*), coalesce(sum(g.wasted), 0) FROM dup_groups g WHERE {clause}"),
                rusqlite::params_from_iter(all.iter()),
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(ApiError::internal)?;
        let order = match f.sort.as_deref() {
            Some("size") => "g.size DESC",
            Some("count") => "g.member_count DESC, g.wasted DESC",
            _ => "g.wasted DESC",
        };
        let sql = format!(
            "SELECT g.id, g.hash, g.size, g.alloc, g.wasted, s.rule, s.keep_member FROM dup_groups g
             LEFT JOIN dup_suggestions s ON s.group_id = g.id WHERE {clause} ORDER BY {order}, g.id LIMIT {} OFFSET {}",
            limit + 1,
            cursor
        );
        let mut stmt = conn.prepare(&sql).map_err(ApiError::internal)?;
        let rows: Vec<GroupRow> = stmt
            .query_map(rusqlite::params_from_iter(all.iter()), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
            })
            .and_then(|it| it.collect())
            .map_err(ApiError::internal)?;
        let more = rows.len() > limit as usize;
        let mut groups = Vec::new();
        for (id, hash, size, alloc, w, rule, keep) in rows.into_iter().take(limit as usize) {
            groups.push(DupGroup {
                id,
                hash,
                size: size.max(0) as u64,
                alloc: alloc.max(0) as u64,
                wasted: w.max(0) as u64,
                rule,
                members: Self::load_members(conn, id, keep).map_err(ApiError::internal)?,
            });
        }
        Ok(DupGroupsPage {
            groups,
            next_cursor: more.then_some(cursor + limit),
            total: total.max(0) as u64,
            total_wasted: wasted.max(0) as u64,
            run: latest,
        })
    }

    pub fn near(&self) -> Result<Vec<NearView>, ApiError> {
        let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
        let mut stmt = conn
            .prepare(
                "SELECT dir_a, dir_b, similarity, shared_files, shared_bytes, differing_bytes, files_a, files_b FROM dup_near
                 WHERE run_id = (SELECT max(id) FROM dup_runs WHERE status = 'complete') ORDER BY similarity DESC, shared_bytes DESC LIMIT 200",
            )
            .map_err(ApiError::internal)?;
        let rows = stmt
            .query_map([], |r| {
                let u = |i: usize| r.get::<_, i64>(i).map(|v| v.max(0) as u64);
                Ok(NearView {
                    dir_a: r.get(0)?,
                    dir_b: r.get(1)?,
                    similarity: r.get(2)?,
                    shared_files: u(3)?,
                    shared_bytes: u(4)?,
                    differing_bytes: u(5)?,
                    files_a: u(6)?,
                    files_b: u(7)?,
                })
            })
            .map_err(ApiError::internal)?;
        rows.collect::<Result<_, _>>().map_err(ApiError::internal)
    }

    pub fn skips(&self, reason: Option<&str>) -> Result<Vec<SkipRow>, ApiError> {
        let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
        let mut stmt = conn
            .prepare(
                "SELECT reason, path FROM dup_skips WHERE run_id = (SELECT max(id) FROM dup_runs WHERE status = 'complete')
                 AND (?1 IS NULL OR reason = ?1) ORDER BY reason, path LIMIT 500",
            )
            .map_err(ApiError::internal)?;
        let rows = stmt
            .query_map([reason], |r| Ok(SkipRow { reason: r.get(0)?, path: r.get(1)? }))
            .map_err(ApiError::internal)?;
        rows.collect::<Result<_, _>>().map_err(ApiError::internal)
    }

    /// Store keep suggestions. Writes only our own catalog rows.
    pub fn suggest(&self, req: &SuggestRequest) -> Result<SuggestResult, ApiError> {
        let rule = rule_from(req)?;
        let mut conn = self.db.connect().map_err(ApiError::internal)?;
        let run: i64 = conn
            .query_row("SELECT max(id) FROM dup_runs WHERE status = 'complete'", [], |r| r.get::<_, Option<i64>>(0))
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::Conflict("no duplicate search has finished yet".into()))?;
        let group_ids: Vec<i64> = match req.group_id {
            Some(g) => {
                let ok: bool = conn
                    .query_row(
                        "SELECT EXISTS (SELECT 1 FROM dup_groups WHERE id = ?1 AND run_id = ?2)",
                        params![g, run],
                        |r| r.get(0),
                    )
                    .map_err(ApiError::internal)?;
                if !ok {
                    return Err(ApiError::NotFound(format!("no duplicate group {g}")));
                }
                vec![g]
            }
            None => {
                let f = GroupFilter {
                    drive: req.drive.clone(),
                    folder: req.folder.clone(),
                    category: req.category.clone(),
                    ..Default::default()
                };
                let (clause, args) = Self::filter_sql(&f);
                let mut all: Vec<rusqlite::types::Value> = vec![run.into()];
                all.extend(args);
                let mut stmt = conn
                    .prepare(&format!("SELECT g.id FROM dup_groups g WHERE {clause}"))
                    .map_err(ApiError::internal)?;
                let ids = stmt
                    .query_map(rusqlite::params_from_iter(all.iter()), |r| r.get(0))
                    .and_then(|it| it.collect::<Result<Vec<i64>, _>>())
                    .map_err(ApiError::internal)?;
                ids
            }
        };
        let tx = conn.transaction().map_err(ApiError::internal)?;
        let mut applied = 0;
        for g in group_ids {
            let members: Vec<Member> = {
                let mut stmt = tx
                    .prepare("SELECT id, path, mtime, drive FROM dup_members WHERE group_id = ?1")
                    .map_err(ApiError::internal)?;
                let rows = stmt
                    .query_map([g], |r| {
                        Ok(Member { id: r.get(0)?, path: r.get(1)?, mtime: r.get(2)?, drive: r.get(3)? })
                    })
                    .and_then(|it| it.collect::<Result<Vec<_>, _>>())
                    .map_err(ApiError::internal)?;
                rows
            };
            if let Some(keep) = choose_keep(&rule, &members) {
                tx.execute(
                    "INSERT INTO dup_suggestions (group_id, rule, keep_member, created_at) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(group_id) DO UPDATE SET rule = excluded.rule, keep_member = excluded.keep_member, created_at = excluded.created_at",
                    params![g, req.rule, keep, now_rfc3339()],
                )
                .map_err(ApiError::internal)?;
                applied += 1;
            }
        }
        tx.commit().map_err(ApiError::internal)?;
        Ok(SuggestResult { applied })
    }

    pub fn clear_suggestions(&self) -> Result<(), ApiError> {
        let conn = self.db.connect().map_err(ApiError::internal)?;
        conn.execute("DELETE FROM dup_suggestions", []).map_err(ApiError::internal)?;
        Ok(())
    }

    fn open_scopes(&self, ids: &[i64]) -> Result<Vec<(snapshots::Snapshot, Connection)>, String> {
        let cat = self.db.connect_read_only().map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for id in ids {
            let s = snapshots::get(&cat, *id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("snapshot {id} is gone"))?;
            let c = snapshots::open_snapshot(&self.own, &s).map_err(|e| e.to_string())?;
            out.push((s, c));
        }
        Ok(out)
    }

    fn disk_of(&self, root: &str) -> (String, bool) {
        let settings = self.db.connect_read_only().ok().and_then(|c| settings::load(&c).ok()).unwrap_or_default();
        let drives = self.inventory.snapshot(&settings).drives;
        match crate::scans::drive_for(&drives, root) {
            Some(d) => {
                let key = d
                    .physical_disks
                    .first()
                    .map_or_else(|| format!("volume-{}", d.id), |p| format!("disk-{}", p.number));
                (key, !matches!(d.media, MediaClass::SATA_SSD | MediaClass::NVME_SSD))
            }
            None => (format!("unknown-{root}"), true),
        }
    }
}

/// Background job: size, partial hash, full hash; then store the groups.
pub struct DupJob {
    svc: Arc<DupService>,
    run_id: i64,
    ids: Vec<i64>,
    roots: Vec<String>,
    min_size: u64,
    near: Option<f64>,
}

impl DupJob {
    fn execute(&self, ctx: &JobContext) -> Result<(), JobError> {
        let svc = &self.svc;
        let cat = svc.db.connect().map_err(|e| JobError::Failed(e.to_string()))?;
        cat.execute("UPDATE dup_runs SET status = 'running' WHERE id = ?1", [self.run_id])
            .map_err(|e| JobError::Failed(e.to_string()))?;

        ctx.progress(0, None, "bytes", Some("Choosing candidate files from the scans"));
        let opened = svc.open_scopes(&self.ids).map_err(JobError::Failed)?;
        let disks: Vec<(String, bool)> = opened.iter().map(|(s, _)| svc.disk_of(&s.root)).collect();
        let scopes: Vec<ScopeSnapshot<'_>> = opened
            .iter()
            .zip(&disks)
            .map(|((s, conn), (disk, hdd))| ScopeSnapshot {
                snapshot_id: s.id,
                root: s.root.clone(),
                volume_id: s.volume_id.clone(),
                volume_serial: serial_of(&cat, s.id),
                disk: disk.clone(),
                hdd: *hdd,
                conn,
            })
            .collect();
        let user = settings::load(&cat).unwrap_or_default();
        let protected = crate::protection::build(svc.env.as_ref(), &user);
        let selection =
            duplicates::select(&scopes, self.min_size, &protected).map_err(|e| JobError::Failed(e.to_string()))?;
        let cands = &selection.candidates;
        ctx.check_cancelled()?;

        let cache = SqlCache {
            conn: Mutex::new(svc.db.connect().map_err(|e| JobError::Failed(e.to_string()))?),
            run: self.run_id,
        };
        let cancelled = || ctx.is_cancelled();
        let progress = |done: u64, planned: u64| {
            ctx.progress(done, Some(planned.max(done)), "bytes", Some("Reading files to compare them"))
        };
        let env = EngineEnv {
            source: svc.source.as_ref(),
            cache: &cache,
            cancelled: &cancelled,
            progress: &progress,
            ssd_readers: std::thread::available_parallelism().map_or(2, |n| n.get().min(4)),
        };
        let outcome = match duplicates::find_duplicates(cands, &env) {
            Ok(o) => o,
            Err(_) => return Err(JobError::Cancelled),
        };

        // Store the result, replacing the previous run only now that this one is complete.
        let mut conn = svc.db.connect().map_err(|e| JobError::Failed(e.to_string()))?;
        let tx = conn.transaction().map_err(|e| JobError::Failed(e.to_string()))?;
        let mut wasted_total = 0u64;
        for g in &outcome.groups {
            let alloc = g.members.iter().map(|&i| cands[i].alloc).max().unwrap_or(g.size);
            let wasted = (g.members.len() as u64 - 1) * alloc;
            wasted_total += wasted;
            tx.execute(
                "INSERT INTO dup_groups (run_id, hash, size, alloc, member_count, wasted) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![self.run_id, g.hash, g.size as i64, alloc as i64, g.members.len() as i64, wasted as i64],
            )
            .map_err(|e| JobError::Failed(e.to_string()))?;
            let gid = tx.last_insert_rowid();
            for &i in &g.members {
                let c = &cands[i];
                tx.execute(
                    "INSERT INTO dup_members (group_id, snapshot_id, entry_id, path, drive, volume_id, mtime, category) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![gid, c.snapshot_id, c.entry_id, c.path, drive_letter(&c.root), c.volume_id, c.mtime, c.category.map(|v| diskdash_core::category::Category::from_code(v as u8).key().to_string())],
                )
                .map_err(|e| JobError::Failed(e.to_string()))?;
            }
        }
        let mut skips: BTreeMap<String, u64> = BTreeMap::new();
        for (key, n) in [
            ("cloud-placeholder", selection.excluded.cloud_placeholder),
            ("reparse-point", selection.excluded.reparse_point),
            ("hard-link", selection.excluded.hard_link),
            ("protected", selection.excluded.protected),
        ] {
            if n > 0 {
                skips.insert(key.into(), n);
            }
        }
        let mut stored = 0usize;
        for s in &outcome.skipped {
            *skips.entry(s.reason.key().to_string()).or_insert(0) += 1;
            if stored < MAX_SKIP_ROWS && s.reason != SkipReason::HardLink {
                stored += 1;
                tx.execute(
                    "INSERT INTO dup_skips (run_id, reason, path) VALUES (?1, ?2, ?3)",
                    params![self.run_id, s.reason.key(), cands[s.candidate].path],
                )
                .map_err(|e| JobError::Failed(e.to_string()))?;
            }
        }
        let mut truncated = false;
        if let Some(threshold) = self.near {
            let mut files = Vec::new();
            for g in &outcome.groups {
                for &i in &g.members {
                    let c = &cands[i];
                    if let Some((dir, name)) = c.path.rsplit_once('\\') {
                        files.push(duplicates::near::NearFile {
                            dir: dir.to_string(),
                            name: name.to_string(),
                            size: c.size,
                            hash: g.hash.clone(),
                        });
                    }
                }
            }
            let near = near_duplicates(&files, threshold, NEAR_MIN_FILES, NEAR_MAX_PAIRS);
            truncated = near.truncated;
            for p in &near.pairs {
                tx.execute(
                    "INSERT INTO dup_near (run_id, dir_a, dir_b, similarity, shared_files, shared_bytes, differing_bytes, files_a, files_b) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![self.run_id, p.dir_a, p.dir_b, p.similarity, p.shared_files as i64, p.shared_bytes as i64, p.differing_bytes as i64, p.files_a as i64, p.files_b as i64],
                )
                .map_err(|e| JobError::Failed(e.to_string()))?;
            }
        }
        tx.execute(
            "UPDATE dup_runs SET status = 'complete', finished_at = ?2, candidates = ?3, files_hashed = ?4, cache_hits = ?5,
               group_count = ?6, wasted = ?7, skips = ?8, near_truncated = ?9, bytes_hashed = ?10 WHERE id = ?1",
            params![
                self.run_id,
                now_rfc3339(),
                cands.len() as i64,
                outcome.files_hashed as i64,
                outcome.cache_hits as i64,
                outcome.groups.len() as i64,
                wasted_total as i64,
                serde_json::to_string(&skips).unwrap_or_else(|_| "{}".into()),
                truncated,
                outcome.bytes_read as i64,
            ],
        )
        .map_err(|e| JobError::Failed(e.to_string()))?;
        tx.execute("DELETE FROM dup_runs WHERE id != ?1", [self.run_id])
            .map_err(|e| JobError::Failed(e.to_string()))?;
        tx.execute("DELETE FROM hash_cache WHERE last_run < ?1", [self.run_id - KEEP_RUNS_IN_CACHE])
            .map_err(|e| JobError::Failed(e.to_string()))?;
        tx.commit().map_err(|e| JobError::Failed(e.to_string()))?;
        ctx.progress(outcome.bytes_read, Some(outcome.bytes_read.max(1)), "bytes", Some("Duplicates ready"));
        Ok(())
    }
}

fn serial_of(cat: &Connection, snapshot: i64) -> u32 {
    cat.query_row("SELECT volume_serial FROM snapshots WHERE id = ?1", [snapshot], |r| r.get::<_, Option<i64>>(0))
        .ok()
        .flatten()
        .unwrap_or(0) as u32
}

impl Job for DupJob {
    fn kind(&self) -> &'static str {
        "duplicates"
    }

    fn params(&self) -> serde_json::Value {
        serde_json::json!({ "run": self.run_id, "roots": self.roots })
    }

    fn locks(&self) -> Vec<PathBuf> {
        self.roots.iter().map(PathBuf::from).collect()
    }

    fn run(&mut self, ctx: &JobContext) -> Result<(), JobError> {
        let result = self.execute(ctx);
        match &result {
            Ok(()) => {}
            Err(JobError::Cancelled) => self.svc.fail(self.run_id, "cancelled", "cancelled"),
            Err(JobError::Failed(m)) => self.svc.fail(self.run_id, "failed", m),
        }
        result
    }
}

/// Map used by tests: file counts per drive from a group page.
pub fn drives_of(page: &DupGroupsPage) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for g in &page.groups {
        for mem in &g.members {
            *m.entry(mem.drive.clone()).or_insert(0) += 1;
        }
    }
    m
}
