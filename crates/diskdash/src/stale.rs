//! Stale files and folders, and old large files, from one snapshot. Read-only:
//! the analysis only reads the snapshot database.

use diskdash_analysis::eval::protected::Protected;
use diskdash_analysis::stale::{self, BasisReason, StaleQuery, StaleRow, TimeBasis};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cleanup::unix_now;
use crate::error::ApiError;
use crate::snapshots::{self, EntryView, Snapshot};

pub const DEFAULT_MONTHS: u32 = 12;
pub const MAX_MONTHS: u32 = 240;
pub const DEFAULT_FLOOR_MIB: u64 = 100;
const MAX_LIMIT: u32 = 100;
const DAYS_PER_MONTH: f64 = 30.4375;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StaleParams {
    pub months: Option<u32>,
    /// Old large files: smallest file size considered, in MiB.
    pub floor_mib: Option<u64>,
    pub limit: Option<u32>,
    pub cursor: Option<u32>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct StaleItem {
    pub entry: EntryView,
    /// The time used: last access or last modified, as the page's time basis says.
    pub time: String,
    #[ts(type = "number")]
    pub age_days: u64,
    /// Size times age in days (old large files only).
    #[ts(type = "number | null")]
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct StalePageView {
    pub items: Vec<StaleItem>,
    #[ts(type = "number | null")]
    pub next_cursor: Option<u32>,
    pub time_basis: TimeBasis,
    pub time_basis_reason: BasisReason,
    pub months: u32,
    /// Items the view leaves out because they have no usable timestamp.
    #[ts(type = "number")]
    pub excluded_invalid: u64,
}

fn to_items(conn: &Connection, rows: Vec<StaleRow>, with_score: bool) -> Result<Vec<StaleItem>, ApiError> {
    rows.into_iter()
        .map(|r| {
            Ok(StaleItem {
                entry: snapshots::entry_by_id(conn, r.id)?,
                time: snapshots::rfc3339(Some(r.time)).unwrap_or_default(),
                age_days: r.age_days,
                score: with_score.then_some(r.score),
            })
        })
        .collect()
}

struct View<'a> {
    snap: &'a Snapshot,
    limit: u32,
    cursor: u32,
    months: u32,
    with_score: bool,
    now: i64,
}

fn page_view(conn: &Connection, page: stale::StalePage, v: View<'_>) -> Result<StalePageView, ApiError> {
    Ok(StalePageView {
        items: to_items(conn, page.rows, v.with_score)?,
        next_cursor: page.more.then_some(v.cursor + v.limit),
        time_basis: v.snap.time_basis,
        time_basis_reason: v.snap.time_basis_reason,
        months: v.months,
        excluded_invalid: stale::invalid_time_count(conn, v.snap.time_basis, v.now).map_err(ApiError::internal)?,
    })
}

fn paging(p: &StaleParams) -> (u32, u32) {
    (p.limit.unwrap_or(20).clamp(1, MAX_LIMIT), p.cursor.unwrap_or(0))
}

/// Files and folders not touched for the chosen number of months, largest first.
pub fn stale_items(
    conn: &Connection,
    snap: &Snapshot,
    protected: &Protected,
    p: &StaleParams,
) -> Result<StalePageView, ApiError> {
    let months = p.months.unwrap_or(DEFAULT_MONTHS);
    if !(1..=MAX_MONTHS).contains(&months) {
        return Err(ApiError::BadRequest(format!("months must be between 1 and {MAX_MONTHS}")));
    }
    let (limit, cursor) = paging(p);
    let now = unix_now();
    let q = StaleQuery {
        basis: snap.time_basis,
        cutoff: now - (f64::from(months) * DAYS_PER_MONTH * 86_400.0) as i64,
        now,
        offset: cursor,
        limit,
        protected,
        min_alloc: 0,
    };
    let page = stale::stale(conn, &q).map_err(ApiError::internal)?;
    page_view(conn, page, View { snap, limit, cursor, months, with_score: false, now })
}

/// Large files ranked by size times age.
pub fn old_large_files(
    conn: &Connection,
    snap: &Snapshot,
    protected: &Protected,
    p: &StaleParams,
) -> Result<StalePageView, ApiError> {
    let floor_mib = p.floor_mib.unwrap_or(DEFAULT_FLOOR_MIB);
    if floor_mib > 10_000_000 {
        return Err(ApiError::BadRequest("floor_mib is too large".into()));
    }
    let (limit, cursor) = paging(p);
    let now = unix_now();
    let page = stale::old_large(conn, snap.time_basis, now, floor_mib << 20, cursor, limit, protected)
        .map_err(ApiError::internal)?;
    let months = p.months.unwrap_or(DEFAULT_MONTHS);
    page_view(conn, page, View { snap, limit, cursor, months, with_score: true, now })
}
