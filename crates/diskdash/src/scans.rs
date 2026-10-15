//! Starting, scheduling and resuming scans. Scans are only ever started by the
//! user (no schedules, no background watchdog).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use diskdash_analysis::stale::{decide, Setting};
use diskdash_core::paths;
use diskdash_core::scan::{self, ScanOptions, ScanProgress, ScanStatus, ScanSummary};
use diskdash_inventory::model::{Drive, MediaClass};
use diskdash_store::{Db, OwnFiles};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cleanup::CleanupService;
use crate::error::ApiError;
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

    pub fn start(&self, req: &ScanRequest, settings: &Settings) -> Result<ScanStarted, ApiError> {
        let drives = self.inventory.snapshot(settings).drives;
        if drives.is_empty() {
            return Err(ApiError::Conflict("the drive list has not loaded yet; try again in a moment".into()));
        }
        let (roots, merged) = validate_roots(&req.roots, &drives)?;
        let conn = self.db.connect().map_err(ApiError::internal)?;
        let setting = (self.last_access)();
        let now = now_rfc3339();
        let mut items = Vec::new();
        let mut started = Vec::new();
        for (root, d) in &roots {
            let hardlinks = hardlinks_for(settings, d);
            let decision = decide(setting, d.capacity);
            conn.execute(
                "INSERT INTO snapshots (root, volume_id, elevated, hardlinks, status, started_at, db_file, time_basis, time_basis_reason)
                 VALUES (?1, ?2, ?3, ?4, 'running', ?5, '', ?6, ?7)",
                params![
                    root,
                    d.id,
                    self.elevated,
                    if hardlinks { "detected" } else { "not_deduplicated" },
                    now,
                    decision.basis.as_str(),
                    decision.reason.as_str()
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
            items.push(ScanItem {
                snapshot_id: id,
                root: root.clone(),
                db_path,
                threads,
                hardlinks,
                expected: self.expected_bytes(root, d),
                resume: false,
                group,
            });
            started.push(StartedSnapshot { id, root: root.clone() });
        }
        let job_id = self.submit(items, &conn)?;
        Ok(ScanStarted { job_id, snapshots: started, merged })
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
        };
        self.submit(vec![item], &conn)
    }

    fn submit(&self, items: Vec<ScanItem>, conn: &rusqlite::Connection) -> Result<u64, ApiError> {
        let ids: Vec<i64> = items.iter().map(|i| i.snapshot_id).collect();
        let job = ScanJob {
            items,
            db: self.db.clone(),
            exclude: vec![paths::from_extended(&self.own.root().to_string_lossy())],
            windows_dir: std::env::var("SystemRoot").ok(),
            cleanup: self.cleanup.clone(),
            plans: self.plans.clone(),
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

/// Scans one or more roots: different physical disks in parallel, roots on
/// the same disk one after another.
pub struct ScanJob {
    pub items: Vec<ScanItem>,
    pub db: Db,
    pub exclude: Vec<String>,
    pub windows_dir: Option<String>,
    pub cleanup: Option<Arc<CleanupService>>,
    pub plans: Option<Arc<crate::plans::PlanService>>,
    /// Snapshots that finished completely during this job.
    pub completed: Mutex<Vec<i64>>,
}

#[derive(Default)]
struct ItemState {
    progress: ScanProgress,
    done_bytes: Option<u64>,
    running: bool,
    error: Option<String>,
}

impl ScanJob {
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
                        let opts = ScanOptions {
                            root: PathBuf::from(&item.root),
                            db_path: item.db_path.clone(),
                            threads: item.threads,
                            detect_hardlinks: item.hardlinks,
                            exclude: this.exclude.clone(),
                            resume: item.resume,
                            windows_dir: this.windows_dir.clone(),
                        };
                        let result = scan::scan(opts).and_then(|handle| {
                            if let Ok(mut st) = states[i].lock() {
                                st.running = true;
                            }
                            while !handle.is_finished() {
                                if ctx.is_cancelled() {
                                    handle.cancel();
                                }
                                if let Ok(mut st) = states[i].lock() {
                                    st.progress = handle.progress();
                                }
                                std::thread::sleep(Duration::from_millis(100));
                            }
                            handle.join()
                        });
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
                            phase = Some(format!(
                                "Scanning {} · {} files · {}",
                                self.items[i].root,
                                p.files,
                                p.current_dir.rsplit('\\').next().unwrap_or("")
                            ));
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
