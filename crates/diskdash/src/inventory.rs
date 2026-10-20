//! Inventory service: cached drive list, refresh (as a job or on a timer while
//! the UI is open), free-space sampling and the volume/disk registry.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use diskdash_inventory::model::{Drive, ExcludedCounts, HealthSources, Inventory, TrimState};
use diskdash_inventory::runner::ProcessRunner;
use diskdash_inventory::{lowspace, smartctl, Collector};
use diskdash_store::Db;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use ts_rs::TS;

use crate::events::EventBus;
use crate::jobs::{Job, JobContext, JobError};
use crate::settings::Settings;
use crate::time::now_rfc3339;

/// Auto-refresh period while at least one UI is connected.
pub const AUTO_REFRESH: Duration = Duration::from_secs(60);
/// Samples are coalesced into 15-minute buckets.
pub const SAMPLE_BUCKET_SECS: u64 = 15 * 60;

/// Produces an inventory (real system or a test fixture).
pub trait InventorySource: Send + Sync {
    fn collect(&self) -> Inventory;
}

/// The real machine: PowerShell/CIM with native fallback, fsutil, smartctl.
pub struct SystemSource {
    pub elevated: bool,
    pub smartctl: Option<PathBuf>,
}

impl SystemSource {
    pub fn detect(elevated: bool) -> Self {
        Self { elevated, smartctl: smartctl::discover() }
    }
}

impl InventorySource for SystemSource {
    fn collect(&self) -> Inventory {
        let runner = ProcessRunner::default();
        Collector {
            runner: &runner,
            elevated: self.elevated,
            smartctl: self.smartctl.clone(),
            use_native_serials: true,
        }
        .collect()
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DrivesResponse {
    pub drives: Vec<Drive>,
    pub excluded: ExcludedCounts,
    pub trim: TrimState,
    pub sources: HealthSources,
    /// Collected without PowerShell (reduced detail).
    pub fallback: bool,
    pub refreshed_at: Option<String>,
    /// The last refresh failed; values are from an earlier refresh.
    pub stale: bool,
    pub error: Option<String>,
}

struct Cache {
    inventory: Option<Inventory>,
    refreshed_at: Option<String>,
    refreshed_instant: Option<Instant>,
    stale: bool,
    error: Option<String>,
}

pub struct InventoryService {
    source: Box<dyn InventorySource>,
    db: Db,
    events: Arc<EventBus>,
    cache: RwLock<Cache>,
    refreshing: Mutex<()>,
}

impl InventoryService {
    pub fn new(source: Box<dyn InventorySource>, db: Db, events: Arc<EventBus>) -> Self {
        let cache = load_cache(&db).unwrap_or(Cache {
            inventory: None,
            refreshed_at: None,
            refreshed_instant: None,
            stale: false,
            error: None,
        });
        Self { source, db, events, cache: RwLock::new(cache), refreshing: Mutex::new(()) }
    }

    pub fn has_data(&self) -> bool {
        self.cache.read().map(|c| c.inventory.is_some()).unwrap_or(false)
    }

    /// Seconds since the last successful refresh in this process.
    pub fn age(&self) -> Option<Duration> {
        self.cache.read().ok().and_then(|c| c.refreshed_instant.map(|i| i.elapsed()))
    }

    /// Collect now (blocking). Concurrent calls wait for the one in progress.
    pub fn refresh(&self) -> Result<(), String> {
        let _guard = self.refreshing.lock().map_err(|_| "refresh lock poisoned".to_string())?;
        let started = Instant::now();
        let inv = self.source.collect();
        let had_data = self.has_data();
        if inv.drives.is_empty() && had_data {
            let msg = "inventory returned no drives; keeping previous values".to_string();
            if let Ok(mut c) = self.cache.write() {
                c.stale = true;
                c.error = Some(msg.clone());
            }
            self.events.publish("inventory.updated", serde_json::json!({ "stale": true }));
            return Err(msg);
        }
        let now = now_rfc3339();
        if let Err(e) = self.persist(&inv, &now) {
            tracing::error!(error = %e, "failed to persist inventory");
        }
        if let Ok(mut c) = self.cache.write() {
            c.inventory = Some(inv);
            c.refreshed_at = Some(now.clone());
            c.refreshed_instant = Some(Instant::now());
            c.stale = false;
            c.error = None;
        }
        tracing::info!(ms = started.elapsed().as_millis() as u64, "inventory refreshed");
        self.events.publish("inventory.updated", serde_json::json!({ "refreshed_at": now }));
        Ok(())
    }

    /// Current view with low-space flags from `settings`.
    pub fn snapshot(&self, settings: &Settings) -> DrivesResponse {
        let c = match self.cache.read() {
            Ok(c) => c,
            Err(p) => p.into_inner(),
        };
        let mut resp = match &c.inventory {
            Some(inv) => DrivesResponse {
                drives: inv.drives.clone(),
                excluded: inv.excluded.clone(),
                trim: inv.trim.clone(),
                sources: inv.sources.clone(),
                fallback: inv.fallback,
                refreshed_at: c.refreshed_at.clone(),
                stale: c.stale,
                error: c.error.clone(),
            },
            None => DrivesResponse {
                drives: Vec::new(),
                excluded: ExcludedCounts::default(),
                trim: TrimState { ntfs: None, refs: None, reason: Some("not collected yet".into()) },
                sources: HealthSources { elevated: false, smartctl: None },
                fallback: false,
                refreshed_at: None,
                stale: c.stale,
                error: c.error.clone(),
            },
        };
        lowspace::apply(&mut resp.drives, settings.low_space_percent, settings.low_space_floor_gib);
        resp
    }

    /// Take a fresh free-space sample right after a scan finished and mark it as such,
    /// so the trend chart can show when scans happened.
    pub fn sample_after_scan(&self) {
        if self.refresh().is_err() {
            return;
        }
        if let Ok(conn) = self.db.connect() {
            let bucket = unix_secs() / SAMPLE_BUCKET_SECS;
            let _ = conn.execute("UPDATE volume_samples SET source = 'scan' WHERE bucket = ?1", [bucket as i64]);
        }
    }

    fn persist(&self, inv: &Inventory, now: &str) -> rusqlite::Result<()> {
        let mut conn = self.db.connect()?;
        let tx = conn.transaction()?;
        let bucket = unix_secs() / SAMPLE_BUCKET_SECS;
        for d in &inv.drives {
            tx.execute(
                "INSERT INTO volumes(volume_id, volume_path, letters, label, filesystem, serial, capacity, media, first_seen, last_seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
                 ON CONFLICT(volume_id) DO UPDATE SET volume_path = excluded.volume_path, letters = excluded.letters,
                   label = excluded.label, filesystem = excluded.filesystem, serial = excluded.serial,
                   capacity = excluded.capacity, media = excluded.media, last_seen = excluded.last_seen",
                params![
                    d.id,
                    d.volume_path,
                    d.letters.join(","),
                    d.label,
                    d.filesystem,
                    d.serial.map(i64::from),
                    d.capacity as i64,
                    format!("{:?}", d.media),
                    now
                ],
            )?;
            for p in &d.physical_disks {
                let key = p.serial.clone().unwrap_or_else(|| format!("disk-{}", p.number));
                tx.execute(
                    "INSERT INTO physical_disks(disk_key, number, model, bus_type, media, size, first_seen, last_seen)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                     ON CONFLICT(disk_key) DO UPDATE SET number = excluded.number, model = excluded.model,
                       bus_type = excluded.bus_type, media = excluded.media, size = excluded.size, last_seen = excluded.last_seen",
                    params![key, p.number, p.model, p.bus_type, format!("{:?}", p.media), p.size as i64, now],
                )?;
                tx.execute(
                    "INSERT OR IGNORE INTO volume_disks(volume_id, disk_key) VALUES (?1, ?2)",
                    params![d.id, key],
                )?;
            }
            tx.execute(
                "INSERT INTO volume_samples(volume_id, bucket, ts, capacity, free, source) VALUES (?1, ?2, ?3, ?4, ?5, 'inventory')
                 ON CONFLICT(volume_id, bucket) DO UPDATE SET ts = excluded.ts, capacity = excluded.capacity, free = excluded.free",
                params![d.id, bucket as i64, now, d.capacity as i64, d.free as i64],
            )?;
        }
        let json = serde_json::to_string(inv).unwrap_or_default();
        tx.execute(
            "INSERT INTO inventory_cache(id, json, refreshed_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET json = excluded.json, refreshed_at = excluded.refreshed_at",
            params![json, now],
        )?;
        tx.commit()
    }
}

fn unix_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The cached inventory from the last run, if any. It is shown immediately and
/// marked by its `refreshed_at`; a fresh refresh replaces it on startup.
fn load_cache(db: &Db) -> Option<Cache> {
    let conn: Connection = db.connect_read_only().ok()?;
    let row: Option<(String, String)> = conn
        .query_row("SELECT json, refreshed_at FROM inventory_cache WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .ok()?;
    let (json, refreshed_at) = row?;
    let inventory: Inventory = serde_json::from_str(&json).ok()?;
    Some(Cache {
        inventory: Some(inventory),
        refreshed_at: Some(refreshed_at),
        refreshed_instant: None,
        stale: false,
        error: None,
    })
}

/// Job wrapper for a manual refresh (`POST /api/v1/drives/refresh`).
pub struct RefreshJob(pub Arc<InventoryService>);

impl Job for RefreshJob {
    fn kind(&self) -> &'static str {
        "inventory-refresh"
    }

    fn run(&mut self, ctx: &JobContext) -> Result<(), JobError> {
        ctx.progress(0, Some(1), "steps", Some("Reading drives"));
        self.0.refresh().map_err(JobError::Failed)?;
        ctx.progress(1, Some(1), "steps", Some("Done"));
        Ok(())
    }
}

/// Refresh on startup, then every minute while a UI is connected.
pub fn spawn_auto_refresh(service: Arc<InventoryService>, events: Arc<EventBus>) {
    tokio::spawn(async move {
        let s = service.clone();
        let _ = tokio::task::spawn_blocking(move || s.refresh()).await;
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tick.tick().await;
            let due = service.age().is_none_or(|a| a >= AUTO_REFRESH);
            if due && events.subscriber_count() > 0 {
                let s = service.clone();
                let _ = tokio::task::spawn_blocking(move || s.refresh()).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskdash_store::{open_catalog, DataDir, OwnFiles};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct FixtureSource {
        pub calls: AtomicUsize,
        pub empty_after: Option<usize>,
    }

    impl InventorySource for FixtureSource {
        fn collect(&self) -> Inventory {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let mut inv = fixture_inventory();
            if self.empty_after.is_some_and(|k| n >= k) {
                inv.drives.clear();
            }
            inv
        }
    }

    pub fn fixture_inventory() -> Inventory {
        let raw = diskdash_inventory::raw::RawInventory::from_json(include_str!(
            "../../diskdash-inventory/tests/fixtures/this_machine.json"
        ))
        .unwrap();
        let inputs = diskdash_inventory::assemble::HealthInputs {
            elevated: false,
            reliability: &[],
            smart: &[],
            windows_letter: Some("C".into()),
        };
        let (drives, excluded) = diskdash_inventory::assemble::assemble(&raw, &inputs);
        Inventory {
            drives,
            excluded,
            trim: TrimState { ntfs: Some(true), refs: Some(true), reason: None },
            sources: HealthSources { elevated: false, smartctl: None },
            fallback: false,
        }
    }

    fn service(empty_after: Option<usize>) -> (tempfile::TempDir, InventoryService, Db) {
        let tmp = tempfile::tempdir().unwrap();
        let dd = DataDir::at(tmp.path().join("dd"));
        let own = OwnFiles::new(&dd).unwrap();
        let db = open_catalog(&dd, &own).unwrap();
        let src = FixtureSource { calls: AtomicUsize::new(0), empty_after };
        let svc = InventoryService::new(Box::new(src), db.clone(), Arc::new(EventBus::new()));
        (tmp, svc, db)
    }

    #[test]
    fn refresh_fills_cache_and_applies_low_space() {
        let (_t, svc, _db) = service(None);
        assert!(!svc.has_data());
        svc.refresh().unwrap();
        let resp = svc.snapshot(&Settings::default());
        assert_eq!(resp.drives.len(), 3);
        let d = resp.drives.iter().find(|d| d.letters == ["D"]).unwrap();
        assert!(d.low_space, "D: has 4.3% free");
        assert_eq!(d.low_space_reason, Some(diskdash_inventory::model::LowSpaceReason::Percent));
        let c = resp.drives.iter().find(|d| d.letters == ["C"]).unwrap();
        assert!(!c.low_space);
        assert_eq!(resp.excluded.optical, 1);
    }

    #[test]
    fn samples_coalesce_into_one_row_per_bucket() {
        let (_t, svc, db) = service(None);
        for _ in 0..10 {
            svc.refresh().unwrap();
        }
        let conn = db.connect().unwrap();
        let rows: i64 = conn.query_row("SELECT count(*) FROM volume_samples", [], |r| r.get(0)).unwrap();
        assert!(rows == 3 || rows == 6, "one row per volume per bucket (6 only if a bucket boundary passed): {rows}");
        let vols: i64 = conn.query_row("SELECT count(*) FROM volumes", [], |r| r.get(0)).unwrap();
        assert_eq!(vols, 3);
    }

    #[test]
    fn empty_result_keeps_previous_values_as_stale() {
        let (_t, svc, db) = service(Some(1));
        svc.refresh().unwrap();
        let samples = |db: &Db| -> i64 {
            db.connect().unwrap().query_row("SELECT count(*) FROM volume_samples", [], |r| r.get(0)).unwrap()
        };
        let before = samples(&db);
        assert!(svc.refresh().is_err());
        assert_eq!(samples(&db), before, "an unavailable volume list records no sample (no zeros)");
        let resp = svc.snapshot(&Settings::default());
        assert_eq!(resp.drives.len(), 3);
        assert!(resp.stale);
        assert!(resp.error.is_some());
    }

    #[test]
    fn restart_serves_cached_inventory() {
        let (tmp, svc, db) = service(None);
        svc.refresh().unwrap();
        drop(svc);
        let src = FixtureSource { calls: AtomicUsize::new(0), empty_after: None };
        let again = InventoryService::new(Box::new(src), db, Arc::new(EventBus::new()));
        assert!(again.has_data());
        assert_eq!(again.snapshot(&Settings::default()).drives.len(), 3);
        drop(tmp);
    }

    #[test]
    fn threshold_change_is_applied_on_next_read() {
        let (_t, svc, _db) = service(None);
        svc.refresh().unwrap();
        let lenient = Settings { low_space_percent: 1.0, low_space_floor_gib: 10, ..Settings::default() };
        let d = svc.snapshot(&lenient).drives.into_iter().find(|d| d.letters == ["D"]).unwrap();
        assert!(!d.low_space, "40 GiB free is fine at 1% / 10 GiB");
    }
}
