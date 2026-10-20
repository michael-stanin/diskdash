//! Typed user settings with bounds. Stored one row per key (JSON value) so new
//! settings get their defaults automatically. Nothing here can enable file
//! changes, network drives or removable drives.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::time::now_rfc3339;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum HardlinkDefault {
    /// Detect hard links only on the volume that holds `%WINDIR%` (default).
    SystemVolume,
    AllVolumes,
    NoVolumes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Settings {
    /// Flag a drive when free space is below this percentage of capacity.
    pub low_space_percent: f64,
    /// ...or below this many GiB.
    pub low_space_floor_gib: u32,
    /// Default age for "stale" data, in months.
    pub stale_months: u32,
    /// Traversal threads per HDD (spindle).
    pub hdd_scan_concurrency: u32,
    /// Traversal threads per SSD.
    pub ssd_scan_concurrency: u32,
    /// Treemap / sunburst children per level before grouping into "Other".
    pub chart_children_per_level: u32,
    pub hardlink_detection: HardlinkDefault,
    /// Per-volume overrides, keyed by volume GUID path.
    pub hardlink_overrides: BTreeMap<String, bool>,
    /// Tiering: data on an SSD untouched for this many months is cold.
    pub tiering_cold_months: u32,
    /// Tiering: smallest folder or file suggested for a move, in GiB.
    pub tiering_min_gib: u32,
    /// Tiering: data on an HDD modified within this many days is hot.
    pub tiering_hot_days: u32,
    /// History: full snapshots kept per scanned folder; older ones shrink to directory totals.
    pub retention_keep: u32,
    /// History: folder depth kept in a shrunk snapshot.
    pub retention_depth: u32,
    /// Folders that are never suggested by any analysis (Settings > Exclusions).
    pub excluded_paths: Vec<String>,
    /// Folders that are never suggested and never added to a plan, on top of the built-in list.
    pub protected_paths: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            low_space_percent: 10.0,
            low_space_floor_gib: 20,
            stale_months: 12,
            hdd_scan_concurrency: 1,
            ssd_scan_concurrency: 4,
            chart_children_per_level: 20,
            hardlink_detection: HardlinkDefault::SystemVolume,
            hardlink_overrides: BTreeMap::new(),
            tiering_cold_months: 12,
            tiering_min_gib: 1,
            tiering_hot_days: 30,
            retention_keep: 5,
            retention_depth: 4,
            excluded_paths: Vec::new(),
            protected_paths: Vec::new(),
        }
    }
}

impl Settings {
    /// Field-level validation errors (empty when valid).
    pub fn validate(&self) -> BTreeMap<String, String> {
        let mut e = BTreeMap::new();
        let mut range = |name: &str, ok: bool, msg: &str| {
            if !ok {
                e.insert(name.to_string(), msg.to_string());
            }
        };
        range(
            "low_space_percent",
            self.low_space_percent.is_finite() && (1.0..=50.0).contains(&self.low_space_percent),
            "must be between 1 and 50 %",
        );
        range("low_space_floor_gib", self.low_space_floor_gib <= 1024, "must be between 0 and 1024 GiB");
        range("stale_months", (1..=120).contains(&self.stale_months), "must be between 1 and 120 months");
        range("tiering_cold_months", (1..=120).contains(&self.tiering_cold_months), "must be between 1 and 120 months");
        range("tiering_min_gib", (1..=10_000).contains(&self.tiering_min_gib), "must be between 1 and 10000 GiB");
        range("tiering_hot_days", (1..=365).contains(&self.tiering_hot_days), "must be between 1 and 365 days");
        range("retention_keep", (2..=50).contains(&self.retention_keep), "must be between 2 and 50 scans");
        range("retention_depth", (1..=8).contains(&self.retention_depth), "must be between 1 and 8 levels");
        range("hdd_scan_concurrency", (1..=2).contains(&self.hdd_scan_concurrency), "must be 1 or 2");
        range("ssd_scan_concurrency", (1..=16).contains(&self.ssd_scan_concurrency), "must be between 1 and 16");
        range(
            "chart_children_per_level",
            (5..=20).contains(&self.chart_children_per_level),
            "must be between 5 and 20",
        );
        let bad_override = self.hardlink_overrides.keys().any(|k| !k.starts_with(r"\\?\Volume{"));
        range("hardlink_overrides", !bad_override, "keys must be volume GUID paths");
        for (name, list) in [("excluded_paths", &self.excluded_paths), ("protected_paths", &self.protected_paths)] {
            let bad =
                list.iter().position(|p| diskdash_analysis::rules::expand::normalize(p).is_none() || p.len() > 400);
            range(name, list.len() <= 200, "at most 200 folders");
            range(name, bad.is_none(), "every entry must be a full folder path such as D:\\Photos");
        }
        e
    }

    fn to_rows(&self) -> Result<BTreeMap<String, String>, serde_json::Error> {
        let value = serde_json::to_value(self)?;
        let mut rows = BTreeMap::new();
        if let serde_json::Value::Object(map) = value {
            for (k, v) in map {
                rows.insert(k, v.to_string());
            }
        }
        Ok(rows)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Load settings: defaults overlaid with stored keys. Unknown or invalid stored
/// keys are ignored (logged), so a bad row never breaks startup.
pub fn load(conn: &Connection) -> Result<Settings, SettingsError> {
    let defaults = Settings::default();
    let mut merged = match serde_json::to_value(&defaults)? {
        serde_json::Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    let known: BTreeSet<String> = merged.keys().cloned().collect();
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (key, raw) = row?;
        if !known.contains(&key) {
            continue;
        }
        match serde_json::from_str(&raw) {
            Ok(v) => {
                merged.insert(key, v);
            }
            Err(e) => tracing::warn!(key, error = %e, "ignoring unreadable stored setting"),
        }
    }
    match serde_json::from_value::<Settings>(serde_json::Value::Object(merged)) {
        Ok(s) if s.validate().is_empty() => Ok(s),
        Ok(_) | Err(_) => {
            tracing::warn!("stored settings are invalid; using defaults");
            Ok(defaults)
        }
    }
}

/// Persist settings (caller validates first).
pub fn save(conn: &mut Connection, settings: &Settings) -> Result<(), SettingsError> {
    let tx = conn.transaction()?;
    let now = now_rfc3339();
    for (key, value) in settings.to_rows()? {
        tx.execute(
            "INSERT INTO settings(key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            rusqlite::params![key, value, now],
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskdash_store::{migrate, MIGRATIONS};

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        migrate(&mut c, MIGRATIONS).unwrap();
        c
    }

    #[test]
    fn first_run_returns_documented_defaults() {
        let s = load(&db()).unwrap();
        assert_eq!(s.low_space_percent, 10.0);
        assert_eq!(s.low_space_floor_gib, 20);
        assert_eq!(s.stale_months, 12);
        assert_eq!(s.hdd_scan_concurrency, 1);
        assert_eq!(s.ssd_scan_concurrency, 4);
        assert_eq!(s.chart_children_per_level, 20);
        assert_eq!(s.hardlink_detection, HardlinkDefault::SystemVolume);
        assert!(s.validate().is_empty());
    }

    #[test]
    fn out_of_range_values_are_reported_per_field() {
        let s = Settings { low_space_percent: 150.0, chart_children_per_level: 30, ..Settings::default() };
        let e = s.validate();
        assert!(e.contains_key("low_space_percent"));
        assert!(e.contains_key("chart_children_per_level"));
        assert_eq!(e.len(), 2);
    }

    #[test]
    fn save_and_reload_round_trips() {
        let mut c = db();
        let mut s = Settings { stale_months: 24, ..Settings::default() };
        s.hardlink_overrides.insert(r"\\?\Volume{abc}\".into(), true);
        save(&mut c, &s).unwrap();
        assert_eq!(load(&c).unwrap(), s);
    }

    #[test]
    fn corrupt_stored_row_falls_back() {
        let c = db();
        c.execute("INSERT INTO settings VALUES ('stale_months', 'not json', 'x')", []).unwrap();
        c.execute("INSERT INTO settings VALUES ('unknown_key', '1', 'x')", []).unwrap();
        assert_eq!(load(&c).unwrap().stale_months, 12);
    }
}
