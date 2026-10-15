//! Storage tiering suggestions: cold data on SSDs, hot data and game libraries
//! on HDDs, where they would fit, and projected free space. Suggestions only.
//! The only files read besides the scan snapshots are launcher manifests
//! (Steam, Epic), read-only. Move instructions are text; nothing is executed.

use std::collections::HashSet;
use std::sync::Arc;

use diskdash_analysis::plans::{lookup_entry, TemplateCtx, Templates};
use diskdash_analysis::rules::Env;
use diskdash_analysis::stale::{self, StaleQuery};
use diskdash_analysis::tiering::{
    assign, epic_apps, hot_folders, project, steam_apps, DriveState, HotParams, Launcher, LibraryApp, Limits,
    ManifestSource, Need, Tier,
};
use diskdash_inventory::apps::read_installed;
use diskdash_inventory::apps::registry::{Hive, RegValue, Registry, View};
use diskdash_inventory::model::{Drive, MediaClass};
use diskdash_store::{Db, OwnFiles};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ApiError;
use crate::inventory::InventoryService;
use crate::settings::{self, Settings};
use crate::snapshots::{self, Snapshot};
use crate::time::now_rfc3339;

const MAX_PER_DRIVE: u32 = 100;
const DAYS_PER_MONTH: f64 = 30.4375;

/// Manifests read from the real disk (read-only).
pub struct DiskManifests;

impl ManifestSource for DiskManifests {
    fn read_to_string(&self, path: &str) -> std::io::Result<String> {
        std::fs::read_to_string(diskdash_core::paths::to_extended(path))
    }

    fn list_files(&self, dir: &str, prefix: &str, ext: &str) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(diskdash_core::paths::to_extended(dir)) else { return Vec::new() };
        let mut out: Vec<String> = rd
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().to_lowercase();
                n.starts_with(prefix) && n.ends_with(ext)
            })
            .map(|e| diskdash_core::paths::from_extended(&e.path().to_string_lossy()))
            .collect();
        out.sort();
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Direction {
    /// Cold data on an SSD, to an HDD.
    ColdToHdd,
    /// Hot data or a game library on an HDD, to an SSD.
    HotToSsd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum TierKind {
    /// Plain data: copy it with robocopy, check, then delete the original.
    Data,
    /// A Steam or Epic game: use the launcher's own move feature.
    AppLibrary,
    /// The install folder of a registered program: reinstall it instead.
    AppInstall,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TierTarget {
    pub drive_id: String,
    pub letter: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TierSuggestion {
    /// Stable key: the lower-case source path.
    pub key: String,
    pub direction: Direction,
    pub kind: TierKind,
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    #[ts(type = "number")]
    pub snapshot_id: i64,
    #[ts(type = "number")]
    pub entry_id: i64,
    #[ts(type = "number")]
    pub allocated: u64,
    pub source_drive: String,
    /// Why it is suggested, in plain language.
    pub reason: String,
    /// Game name or program name, for app libraries and installs.
    pub app: Option<String>,
    pub target: Option<TierTarget>,
    /// `no_target_with_space` or `no_target_tier` when there is no target.
    pub no_target_reason: Option<String>,
    /// Where the user could put it (the same path on the target drive).
    pub destination: Option<String>,
    pub planned: bool,
    pub instructions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TierDrive {
    pub id: String,
    pub letter: String,
    pub media: MediaClass,
    #[ts(type = "number")]
    pub capacity: u64,
    #[ts(type = "number")]
    pub free: u64,
    /// Free space after the moves marked as planned.
    #[ts(type = "number")]
    pub projected_free: u64,
    /// Below the low-space rule after the planned moves.
    pub low_after: bool,
    pub scanned: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TierNote {
    /// no_hdd_tier | no_ssd_tier | unscanned | nothing_found
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TierThresholds {
    pub cold_months: u32,
    #[ts(type = "number")]
    pub min_gib: u32,
    pub hot_days: u32,
    pub low_percent: f64,
    pub low_floor_gib: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TieringResponse {
    pub suggestions: Vec<TierSuggestion>,
    pub drives: Vec<TierDrive>,
    pub notes: Vec<TierNote>,
    pub thresholds: TierThresholds,
    /// When the oldest scan used here finished: the numbers are as old as this.
    pub as_of: Option<String>,
    /// Launcher manifests that could not be read (skipped).
    pub manifest_errors: u32,
}

#[derive(Debug, Deserialize)]
pub struct PlannedRequest {
    pub path: String,
    pub planned: bool,
}

pub struct TieringService {
    pub db: Db,
    pub own: OwnFiles,
    pub inventory: Arc<InventoryService>,
    pub env: Arc<dyn Env>,
    pub manifests: Arc<dyn ManifestSource>,
    pub registry: Arc<dyn Registry + Send + Sync>,
    pub templates: Templates,
}

fn tier_of(d: &Drive) -> Option<Tier> {
    match d.media {
        MediaClass::SATA_SSD | MediaClass::NVME_SSD => Some(Tier::Ssd),
        MediaClass::HDD => Some(Tier::Hdd),
        MediaClass::UNKNOWN => None,
    }
}

fn drive_state(d: &Drive, tier: Tier) -> DriveState {
    DriveState {
        id: d.id.clone(),
        letter: d.letters.first().cloned().unwrap_or_default().to_uppercase(),
        tier,
        disks: if d.physical_disks.is_empty() {
            vec![format!("volume-{}", d.id)]
        } else {
            d.physical_disks.iter().map(|p| format!("disk-{}", p.number)).collect()
        },
        capacity: d.capacity,
        free: d.free,
    }
}

fn age_text(days: u64, basis_atime: bool) -> String {
    let what = if basis_atime { "opened" } else { "modified" };
    if days >= 730 {
        format!("not {what} for {:.1} years", days as f64 / 365.0)
    } else {
        format!("not {what} for {} months", (days as f64 / DAYS_PER_MONTH).round() as u64)
    }
}

fn days_ago(now: i64, t: i64) -> u64 {
    ((now - t).max(0) / 86_400) as u64
}

fn under(path: &str, base: &str) -> bool {
    let (p, b) = (path.to_lowercase(), base.trim_end_matches('\\').to_lowercase());
    p == b || p.starts_with(&format!("{b}\\"))
}

struct Raw {
    direction: Direction,
    kind: TierKind,
    path: String,
    name: String,
    is_dir: bool,
    snapshot_id: i64,
    entry_id: i64,
    alloc: u64,
    source_drive: String,
    reason: String,
    app: Option<String>,
    launcher: Option<Launcher>,
}

impl TieringService {
    fn steam_root(&self) -> Option<String> {
        let reg = self.registry.values(Hive::Hkcu, View::V64, r"Software\Valve\Steam").ok();
        let from_reg =
            reg.as_ref().and_then(|v| v.get("SteamPath")).and_then(RegValue::as_str).map(|s| s.replace('/', "\\"));
        from_reg.or_else(|| self.env.var("ProgramFiles(x86)").map(|p| format!("{}\\Steam", p.trim_end_matches('\\'))))
    }

    fn planned_keys(&self, conn: &Connection) -> HashSet<String> {
        conn.prepare("SELECT path_key FROM tiering_planned")
            .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0))?.collect())
            .unwrap_or_default()
    }

    pub fn suggestions(&self) -> Result<TieringResponse, ApiError> {
        let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
        let settings = settings::load(&conn).map_err(ApiError::internal)?;
        let protected = crate::protection::build(self.env.as_ref(), &settings);
        let all_drives = self.inventory.snapshot(&settings).drives;
        let now = crate::cleanup::unix_now();
        let planned = self.planned_keys(&conn);

        // Drives by tier (internal fixed drives only: the inventory already excludes the rest).
        let mut states: Vec<DriveState> = Vec::new();
        for d in &all_drives {
            if let (Some(t), false) = (tier_of(d), d.letters.is_empty()) {
                states.push(drive_state(d, t));
            }
        }
        let latest = snapshots::latest(&conn).map_err(ApiError::internal)?;
        let snap_for = |letter: &str| -> Option<&Snapshot> {
            latest.iter().find(|s| {
                crate::cleanup::is_drive_root(&s.root) && s.root.to_uppercase().starts_with(&format!("{letter}:"))
            })
        };
        let mut notes = Vec::new();
        let (has_ssd, has_hdd) =
            (states.iter().any(|d| d.tier == Tier::Ssd), states.iter().any(|d| d.tier == Tier::Hdd));
        if !has_hdd {
            notes.push(TierNote { code: "no_hdd_tier".into(), message: "No internal hard disk (HDD) was detected, so there is nowhere to move cold data to. USB and network drives are not used.".into() });
        }
        if !has_ssd {
            notes.push(TierNote {
                code: "no_ssd_tier".into(),
                message: "No internal SSD was detected, so there is nowhere faster to move hot data to.".into(),
            });
        }

        let min_alloc = u64::from(settings.tiering_min_gib) << 30;
        let mut raws: Vec<Raw> = Vec::new();
        let mut unscanned = Vec::new();
        let mut oldest: Option<String> = None;
        let mut manifest_errors: Vec<String> = Vec::new();
        let installs = read_installed(self.registry.as_ref()).map(|r| r.apps).unwrap_or_default();

        // Game libraries (read once; matched to scans below).
        let mut libs: Vec<LibraryApp> = Vec::new();
        if has_hdd {
            if let Some(root) = self.steam_root() {
                libs.extend(steam_apps(self.manifests.as_ref(), &root, &mut manifest_errors));
            }
            if let Some(pd) = self.env.var("ProgramData") {
                libs.extend(epic_apps(
                    self.manifests.as_ref(),
                    &format!("{}\\Epic\\EpicGamesLauncher\\Data\\Manifests", pd.trim_end_matches('\\')),
                    &mut manifest_errors,
                ));
            }
        }
        for e in &manifest_errors {
            tracing::warn!(error = %e, "launcher manifest skipped");
        }

        for st in &states {
            let Some(snap) = snap_for(&st.letter) else {
                unscanned.push(format!("{}:", st.letter));
                continue;
            };
            if oldest.as_ref().is_none_or(|o| snap.finished_at.as_ref().is_some_and(|f| f < o)) {
                oldest = snap.finished_at.clone().or(oldest);
            }
            let sconn = snapshots::open_snapshot(&self.own, snap)?;
            match st.tier {
                Tier::Ssd if has_hdd => {
                    let q = StaleQuery {
                        basis: snap.time_basis,
                        cutoff: now - (f64::from(settings.tiering_cold_months) * DAYS_PER_MONTH * 86_400.0) as i64,
                        now,
                        offset: 0,
                        limit: MAX_PER_DRIVE,
                        protected: &protected,
                        min_alloc,
                    };
                    let page = stale::stale(&sconn, &q).map_err(ApiError::internal)?;
                    for r in page.rows {
                        let name = r.path.rsplit('\\').next().unwrap_or(&r.path).to_string();
                        raws.push(Raw {
                            direction: Direction::ColdToHdd,
                            kind: TierKind::Data,
                            reason: age_text(r.age_days, snap.time_basis == stale::TimeBasis::Atime),
                            path: r.path,
                            name,
                            is_dir: r.is_dir,
                            snapshot_id: snap.id,
                            entry_id: r.id,
                            alloc: r.alloc,
                            source_drive: st.id.clone(),
                            app: None,
                            launcher: None,
                        });
                    }
                }
                Tier::Hdd if has_ssd => {
                    // Game libraries on this drive.
                    for app in libs.iter().filter(|a| a.path.to_uppercase().starts_with(&format!("{}:", st.letter))) {
                        let Ok(Some(e)) = lookup_entry(&sconn, &snap.root, &app.path) else { continue };
                        if e.alloc < min_alloc || protected.is_protected(&app.path) {
                            continue;
                        }
                        let active = app.last_active.or(e.newest_mtime);
                        let Some(active) = active.filter(|t| now - t <= i64::from(settings.tiering_hot_days) * 86_400)
                        else {
                            continue;
                        };
                        let ago = days_ago(now, active);
                        let (verb, launcher) = match app.launcher {
                            Launcher::Steam => ("Steam game last played", "Steam"),
                            Launcher::Epic => ("Epic game, files changed", "Epic"),
                        };
                        raws.push(Raw {
                            direction: Direction::HotToSsd,
                            kind: TierKind::AppLibrary,
                            reason: format!(
                                "{verb} {} (app library, {launcher})",
                                if ago == 0 { "today".to_string() } else { format!("{ago} days ago") }
                            ),
                            path: app.path.clone(),
                            name: app.name.clone(),
                            is_dir: true,
                            snapshot_id: snap.id,
                            entry_id: e.id,
                            alloc: e.alloc,
                            source_drive: st.id.clone(),
                            app: Some(app.name.clone()),
                            launcher: Some(app.launcher),
                        });
                    }
                    let hp = HotParams { days: settings.tiering_hot_days, min_alloc, ..HotParams::default() };
                    for h in hot_folders(&sconn, now, &hp, &protected).map_err(ApiError::internal)? {
                        if libs.iter().any(|a| under(&h.path, &a.path) || under(&a.path, &h.path)) {
                            continue;
                        }
                        let name = h.path.rsplit('\\').next().unwrap_or(&h.path).to_string();
                        raws.push(Raw {
                            direction: Direction::HotToSsd,
                            kind: TierKind::Data,
                            reason: h.reason,
                            path: h.path,
                            name,
                            is_dir: true,
                            snapshot_id: snap.id,
                            entry_id: h.entry_id,
                            alloc: h.alloc,
                            source_drive: st.id.clone(),
                            app: None,
                            launcher: None,
                        });
                    }
                }
                _ => {}
            }
        }
        if !unscanned.is_empty() {
            notes.push(TierNote {
                code: "unscanned".into(),
                message: format!(
                    "Not scanned yet: {}. Suggestions come from a finished scan of each drive.",
                    unscanned.join(", ")
                ),
            });
        }

        // Registered programs are moved by reinstalling, not by hand.
        for r in &mut raws {
            if r.kind == TierKind::Data {
                if let Some(app) = installs
                    .iter()
                    .find(|a| a.install_location.as_deref().is_some_and(|l| under(&r.path, l) || under(l, &r.path)))
                {
                    r.kind = TierKind::AppInstall;
                    r.app = Some(app.name.clone());
                    r.reason = format!("{} (install folder of {})", r.reason, app.name);
                }
            }
        }
        raws.sort_by(|a, b| b.alloc.cmp(&a.alloc).then(a.path.cmp(&b.path)));
        raws.dedup_by(|a, b| a.path.eq_ignore_ascii_case(&b.path));

        let limits = Limits {
            low_percent: settings.low_space_percent,
            low_floor: u64::from(settings.low_space_floor_gib) << 30,
            headroom_percent: 10.0,
            headroom_floor: 20 << 30,
        };
        let needs: Vec<Need> = raws
            .iter()
            .map(|r| Need {
                key: r.path.to_lowercase(),
                bytes: r.alloc,
                source_drive: r.source_drive.clone(),
                target_tier: if r.direction == Direction::ColdToHdd { Tier::Hdd } else { Tier::Ssd },
                planned: planned.contains(&r.path.to_lowercase()),
            })
            .collect();
        let assignments = assign(&states, &needs, &limits);

        let mut suggestions = Vec::new();
        let mut moves: Vec<(String, String, u64)> = Vec::new();
        for (r, a) in raws.into_iter().zip(assignments) {
            let key = r.path.to_lowercase();
            let is_planned = planned.contains(&key);
            let target = a.target.as_ref().and_then(|id| states.iter().find(|d| &d.id == id));
            let destination = target.map(|t| format!("{}:\\{}", t.letter, r.path.get(3..).unwrap_or(&r.name)));
            if let (true, Some(t)) = (is_planned, target) {
                moves.push((r.source_drive.clone(), t.id.clone(), r.alloc));
            }
            let ctx = TemplateCtx { path: r.path.clone(), destination: destination.clone() };
            let template = match (r.kind, r.launcher) {
                (TierKind::AppLibrary, Some(Launcher::Epic)) => "tier-epic",
                (TierKind::AppLibrary, _) => "tier-steam",
                (TierKind::AppInstall, _) => "tier-app-install",
                (TierKind::Data, _) if r.is_dir => "manual-move-dir",
                (TierKind::Data, _) => "manual-move-file",
            };
            suggestions.push(TierSuggestion {
                key,
                direction: r.direction,
                kind: r.kind,
                name: r.name,
                is_dir: r.is_dir,
                snapshot_id: r.snapshot_id,
                entry_id: r.entry_id,
                allocated: r.alloc,
                source_drive: r.source_drive,
                reason: r.reason,
                app: r.app,
                target: target.map(|t| TierTarget { drive_id: t.id.clone(), letter: t.letter.clone() }),
                no_target_reason: a.reason.map(str::to_string),
                destination,
                planned: is_planned,
                instructions: self.templates.render(template, &ctx),
                path: r.path,
            });
        }
        if suggestions.is_empty() && notes.iter().all(|n| n.code == "unscanned") {
            notes.push(TierNote { code: "nothing_found".into(), message: "Nothing on the SSDs is cold enough, and nothing on the HDDs is busy enough, for the current thresholds.".into() });
        }

        let projections = project(&states, &moves, &limits);
        let drives = states
            .iter()
            .zip(&projections)
            .filter_map(|(s, p)| {
                let d = all_drives.iter().find(|d| d.id == s.id)?;
                Some(TierDrive {
                    id: s.id.clone(),
                    letter: s.letter.clone(),
                    media: d.media,
                    capacity: s.capacity,
                    free: p.free,
                    projected_free: p.projected_free,
                    low_after: p.low_after,
                    scanned: snap_for(&s.letter).is_some(),
                })
            })
            .collect();
        Ok(TieringResponse {
            suggestions,
            drives,
            notes,
            thresholds: TierThresholds {
                cold_months: settings.tiering_cold_months,
                min_gib: settings.tiering_min_gib,
                hot_days: settings.tiering_hot_days,
                low_percent: settings.low_space_percent,
                low_floor_gib: settings.low_space_floor_gib,
            },
            as_of: oldest,
            manifest_errors: manifest_errors.len() as u32,
        })
    }

    /// Mark a suggestion as planned (or not) and return the recomputed view.
    pub fn set_planned(&self, req: &PlannedRequest) -> Result<TieringResponse, ApiError> {
        let key = req.path.to_lowercase();
        if req.planned && !self.suggestions()?.suggestions.iter().any(|s| s.key == key) {
            return Err(ApiError::NotFound("that is not a current tiering suggestion".into()));
        }
        let conn = self.db.connect().map_err(ApiError::internal)?;
        if req.planned {
            conn.execute(
                "INSERT INTO tiering_planned (path_key, created_at) VALUES (?1, ?2) ON CONFLICT(path_key) DO NOTHING",
                params![key, now_rfc3339()],
            )
            .map_err(ApiError::internal)?;
        } else {
            conn.execute("DELETE FROM tiering_planned WHERE path_key = ?1", [key]).map_err(ApiError::internal)?;
        }
        self.suggestions()
    }
}

/// Re-exported for settings validation tests.
pub fn default_limits(s: &Settings) -> Limits {
    Limits {
        low_percent: s.low_space_percent,
        low_floor: u64::from(s.low_space_floor_gib) << 30,
        headroom_percent: 10.0,
        headroom_floor: 20 << 30,
    }
}
