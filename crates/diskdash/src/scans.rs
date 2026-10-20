//! Starting, scheduling and resuming scans. Scans are only ever started by the
//! user (no schedules, no background watchdog).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use diskdash_analysis::stale::{decide, Setting};
use diskdash_core::ntfs::import::{import, ImportError, ImportOptions, Stage};
use diskdash_core::ntfs::incremental::{quick_rescan, Changes};
use diskdash_core::ntfs::usn::{JournalState, StoredUsn};
use diskdash_core::paths;
use diskdash_core::scan::{self, ScanOptions, ScanProgress, ScanStatus, ScanSummary};
use diskdash_helper::proto::UsnOutcome;
use diskdash_inventory::model::{Drive, MediaClass};
use diskdash_store::{Db, OwnFiles};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cleanup::CleanupService;
use crate::error::ApiError;
use crate::helper::HelperLink;
use crate::inventory::InventoryService;
use crate::jobs::{Job, JobContext, JobError, JobRunner};
use crate::settings::{HardlinkDefault, Settings};
use crate::snapshots::{self, SnapshotStatus};
use crate::time::now_rfc3339;

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ScanRequest {
    /// Drive roots (`D:\`) and/or folders on fixed internal drives.
    pub roots: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct StartedSnapshot {
    #[ts(type = "number")]
    pub id: i64,
    pub root: String,
    /// `walk`, `mft` or `mft-quick`.
    pub method: String,
    /// Why the walk is used instead of the fast scan, or what a quick rescan did.
    pub note: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ScanModeInfo {
    pub root: String,
    pub volume_id: String,
    /// `mft` (fast scan) or `walk` (directory walk).
    pub mode: String,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ScanModes {
    pub helper: crate::helper::HelperStatus,
    pub drives: Vec<ScanModeInfo>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct QuickRequest {
    pub root: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ScanStarted {
    #[ts(type = "number")]
    pub job_id: u64,
    pub snapshots: Vec<StartedSnapshot>,
    /// Requested roots dropped because they lie inside another requested root.
    pub merged: Vec<String>,
}

/// One root to scan, fully planned.
#[derive(Debug, Clone)]
pub struct ScanItem {
    pub snapshot_id: i64,
    pub root: String,
    pub db_path: PathBuf,
    pub threads: usize,
    pub hardlinks: bool,
    /// For the ETA: bytes expected (volume used space, or the last scan's total).
    pub expected: Option<u64>,
    pub resume: bool,
    /// Physical-disk group: roots on one disk run one after another.
    pub group: String,
    pub mode: ItemMode,
    pub note: Option<String>,
}

/// How an item is scanned.
#[derive(Debug, Clone)]
pub enum ItemMode {
    Walk,
    /// Read the MFT through the elevated helper.
    Mft {
        volume_id: String,
    },
    /// Previous snapshot plus the USN changes the helper returned.
    Quick {
        volume_id: String,
        previous: PathBuf,
        changes: Changes,
        state: JournalState,
    },
}

pub struct ScanService {
    pub db: Db,
    pub own: OwnFiles,
    pub inventory: Arc<InventoryService>,
    pub jobs: JobRunner,
    pub elevated: bool,
    /// Evaluates the cleanup rules on every scan that completes.
    pub cleanup: Option<Arc<CleanupService>>,
    /// Verifies cleanup plans against every scan that completes.
    pub plans: Option<Arc<crate::plans::PlanService>>,
    /// Reads the Windows last-access setting (`fsutil`); a function so tests can fix the answer.
    pub last_access: fn() -> Option<Setting>,
    /// The elevated read-only helper (fast MFT scans, quick rescans).
    pub helper: Arc<dyn HelperLink>,
}

pub(crate) fn drive_for<'a>(drives: &'a [Drive], root: &str) -> Option<&'a Drive> {
    let letter = root.get(0..2).map(|s| s.to_ascii_uppercase());
    drives
        .iter()
        .find(|d| d.mount_paths.iter().any(|m| paths::is_within(root, m)))
        .or_else(|| drives.iter().find(|d| d.letters.iter().any(|l| Some(format!("{l}:")) == letter)))
}

fn is_drive_root(root: &str) -> bool {
    root.len() == 3 && root.ends_with(":\\")
}

fn hardlinks_for(settings: &Settings, d: &Drive) -> bool {
    if let Some(v) = settings.hardlink_overrides.get(&d.volume_path).or_else(|| settings.hardlink_overrides.get(&d.id))
    {
        return *v;
    }
    match settings.hardlink_detection {
        HardlinkDefault::SystemVolume => d.is_system_volume,
        HardlinkDefault::AllVolumes => true,
        HardlinkDefault::NoVolumes => false,
    }
}

fn group_and_threads(settings: &Settings, d: &Drive) -> (String, usize) {
    let group =
        d.physical_disks.first().map(|p| format!("disk-{}", p.number)).unwrap_or_else(|| format!("volume-{}", d.id));
    let threads = match d.media {
        MediaClass::SATA_SSD | MediaClass::NVME_SSD => settings.ssd_scan_concurrency,
        // HDD and unknown: never parallel on one spindle.
        _ => settings.hdd_scan_concurrency,
    };
    (group, threads.max(1) as usize)
}

/// Accepted roots with their drive, plus the roots merged into another one.
pub type ValidatedRoots = (Vec<(String, Drive)>, Vec<String>);

/// Validate and normalise requested roots; drop nested ones.
pub fn validate_roots(raw: &[String], drives: &[Drive]) -> Result<ValidatedRoots, ApiError> {
    let mut errors = BTreeMap::new();
    let mut ok: Vec<(String, Drive)> = Vec::new();
    if raw.is_empty() {
        errors.insert("roots".to_string(), "choose at least one drive or folder".to_string());
    }
    for (i, r) in raw.iter().enumerate() {
        let key = format!("roots[{i}]");
        let trimmed = r.trim();
        if trimmed.is_empty() {
            errors.insert(key, "empty path".into());
            continue;
        }
        let Ok(root) = paths::normalize_root(std::path::Path::new(trimmed)) else {
            errors.insert(key, "not a valid path".into());
            continue;
        };
        if root.starts_with(r"\\") {
            errors.insert(key, "only fixed internal drives can be scanned (network paths are excluded)".into());
            continue;
        }
        let Some(drive) = drive_for(drives, &root) else {
            errors.insert(key, "only fixed internal drives can be scanned".into());
            continue;
        };
        match std::fs::metadata(paths::to_extended(&root)) {
            Ok(m) if m.is_dir() => ok.push((root, drive.clone())),
            Ok(_) => {
                errors.insert(key, "not a folder".into());
            }
            Err(_) => {
                errors.insert(key, "does not exist or cannot be read".into());
            }
        }
    }
    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }
    // Merge nested roots into the outermost one.
    ok.sort_by_key(|(r, _)| r.len());
    let mut kept: Vec<(String, Drive)> = Vec::new();
    let mut merged = Vec::new();
    for (root, d) in ok {
        if kept.iter().any(|(k, _)| paths::is_within(&root, k)) {
            merged.push(root);
        } else {
            kept.push((root, d));
        }
    }
    Ok((kept, merged))
}

impl ScanService {
    fn snapshots_dir(&self) -> PathBuf {
        self.own.root().join("snapshots")
    }

    fn previous_total(&self, root: &str) -> Option<u64> {
        let conn = self.db.connect_read_only().ok()?;
        conn.query_row(
            "SELECT total_size FROM snapshots WHERE lower(root) = lower(?1) AND status = 'complete' ORDER BY id DESC LIMIT 1",
            [root],
            |r| r.get::<_, i64>(0),
        )
        .ok()
        .map(|v| v.max(0) as u64)
    }

    fn expected_bytes(&self, root: &str, d: &Drive) -> Option<u64> {
        if is_drive_root(root) {
            Some(d.used)
        } else {
            self.previous_total(root)
        }
    }

    /// How `root` on drive `d` will be scanned, and why not by MFT when it is not.
    pub fn plan_mode(&self, root: &str, d: &Drive) -> (ItemMode, Option<String>) {
        if !is_drive_root(root) {
            return (ItemMode::Walk, Some("MFT scan needs a whole drive, not a folder".into()));
        }
        if !d.filesystem.eq_ignore_ascii_case("NTFS") {
            return (ItemMode::Walk, Some("MFT scan supports NTFS only".into()));
        }
        let status = self.helper.status();
        if !status.running() {
            return (ItemMode::Walk, Some(status.why_not()));
        }
        (ItemMode::Mft { volume_id: d.id.clone() }, None)
    }

    /// Scan mode of every drive, for the scan dialog.
    pub fn modes(&self, settings: &Settings) -> ScanModes {
        let drives = self.inventory.snapshot(settings).drives;
        let list = drives
            .iter()
            .filter_map(|d| {
                let letter = d.letters.first()?;
                let root = format!("{letter}:\\");
                let (mode, reason) = self.plan_mode(&root, d);
                Some(ScanModeInfo {
                    root,
                    volume_id: d.id.clone(),
                    mode: if matches!(mode, ItemMode::Walk) { "walk" } else { "mft" }.into(),
                    reason,
                })
            })
            .collect();
        ScanModes { helper: self.helper.status(), drives: list }
    }

    /// Insert the catalog row and plan the file for one root.
    fn create_item(
        &self,
        conn: &rusqlite::Connection,
        root: &str,
        d: &Drive,
        settings: &Settings,
        method: &str,
        note: Option<&str>,
    ) -> Result<ScanItem, ApiError> {
        let hardlinks = hardlinks_for(settings, d);
        let decision = decide((self.last_access)(), d.capacity);
        conn.execute(
            "INSERT INTO snapshots (root, volume_id, elevated, hardlinks, status, started_at, db_file, time_basis, time_basis_reason, method, scan_note)
             VALUES (?1, ?2, ?3, ?4, 'running', ?5, '', ?6, ?7, ?8, ?9)",
            params![
                root,
                d.id,
                self.elevated,
                if hardlinks { "detected" } else { "not_deduplicated" },
                now_rfc3339(),
                decision.basis.as_str(),
                decision.reason.as_str(),
                method,
                note
            ],
        )
        .map_err(ApiError::internal)?;
        let id = conn.last_insert_rowid();
        let rel = format!("snapshots/{id}.db");
        let db_path =
            self.own.prepare_db_path(&self.snapshots_dir().join(format!("{id}.db"))).map_err(ApiError::internal)?;
        conn.execute("UPDATE snapshots SET db_file = ?2 WHERE id = ?1", params![id, rel])
            .map_err(ApiError::internal)?;
        let (group, threads) = group_and_threads(settings, d);
        Ok(ScanItem {
            snapshot_id: id,
            root: root.to_string(),
            db_path,
            threads,
            hardlinks,
            expected: self.expected_bytes(root, d),
            resume: false,
            group,
            mode: ItemMode::Walk,
            note: note.map(str::to_string),
        })
    }

    pub fn start(&self, req: &ScanRequest, settings: &Settings) -> Result<ScanStarted, ApiError> {
        self.start_with_note(req, settings, None)
    }

    /// `note` explains a fast scan that a quick rescan had to turn into a full one.
    fn start_with_note(
        &self,
        req: &ScanRequest,
        settings: &Settings,
        note: Option<&str>,
    ) -> Result<ScanStarted, ApiError> {
        let drives = self.inventory.snapshot(settings).drives;
        if drives.is_empty() {
            return Err(ApiError::Conflict("the drive list has not loaded yet; try again in a moment".into()));
        }
        let (roots, merged) = validate_roots(&req.roots, &drives)?;
        let conn = self.db.connect().map_err(ApiError::internal)?;
        let mut items = Vec::new();
        let mut started = Vec::new();
        for (root, d) in &roots {
            let (mode, reason) = self.plan_mode(root, d);
            let (method, why) = match &mode {
                ItemMode::Walk => ("walk", reason),
                _ => ("mft", note.map(str::to_string)),
            };
            let mut item = self.create_item(&conn, root, d, settings, method, why.as_deref())?;
            item.mode = mode;
            started.push(StartedSnapshot {
                id: item.snapshot_id,
                root: root.clone(),
                method: method.into(),
                note: why,
            });
            items.push(item);
        }
        let job_id = self.submit(items, &conn, settings)?;
        Ok(ScanStarted { job_id, snapshots: started, merged })
    }

    /// Manual "Quick rescan": read the USN journal from where the last fast scan stopped and
    /// re-read only what changed. Falls back to a full fast scan, saying why.
    pub fn quick(&self, req: &QuickRequest, settings: &Settings) -> Result<ScanStarted, ApiError> {
        let drives = self.inventory.snapshot(settings).drives;
        if drives.is_empty() {
            return Err(ApiError::Conflict("the drive list has not loaded yet; try again in a moment".into()));
        }
        let (roots, _) = validate_roots(std::slice::from_ref(&req.root), &drives)?;
        let Some((root, d)) = roots.into_iter().next() else {
            return Err(ApiError::BadRequest("choose a drive".into()));
        };
        let (mode, reason) = self.plan_mode(&root, &d);
        let ItemMode::Mft { volume_id } = mode else {
            return Err(ApiError::Conflict(format!("Quick rescan is not available: {}", reason.unwrap_or_default())));
        };
        let full = |why: String| -> Result<ScanStarted, ApiError> {
            self.start_with_note(&ScanRequest { roots: vec![root.clone()] }, settings, Some(&why))
        };
        let conn = self.db.connect().map_err(ApiError::internal)?;
        let prev = snapshots::list(
            &conn,
            &snapshots::ListFilter { root: Some(root.clone()), volume_id: None, limit: Some(20) },
        )
        .map_err(ApiError::internal)?
        .into_iter()
        .find(|s| s.status == SnapshotStatus::Complete && s.method.starts_with("mft"));
        let stored: Option<(i64, i64, i64)> = prev.as_ref().and_then(|p| {
            let _ = p;
            conn.query_row(
                "SELECT journal_id, next_usn, snapshot_id FROM volume_usn_state WHERE volume_guid = ?1",
                [&volume_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok()
        });
        let no_prior = diskdash_core::ntfs::usn::FullReason::NoPriorState.message();
        let (Some(prev), Some((journal_id, next_usn, state_snapshot))) = (prev, stored) else {
            return full(no_prior);
        };
        if state_snapshot != prev.id {
            return full(no_prior);
        }
        if prev.summarised {
            return full("the earlier fast scan was reduced to folder totals; running a full scan".into());
        }
        let total = prev.file_count + prev.dir_count + prev.link_count;
        let outcome = self
            .helper
            .usn_read(&volume_id, Some(StoredUsn { journal_id: journal_id as u64, next_usn }), total)
            .map_err(|e| ApiError::Conflict(format!("could not read the change journal: {e}")))?;
        match outcome {
            UsnOutcome::Full { reason, .. } => full(reason),
            UsnOutcome::Changes { changes, state } => {
                let mut item = self.create_item(&conn, &root, &d, settings, "mft-quick", None)?;
                item.mode =
                    ItemMode::Quick { volume_id, previous: self.own.root().join(&prev.db_file), changes, state };
                let started = vec![StartedSnapshot {
                    id: item.snapshot_id,
                    root: root.clone(),
                    method: "mft-quick".into(),
                    note: None,
                }];
                let job_id = self.submit(vec![item], &conn, settings)?;
                Ok(ScanStarted { job_id, snapshots: started, merged: Vec::new() })
            }
        }
    }

    pub fn resume(&self, snapshot_id: i64, settings: &Settings) -> Result<u64, ApiError> {
        let conn = self.db.connect().map_err(ApiError::internal)?;
        let snap = snapshots::get(&conn, snapshot_id)
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::NotFound(format!("snapshot {snapshot_id} not found")))?;
        if snap.status != SnapshotStatus::Partial {
            return Err(ApiError::Conflict("only cancelled or interrupted scans can be resumed".into()));
        }
        let drives = self.inventory.snapshot(settings).drives;
        let d = drive_for(&drives, &snap.root)
            .ok_or_else(|| ApiError::Conflict("this drive is not connected any more".into()))?
            .clone();
        let db_path = self.own.prepare_db_path(&self.own.root().join(&snap.db_file)).map_err(ApiError::internal)?;
        conn.execute("UPDATE snapshots SET status = 'running', resumed = 1, error = NULL WHERE id = ?1", [snapshot_id])
            .map_err(ApiError::internal)?;
        let (group, threads) = group_and_threads(settings, &d);
        let item = ScanItem {
            snapshot_id,
            root: snap.root.clone(),
            db_path,
            threads,
            hardlinks: snap.hardlinks_deduplicated,
            expected: self.expected_bytes(&snap.root, &d),
            resume: true,
            group,
            mode: ItemMode::Walk,
            note: None,
        };
        self.submit(vec![item], &conn, settings)
    }

    fn submit(&self, items: Vec<ScanItem>, conn: &rusqlite::Connection, settings: &Settings) -> Result<u64, ApiError> {
        let ids: Vec<i64> = items.iter().map(|i| i.snapshot_id).collect();
        let job = ScanJob {
            items,
            db: self.db.clone(),
            exclude: vec![paths::from_extended(&self.own.root().to_string_lossy())],
            windows_dir: std::env::var("SystemRoot").ok(),
            cleanup: self.cleanup.clone(),
            plans: self.plans.clone(),
            helper: self.helper.clone(),
            own_files: self.own.clone(),
            history: Some(HistoryHooks {
                own: self.own.clone(),
                inventory: self.inventory.clone(),
                keep: settings.retention_keep,
                depth: settings.retention_depth,
            }),
            completed: Mutex::new(Vec::new()),
        };
        let job_id = self.jobs.submit(Box::new(job)).map_err(ApiError::from)?;
        for id in ids {
            conn.execute("UPDATE snapshots SET job_id = ?2 WHERE id = ?1", params![id, job_id as i64])
                .map_err(ApiError::internal)?;
        }
        Ok(job_id)
    }
}

/// What a finished scan does for the history views.
pub struct HistoryHooks {
    pub own: OwnFiles,
    pub inventory: Arc<InventoryService>,
    pub keep: u32,
    pub depth: u32,
}

/// Scans one or more roots: different physical disks in parallel, roots on
/// the same disk one after another.
pub struct ScanJob {
    pub items: Vec<ScanItem>,
    pub db: Db,
    pub exclude: Vec<String>,
    pub windows_dir: Option<String>,
    pub cleanup: Option<Arc<CleanupService>>,
    pub plans: Option<Arc<crate::plans::PlanService>>,
    pub helper: Arc<dyn HelperLink>,
    pub own_files: OwnFiles,
    /// Free-space sample and old-scan clean-up after scans.
    pub history: Option<HistoryHooks>,
    /// Snapshots that finished completely during this job.
    pub completed: Mutex<Vec<i64>>,
}

#[derive(Default)]
struct ItemState {
    progress: ScanProgress,
    done_bytes: Option<u64>,
    running: bool,
    error: Option<String>,
    /// Reading the MFT (not the directory walk) right now.
    mft: bool,
}

/// Why a fast scan did not happen, or a real failure.
enum MftFail {
    /// Use the directory walk instead, and say why.
    Fallback(String),
    Fatal(scan::ScanError),
}

impl ScanJob {
    /// Scan one item, preferring the fast path when the plan says so and falling back to the walk.
    fn run_item(
        &self,
        item: &ScanItem,
        ctx: &JobContext,
        state: &Mutex<ItemState>,
    ) -> Result<ScanSummary, scan::ScanError> {
        let mut mode = item.mode.clone();
        loop {
            match &mode {
                ItemMode::Walk => return self.walk(item, ctx, state),
                ItemMode::Mft { .. } | ItemMode::Quick { .. } => match self.run_mft(item, &mode, ctx, state) {
                    Ok(summary) => return Ok(summary),
                    Err(MftFail::Fatal(e)) => return Err(e),
                    Err(MftFail::Fallback(reason)) => {
                        tracing::warn!(root = %item.root, %reason, "fast scan not possible");
                        if let Ok(mut st) = state.lock() {
                            st.mft = false;
                        }
                        // A failed quick rescan retries as a full fast scan; a failed fast scan walks.
                        let (next, method) = match &mode {
                            ItemMode::Quick { volume_id, .. } => {
                                (ItemMode::Mft { volume_id: volume_id.clone() }, "mft")
                            }
                            _ => (ItemMode::Walk, "walk"),
                        };
                        self.set_method(item.snapshot_id, method, &reason);
                        mode = next;
                    }
                },
            }
        }
    }

    fn set_method(&self, snapshot_id: i64, method: &str, note: &str) {
        if let Ok(conn) = self.db.connect() {
            let _ = conn.execute(
                "UPDATE snapshots SET method = ?2, scan_note = ?3 WHERE id = ?1",
                params![snapshot_id, method, note],
            );
        }
    }

    fn walk(
        &self,
        item: &ScanItem,
        ctx: &JobContext,
        state: &Mutex<ItemState>,
    ) -> Result<ScanSummary, scan::ScanError> {
        let opts = ScanOptions {
            root: PathBuf::from(&item.root),
            db_path: item.db_path.clone(),
            threads: item.threads,
            detect_hardlinks: item.hardlinks,
            exclude: self.exclude.clone(),
            resume: item.resume,
            windows_dir: self.windows_dir.clone(),
        };
        scan::scan(opts).and_then(|handle| {
            if let Ok(mut st) = state.lock() {
                st.running = true;
            }
            while !handle.is_finished() {
                if ctx.is_cancelled() {
                    handle.cancel();
                }
                if let Ok(mut st) = state.lock() {
                    st.progress = handle.progress();
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            handle.join()
        })
    }

    fn run_mft(
        &self,
        item: &ScanItem,
        mode: &ItemMode,
        ctx: &JobContext,
        state: &Mutex<ItemState>,
    ) -> Result<ScanSummary, MftFail> {
        if let Ok(mut st) = state.lock() {
            st.running = true;
            st.mft = true;
        }
        let cancel = AtomicBool::new(false);
        let opts = ImportOptions {
            root: item.root.clone(),
            db_path: item.db_path.clone(),
            windows_dir: self.windows_dir.clone(),
            max_bad_fraction: 0.01,
        };
        let mut progress = |stage: Stage, n: u64| {
            if ctx.is_cancelled() {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let Ok(mut st) = state.lock() {
                st.progress.files = n;
                st.progress.current_dir = match stage {
                    Stage::Indexing => "indexing".into(),
                    Stage::Writing => "writing the snapshot".into(),
                };
            }
        };
        let (result, journal, volume_id) = match mode {
            ItemMode::Mft { volume_id } => {
                let mut scan = self
                    .helper
                    .open_scan(volume_id)
                    .map_err(|e| MftFail::Fallback(format!("the volume could not be read through the helper: {e}")))?;
                let r = import(scan.source(), &opts, &cancel, &mut progress);
                (r, scan.journal(), volume_id.clone())
            }
            ItemMode::Quick { volume_id, previous, changes, state: journal } => {
                let r = quick_rescan(previous, changes.clone(), &opts, &cancel, &mut progress);
                (r, Some(*journal), volume_id.clone())
            }
            ItemMode::Walk => return Err(MftFail::Fatal(scan::ScanError::Db("not a fast scan".into()))),
        };
        match result {
            Ok(summary) => {
                if let Some(j) = journal {
                    self.store_usn(&volume_id, &j, item.snapshot_id);
                }
                Ok(summary)
            }
            Err(e) => {
                // Whatever was written is useless: the next attempt starts from an empty file.
                let _ = crate::snapshots::remove_snapshot_files(&self.own_files, &item.db_path);
                match e {
                    ImportError::Cancelled => Err(MftFail::Fatal(scan::ScanError::Db("cancelled".into()))),
                    ImportError::Db(m) => Err(MftFail::Fatal(scan::ScanError::Db(m))),
                    ImportError::Mft(m) => Err(MftFail::Fallback(m.to_string())),
                }
            }
        }
    }

    /// Remember where the change journal stood, so the next Quick rescan can start there.
    fn store_usn(&self, volume_id: &str, j: &JournalState, snapshot_id: i64) {
        let Ok(conn) = self.db.connect() else { return };
        let r = conn.execute(
            "INSERT INTO volume_usn_state (volume_guid, journal_id, next_usn, snapshot_id, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(volume_guid) DO UPDATE SET journal_id = excluded.journal_id, next_usn = excluded.next_usn,
               snapshot_id = excluded.snapshot_id, updated_at = excluded.updated_at",
            params![volume_id, j.journal_id as i64, j.next_usn, snapshot_id, now_rfc3339()],
        );
        if let Err(e) = r {
            tracing::warn!(error = %e, "cannot store the USN position");
        }
    }

    fn record(&self, item: &ScanItem, result: &Result<ScanSummary, scan::ScanError>, cancelled: bool) {
        let Ok(conn) = self.db.connect() else { return };
        let now = now_rfc3339();
        let res = match result {
            Ok(s) => {
                if s.status == ScanStatus::Complete {
                    if let Ok(mut done) = self.completed.lock() {
                        done.push(item.snapshot_id);
                    }
                }
                let status = match s.status {
                    ScanStatus::Complete => "complete",
                    ScanStatus::Partial => "partial",
                };
                conn.execute(
                    "UPDATE snapshots SET status = ?2, finished_at = ?3, volume_serial = ?4, filesystem = ?5, cluster_size = ?6,
                       hardlinks = ?7, file_count = ?8, dir_count = ?9, link_count = ?10, total_size = ?11, total_alloc = ?12,
                       error_count = ?13, access_denied_count = ?14, link_unknown_count = ?15,
                       error = CASE WHEN ?2 = 'partial' THEN ?16 ELSE NULL END
                     WHERE id = ?1",
                    params![
                        item.snapshot_id,
                        status,
                        now,
                        i64::from(s.volume_serial),
                        s.filesystem,
                        s.cluster_size as i64,
                        if s.hardlinks_deduplicated { "detected" } else { "not_deduplicated" },
                        s.stats.files as i64,
                        s.stats.dirs as i64,
                        s.stats.links as i64,
                        s.stats.size as i64,
                        s.stats.alloc as i64,
                        s.stats.errors as i64,
                        s.stats.access_denied as i64,
                        s.stats.link_count_unknown as i64,
                        if cancelled { "cancelled" } else { "interrupted" },
                    ],
                )
            }
            Err(e) => conn.execute(
                "UPDATE snapshots SET status = 'failed', finished_at = ?2, error = ?3 WHERE id = ?1",
                params![item.snapshot_id, now, e.to_string()],
            ),
        };
        if let Err(e) = res {
            tracing::error!(error = %e, snapshot = item.snapshot_id, "cannot record scan result");
        }
    }
}

impl Job for ScanJob {
    fn kind(&self) -> &'static str {
        "scan"
    }

    fn params(&self) -> serde_json::Value {
        serde_json::json!({
            "roots": self.items.iter().map(|i| &i.root).collect::<Vec<_>>(),
            "snapshots": self.items.iter().map(|i| i.snapshot_id).collect::<Vec<_>>(),
        })
    }

    fn locks(&self) -> Vec<PathBuf> {
        self.items.iter().map(|i| PathBuf::from(&i.root)).collect()
    }

    fn run(&mut self, ctx: &JobContext) -> Result<(), JobError> {
        let states: Vec<Mutex<ItemState>> = self.items.iter().map(|_| Mutex::new(ItemState::default())).collect();
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, it) in self.items.iter().enumerate() {
            groups.entry(it.group.clone()).or_default().push(i);
        }
        let total_expected: Option<u64> = self.items.iter().map(|i| i.expected).sum();

        std::thread::scope(|s| {
            let mut workers = Vec::new();
            for indices in groups.values() {
                let (states, items, this) = (&states, &self.items, &*self);
                workers.push(s.spawn(move || {
                    for &i in indices {
                        let item = &items[i];
                        if ctx.is_cancelled() {
                            break;
                        }
                        let result = this.run_item(item, ctx, &states[i]);
                        this.record(item, &result, ctx.is_cancelled());
                        if let Ok(mut st) = states[i].lock() {
                            st.running = false;
                            match &result {
                                Ok(sum) => st.done_bytes = Some(sum.stats.size),
                                Err(e) => st.error = Some(format!("{}: {e}", item.root)),
                            }
                        }
                    }
                }));
            }

            // Report combined progress while the workers run.
            while !workers.iter().all(|w| w.is_finished()) {
                let (mut bytes, mut phase) = (0u64, None);
                for (i, st) in states.iter().enumerate() {
                    if let Ok(st) = st.lock() {
                        bytes += st.done_bytes.unwrap_or(st.progress.bytes);
                        if st.running && phase.is_none() {
                            let p = &st.progress;
                            phase = Some(if st.mft {
                                format!(
                                    "Reading the MFT of {} · {} records · {}",
                                    self.items[i].root, p.files, p.current_dir
                                )
                            } else {
                                format!(
                                    "Scanning {} · {} files · {}",
                                    self.items[i].root,
                                    p.files,
                                    p.current_dir.rsplit('\\').next().unwrap_or("")
                                )
                            });
                        }
                    }
                }
                let total = total_expected.map(|t| t.max(bytes));
                ctx.progress(bytes, total, "bytes", Some(phase.as_deref().unwrap_or("Starting scan")));
                std::thread::sleep(Duration::from_millis(250));
            }
        });

        // Compare the plans' items with the new scan (snapshot data only).
        if let Some(plans) = &self.plans {
            let ids = self.completed.lock().map(|d| d.clone()).unwrap_or_default();
            if !ids.is_empty() && plans.has_items() {
                if let Err(e) = plans.submit_verify(&ids, None) {
                    tracing::warn!(error = %e, "cannot queue plan verification");
                }
            }
        }
        // Suggestions for every scan that finished, even when another root failed or was cancelled.
        if let Some(cleanup) = &self.cleanup {
            let ids = self.completed.lock().map(|d| d.clone()).unwrap_or_default();
            if !ids.is_empty() {
                if let Err(e) = cleanup.submit(&ids) {
                    tracing::warn!(error = %e, "cannot queue cleanup evaluation");
                }
            }
        }

        // History: a free-space sample at the moment the scan finished, then shrink old scans.
        if let Some(h) = &self.history {
            let any = self.completed.lock().map(|d| !d.is_empty()).unwrap_or(false);
            if any {
                h.inventory.sample_after_scan();
                if let Err(e) = crate::history::retain(
                    &self.db,
                    &h.own,
                    h.keep,
                    h.depth,
                    crate::time::unix_now(),
                    crate::history::Stop::Never,
                ) {
                    tracing::warn!(error = %e, "cannot shrink old scans");
                }
            }
        }

        if ctx.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        let errors: Vec<String> = states.iter().filter_map(|s| s.lock().ok().and_then(|s| s.error.clone())).collect();
        if errors.is_empty() {
            let bytes: u64 = states.iter().filter_map(|s| s.lock().ok().and_then(|s| s.done_bytes)).sum();
            ctx.progress(bytes, Some(bytes), "bytes", Some("Scan complete"));
            Ok(())
        } else {
            Err(JobError::Failed(errors.join("; ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskdash_inventory::model::{DiskHealth, HealthField, HealthStatus, PhysicalDisk};

    fn health() -> DiskHealth {
        let u = || HealthField::unavailable("x");
        DiskHealth {
            overall: HealthStatus::Healthy,
            reasons: vec![],
            cmdlet_status: HealthStatus::Healthy,
            temperature_c: u(),
            power_on_hours: u(),
            wear_percent: u(),
            read_errors_total: u(),
            write_errors_total: u(),
            reallocated_sectors: u(),
            data_written_bytes: u(),
            smart_passed: None,
        }
    }

    fn drive(letter: &str, media: MediaClass, disk: i32, system: bool) -> Drive {
        Drive {
            id: format!("vol-{letter}"),
            volume_path: format!(r"\\?\Volume{{{letter}}}\"),
            letters: vec![letter.into()],
            mount_paths: vec![],
            label: String::new(),
            filesystem: "NTFS".into(),
            serial: None,
            cluster_size: 4096,
            capacity: 100,
            free: 50,
            used: 50,
            used_fraction: 0.5,
            media,
            physical_disks: vec![PhysicalDisk {
                number: disk,
                model: "m".into(),
                serial: None,
                bus_type: "SATA".into(),
                media,
                spindle_rpm: None,
                size: 100,
                is_system: system,
                health: health(),
            }],
            health: HealthStatus::Healthy,
            is_system_volume: system,
            low_space: false,
            low_space_reason: None,
        }
    }

    #[test]
    fn validation_rejects_out_of_scope_and_missing() {
        let drives = vec![drive("C", MediaClass::NVME_SSD, 2, true)];
        let err = validate_roots(&[r"Z:\".into(), r"\\nas\share".into(), String::new()], &drives).unwrap_err();
        let ApiError::Validation(f) = err else { panic!("expected validation") };
        assert!(f["roots[0]"].contains("fixed internal"));
        assert!(f["roots[1]"].contains("network"));
        assert_eq!(f["roots[2]"], "empty path");
        let ApiError::Validation(f) = validate_roots(&[r"C:\definitely\missing\dir".into()], &drives).unwrap_err()
        else {
            panic!()
        };
        assert!(f["roots[0]"].contains("does not exist"));
    }

    #[test]
    fn nested_roots_are_merged() {
        let drives = vec![drive("C", MediaClass::NVME_SSD, 2, true)];
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let (kept, merged) = validate_roots(&[format!(r"{windir}\System32"), r"c:\".into()], &drives).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, r"C:\");
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn scheduling_by_media() {
        let s = Settings::default();
        let hdd = drive("D", MediaClass::HDD, 0, false);
        let ssd = drive("G", MediaClass::SATA_SSD, 1, false);
        let unknown = drive("X", MediaClass::UNKNOWN, 5, false);
        assert_eq!(group_and_threads(&s, &hdd), ("disk-0".to_string(), 1));
        assert_eq!(group_and_threads(&s, &ssd), ("disk-1".to_string(), 4));
        assert_eq!(group_and_threads(&s, &unknown).1, 1, "unknown media is treated as HDD");
    }

    #[test]
    fn hardlink_defaults_and_overrides() {
        let mut s = Settings::default();
        let c = drive("C", MediaClass::NVME_SSD, 2, true);
        let d = drive("D", MediaClass::HDD, 0, false);
        assert!(hardlinks_for(&s, &c));
        assert!(!hardlinks_for(&s, &d));
        s.hardlink_overrides.insert(d.volume_path.clone(), true);
        assert!(hardlinks_for(&s, &d), "per-drive toggle");
        s.hardlink_detection = HardlinkDefault::NoVolumes;
        assert!(!hardlinks_for(&s, &c));
    }
}
