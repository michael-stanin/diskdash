//! Cleanup suggestions: runs the rules engine over scan snapshots (as a
//! background job), keeps the totals in the catalog and serves candidates.
//!
//! Strictly read-only towards the user's files. Evaluation reads snapshot data
//! and writes only its own results into the snapshot database. Instructions
//! are text for the user; nothing in here executes them.

use std::path::PathBuf;
use std::sync::Arc;

use diskdash_analysis::eval::probe::Probe;
use diskdash_analysis::eval::{evaluate, EvalError, EvalOptions};
use diskdash_analysis::rules::{
    self, expand, AppliesTo, Conditions, Env, LoadedRule, Risk, RuleIssue, RuleKind, RuleSet, RuleSource, UserRulesDir,
    VirtualSource,
};
use diskdash_analysis::stale::TimeBasis;
use diskdash_store::{Db, OwnFiles};
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ApiError;
use crate::jobs::{Job, JobContext, JobError, JobRunner};
use crate::snapshots::{self, Snapshot, SnapshotStatus};
use crate::time::now_rfc3339;

pub const DEFAULT_PAGE: u32 = 20;
pub const MAX_PAGE: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum RunStatus {
    Queued,
    Running,
    Complete,
    Failed,
}

impl RunStatus {
    fn parse(s: &str) -> Self {
        match s {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "complete" => Self::Complete,
            _ => Self::Failed,
        }
    }
}

/// State of the rule evaluation for one snapshot.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CleanupRun {
    #[ts(type = "number")]
    pub snapshot_id: i64,
    pub status: RunStatus,
    pub error: Option<String>,
    pub finished_at: Option<String>,
    pub rule_errors: u32,
    #[ts(type = "number")]
    pub match_count: u64,
    /// The rules changed since this result was produced.
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Saving {
    pub risk: Risk,
    pub kind: RuleKind,
    /// Allocated bytes, each counted once.
    #[ts(type = "number")]
    pub allocated: u64,
    #[ts(type = "number")]
    pub logical: u64,
    #[ts(type = "number")]
    pub matches: u64,
    /// Candidates whose size could not be measured.
    #[ts(type = "number")]
    pub unknown: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SnapshotCleanup {
    pub run: Option<CleanupRun>,
    pub savings: Vec<Saving>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DriveCleanup {
    #[ts(type = "number")]
    pub snapshot_id: i64,
    pub root: String,
    pub volume_id: Option<String>,
    pub scanned_at: Option<String>,
    pub cleanup: SnapshotCleanup,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CleanupOverview {
    pub drives: Vec<DriveCleanup>,
    /// Rule files that could not be loaded.
    pub rule_issues: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CandidateView {
    #[ts(type = "number")]
    pub snapshot_id: i64,
    #[ts(type = "number")]
    pub match_id: i64,
    pub root: String,
    pub rule_id: String,
    pub title: String,
    pub category: String,
    pub explanation: String,
    pub risk: Risk,
    pub kind: RuleKind,
    pub path: String,
    pub is_dir: bool,
    /// Snapshot entry, when the candidate is a real file or folder (enables Open in Explorer).
    #[ts(type = "number | null")]
    pub entry_id: Option<i64>,
    pub virtual_key: Option<String>,
    #[ts(type = "number | null")]
    pub logical: Option<u64>,
    #[ts(type = "number | null")]
    pub allocated: Option<u64>,
    #[ts(type = "number | null")]
    pub file_count: Option<u64>,
    pub modified: Option<String>,
    pub note: Option<String>,
    pub instructions: Vec<String>,
    /// Smaller matches listed under this one (not counted again in totals).
    #[ts(type = "number")]
    pub nested: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CandidatePage {
    pub items: Vec<CandidateView>,
    #[ts(type = "number | null")]
    pub next_cursor: Option<u32>,
    /// Candidates matching the filters.
    #[ts(type = "number")]
    pub total: u64,
    #[ts(type = "number")]
    pub total_allocated: u64,
    /// Completed scans these candidates come from.
    pub scanned: u32,
    /// Scans whose rules are still being evaluated.
    pub evaluating: u32,
    /// Scans whose evaluation failed, with the reason of the first one.
    pub failed: u32,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CandidateQuery {
    pub snapshot_id: Option<i64>,
    pub drive: Option<String>,
    pub risk: Option<String>,
    pub category: Option<String>,
    pub kind: Option<String>,
    /// Matches nested under this match (needs `snapshot_id`).
    pub parent: Option<i64>,
    pub q: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<u32>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ExpandedPath {
    pub template: String,
    pub path: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RuleView {
    pub id: String,
    pub title: String,
    pub category: String,
    pub explanation: String,
    pub risk: Risk,
    pub kind: RuleKind,
    pub source: RuleSource,
    pub overrides_builtin: bool,
    pub disabled: bool,
    pub expanded_paths: Vec<ExpandedPath>,
    pub globs: Vec<String>,
    pub min_age_days: Option<u32>,
    pub applies_to: Option<AppliesTo>,
    pub conditions: Conditions,
    pub virtual_source: Option<VirtualSource>,
    pub instructions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RulesView {
    pub rules: Vec<RuleView>,
    pub issues: Vec<RuleIssue>,
    pub user_dir: UserRulesDir,
    pub user_files: u32,
}

#[derive(Debug, Deserialize, Default)]
pub struct EvaluateRequest {
    /// Snapshots to evaluate again; empty = the latest complete scan of every root.
    #[serde(default)]
    pub snapshot_ids: Vec<i64>,
}

pub fn is_drive_root(root: &str) -> bool {
    root.len() == 3 && root.ends_with(":\\")
}

pub struct CleanupService {
    pub db: Db,
    pub own: OwnFiles,
    pub jobs: JobRunner,
    pub rules_dir: PathBuf,
    pub env: Arc<dyn Env>,
    pub probe: Arc<dyn Probe>,
}

const RUN_COLS: &str = "snapshot_id, status, error, rules_fingerprint, finished_at, match_count, rule_errors";

fn run_row(r: &rusqlite::Row<'_>, fingerprint: &str) -> rusqlite::Result<CleanupRun> {
    let stored: Option<String> = r.get(3)?;
    let status = RunStatus::parse(&r.get::<_, String>(1)?);
    Ok(CleanupRun {
        snapshot_id: r.get(0)?,
        status,
        error: r.get(2)?,
        stale: status == RunStatus::Complete && stored.as_deref() != Some(fingerprint),
        finished_at: r.get(4)?,
        match_count: r.get::<_, i64>(5)?.max(0) as u64,
        rule_errors: r.get::<_, i64>(6)?.max(0) as u32,
    })
}

impl CleanupService {
    /// Reload the rule files (built-ins plus the user folder). Cheap; done per use.
    pub fn load_rules(&self) -> RuleSet {
        // The folder is created so the user can find it; this is our own data directory.
        if let Err(e) = self.own.create_dir_all(&self.rules_dir) {
            tracing::warn!(error = %e, "cannot create the rules folder");
        }
        rules::load_all(Some(&self.rules_dir))
    }

    /// Queue evaluation of completed snapshots. Returns the job id.
    pub fn submit(self: &Arc<Self>, snapshot_ids: &[i64]) -> Result<u64, ApiError> {
        let conn = self.db.connect().map_err(ApiError::internal)?;
        let mut roots = Vec::new();
        let mut ids = Vec::new();
        for id in snapshot_ids {
            let snap = snapshots::get(&conn, *id)
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::NotFound(format!("snapshot {id} not found")))?;
            if snap.status != SnapshotStatus::Complete {
                return Err(ApiError::Conflict(format!(
                    "snapshot {id} is {}; only complete scans are evaluated",
                    snap.status.as_str()
                )));
            }
            if ids.contains(id) {
                continue;
            }
            ids.push(*id);
            roots.push(snap.root);
        }
        if ids.is_empty() {
            return Err(ApiError::Conflict("there is no completed scan to evaluate yet".into()));
        }
        let now = now_rfc3339();
        for id in &ids {
            conn.execute(
                "INSERT INTO cleanup_runs (snapshot_id, status, started_at, error) VALUES (?1, 'queued', ?2, NULL)
                 ON CONFLICT(snapshot_id) DO UPDATE SET status = 'queued', started_at = ?2, error = NULL",
                params![id, now],
            )
            .map_err(ApiError::internal)?;
        }
        let job = CleanupJob { svc: self.clone(), ids: ids.clone(), roots };
        let job_id = self.jobs.submit(Box::new(job)).map_err(ApiError::from)?;
        for id in &ids {
            conn.execute("UPDATE cleanup_runs SET job_id = ?2 WHERE snapshot_id = ?1", params![id, job_id as i64])
                .map_err(ApiError::internal)?;
        }
        Ok(job_id)
    }

    /// Re-run for the given snapshots, or for the latest complete scan of every root.
    pub fn rerun(self: &Arc<Self>, requested: &[i64]) -> Result<u64, ApiError> {
        let ids: Vec<i64> = if requested.is_empty() {
            let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
            snapshots::latest(&conn).map_err(ApiError::internal)?.into_iter().map(|s| s.id).collect()
        } else {
            requested.to_vec()
        };
        self.submit(&ids)
    }

    /// Startup: evaluations that were queued or running when the app stopped failed.
    pub fn reconcile(db: &Db) -> Result<(), String> {
        let conn = db.connect().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE cleanup_runs SET status = 'failed', error = 'interrupted' WHERE status IN ('queued', 'running')",
            [],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn set_failed(&self, id: i64, message: &str) {
        if let Ok(conn) = self.db.connect() {
            let _ = conn.execute(
                "UPDATE cleanup_runs SET status = 'failed', error = ?2, finished_at = ?3 WHERE snapshot_id = ?1",
                params![id, message, now_rfc3339()],
            );
        }
    }

    /// Evaluate one snapshot and record the result. `progress(done, total, title)`
    /// returns false to cancel.
    fn evaluate_snapshot(
        &self,
        id: i64,
        set: &RuleSet,
        progress: &mut dyn FnMut(usize, usize, &str) -> bool,
    ) -> Result<(), EvalError> {
        let mut cat = self.db.connect().map_err(EvalError::Sqlite)?;
        let snap = snapshots::get(&cat, id)?.ok_or_else(|| EvalError::Snapshot(format!("snapshot {id} not found")))?;
        if snap.status != SnapshotStatus::Complete {
            return Err(EvalError::Snapshot(format!("snapshot {id} is not complete")));
        }
        cat.execute(
            "UPDATE cleanup_runs SET status = 'running', started_at = ?2, error = NULL WHERE snapshot_id = ?1",
            params![id, now_rfc3339()],
        )?;
        // The snapshot database belongs to us: open it for writing, but only inside the data directory.
        let path =
            self.own.check(&self.own.root().join(&snap.db_file)).map_err(|e| EvalError::Snapshot(e.to_string()))?;
        let mut conn =
            Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        let rules: Vec<&LoadedRule> = set.active().collect();
        let opts = EvalOptions {
            env: self.env.as_ref(),
            probe: self.probe.as_ref(),
            now: unix_now(),
            use_atime: snap.time_basis == TimeBasis::Atime,
        };
        let summary = evaluate(&mut conn, &rules, &opts, progress)?;
        // Release the snapshot file before the run is marked complete (it may be deleted right away).
        drop(conn);

        let tx = cat.transaction()?;
        tx.execute("DELETE FROM cleanup_savings WHERE snapshot_id = ?1", [id])?;
        for s in &summary.savings {
            tx.execute(
                "INSERT INTO cleanup_savings (snapshot_id, risk, kind, allocated, logical, matches, unknown) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, s.risk.as_str(), s.kind.as_str(), s.allocated as i64, s.logical as i64, s.matches as i64, s.unknown as i64],
            )?;
        }
        tx.execute(
            "UPDATE cleanup_runs SET status = 'complete', rules_fingerprint = ?2, finished_at = ?3, error = NULL,
               match_count = ?4, rule_errors = ?5 WHERE snapshot_id = ?1",
            params![
                id,
                set.fingerprint(),
                now_rfc3339(),
                summary.counted as i64,
                (summary.rule_failures.len() + set.issues.len()) as i64
            ],
        )?;
        tx.commit()?;
        for f in &summary.rule_failures {
            tracing::warn!(snapshot = id, rule = %f.rule_id, message = %f.message, "rule failed");
        }
        Ok(())
    }

    // -- queries ----------------------------------------------------------

    fn savings_for(&self, conn: &Connection, id: i64) -> rusqlite::Result<Vec<Saving>> {
        let mut stmt = conn.prepare(
            "SELECT risk, kind, allocated, logical, matches, unknown FROM cleanup_savings WHERE snapshot_id = ?1",
        )?;
        let rows = stmt.query_map([id], |r| {
            let risk: String = r.get(0)?;
            let kind: String = r.get(1)?;
            Ok((risk, kind, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?))
        })?;
        let mut out: Vec<Saving> = rows
            .filter_map(Result::ok)
            .filter_map(|(risk, kind, a, l, m, u)| {
                Some(Saving {
                    risk: Risk::parse(&risk)?,
                    kind: RuleKind::parse(&kind)?,
                    allocated: a.max(0) as u64,
                    logical: l.max(0) as u64,
                    matches: m.max(0) as u64,
                    unknown: u.max(0) as u64,
                })
            })
            .collect();
        out.sort_by_key(|s| (s.risk, s.kind.as_str()));
        Ok(out)
    }

    fn snapshot_cleanup_with(
        &self,
        conn: &Connection,
        id: i64,
        fingerprint: &str,
    ) -> Result<SnapshotCleanup, ApiError> {
        let run = conn
            .query_row(&format!("SELECT {RUN_COLS} FROM cleanup_runs WHERE snapshot_id = ?1"), [id], |r| {
                run_row(r, fingerprint)
            })
            .ok();
        let savings = self.savings_for(conn, id).map_err(ApiError::internal)?;
        Ok(SnapshotCleanup { run, savings })
    }

    pub fn snapshot_cleanup(&self, id: i64) -> Result<SnapshotCleanup, ApiError> {
        let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
        snapshots::get(&conn, id)
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::NotFound(format!("snapshot {id} not found")))?;
        let fp = self.load_rules().fingerprint();
        self.snapshot_cleanup_with(&conn, id, &fp)
    }

    pub fn overview(&self) -> Result<CleanupOverview, ApiError> {
        let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
        let set = self.load_rules();
        let fp = set.fingerprint();
        let mut drives = Vec::new();
        for s in snapshots::latest(&conn).map_err(ApiError::internal)? {
            if !is_drive_root(&s.root) {
                continue;
            }
            drives.push(DriveCleanup {
                snapshot_id: s.id,
                root: s.root.clone(),
                volume_id: s.volume_id.clone(),
                scanned_at: s.finished_at.clone(),
                cleanup: self.snapshot_cleanup_with(&conn, s.id, &fp)?,
            });
        }
        Ok(CleanupOverview { drives, rule_issues: set.issues.len() as u32 })
    }

    pub fn rules_view(&self) -> RulesView {
        let set = self.load_rules();
        let rules = set
            .rules
            .iter()
            .map(|r| RuleView {
                id: r.def.id.clone(),
                title: r.def.title.clone(),
                category: r.def.category.clone(),
                explanation: r.def.explanation.clone(),
                risk: r.def.risk,
                kind: r.def.kind,
                source: r.source.clone(),
                overrides_builtin: r.overrides_builtin,
                disabled: r.disabled,
                expanded_paths: r
                    .def
                    .paths
                    .iter()
                    .map(|t| match expand(t, self.env.as_ref()) {
                        Ok(p) => ExpandedPath { template: t.clone(), path: Some(p), error: None },
                        Err(e) => ExpandedPath { template: t.clone(), path: None, error: Some(e) },
                    })
                    .collect(),
                globs: r.def.globs.clone(),
                min_age_days: r.def.min_age_days,
                applies_to: r.def.applies_to,
                conditions: r.def.conditions.clone(),
                virtual_source: r.def.virtual_source,
                instructions: r.def.instructions.clone(),
            })
            .collect();
        RulesView {
            rules,
            issues: set.issues.clone(),
            user_dir: UserRulesDir {
                path: self.rules_dir.to_string_lossy().into_owned(),
                exists: self.rules_dir.is_dir(),
            },
            user_files: set.user_files as u32,
        }
    }

    /// Candidates across the latest complete scan of each drive (or one snapshot),
    /// ranked by bytes weighted by risk.
    pub fn candidates(&self, q: &CandidateQuery) -> Result<CandidatePage, ApiError> {
        let cat = self.db.connect_read_only().map_err(ApiError::internal)?;
        let limit = q.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
        let cursor = q.cursor.unwrap_or(0);
        for (name, value, allowed) in [
            ("risk", &q.risk, &["Safe", "Review", "Expert"][..]),
            ("kind", &q.kind, &["manual-cleanup", "system-setting"][..]),
        ] {
            if let Some(v) = value {
                if !allowed.contains(&v.as_str()) {
                    return Err(ApiError::BadRequest(format!("unknown {name} {v}")));
                }
            }
        }
        if q.parent.is_some() && q.snapshot_id.is_none() {
            return Err(ApiError::BadRequest("`parent` needs `snapshot_id`".into()));
        }

        let targets: Vec<Snapshot> = match q.snapshot_id {
            Some(id) => vec![snapshots::get(&cat, id)
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::NotFound(format!("snapshot {id} not found")))?],
            None => snapshots::latest(&cat)
                .map_err(ApiError::internal)?
                .into_iter()
                .filter(|s| is_drive_root(&s.root) && q.drive.as_ref().is_none_or(|d| s.volume_id.as_ref() == Some(d)))
                .collect(),
        };

        let mut page = CandidatePage {
            items: Vec::new(),
            next_cursor: None,
            total: 0,
            total_allocated: 0,
            scanned: 0,
            evaluating: 0,
            failed: 0,
            error: None,
        };
        let mut ranked: Vec<(f64, CandidateView)> = Vec::new();
        for snap in &targets {
            if snap.status != SnapshotStatus::Complete {
                continue;
            }
            page.scanned += 1;
            let status: Option<(String, Option<String>)> = cat
                .query_row("SELECT status, error FROM cleanup_runs WHERE snapshot_id = ?1", [snap.id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .ok();
            match status.as_ref().map(|(s, _)| RunStatus::parse(s)) {
                Some(RunStatus::Complete) => {}
                Some(RunStatus::Queued | RunStatus::Running) => {
                    page.evaluating += 1;
                    continue;
                }
                Some(RunStatus::Failed) => {
                    page.failed += 1;
                    page.error.get_or_insert_with(|| {
                        status.as_ref().and_then(|(_, e)| e.clone()).unwrap_or_else(|| "evaluation failed".into())
                    });
                    continue;
                }
                None => continue,
            }
            let conn = snapshots::open_snapshot(&self.own, snap)?;
            let (rows, total, bytes) =
                snapshot_candidates(&conn, snap, q, cursor + limit + 1).map_err(ApiError::internal)?;
            page.total += total;
            page.total_allocated += bytes;
            ranked.extend(rows);
        }
        ranked.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then(b.1.allocated.cmp(&a.1.allocated))
                .then(a.1.snapshot_id.cmp(&b.1.snapshot_id))
                .then(a.1.match_id.cmp(&b.1.match_id))
        });
        let more = ranked.len() > (cursor + limit) as usize;
        page.items = ranked.into_iter().skip(cursor as usize).take(limit as usize).map(|(_, c)| c).collect();
        page.next_cursor = more.then_some(cursor + limit);
        Ok(page)
    }
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Rows (with their ranking weight) from one snapshot, plus the filtered count and bytes.
type SnapshotRows = (Vec<(f64, CandidateView)>, u64, u64);

fn snapshot_candidates(
    conn: &Connection,
    snap: &Snapshot,
    q: &CandidateQuery,
    take: u32,
) -> rusqlite::Result<SnapshotRows> {
    let needle = q.q.as_ref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
    let filter = "WHERE (?1 IS NULL OR m.parent_match = ?1) AND (?1 IS NOT NULL OR m.counted = 1)
         AND (?2 IS NULL OR m.risk = ?2) AND (?3 IS NULL OR coalesce(d.category, '') = ?3)
         AND (?4 IS NULL OR m.kind = ?4) AND (?5 IS NULL OR instr(lower(m.path), ?5) > 0)";
    let fargs = params![q.parent, q.risk, q.category, q.kind, needle];
    let (total, bytes): (i64, i64) = conn.query_row(
        &format!("SELECT count(*), coalesce(sum(m.allocated), 0) FROM rule_matches m LEFT JOIN rule_defs d ON d.rule_id = m.rule_id {filter}"),
        fargs,
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let sql = format!(
        "SELECT m.id, m.rule_id, coalesce(d.title, m.rule_id), coalesce(d.category, m.category), coalesce(d.explanation, ''),
                m.risk, m.kind, m.path, m.is_dir, m.entry_id, m.virtual_key, m.logical, m.allocated, m.file_count,
                m.modified, m.note, coalesce(d.instructions, '[]'),
                (SELECT count(*) FROM rule_matches c WHERE c.parent_match = m.id),
                coalesce(m.allocated, 0) * CASE m.risk WHEN 'Safe' THEN 1.0 WHEN 'Review' THEN 0.5 ELSE 0.2 END
         FROM rule_matches m LEFT JOIN rule_defs d ON d.rule_id = m.rule_id {filter}
         ORDER BY 19 DESC, m.allocated DESC, m.id LIMIT ?6"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![q.parent, q.risk, q.category, q.kind, needle, take], |r| {
        let risk: String = r.get(5)?;
        let kind: String = r.get(6)?;
        let instructions: String = r.get(16)?;
        let opt =
            |i: usize| -> rusqlite::Result<Option<u64>> { Ok(r.get::<_, Option<i64>>(i)?.map(|v| v.max(0) as u64)) };
        Ok((
            r.get::<_, f64>(18)?,
            CandidateView {
                snapshot_id: snap.id,
                match_id: r.get(0)?,
                root: snap.root.clone(),
                rule_id: r.get(1)?,
                title: r.get(2)?,
                category: r.get(3)?,
                explanation: r.get(4)?,
                risk: Risk::parse(&risk).unwrap_or(Risk::Review),
                kind: RuleKind::parse(&kind).unwrap_or(RuleKind::ManualCleanup),
                path: r.get(7)?,
                is_dir: r.get::<_, i64>(8)? != 0,
                entry_id: r.get(9)?,
                virtual_key: r.get(10)?,
                logical: opt(11)?,
                allocated: opt(12)?,
                file_count: opt(13)?,
                modified: snapshots::rfc3339(r.get(14)?),
                note: r.get(15)?,
                instructions: serde_json::from_str(&instructions).unwrap_or_default(),
                nested: r.get::<_, i64>(17)?.max(0) as u64,
            },
        ))
    })?;
    Ok((rows.collect::<Result<_, _>>()?, total.max(0) as u64, bytes.max(0) as u64))
}

/// Background job: evaluate the rules over one or more snapshots.
pub struct CleanupJob {
    svc: Arc<CleanupService>,
    ids: Vec<i64>,
    roots: Vec<String>,
}

impl Job for CleanupJob {
    fn kind(&self) -> &'static str {
        "cleanup"
    }

    fn params(&self) -> serde_json::Value {
        serde_json::json!({ "snapshots": self.ids })
    }

    /// Waits for a scan of the same root to finish first.
    fn locks(&self) -> Vec<PathBuf> {
        self.roots.iter().map(PathBuf::from).collect()
    }

    fn run(&mut self, ctx: &JobContext) -> Result<(), JobError> {
        let set = self.svc.load_rules();
        let per = set.active().count() + 1;
        let total = (self.ids.len() * per) as u64;
        let mut errors = Vec::new();
        for (n, id) in self.ids.iter().enumerate() {
            if ctx.is_cancelled() {
                self.svc.set_failed(*id, "cancelled");
                continue;
            }
            let base = (n * per) as u64;
            let root = &self.roots[n];
            let result = self.svc.evaluate_snapshot(*id, &set, &mut |done, _, title| {
                ctx.progress(base + done as u64, Some(total), "rules", Some(&format!("Checking {root}: {title}")));
                !ctx.is_cancelled()
            });
            match result {
                Ok(()) => {}
                Err(EvalError::Cancelled) => self.svc.set_failed(*id, "cancelled"),
                Err(e) => {
                    tracing::error!(snapshot = id, error = %e, "cleanup evaluation failed");
                    self.svc.set_failed(*id, &e.to_string());
                    errors.push(format!("{root}: {e}"));
                }
            }
        }
        if ctx.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        if errors.is_empty() {
            ctx.progress(total, Some(total), "rules", Some("Suggestions ready"));
            Ok(())
        } else {
            Err(JobError::Failed(errors.join("; ")))
        }
    }
}
