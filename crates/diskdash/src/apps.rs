//! Installed applications: registry inventory with size and best-effort
//! last-used time. Read-only. Uninstalling is left to Windows (the UI links to
//! Apps & features); `UninstallString` is never read or run here.

use std::collections::HashMap;
use std::sync::Arc;

use diskdash_core::paths;
use diskdash_inventory::apps::registry::Registry;
use diskdash_inventory::apps::userassist::{self, Launch};
use diskdash_inventory::apps::{self, AppEntry, AppSource};
use diskdash_store::{Db, OwnFiles};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use ts_rs::TS;

use crate::error::ApiError;
use crate::snapshots;

/// Prefetch last-run times, available only through the read-only elevated
/// helper (a later change). The server itself never reads `C:\Windows\Prefetch`.
pub trait PrefetchSource: Send + Sync {
    /// Program file name (lower case) to last run, unix seconds; `None` when the helper is not running.
    fn last_runs(&self) -> Option<HashMap<String, i64>>;
}

/// No helper: Prefetch is not available.
pub struct NoHelper;

impl PrefetchSource for NoHelper {
    fn last_runs(&self) -> Option<HashMap<String, i64>> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SizeSource {
    /// `EstimatedSize` from the registry.
    Estimated,
    /// The install folder in a scan.
    Scan,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LastUsedSource {
    Prefetch,
    Userassist,
    Unknown,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AppView {
    pub name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub install_date: Option<String>,
    pub install_location: Option<String>,
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    pub size_source: SizeSource,
    pub last_used: Option<String>,
    pub last_used_source: LastUsedSource,
    pub sources: Vec<AppSource>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AppsResponse {
    pub apps: Vec<AppView>,
    /// Registry entries that could not be read.
    pub skipped: u32,
    /// Whether Prefetch data (helper) contributed to last-used times.
    pub prefetch_available: bool,
}

pub struct AppsService {
    pub db: Db,
    pub own: OwnFiles,
    pub registry: Arc<dyn Registry + Send + Sync>,
    pub prefetch: Arc<dyn PrefetchSource>,
}

/// Allocated size of the folder `path` in a snapshot (walks names, no full scan).
fn dir_alloc(conn: &Connection, root: &str, path: &str) -> Option<u64> {
    let root_trim = root.trim_end_matches('\\');
    let rel = if path.eq_ignore_ascii_case(root_trim) {
        ""
    } else {
        let lower = path.to_lowercase();
        let prefix = format!("{}\\", root_trim.to_lowercase());
        lower.starts_with(&prefix).then(|| &path[prefix.len()..])?
    };
    let mut id = 0i64;
    let mut alloc: i64 = conn.query_row("SELECT alloc FROM entries WHERE id = 0", [], |r| r.get(0)).ok()?;
    for seg in rel.split('\\').filter(|s| !s.is_empty()) {
        let (next, a, kind): (i64, i64, i64) = conn
            .query_row(
                "SELECT id, alloc, kind FROM entries WHERE parent_id = ?1 AND name_lower = ?2 AND id != 0 LIMIT 1",
                params![id, seg.to_lowercase()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .ok()??;
        if kind != 1 {
            return None;
        }
        (id, alloc) = (next, a);
    }
    Some(alloc.max(0) as u64)
}

fn view(
    app: &AppEntry,
    scans: &[(String, Connection)],
    launches: &[Launch],
    prefetch: Option<&HashMap<String, i64>>,
) -> AppView {
    let (size, size_source) = match (app.estimated_size, app.install_location.as_deref()) {
        (Some(s), _) => (Some(s), SizeSource::Estimated),
        (None, Some(loc)) => {
            let found = scans
                .iter()
                .filter(|(root, _)| paths::is_within(loc, root))
                .find_map(|(root, conn)| dir_alloc(conn, root, loc));
            match found {
                Some(s) => (Some(s), SizeSource::Scan),
                None => (None, SizeSource::Unknown),
            }
        }
        (None, None) => (None, SizeSource::Unknown),
    };
    let from_prefetch = prefetch
        .and_then(|m| app.display_icon.as_deref().and_then(apps::exe_name).and_then(|exe| m.get(&exe).copied()));
    let (last, last_source) = match (from_prefetch, apps::last_launch(app, launches)) {
        (Some(t), _) => (Some(t), LastUsedSource::Prefetch),
        (None, Some(t)) => (Some(t), LastUsedSource::Userassist),
        (None, None) => (None, LastUsedSource::Unknown),
    };
    AppView {
        name: app.name.clone(),
        publisher: app.publisher.clone(),
        version: app.version.clone(),
        install_date: app.install_date.clone(),
        install_location: app.install_location.clone(),
        size,
        size_source,
        last_used: snapshots::rfc3339(last),
        last_used_source: last_source,
        sources: app.sources.clone(),
    }
}

impl AppsService {
    pub fn list(&self) -> Result<AppsResponse, ApiError> {
        let installed = apps::read_installed(self.registry.as_ref())
            .map_err(|e| ApiError::Internal(format!("cannot read the installed applications: {e}")))?;
        let launches = userassist::read(self.registry.as_ref());
        let prefetch = self.prefetch.last_runs();

        // Latest complete scans, opened once, for install-folder sizes.
        let cat = self.db.connect_read_only().map_err(ApiError::internal)?;
        let mut scans: Vec<(String, Connection)> = Vec::new();
        for snap in snapshots::latest(&cat).map_err(ApiError::internal)? {
            if let Ok(conn) = snapshots::open_snapshot(&self.own, &snap) {
                scans.push((snap.root.clone(), conn));
            }
        }
        let apps = installed.apps.iter().map(|a| view(a, &scans, &launches, prefetch.as_ref())).collect();
        Ok(AppsResponse { apps, skipped: installed.skipped, prefetch_available: prefetch.is_some() })
    }
}
