//! Builds a [`Report`] from what diskdash already stored. Read-only: it calls the
//! same query functions the pages use and never touches user files.

use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use diskdash_store::{Db, OwnFiles};
use serde::Serialize;

use super::model::*;
use crate::cleanup::{CandidateQuery, CleanupService};
use crate::duplicates::{DupService, DupStatus, GroupFilter};
use crate::error::ApiError;
use crate::history;
use crate::inventory::InventoryService;
use crate::plans::PlanService;
use crate::settings::{self, Settings};
use crate::snapshots::{self, Measure, Snapshot, TopQuery};
use crate::stale::{self, StaleParams};
use crate::tiering::TieringService;

pub struct ReportService {
    pub db: Db,
    pub own: OwnFiles,
    pub inventory: Arc<InventoryService>,
    pub cleanup: Arc<CleanupService>,
    pub dups: Arc<DupService>,
    pub plans: Arc<PlanService>,
    pub tiering: Arc<TieringService>,
}

struct Ctx {
    settings: Settings,
    drive: Option<String>,
    top_n: usize,
    now: i64,
}

/// The string a serde enum serialises to (`"safe"`, `"manual-cleanup"`).
fn label<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn rfc3339(secs: i64) -> String {
    humantime::format_rfc3339_seconds(UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64)).to_string()
}

fn cap<T>(mut v: Vec<T>) -> (Vec<T>, usize) {
    let total = v.len();
    v.truncate(ROW_CAP);
    (v, total)
}

impl Ctx {
    fn letter_of(&self, path_or_root: &str) -> bool {
        match &self.drive {
            None => true,
            Some(d) => {
                path_or_root.chars().next().is_some_and(|c| c.eq_ignore_ascii_case(&d.chars().next().unwrap_or('?')))
            }
        }
    }
}

impl ReportService {
    fn ctx(&self, req: &ReportRequest) -> Result<Ctx, ApiError> {
        let conn = self.db.connect_read_only().map_err(ApiError::internal)?;
        let settings = settings::load(&conn).map_err(ApiError::internal)?;
        let drive = match req.drive.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            None => None,
            Some(d) => {
                let letter = d.trim_end_matches([':', '\\']);
                if letter.len() != 1 || !letter.chars().all(|c| c.is_ascii_alphabetic()) {
                    return Err(ApiError::BadRequest("drive must be a single drive letter such as C".into()));
                }
                Some(letter.to_ascii_uppercase())
            }
        };
        let top_n = req.top_n.unwrap_or(DEFAULT_TOP_N);
        if !(1..=MAX_TOP_N).contains(&top_n) {
            return Err(ApiError::BadRequest(format!("top_n must be between 1 and {MAX_TOP_N}")));
        }
        Ok(Ctx { settings, drive, top_n: top_n as usize, now: crate::time::unix_now() })
    }

    /// Which sections have data right now, and why not when they do not.
    pub fn availability(&self, drive: Option<&str>) -> Result<Vec<SectionInfo>, ApiError> {
        let req = ReportRequest {
            format: Format::Json,
            sections: None,
            drive: drive.map(str::to_string),
            top_n: Some(1),
            folder: None,
        };
        let ctx = self.ctx(&req)?;
        Ok(SectionId::ALL
            .iter()
            .map(|id| {
                let (available, reason) = match self.section_status(*id, &ctx) {
                    Ok(()) => (true, None),
                    Err(r) => (false, Some(r)),
                };
                SectionInfo { id: *id, title: id.title().into(), available, reason }
            })
            .collect())
    }

    fn section_status(&self, id: SectionId, ctx: &Ctx) -> Result<(), String> {
        let ok = |s_ok: bool, reason: &str| if s_ok { Ok(()) } else { Err(reason.to_string()) };
        match id {
            SectionId::Drives => {
                ok(!self.inventory.snapshot(&ctx.settings).drives.is_empty(), "The drive list has not loaded yet.")
            }
            SectionId::Top | SectionId::Stale => ok(!self.scanned(ctx).is_empty(), "No drive has been scanned yet."),
            SectionId::Cleanup => ok(
                self.cleanup
                    .overview()
                    .is_ok_and(|o| o.drives.iter().any(|d| ctx.letter_of(&d.root) && d.cleanup.run.is_some())),
                "No scan has been checked against the cleanup rules yet.",
            ),
            SectionId::Duplicates => ok(
                self.dups.status().ok().flatten().is_some_and(|r| r.status == DupStatus::Complete),
                "No duplicate analysis has completed yet.",
            ),
            SectionId::Tiering => ok(
                self.tiering.suggestions().is_ok_and(|t| t.drives.iter().any(|d| d.scanned)),
                "Tiering needs scans of the drives it compares.",
            ),
            SectionId::Plans => ok(self.plans.list().is_ok_and(|p| !p.is_empty()), "There are no cleanup plans yet."),
            SectionId::Forecast => ok(
                history::volume_trends(&self.db, &ctx.settings, ctx.now)
                    .is_ok_and(|t| t.volumes.iter().any(|v| !v.samples.is_empty())),
                "No free-space history has been recorded yet.",
            ),
        }
    }

    fn scanned(&self, ctx: &Ctx) -> Vec<Snapshot> {
        let Ok(conn) = self.db.connect_read_only() else { return Vec::new() };
        snapshots::latest(&conn)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !s.summarised && ctx.letter_of(&s.root))
            .collect()
    }

    /// Assemble the selected sections.
    pub fn build(&self, req: &ReportRequest) -> Result<Report, ApiError> {
        let ctx = self.ctx(req)?;
        let wanted: Vec<SectionId> = req.sections.clone().unwrap_or_else(|| SectionId::ALL.to_vec());
        let mut s = Sections::default();
        for id in SectionId::ALL.iter().filter(|i| wanted.contains(i)) {
            match id {
                SectionId::Drives => s.drives = Some(self.drives(&ctx)),
                SectionId::Top => s.top = Some(self.top(&ctx)),
                SectionId::Cleanup => s.cleanup = Some(self.cleanup_section(&ctx)),
                SectionId::Duplicates => s.duplicates = Some(self.duplicates(&ctx)),
                SectionId::Stale => s.stale = Some(self.stale(&ctx)),
                SectionId::Tiering => s.tiering = Some(self.tiering(&ctx)),
                SectionId::Plans => s.plans = Some(self.plans(&ctx)),
                SectionId::Forecast => s.forecast = Some(self.forecast(&ctx)),
            }
        }
        Ok(Report {
            schema_version: SCHEMA_VERSION,
            generated_at: rfc3339(ctx.now),
            app_version: env!("CARGO_PKG_VERSION"),
            scope: Scope { drive: ctx.drive.clone().map(|d| format!("{d}:")) },
            top_n: ctx.top_n as u32,
            sections: s,
        })
    }

    fn drives(&self, ctx: &Ctx) -> Section<Vec<DriveRow>> {
        let drives = self.inventory.snapshot(&ctx.settings).drives;
        let rows: Vec<DriveRow> = drives
            .iter()
            .filter(|d| d.letters.first().is_some_and(|l| ctx.letter_of(l)))
            .map(|d| DriveRow {
                drive: format!("{}:", d.letters.first().cloned().unwrap_or_default()),
                label: d.label.clone(),
                filesystem: d.filesystem.clone(),
                media: label(&d.media),
                capacity: d.capacity,
                free: d.free,
                used: d.used,
                used_percent: (d.used_fraction * 1000.0).round() / 10.0,
                health: label(&d.health),
                low_space: d.low_space,
            })
            .collect();
        if rows.is_empty() {
            return Section::unavailable("The drive list has not loaded yet.");
        }
        Section::ok(rows)
    }

    fn top(&self, ctx: &Ctx) -> Section<Vec<TopRoot>> {
        let snaps = self.scanned(ctx);
        if snaps.is_empty() {
            return Section::unavailable("No drive has been scanned yet. Scan a drive first.");
        }
        let mut roots = Vec::new();
        for snap in snaps {
            let Ok(conn) = snapshots::open_snapshot(&self.own, &snap) else { continue };
            let Ok(root) = snapshots::find_dir(&conn, None) else { continue };
            let q = TopQuery { limit: ctx.top_n as u32, offset: 0, category: None };
            let to_row = |e: snapshots::EntryView| TopEntry { path: e.path, allocated: e.alloc, logical: e.size };
            let folders = snapshots::top_dirs(&conn, &root, Some(3), Measure::Alloc, &q)
                .unwrap_or_default()
                .into_iter()
                .map(to_row)
                .collect();
            let files = snapshots::top_files(&conn, &root, Measure::Alloc, &q)
                .unwrap_or_default()
                .into_iter()
                .map(to_row)
                .collect();
            roots.push(TopRoot {
                root: snap.root.clone(),
                snapshot_id: snap.id,
                scanned_at: snap.finished_at.clone(),
                folders,
                files,
            });
        }
        if roots.is_empty() {
            return Section::unavailable("The scans could not be read.");
        }
        Section::ok(roots)
    }

    fn cleanup_section(&self, ctx: &Ctx) -> Section<CleanupSection> {
        let overview = match self.cleanup.overview() {
            Ok(o) => o,
            Err(e) => return Section::unavailable(format!("Cleanup suggestions could not be read: {e:?}")),
        };
        let drives: Vec<_> =
            overview.drives.iter().filter(|d| ctx.letter_of(&d.root) && d.cleanup.run.is_some()).collect();
        if drives.is_empty() {
            return Section::unavailable("No scan has been checked against the cleanup rules yet.");
        }
        let savings: Vec<SavingRow> = drives
            .iter()
            .flat_map(|d| {
                d.cleanup.savings.iter().map(|s| SavingRow {
                    root: d.root.clone(),
                    risk: label(&s.risk),
                    kind: label(&s.kind),
                    allocated: s.allocated,
                    logical: s.logical,
                    matches: s.matches,
                })
            })
            .collect();
        let page = self
            .cleanup
            .candidates(&CandidateQuery { drive: ctx.drive.clone(), limit: Some(ROW_CAP as u32), ..Default::default() })
            .ok();
        let total = page.as_ref().map_or(0, |p| p.total as usize);
        let candidates: Vec<CandidateRow> = page
            .map(|p| {
                p.items
                    .into_iter()
                    .map(|c| CandidateRow {
                        root: c.root,
                        rule_id: c.rule_id,
                        title: c.title,
                        risk: label(&c.risk),
                        kind: label(&c.kind),
                        path: c.path,
                        allocated: c.allocated,
                        logical: c.logical,
                        instructions: c.instructions,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let kept = candidates.len();
        Section::ok(CleanupSection { savings, candidates }).cut(total, kept)
    }

    fn duplicates(&self, ctx: &Ctx) -> Section<DuplicatesSection> {
        let Some(run) = self.dups.status().ok().flatten().filter(|r| r.status == DupStatus::Complete) else {
            return Section::unavailable("No duplicate analysis has completed yet. Run one from the Duplicates page.");
        };
        let page = match self.dups.groups(&GroupFilter {
            drive: ctx.drive.clone(),
            limit: Some(ROW_CAP as u32),
            ..Default::default()
        }) {
            Ok(p) => p,
            Err(e) => return Section::unavailable(format!("Duplicate groups could not be read: {e:?}")),
        };
        let groups: Vec<DupRow> = page
            .groups
            .iter()
            .map(|g| DupRow {
                size: g.size,
                wasted: g.wasted,
                copies: g
                    .members
                    .iter()
                    .map(|m| DupCopy { path: m.path.clone(), suggested: m.suggested.clone() })
                    .collect(),
            })
            .collect();
        let kept = groups.len();
        let total = page.total as usize;
        Section::ok(DuplicatesSection {
            total_groups: total as u64,
            total_wasted: page.total_wasted.max(if ctx.drive.is_none() { run.wasted } else { 0 }),
            groups,
        })
        .cut(total, kept)
    }

    fn stale(&self, ctx: &Ctx) -> Section<StaleSection> {
        let snaps = self.scanned(ctx);
        if snaps.is_empty() {
            return Section::unavailable("No drive has been scanned yet.");
        }
        let protected = crate::protection::build(self.cleanup.env.as_ref(), &ctx.settings);
        let mut roots = Vec::new();
        let mut total = 0usize;
        for snap in snaps {
            let Ok(conn) = snapshots::open_snapshot(&self.own, &snap) else { continue };
            let params = StaleParams {
                months: Some(ctx.settings.stale_months),
                limit: Some(ROW_CAP as u32),
                ..Default::default()
            };
            let Ok(page) = stale::stale_items(&conn, &snap, &protected, &params) else { continue };
            total += page.items.len();
            roots.push(StaleRoot {
                root: snap.root.clone(),
                months: page.months,
                time_basis: label(&page.time_basis),
                items: page
                    .items
                    .into_iter()
                    .map(|i| StaleRow {
                        path: i.entry.path,
                        allocated: i.entry.alloc,
                        last_used: i.time,
                        age_days: i.age_days,
                    })
                    .collect(),
            });
        }
        let _ = total;
        Section::ok(StaleSection { roots })
    }

    fn tiering(&self, ctx: &Ctx) -> Section<TieringSection> {
        let t = match self.tiering.suggestions() {
            Ok(t) => t,
            Err(e) => return Section::unavailable(format!("Tiering suggestions could not be read: {e:?}")),
        };
        if !t.drives.iter().any(|d| d.scanned) {
            return Section::unavailable("Tiering needs scans of the drives it compares.");
        }
        let (suggestions, total) = cap(t
            .suggestions
            .iter()
            .filter(|s| ctx.letter_of(&s.source_drive))
            .map(|s| TierRow {
                path: s.path.clone(),
                direction: label(&s.direction),
                kind: label(&s.kind),
                allocated: s.allocated,
                from_drive: s.source_drive.clone(),
                to_drive: s.target.as_ref().map(|x| x.letter.clone()),
                destination: s.destination.clone(),
                reason: s.reason.clone(),
                instructions: s.instructions.clone(),
            })
            .collect());
        let kept = suggestions.len();
        Section::ok(TieringSection {
            as_of: t.as_of,
            drives: t
                .drives
                .iter()
                .filter(|d| ctx.letter_of(&d.letter))
                .map(|d| TierDriveRow {
                    drive: format!("{}:", d.letter),
                    capacity: d.capacity,
                    free: d.free,
                    projected_free: d.projected_free,
                })
                .collect(),
            suggestions,
            notes: t.notes.into_iter().map(|n| n.message).collect(),
        })
        .cut(total, kept)
    }

    fn plans(&self, ctx: &Ctx) -> Section<PlansSection> {
        let list = match self.plans.list() {
            Ok(l) => l,
            Err(e) => return Section::unavailable(format!("Plans could not be read: {e:?}")),
        };
        if list.is_empty() {
            return Section::unavailable("There are no cleanup plans yet.");
        }
        let mut plans = Vec::new();
        let mut total_items = 0usize;
        let mut kept_items = 0usize;
        for p in list {
            let Ok(view) = self.plans.get(p.id) else { continue };
            let (items, total) = cap(view
                .items
                .iter()
                .filter(|i| ctx.letter_of(&i.drive))
                .map(|i| PlanItemRow {
                    path: i.path.clone(),
                    title: i.title.clone(),
                    risk: i.risk.clone(),
                    allocated: i.allocated,
                    status: i.status.clone(),
                    destination: i.destination.clone(),
                    instructions: i.instructions.clone(),
                })
                .collect());
            total_items += total;
            kept_items += items.len();
            plans.push(PlanRow {
                name: p.name.clone(),
                created_at: p.created_at.clone(),
                freed_total: view.freed_total,
                items,
            });
        }
        Section::ok(PlansSection { plans }).cut(total_items, kept_items)
    }

    fn forecast(&self, ctx: &Ctx) -> Section<ForecastSection> {
        let trends = match history::volume_trends(&self.db, &ctx.settings, ctx.now) {
            Ok(t) => t,
            Err(e) => return Section::unavailable(format!("History could not be read: {e:?}")),
        };
        let volumes: Vec<ForecastRow> = trends
            .volumes
            .iter()
            .filter(|v| v.letters.first().is_some_and(|l| ctx.letter_of(l)) && !v.samples.is_empty())
            .map(|v| {
                let (state, low, full, rate, confidence) = match &v.forecast {
                    diskdash_analysis::history::Forecast::NotEnoughHistory { .. } => {
                        ("not-enough-history", None, None, None, "none")
                    }
                    diskdash_analysis::history::Forecast::NotFilling { .. } => {
                        ("not-filling", None, None, None, "none")
                    }
                    diskdash_analysis::history::Forecast::Filling {
                        days_to_low,
                        days_to_full,
                        bytes_per_day,
                        low_confidence,
                        ..
                    } => (
                        "filling",
                        Some(*days_to_low),
                        Some(*days_to_full),
                        Some(*bytes_per_day),
                        if *low_confidence { "low" } else { "high" },
                    ),
                };
                ForecastRow {
                    drive: v.letters.first().map(|l| format!("{l}:")).unwrap_or_default(),
                    capacity: v.capacity,
                    state: state.into(),
                    days_to_low: low,
                    days_to_full: full,
                    bytes_per_day: rate,
                    confidence: confidence.into(),
                    samples: thin(&v.samples),
                }
            })
            .collect();
        if volumes.is_empty() {
            return Section::unavailable("No free-space history has been recorded yet.");
        }
        Section::ok(ForecastSection { volumes })
    }
}

/// At most 120 evenly spaced samples, always keeping the first and the last.
fn thin(samples: &[history::SamplePoint]) -> Vec<[i64; 2]> {
    const MAX: usize = 120;
    let pick = |s: &history::SamplePoint| [s.ts, s.free as i64];
    if samples.len() <= MAX {
        return samples.iter().map(pick).collect();
    }
    let step = samples.len() as f64 / (MAX - 1) as f64;
    let mut out: Vec<[i64; 2]> = (0..MAX - 1).map(|i| pick(&samples[(i as f64 * step) as usize])).collect();
    if let Some(last) = samples.last() {
        out.push(pick(last));
    }
    out
}
