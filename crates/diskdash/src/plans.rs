//! Cleanup plans: checklists of suggestions the user carries out by hand,
//! with manual instructions, status tracking, export, and verification against
//! a later manual rescan.
//!
//! This module never touches the user's files. Its only writes are its own
//! catalog rows. Instructions are text; nothing here runs a command.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use diskdash_analysis::plans::{
    classify, file_name, lookup_alloc, to_csv, to_txt, within_root, ExportItem, TemplateCtx, Templates,
};
use diskdash_analysis::rules::Env;
use diskdash_store::{Db, OwnFiles};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ApiError;
use crate::jobs::{Job, JobContext, JobError, JobRunner};
use crate::settings;
use crate::snapshots::{self, SnapshotStatus};
use crate::time::now_rfc3339;

const MAX_NAME: usize = 80;
const MAX_NOTE: usize = 500;

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct Bucket {
    #[ts(type = "number")]
    pub count: u64,
    #[ts(type = "number")]
    pub logical: u64,
    #[ts(type = "number")]
    pub allocated: u64,
}

impl Bucket {
    fn add(&mut self, logical: Option<u64>, allocated: Option<u64>) {
        self.count += 1;
        self.logical += logical.unwrap_or(0);
        self.allocated += allocated.unwrap_or(0);
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DriveBucket {
    pub drive: String,
    #[ts(type = "number")]
    pub count: u64,
    #[ts(type = "number")]
    pub logical: u64,
    #[ts(type = "number")]
    pub allocated: u64,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct PlanTotals {
    pub overall: Bucket,
    pub todo: Bucket,
    pub done: Bucket,
    pub skipped: Bucket,
    pub by_drive: Vec<DriveBucket>,
    /// Items whose size could not be measured (counted, not summed).
    #[ts(type = "number")]
    pub unknown_size: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PlanSummary {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub totals: PlanTotals,
    /// Items now inside a protected or excluded folder (left out of the totals).
    #[ts(type = "number")]
    pub blocked: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct VerifyView {
    /// removed | present | partial | grown | moved | unverified
    pub status: String,
    #[ts(type = "number | null")]
    pub freed: Option<i64>,
    pub verified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PlanItemView {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number | null")]
    pub snapshot_id: Option<i64>,
    #[ts(type = "number | null")]
    pub entry_id: Option<i64>,
    pub path: String,
    pub drive: String,
    pub is_dir: bool,
    /// manual-cleanup | system-setting | duplicate | stale | manual-move
    pub kind: String,
    /// cleanup-rules | duplicates | stale | tiering
    pub source: String,
    pub title: String,
    pub category: Option<String>,
    pub risk: String,
    #[ts(type = "number | null")]
    pub logical: Option<u64>,
    #[ts(type = "number | null")]
    pub allocated: Option<u64>,
    pub destination: Option<String>,
    pub instructions: Vec<String>,
    pub status: String,
    pub note: Option<String>,
    pub added_at: String,
    /// Set when the item is now in a protected or excluded folder.
    pub blocked: Option<String>,
    pub verify: VerifyView,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct VerificationView {
    pub root: String,
    #[ts(type = "number")]
    pub snapshot_id: i64,
    pub verified_at: String,
    #[ts(type = "number")]
    pub items: u64,
    /// Bytes freed by the planned items on this root.
    #[ts(type = "number")]
    pub freed: i64,
    /// Change in scanned size between the baseline scan and this one (includes unrelated activity).
    #[ts(type = "number | null")]
    pub scanned_change: Option<i64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PlanView {
    pub plan: PlanSummary,
    pub items: Vec<PlanItemView>,
    pub verifications: Vec<VerificationView>,
    /// Total verified freed bytes across the plan.
    #[ts(type = "number")]
    pub freed_total: i64,
    /// When the oldest baseline scan finished: the plan's numbers are as old as this.
    pub baseline_as_of: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddItemRequest {
    /// cleanup | duplicate | stale | tiering
    pub source: String,
    pub snapshot_id: Option<i64>,
    /// cleanup: the candidate's match id.
    pub match_id: Option<i64>,
    /// stale / tiering: the snapshot entry.
    pub entry_id: Option<i64>,
    /// duplicate: the copy to remove.
    pub member_id: Option<i64>,
    /// tiering: where the user plans to move it.
    pub destination: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AddItemResult {
    pub added: bool,
    /// `already-planned` when the path is already in the plan.
    pub reason: Option<String>,
    pub item: PlanItemView,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ItemPatch {
    pub status: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlanPatch {
    pub name: String,
}

pub struct PlanService {
    pub db: Db,
    pub own: OwnFiles,
    pub jobs: JobRunner,
    pub env: Arc<dyn Env>,
    pub templates: Templates,
}

struct NewItem {
    snapshot_id: Option<i64>,
    entry_id: Option<i64>,
    path: String,
    is_dir: bool,
    kind: String,
    source: String,
    source_ref: Option<String>,
    title: String,
    category: Option<String>,
    risk: String,
    logical: Option<u64>,
    allocated: Option<u64>,
    mtime: Option<i64>,
    file_id: Option<String>,
    destination: Option<String>,
    instructions: Vec<String>,
}

fn drive_of(path: &str) -> String {
    path.get(..2).filter(|d| d.ends_with(':')).map_or_else(|| "?".into(), str::to_uppercase)
}

fn validation(field: &str, msg: &str) -> ApiError {
    ApiError::Validation(BTreeMap::from([(field.to_string(), msg.to_string())]))
}

const ITEM_COLS: &str =
    "id, snapshot_id, entry_id, path, is_dir, kind, source, title, category, risk, baseline_logical,
    baseline_allocated, destination, instructions, status, note, added_at, verify_status, verify_freed, verified_at";

fn item_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<PlanItemView> {
    let path: String = r.get(3)?;
    let opt = |i: usize| -> rusqlite::Result<Option<u64>> { Ok(r.get::<_, Option<i64>>(i)?.map(|v| v.max(0) as u64)) };
    let instructions: String = r.get(13)?;
    Ok(PlanItemView {
        id: r.get(0)?,
        snapshot_id: r.get(1)?,
        entry_id: r.get(2)?,
        drive: drive_of(&path),
        path,
        is_dir: r.get::<_, i64>(4)? != 0,
        kind: r.get(5)?,
        source: r.get(6)?,
        title: r.get(7)?,
        category: r.get(8)?,
        risk: r.get(9)?,
        logical: opt(10)?,
        allocated: opt(11)?,
        destination: r.get(12)?,
        instructions: serde_json::from_str(&instructions).unwrap_or_default(),
        status: r.get(14)?,
        note: r.get(15)?,
        added_at: r.get(16)?,
        blocked: None,
        verify: VerifyView { status: r.get(17)?, freed: r.get(18)?, verified_at: r.get(19)? },
    })
}

impl PlanService {
    fn settings(&self, conn: &Connection) -> settings::Settings {
        settings::load(conn).unwrap_or_default()
    }

    fn touch(conn: &Connection, plan: i64) {
        let _ = conn.execute("UPDATE plans SET updated_at = ?2 WHERE id = ?1", params![plan, now_rfc3339()]);
    }

    fn plan_exists(conn: &Connection, id: i64) -> Result<(String, String, String), ApiError> {
        conn.query_row("SELECT name, created_at, updated_at FROM plans WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::NotFound(format!("plan {id} not found")))
    }

    fn clean_name(name: &str) -> Result<String, ApiError> {
        let n = name.trim();
        if n.is_empty() || n.chars().count() > MAX_NAME {
            return Err(validation("name", &format!("give the plan a name of 1 to {MAX_NAME} characters")));
        }
        Ok(n.to_string())
    }

    pub fn create(&self, name: &str) -> Result<PlanSummary, ApiError> {
        let name = Self::clean_name(name)?;
        let conn = self.db.connect().map_err(ApiError::internal)?;
        let now = now_rfc3339();
        conn.execute("INSERT INTO plans (name, created_at, updated_at) VALUES (?1, ?2, ?2)", params![name, now])
            .map_err(ApiError::internal)?;
        self.summary(&conn, conn.last_insert_rowid())
    }

    pub fn rename(&self, id: i64, name: &str) -> Result<PlanSummary, ApiError> {
        let name = Self::clean_name(name)?;
        let conn = self.db.connect().map_err(ApiError::internal)?;
        Self::plan_exists(&conn, id)?;
        conn.execute("UPDATE plans SET name = ?2, updated_at = ?3 WHERE id = ?1", params![id, name, now_rfc3339()])
            .map_err(ApiError::internal)?;
        self.summary(&conn, id)
    }

    /// Delete the plan (our own rows only).
    pub fn delete(&self, id: i64) -> Result<(), ApiError> {
        let conn = self.db.connect().map_err(ApiError::internal)?;
        Self::plan_exists(&conn, id)?;
        conn.execute("DELETE FROM plans WHERE id = ?1", [id]).map_err(ApiError::internal)?;
        Ok(())
    }

    fn items(&self, conn: &Connection, plan: i64) -> Result<Vec<PlanItemView>, ApiError> {
        let protected = crate::protection::build(self.env.as_ref(), &self.settings(conn));
        let mut stmt = conn
            .prepare(&format!("SELECT {ITEM_COLS} FROM plan_items WHERE plan_id = ?1 ORDER BY id"))
            .map_err(ApiError::internal)?;
        let rows = stmt.query_map([plan], item_row).map_err(ApiError::internal)?;
        let mut out = Vec::new();
        for r in rows {
            let mut i = r.map_err(ApiError::internal)?;
            // Protection is re-applied on every read: a folder excluded later flags its items.
            if i.kind != "system-setting" {
                i.blocked = protected.check(&i.path);
            }
            out.push(i);
        }
        Ok(out)
    }

    fn totals(items: &[PlanItemView]) -> (PlanTotals, u64) {
        let mut t = PlanTotals::default();
        let mut blocked = 0;
        let mut drives: BTreeMap<String, DriveBucket> = BTreeMap::new();
        for i in items {
            if i.blocked.is_some() {
                blocked += 1;
                continue;
            }
            t.overall.add(i.logical, i.allocated);
            match i.status.as_str() {
                "done" => t.done.add(i.logical, i.allocated),
                "skipped" => t.skipped.add(i.logical, i.allocated),
                _ => t.todo.add(i.logical, i.allocated),
            }
            if i.allocated.is_none() {
                t.unknown_size += 1;
            }
            let d = drives.entry(i.drive.clone()).or_insert(DriveBucket {
                drive: i.drive.clone(),
                count: 0,
                logical: 0,
                allocated: 0,
            });
            d.count += 1;
            d.logical += i.logical.unwrap_or(0);
            d.allocated += i.allocated.unwrap_or(0);
        }
        t.by_drive = drives.into_values().collect();
        (t, blocked)
    }

    fn summary(&self, conn: &Connection, id: i64) -> Result<PlanSummary, ApiError> {
        let (name, created_at, updated_at) = Self::plan_exists(conn, id)?;
        let items = self.items(conn, id)?;
        let (totals, blocked) = Self::totals(&items);
        Ok(PlanSummary { id, name, created_at, updated_at, totals, blocked })
    }

    pub fn list(&self) -> Result<Vec<PlanSummary>, ApiError> {
        let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
        let ids: Vec<i64> = conn
            .prepare("SELECT id FROM plans ORDER BY updated_at DESC, id DESC")
            .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
            .map_err(ApiError::internal)?;
        ids.into_iter().map(|id| self.summary(&conn, id)).collect()
    }

    pub fn get(&self, id: i64) -> Result<PlanView, ApiError> {
        let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
        let plan = self.summary(&conn, id)?;
        let items = self.items(&conn, id)?;
        let verifications: Vec<VerificationView> = conn
            .prepare("SELECT root, snapshot_id, verified_at, items, freed, scanned_change FROM plan_verifications WHERE plan_id = ?1 ORDER BY root")
            .and_then(|mut s| {
                s.query_map([id], |r| {
                    Ok(VerificationView {
                        root: r.get(0)?,
                        snapshot_id: r.get(1)?,
                        verified_at: r.get(2)?,
                        items: r.get::<_, i64>(3)?.max(0) as u64,
                        freed: r.get(4)?,
                        scanned_change: r.get(5)?,
                    })
                })?
                .collect()
            })
            .map_err(ApiError::internal)?;
        let freed_total = items.iter().filter(|i| i.blocked.is_none()).filter_map(|i| i.verify.freed).sum();
        let baseline_as_of: Option<String> = conn
            .query_row(
                "SELECT min(s.finished_at) FROM snapshots s WHERE s.id IN (SELECT DISTINCT snapshot_id FROM plan_items WHERE plan_id = ?1)",
                [id],
                |r| r.get(0),
            )
            .ok()
            .flatten();
        Ok(PlanView { plan, items, verifications, freed_total, baseline_as_of })
    }

    // -- adding items -------------------------------------------------------

    fn resolve(&self, conn: &Connection, req: &AddItemRequest) -> Result<NewItem, ApiError> {
        let need = |v: Option<i64>, what: &str| v.ok_or_else(|| ApiError::BadRequest(format!("{what} is required")));
        match req.source.as_str() {
            "cleanup" => {
                let (sid, mid) = (need(req.snapshot_id, "snapshot_id")?, need(req.match_id, "match_id")?);
                let snap = snapshots::get(conn, sid)
                    .map_err(ApiError::internal)?
                    .ok_or_else(|| ApiError::NotFound(format!("snapshot {sid} not found")))?;
                let sconn = snapshots::open_snapshot(&self.own, &snap)?;
                #[allow(clippy::type_complexity)]
                let row: Option<(String, i64, Option<i64>, String, String, String, Option<i64>, Option<i64>, Option<i64>, String, String, String)> = sconn
                    .query_row(
                        "SELECT m.path, m.is_dir, m.entry_id, m.risk, m.kind, m.category, m.logical, m.allocated, m.modified,
                                coalesce(d.title, m.rule_id), coalesce(d.instructions, '[]'), m.rule_id
                         FROM rule_matches m LEFT JOIN rule_defs d ON d.rule_id = m.rule_id WHERE m.id = ?1",
                        [mid],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?)),
                    )
                    .optional()
                    .map_err(ApiError::internal)?;
                let (path, is_dir, entry_id, risk, kind, category, logical, allocated, mtime, title, instr, rule_id) =
                    row.ok_or_else(|| ApiError::NotFound(format!("suggestion {mid} not found in snapshot {sid}")))?;
                let mut instructions: Vec<String> = serde_json::from_str(&instr).unwrap_or_default();
                if instructions.is_empty() {
                    instructions = self
                        .templates
                        .render("explorer-delete", &TemplateCtx { path: path.clone(), destination: None });
                }
                let file_id = entry_id.and_then(|e| {
                    sconn
                        .query_row("SELECT file_id FROM entries WHERE id = ?1", [e], |r| r.get::<_, Option<String>>(0))
                        .ok()
                        .flatten()
                });
                Ok(NewItem {
                    snapshot_id: Some(sid),
                    entry_id,
                    path,
                    is_dir: is_dir != 0,
                    kind,
                    source: "cleanup-rules".into(),
                    source_ref: Some(rule_id),
                    title,
                    category: Some(category),
                    risk,
                    logical: logical.map(|v| v.max(0) as u64),
                    allocated: allocated.map(|v| v.max(0) as u64),
                    mtime,
                    file_id,
                    destination: None,
                    instructions,
                })
            }
            "duplicate" => {
                let mid = need(req.member_id, "member_id")?;
                let row: Option<(i64, i64, i64, String, i64, i64, i64)> = conn
                    .query_row(
                        "SELECT m.snapshot_id, m.entry_id, m.group_id, m.path, m.mtime, g.size, g.alloc
                         FROM dup_members m JOIN dup_groups g ON g.id = m.group_id WHERE m.id = ?1",
                        [mid],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
                    )
                    .optional()
                    .map_err(ApiError::internal)?;
                let (sid, eid, gid, path, mtime, size, alloc) =
                    row.ok_or_else(|| ApiError::NotFound(format!("duplicate copy {mid} not found")))?;
                let name = path.rsplit('\\').next().unwrap_or(&path).to_string();
                Ok(NewItem {
                    snapshot_id: Some(sid),
                    entry_id: Some(eid),
                    instructions: self
                        .templates
                        .render("duplicate-remove", &TemplateCtx { path: path.clone(), destination: None }),
                    path,
                    is_dir: false,
                    kind: "duplicate".into(),
                    source: "duplicates".into(),
                    source_ref: Some(gid.to_string()),
                    title: format!("Duplicate copy of {name}"),
                    category: None,
                    risk: "Review".into(),
                    logical: Some(size.max(0) as u64),
                    allocated: Some(alloc.max(0) as u64),
                    mtime: Some(mtime),
                    file_id: None,
                    destination: None,
                })
            }
            "stale" | "tiering" => {
                let (sid, eid) = (need(req.snapshot_id, "snapshot_id")?, need(req.entry_id, "entry_id")?);
                let snap = snapshots::get(conn, sid)
                    .map_err(ApiError::internal)?
                    .ok_or_else(|| ApiError::NotFound(format!("snapshot {sid} not found")))?;
                let sconn = snapshots::open_snapshot(&self.own, &snap)?;
                let e = snapshots::entry_by_id(&sconn, eid)
                    .map_err(|_| ApiError::NotFound(format!("entry {eid} not found in snapshot {sid}")))?;
                let (mtime, file_id): (Option<i64>, Option<String>) = sconn
                    .query_row("SELECT coalesce(max_mtime, mtime), file_id FROM entries WHERE id = ?1", [eid], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })
                    .map_err(ApiError::internal)?;
                let is_dir = e.kind == snapshots::EntryKind::Dir;
                let (kind, source, key, destination, title) = if req.source == "tiering" {
                    let dest = req
                        .destination
                        .clone()
                        .filter(|d| !d.trim().is_empty())
                        .ok_or_else(|| ApiError::BadRequest("destination is required for a move".into()))?;
                    (
                        "manual-move",
                        "tiering",
                        if is_dir { "manual-move-dir" } else { "manual-move-file" },
                        Some(dest),
                        format!("Move {}", e.name),
                    )
                } else {
                    ("stale", "stale", "stale-review", None, format!("Not used for a long time: {}", e.name))
                };
                let instructions =
                    self.templates.render(key, &TemplateCtx { path: e.path.clone(), destination: destination.clone() });
                Ok(NewItem {
                    snapshot_id: Some(sid),
                    entry_id: Some(eid),
                    path: e.path,
                    is_dir,
                    kind: kind.into(),
                    source: source.into(),
                    source_ref: None,
                    title,
                    category: if is_dir { e.top_category } else { e.category },
                    risk: "Review".into(),
                    logical: Some(e.size),
                    allocated: Some(e.alloc),
                    mtime,
                    file_id,
                    destination,
                    instructions,
                })
            }
            other => Err(ApiError::BadRequest(format!("unknown source {other}"))),
        }
    }

    pub fn add_item(&self, plan: i64, req: &AddItemRequest) -> Result<AddItemResult, ApiError> {
        let conn = self.db.connect().map_err(ApiError::internal)?;
        Self::plan_exists(&conn, plan)?;
        let item = self.resolve(&conn, req)?;
        // Backend enforcement: protected and excluded paths are never planned, whatever the client sends.
        if item.kind != "system-setting" {
            if let Some(reason) = crate::protection::build(self.env.as_ref(), &self.settings(&conn)).check(&item.path) {
                return Err(validation("path", &reason));
            }
        }
        let key = item.path.to_lowercase();
        let changed = conn
            .execute(
                "INSERT INTO plan_items (plan_id, snapshot_id, entry_id, path, path_key, is_dir, kind, source, source_ref, title, category, risk,
                    baseline_logical, baseline_allocated, baseline_mtime, baseline_file_id, destination, instructions, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
                 ON CONFLICT(plan_id, path_key) DO NOTHING",
                params![
                    plan,
                    item.snapshot_id,
                    item.entry_id,
                    item.path,
                    key,
                    item.is_dir,
                    item.kind,
                    item.source,
                    item.source_ref,
                    item.title,
                    item.category,
                    item.risk,
                    item.logical.map(|v| v as i64),
                    item.allocated.map(|v| v as i64),
                    item.mtime,
                    item.file_id,
                    item.destination,
                    serde_json::to_string(&item.instructions).unwrap_or_else(|_| "[]".into()),
                    now_rfc3339(),
                ],
            )
            .map_err(ApiError::internal)?;
        let id: i64 = conn
            .query_row("SELECT id FROM plan_items WHERE plan_id = ?1 AND path_key = ?2", params![plan, key], |r| {
                r.get(0)
            })
            .map_err(ApiError::internal)?;
        Self::touch(&conn, plan);
        let view = conn
            .query_row(&format!("SELECT {ITEM_COLS} FROM plan_items WHERE id = ?1"), [id], item_row)
            .map_err(ApiError::internal)?;
        Ok(AddItemResult { added: changed > 0, reason: (changed == 0).then(|| "already-planned".into()), item: view })
    }

    pub fn patch_item(&self, plan: i64, item: i64, p: &ItemPatch) -> Result<PlanItemView, ApiError> {
        if let Some(s) = &p.status {
            if !["todo", "done", "skipped"].contains(&s.as_str()) {
                return Err(validation("status", "must be todo, done or skipped"));
            }
        }
        if p.note.as_ref().is_some_and(|n| n.chars().count() > MAX_NOTE) {
            return Err(validation("note", &format!("at most {MAX_NOTE} characters")));
        }
        let conn = self.db.connect().map_err(ApiError::internal)?;
        let n = conn
            .execute(
                "UPDATE plan_items SET status = coalesce(?3, status), note = CASE WHEN ?4 IS NULL THEN note ELSE nullif(?4, '') END
                 WHERE id = ?2 AND plan_id = ?1",
                params![plan, item, p.status, p.note],
            )
            .map_err(ApiError::internal)?;
        if n == 0 {
            return Err(ApiError::NotFound(format!("item {item} is not in plan {plan}")));
        }
        Self::touch(&conn, plan);
        conn.query_row(&format!("SELECT {ITEM_COLS} FROM plan_items WHERE id = ?1"), [item], item_row)
            .map_err(ApiError::internal)
    }

    pub fn remove_item(&self, plan: i64, item: i64) -> Result<(), ApiError> {
        let conn = self.db.connect().map_err(ApiError::internal)?;
        let n = conn
            .execute("DELETE FROM plan_items WHERE id = ?2 AND plan_id = ?1", params![plan, item])
            .map_err(ApiError::internal)?;
        if n == 0 {
            return Err(ApiError::NotFound(format!("item {item} is not in plan {plan}")));
        }
        Self::touch(&conn, plan);
        Ok(())
    }

    // -- export ---------------------------------------------------------------

    /// (file name, content type, body). Paths and instructions only.
    pub fn export(&self, id: i64, format: &str) -> Result<(String, &'static str, String), ApiError> {
        let csv = match format {
            "csv" => true,
            "txt" => false,
            other => return Err(ApiError::BadRequest(format!("unknown format {other}; use csv or txt"))),
        };
        let view = self.get(id)?;
        let items: Vec<ExportItem> = view
            .items
            .iter()
            .filter(|i| i.blocked.is_none())
            .map(|i| ExportItem {
                status: i.status.clone(),
                risk: i.risk.clone(),
                kind: i.kind.clone(),
                path: i.path.clone(),
                logical: i.logical,
                allocated: i.allocated,
                instructions: i.instructions.clone(),
                note: i.note.clone(),
            })
            .collect();
        let name = file_name(&view.plan.name, csv);
        Ok(if csv {
            // A BOM so spreadsheets read non-ASCII paths correctly.
            (name, "text/csv; charset=utf-8", format!("\u{FEFF}{}", to_csv(&items)))
        } else {
            (name, "text/plain; charset=utf-8", to_txt(&view.plan.name, &items))
        })
    }

    // -- verification -----------------------------------------------------------

    /// Queue verification against the given scans (or the latest scan of every root).
    pub fn submit_verify(self: &Arc<Self>, snapshot_ids: &[i64], plan: Option<i64>) -> Result<u64, ApiError> {
        let ids: Vec<i64> = if snapshot_ids.is_empty() {
            let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
            snapshots::latest(&conn).map_err(ApiError::internal)?.into_iter().map(|s| s.id).collect()
        } else {
            snapshot_ids.to_vec()
        };
        if let Some(p) = plan {
            let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
            Self::plan_exists(&conn, p)?;
        }
        if ids.is_empty() {
            return Err(ApiError::Conflict("there is no completed scan to verify against yet".into()));
        }
        self.jobs.submit(Box::new(VerifyJob { svc: self.clone(), ids, plan })).map_err(ApiError::from)
    }

    /// Whether any plan has items worth verifying (cheap check before queueing after a scan).
    pub fn has_items(&self) -> bool {
        self.db
            .connect_read_only()
            .ok()
            .and_then(|c| {
                c.query_row("SELECT EXISTS (SELECT 1 FROM plan_items WHERE status != 'skipped')", [], |r| {
                    r.get::<_, bool>(0)
                })
                .ok()
            })
            .unwrap_or(false)
    }

    /// Compare plan items with the given complete scans. Snapshot data only.
    pub fn verify(&self, snapshot_ids: &[i64], plan_filter: Option<i64>) -> Result<u64, String> {
        let mut cat = self.db.connect().map_err(|e| e.to_string())?;
        let mut scans = Vec::new();
        for id in snapshot_ids {
            let Some(s) = snapshots::get(&cat, *id).map_err(|e| e.to_string())? else { continue };
            if s.status != SnapshotStatus::Complete {
                continue;
            }
            if let Ok(c) = snapshots::open_snapshot(&self.own, &s) {
                scans.push((s, c));
            }
        }
        // Newest scan first, so each item is judged against the latest scan that covers it.
        scans.sort_by_key(|(s, _)| std::cmp::Reverse(s.id));

        #[allow(clippy::type_complexity)]
        let items: Vec<(i64, i64, String, Option<i64>, Option<i64>, Option<String>)> = cat
            .prepare(
                "SELECT id, plan_id, path, snapshot_id, baseline_allocated, destination FROM plan_items
                 WHERE status != 'skipped' AND (?1 IS NULL OR plan_id = ?1)",
            )
            .and_then(|mut s| {
                s.query_map([plan_filter], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
                    .collect()
            })
            .map_err(|e| e.to_string())?;

        // (plan, root) -> (snapshot id, baseline snapshots, items, freed)
        let mut agg: HashMap<(i64, String), RootAgg> = HashMap::new();
        let now = now_rfc3339();
        let tx = cat.transaction().map_err(|e| e.to_string())?;
        let mut verified = 0u64;
        for (id, plan, path, baseline_snap, baseline_alloc, destination) in items {
            let Some((snap, conn)) =
                scans.iter().find(|(s, _)| within_root(&s.root, &path) && baseline_snap.is_none_or(|b| s.id > b))
            else {
                continue;
            };
            let current = lookup_alloc(conn, &snap.root, &path).map_err(|e| e.to_string())?;
            let dest = match (&current, &destination) {
                (None, Some(d)) => scans
                    .iter()
                    .find(|(s, _)| within_root(&s.root, d))
                    .and_then(|(s, c)| lookup_alloc(c, &s.root, d).ok().flatten()),
                _ => None,
            };
            let v = classify(baseline_alloc.unwrap_or(0).max(0) as u64, current, dest);
            tx.execute(
                "UPDATE plan_items SET verify_status = ?2, verify_freed = ?3, verified_at = ?4, verified_snapshot_id = ?5 WHERE id = ?1",
                params![id, v.outcome.key(), v.freed, now, snap.id],
            )
            .map_err(|e| e.to_string())?;
            let e = agg.entry((plan, snap.root.clone())).or_insert((snap.id, Vec::new(), 0, 0));
            e.1.extend(baseline_snap);
            e.2 += 1;
            e.3 += v.freed;
            verified += 1;
        }
        for ((plan, root), (snap_id, baselines, n, freed)) in agg {
            let new_total: Option<i64> =
                tx.query_row("SELECT total_alloc FROM snapshots WHERE id = ?1", [snap_id], |r| r.get(0)).ok();
            let base = baselines.iter().min().copied();
            let base_total: Option<i64> = base
                .and_then(|b| tx.query_row("SELECT total_alloc FROM snapshots WHERE id = ?1", [b], |r| r.get(0)).ok());
            let change = match (base_total, new_total) {
                (Some(b), Some(n)) => Some(b - n),
                _ => None,
            };
            tx.execute(
                "INSERT INTO plan_verifications (plan_id, root, snapshot_id, baseline_snapshot_id, verified_at, items, freed, scanned_change)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(plan_id, root) DO UPDATE SET snapshot_id = excluded.snapshot_id, baseline_snapshot_id = excluded.baseline_snapshot_id,
                   verified_at = excluded.verified_at, items = excluded.items, freed = excluded.freed, scanned_change = excluded.scanned_change",
                params![plan, root, snap_id, base, now, n as i64, freed, change],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(verified)
    }
}

/// Per (plan, root): snapshot id, baseline snapshot ids, items checked, bytes freed.
type RootAgg = (i64, Vec<i64>, u64, i64);

/// Background job: verify plans against completed scans (snapshot data only).
pub struct VerifyJob {
    svc: Arc<PlanService>,
    ids: Vec<i64>,
    plan: Option<i64>,
}

impl Job for VerifyJob {
    fn kind(&self) -> &'static str {
        "verify-plans"
    }

    fn params(&self) -> serde_json::Value {
        serde_json::json!({ "snapshots": self.ids, "plan": self.plan })
    }

    fn locks(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn run(&mut self, ctx: &JobContext) -> Result<(), JobError> {
        ctx.progress(0, None, "items", Some("Comparing plans with the new scan"));
        let n = self.svc.verify(&self.ids, self.plan).map_err(JobError::Failed)?;
        ctx.progress(n, Some(n.max(1)), "items", Some("Plans verified"));
        Ok(())
    }
}
