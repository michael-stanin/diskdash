//! In-app alerts: low free space and fast-growing folders.
//!
//! Evaluated only while the app runs, after every inventory refresh and every
//! finished scan. Delivered only inside the dashboard (bell, Alerts page,
//! banners) over the SSE stream. There are no OS notifications, shortcuts,
//! scheduled tasks or background processes anywhere in diskdash.

use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use diskdash_analysis::alerts::{decide, growth_rate_per_day, low_space, low_space_cleared, Action, AlertState};
use diskdash_inventory::model::Drive;
use diskdash_store::{Db, OwnFiles};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ApiError;
use crate::events::EventBus;
use crate::history::{self, DiffQuery};
use crate::inventory::InventoryService;
use crate::settings::{self, Settings};
use crate::snapshots::{self, SnapshotStatus};

const GIB: f64 = (1u64 << 30) as f64;
/// At most this many folders per scanned root are reported as growing too fast.
const MAX_GROWTH_ALERTS_PER_ROOT: usize = 5;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct HistoryLink {
    #[ts(type = "number")]
    pub old: i64,
    #[ts(type = "number")]
    pub new: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AlertView {
    #[ts(type = "number")]
    pub id: i64,
    /// `low_space` or `growth`.
    pub kind: String,
    /// `warning` or `critical`.
    pub severity: String,
    /// Drive letter such as `C:`.
    pub drive: Option<String>,
    pub volume_id: Option<String>,
    /// Growth alerts: the folder.
    pub path: Option<String>,
    pub title: String,
    pub message: String,
    /// Low space: free bytes. Growth: bytes per day.
    pub value: f64,
    pub threshold: f64,
    pub raised_at: String,
    pub resolved_at: Option<String>,
    pub dismissed: bool,
    /// Growth alerts: the two scans to compare in History.
    pub history: Option<HistoryLink>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AlertThresholds {
    pub low_percent: f64,
    pub low_floor_gib: u32,
    pub growth_gib_per_day: f64,
    pub cooldown_hours: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AlertsResponse {
    pub alerts: Vec<AlertView>,
    /// Unresolved alerts (the bell count).
    #[ts(type = "number")]
    pub active: u32,
    /// Unresolved alerts the user has not dismissed (banners).
    #[ts(type = "number")]
    pub banners: u32,
    pub thresholds: AlertThresholds,
    /// When this server process started: alerts are only checked while it runs.
    pub running_since: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct AlertQuery {
    /// `active` (default), `resolved` or `all`.
    pub status: Option<String>,
}

pub struct AlertService {
    pub db: Db,
    pub own: OwnFiles,
    pub events: Arc<EventBus>,
    pub inventory: Arc<InventoryService>,
    started_at: i64,
    /// Evaluations run one at a time.
    gate: Mutex<()>,
}

fn rfc3339(secs: i64) -> String {
    humantime::format_rfc3339_seconds(UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64)).to_string()
}

fn gib(bytes: f64) -> String {
    format!("{:.1} GiB", bytes / GIB)
}

impl AlertService {
    pub fn new(db: Db, own: OwnFiles, events: Arc<EventBus>, inventory: Arc<InventoryService>) -> Self {
        Self { db, own, events, inventory, started_at: crate::time::unix_now(), gate: Mutex::new(()) }
    }

    fn settings(&self, conn: &Connection) -> Settings {
        settings::load(conn).unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Evaluation
    // -----------------------------------------------------------------------

    /// After an inventory refresh: low space on every internal drive.
    pub fn on_inventory(&self, drives: &[Drive]) {
        let Ok(_g) = self.gate.lock() else { return };
        let Ok(conn) = self.db.connect() else { return };
        let settings = self.settings(&conn);
        match self.evaluate_drives(&conn, drives, &settings, crate::time::unix_now()) {
            Ok(changed) => self.announce(&conn, changed),
            Err(e) => tracing::warn!(error = %e, "cannot evaluate low-space alerts"),
        }
    }

    /// After a finished scan: low space again (the scan took a fresh sample) and fast growth in the scanned roots.
    pub fn after_scan(&self, snapshot_ids: &[i64]) {
        let Ok(_g) = self.gate.lock() else { return };
        let Ok(conn) = self.db.connect() else { return };
        let settings = self.settings(&conn);
        let now = crate::time::unix_now();
        let mut changed = 0;
        let drives = self.inventory.snapshot(&settings).drives;
        match self.evaluate_drives(&conn, &drives, &settings, now) {
            Ok(n) => changed += n,
            Err(e) => tracing::warn!(error = %e, "cannot evaluate low-space alerts"),
        }
        for id in snapshot_ids {
            match self.evaluate_growth(&conn, *id, &settings, now) {
                Ok(n) => changed += n,
                Err(e) => tracing::warn!(error = %e, snapshot = id, "cannot evaluate growth alerts"),
            }
        }
        self.announce(&conn, changed);
    }

    fn announce(&self, conn: &Connection, changed: u32) {
        if changed == 0 {
            return;
        }
        let (active, banners) = counts(conn);
        self.events.publish("alert.changed", serde_json::json!({ "active": active, "banners": banners }));
    }

    fn state(conn: &Connection, key: &str) -> Option<AlertState> {
        conn.query_row("SELECT active, last_raised_at FROM alert_state WHERE alert_key = ?1", [key], |r| {
            Ok(AlertState { active: r.get::<_, i64>(0)? != 0, last_raised_at: r.get(1)? })
        })
        .optional()
        .ok()
        .flatten()
    }

    /// Apply a decision for `key`. Returns 1 when something changed that the UI should show.
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &self,
        conn: &Connection,
        key: &str,
        action: Action,
        value: f64,
        now: i64,
        new_alert: impl FnOnce() -> NewAlert,
    ) -> rusqlite::Result<u32> {
        match action {
            Action::Nothing => Ok(0),
            Action::Clear => {
                conn.execute(
                    "UPDATE alerts SET resolved_at = ?2 WHERE alert_key = ?1 AND resolved_at IS NULL",
                    params![key, now],
                )?;
                conn.execute("UPDATE alert_state SET active = 0 WHERE alert_key = ?1", [key])?;
                Ok(1)
            }
            Action::Raise { .. } => {
                // An alert that is being raised again replaces the one before it.
                conn.execute(
                    "UPDATE alerts SET resolved_at = ?2 WHERE alert_key = ?1 AND resolved_at IS NULL",
                    params![key, now],
                )?;
                let a = new_alert();
                conn.execute(
                    "INSERT INTO alerts (alert_key, kind, severity, volume_id, drive, path, title, message, value, threshold, detail, raised_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![key, a.kind, a.severity, a.volume_id, a.drive, a.path, a.title, a.message, value, a.threshold, a.detail, now],
                )?;
                conn.execute(
                    "INSERT INTO alert_state (alert_key, last_raised_at, last_value, active, dismissed_at) VALUES (?1, ?2, ?3, 1, NULL)
                     ON CONFLICT(alert_key) DO UPDATE SET last_raised_at = excluded.last_raised_at, last_value = excluded.last_value,
                       active = 1, dismissed_at = NULL",
                    params![key, now, value],
                )?;
                Ok(1)
            }
        }
    }

    fn evaluate_drives(&self, conn: &Connection, drives: &[Drive], s: &Settings, now: i64) -> rusqlite::Result<u32> {
        let cooldown = i64::from(s.alert_cooldown_hours) * 3600;
        let mut changed = 0;
        for d in drives {
            let Some(letter) = d.letters.first() else { continue };
            if d.capacity == 0 {
                continue;
            }
            let key = format!("low_space:{}", d.id);
            let cond = low_space(d.capacity, d.free, s.low_space_percent, s.low_space_floor_gib);
            let cleared = low_space_cleared(d.capacity, d.free, s.low_space_percent, s.low_space_floor_gib);
            let action = decide(cond, cleared, Self::state(conn, &key), now, cooldown);
            let pct = d.free as f64 / d.capacity as f64 * 100.0;
            let limit = (d.capacity as f64 * s.low_space_percent / 100.0).max(f64::from(s.low_space_floor_gib) * GIB);
            changed += self.apply(conn, &key, action, d.free as f64, now, || NewAlert {
                kind: "low_space",
                severity: if (d.free as f64) < limit / 2.0 { "critical" } else { "warning" },
                volume_id: Some(d.id.clone()),
                drive: Some(format!("{letter}:")),
                path: None,
                title: format!("{letter}: is low on space"),
                message: format!(
                    "{} free of {} ({pct:.1}%). The limit is {}% or {} GiB.",
                    gib(d.free as f64),
                    gib(d.capacity as f64),
                    s.low_space_percent,
                    s.low_space_floor_gib
                ),
                threshold: limit,
                detail: None,
            })?;
        }
        Ok(changed)
    }

    fn evaluate_growth(&self, conn: &Connection, snapshot_id: i64, s: &Settings, now: i64) -> Result<u32, String> {
        let Some(new) = snapshots::get(conn, snapshot_id).map_err(|e| e.to_string())? else { return Ok(0) };
        if new.status != SnapshotStatus::Complete {
            return Ok(0);
        }
        let ts = |snap: &snapshots::Snapshot| {
            snap.finished_at
                .as_deref()
                .and_then(|f| humantime::parse_rfc3339(f).ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
        };
        let Some(new_ts) = ts(&new) else { return Ok(0) };
        // The most recent earlier scan of the same root that is at least a day older.
        let older = snapshots::list(
            conn,
            &snapshots::ListFilter { root: Some(new.root.clone()), volume_id: None, limit: Some(100) },
        )
        .map_err(|e| e.to_string())?;
        let old = older
            .into_iter()
            .filter(|o| o.id < new.id && o.status == SnapshotStatus::Complete)
            .find(|o| ts(o).is_some_and(|t| new_ts - t >= 86_400));
        let Some(old) = old else { return Ok(0) };
        let old_ts = ts(&old).unwrap_or(new_ts);
        let limit_per_day = s.alert_growth_gib_per_day * GIB;

        let diff = match history::diff(&self.db, &self.own, &DiffQuery { old: old.id, new: new.id, limit: Some(100) }) {
            Ok(d) => d,
            Err(ApiError::BadRequest(_) | ApiError::Conflict(_)) => return Ok(0),
            Err(e) => return Err(format!("{e:?}")),
        };
        // Folders that grew, and brand-new folders that arrived with a lot of data.
        let mut fast: Vec<(&history::DirChange, f64)> = diff
            .growing
            .iter()
            .chain(diff.appeared.iter())
            .filter_map(|d| growth_rate_per_day(d.old_alloc, d.new_alloc, old_ts, new_ts).map(|r| (d, r)))
            .filter(|(_, r)| *r > limit_per_day)
            .collect();
        fast.sort_by(|a, b| b.1.total_cmp(&a.1));
        fast.truncate(MAX_GROWTH_ALERTS_PER_ROOT);

        let cooldown = i64::from(s.alert_cooldown_hours) * 3600;
        let mut changed = 0;
        let mut seen = std::collections::HashSet::new();
        let days = (new_ts - old_ts) as f64 / 86_400.0;
        for (d, rate) in &fast {
            let key = format!("growth:{}", d.path.to_lowercase());
            seen.insert(key.clone());
            let action = decide(true, false, Self::state(conn, &key), now, cooldown);
            let grew = d.new_alloc as f64 - d.old_alloc as f64;
            changed += self
                .apply(conn, &key, action, *rate, now, || NewAlert {
                    kind: "growth",
                    severity: "warning",
                    volume_id: new.volume_id.clone(),
                    drive: d.path.get(..2).map(str::to_string),
                    path: Some(d.path.clone()),
                    title: format!("{} is growing fast", d.path),
                    message: format!(
                        "{} grew by {} in {days:.1} days: {}/day, over the {} GiB/day limit.",
                        d.path,
                        gib(grew),
                        gib(*rate),
                        s.alert_growth_gib_per_day
                    ),
                    threshold: limit_per_day,
                    detail: Some(serde_json::json!({ "old": old.id, "new": new.id }).to_string()),
                })
                .map_err(|e| e.to_string())?;
        }
        // Alerts for folders of this root that are no longer growing past the limit are over.
        let prefix = format!("growth:{}", new.root.to_lowercase());
        let mut stmt = conn
            .prepare("SELECT alert_key FROM alert_state WHERE active = 1 AND alert_key LIKE ?1 ESCAPE '\\'")
            .map_err(|e| e.to_string())?;
        let pattern = format!("{}%", prefix.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let open: Vec<String> = stmt
            .query_map([pattern], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        for key in open.into_iter().filter(|k| !seen.contains(k)) {
            let action = decide(false, true, Self::state(conn, &key), now, cooldown);
            changed += self.apply(conn, &key, action, 0.0, now, unreachable_alert).map_err(|e| e.to_string())?;
        }
        Ok(changed)
    }

    // -----------------------------------------------------------------------
    // Reading and dismissing
    // -----------------------------------------------------------------------

    pub fn list(&self, q: &AlertQuery) -> Result<AlertsResponse, ApiError> {
        let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
        let status = q.status.as_deref().unwrap_or("active");
        let filter = match status {
            "active" => "WHERE resolved_at IS NULL",
            "resolved" => "WHERE resolved_at IS NOT NULL",
            "all" => "",
            other => return Err(ApiError::BadRequest(format!("unknown status {other}"))),
        };
        let mut stmt = conn
            .prepare(&format!(
                "SELECT id, kind, severity, drive, volume_id, path, title, message, value, threshold, detail, raised_at, resolved_at, dismissed_at
                 FROM alerts {filter} ORDER BY raised_at DESC, id DESC LIMIT 200"
            ))
            .map_err(ApiError::internal)?;
        let alerts: Vec<AlertView> = stmt
            .query_map([], |r| {
                let detail: Option<String> = r.get(10)?;
                let history = detail
                    .and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok())
                    .and_then(|v| Some(HistoryLink { old: v.get("old")?.as_i64()?, new: v.get("new")?.as_i64()? }));
                Ok(AlertView {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    severity: r.get(2)?,
                    drive: r.get(3)?,
                    volume_id: r.get(4)?,
                    path: r.get(5)?,
                    title: r.get(6)?,
                    message: r.get(7)?,
                    value: r.get(8)?,
                    threshold: r.get(9)?,
                    raised_at: rfc3339(r.get(11)?),
                    resolved_at: r.get::<_, Option<i64>>(12)?.map(rfc3339),
                    dismissed: r.get::<_, Option<i64>>(13)?.is_some(),
                    history,
                })
            })
            .and_then(|rows| rows.collect())
            .map_err(ApiError::internal)?;
        let s = self.settings(&conn);
        let (active, banners) = counts(&conn);
        Ok(AlertsResponse {
            alerts,
            active,
            banners,
            thresholds: AlertThresholds {
                low_percent: s.low_space_percent,
                low_floor_gib: s.low_space_floor_gib,
                growth_gib_per_day: s.alert_growth_gib_per_day,
                cooldown_hours: s.alert_cooldown_hours,
            },
            running_since: rfc3339(self.started_at),
        })
    }

    /// Hide an alert's banner until it is raised again.
    pub fn dismiss(&self, id: i64) -> Result<AlertView, ApiError> {
        let conn = self.db.connect().map_err(ApiError::internal)?;
        let now = crate::time::unix_now();
        let key: Option<String> = conn
            .query_row("SELECT alert_key FROM alerts WHERE id = ?1", [id], |r| r.get(0))
            .optional()
            .map_err(ApiError::internal)?;
        let Some(key) = key else { return Err(ApiError::NotFound(format!("alert {id} not found"))) };
        conn.execute("UPDATE alerts SET dismissed_at = ?2 WHERE id = ?1", params![id, now])
            .map_err(ApiError::internal)?;
        conn.execute("UPDATE alert_state SET dismissed_at = ?2 WHERE alert_key = ?1", params![key, now])
            .map_err(ApiError::internal)?;
        let (active, banners) = counts(&conn);
        self.events.publish("alert.changed", serde_json::json!({ "active": active, "banners": banners }));
        let all = self.list(&AlertQuery { status: Some("all".into()) })?;
        all.alerts.into_iter().find(|a| a.id == id).ok_or_else(|| ApiError::NotFound(format!("alert {id} not found")))
    }
}

fn counts(conn: &Connection) -> (u32, u32) {
    conn.query_row(
        "SELECT count(*), coalesce(sum(dismissed_at IS NULL), 0) FROM alerts WHERE resolved_at IS NULL",
        [],
        |r| Ok((r.get::<_, i64>(0)? as u32, r.get::<_, i64>(1)? as u32)),
    )
    .unwrap_or((0, 0))
}

struct NewAlert {
    kind: &'static str,
    severity: &'static str,
    volume_id: Option<String>,
    drive: Option<String>,
    path: Option<String>,
    title: String,
    message: String,
    threshold: f64,
    detail: Option<String>,
}

/// `Clear` never builds a new alert; this only satisfies the closure's type.
fn unreachable_alert() -> NewAlert {
    NewAlert {
        kind: "growth",
        severity: "warning",
        volume_id: None,
        drive: None,
        path: None,
        title: String::new(),
        message: String::new(),
        threshold: 0.0,
        detail: None,
    }
}
