//! Stale-data queries over one snapshot database.
//!
//! Everything here takes only a `&Connection` to the snapshot: the analysis
//! has no way to open, read or touch a user file.

use rusqlite::{params, Connection};

use super::lastaccess::TimeBasis;
use crate::eval::protected::Protected;

/// Timestamps before 1980 (or zero) are not real (FAT epoch, unset values).
pub const MIN_VALID_TIME: i64 = 315_532_800;
const DAY: i64 = 86_400;

/// The effective time of entry alias `e`: folders use their newest descendant.
fn time_expr(alias: &str, basis: TimeBasis) -> String {
    let (own, newest) = match basis {
        TimeBasis::Atime => ("atime", "max_atime"),
        TimeBasis::Mtime => ("mtime", "max_mtime"),
    };
    format!(
        "(CASE WHEN {alias}.kind = 1 THEN coalesce(nullif({alias}.{newest}, 0), {alias}.{own}) ELSE {alias}.{own} END)"
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct StaleRow {
    pub id: i64,
    pub path: String,
    pub is_dir: bool,
    pub alloc: u64,
    /// The time used (unix seconds).
    pub time: i64,
    pub age_days: u64,
    /// allocated bytes × age in days (only filled for the old-large list).
    pub score: f64,
}

#[derive(Debug, Default)]
pub struct StalePage {
    pub rows: Vec<StaleRow>,
    pub more: bool,
}

pub struct StaleQuery<'a> {
    pub basis: TimeBasis,
    /// Entries newer than this are not stale (unix seconds).
    pub cutoff: i64,
    pub now: i64,
    pub offset: u32,
    pub limit: u32,
    pub protected: &'a Protected,
    /// Smallest allocated size listed (0 = no minimum).
    pub min_alloc: u64,
}

fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('\\') {
        format!("{dir}{name}")
    } else {
        format!("{dir}\\{name}")
    }
}

const CHUNK: u32 = 200;

/// Files and folders not touched since `cutoff`, largest first. A folder is
/// stale only when its newest descendant is; nested stale folders and the
/// files inside them are collapsed under their highest stale ancestor.
pub fn stale(conn: &Connection, q: &StaleQuery<'_>) -> rusqlite::Result<StalePage> {
    let (te, tp) = (time_expr("e", q.basis), time_expr("p", q.basis));
    let sql = format!(
        "SELECT e.id, e.kind, e.alloc, {te}, coalesce(e.path, p.path), e.name
         FROM entries e LEFT JOIN entries p ON p.id = e.parent_id
         WHERE e.id != 0 AND e.kind IN (0, 1) AND (e.flags & 2) = 0 AND e.alloc > 0 AND e.alloc >= ?5
           AND {te} >= ?1 AND {te} < ?2
           AND (p.id = 0 OR NOT ({tp} >= ?1 AND {tp} < ?2))
         ORDER BY e.alloc DESC, e.id LIMIT ?3 OFFSET ?4"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut page = StalePage::default();
    let (mut accepted, mut sql_offset) = (0u32, 0u32);
    loop {
        let rows: Vec<(i64, i64, i64, i64, Option<String>, String)> = stmt
            .query_map(params![MIN_VALID_TIME, q.cutoff, CHUNK, sql_offset, q.min_alloc as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })?
            .collect::<Result<_, _>>()?;
        let exhausted = (rows.len() as u32) < CHUNK;
        sql_offset += rows.len() as u32;
        for (id, kind, alloc, time, path, name) in rows {
            let path = if kind == 1 { path.unwrap_or_default() } else { join(&path.unwrap_or_default(), &name) };
            if q.protected.is_protected(&path) {
                continue;
            }
            if accepted >= q.offset {
                if page.rows.len() as u32 == q.limit {
                    page.more = true;
                    return Ok(page);
                }
                page.rows.push(StaleRow {
                    id,
                    path,
                    is_dir: kind == 1,
                    alloc: alloc.max(0) as u64,
                    time,
                    age_days: ((q.now - time) / DAY).max(0) as u64,
                    score: 0.0,
                });
            }
            accepted += 1;
        }
        if exhausted {
            return Ok(page);
        }
    }
}

/// Large files ranked by allocated size × age in days (α = 1).
pub fn old_large(
    conn: &Connection,
    basis: TimeBasis,
    now: i64,
    floor: u64,
    offset: u32,
    limit: u32,
    protected: &Protected,
) -> rusqlite::Result<StalePage> {
    let te = time_expr("e", basis);
    let sql = format!(
        "SELECT e.id, e.alloc, {te}, p.path, e.name,
                e.alloc * ((?2 - {te}) / 86400.0) AS score
         FROM entries e LEFT JOIN entries p ON p.id = e.parent_id
         WHERE e.kind = 0 AND (e.flags & 2) = 0 AND e.alloc >= ?3
           AND {te} >= ?1 AND {te} <= ?2 - 86400
         ORDER BY score DESC, e.id LIMIT ?4 OFFSET ?5"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut page = StalePage::default();
    let (mut accepted, mut sql_offset) = (0u32, 0u32);
    loop {
        let rows: Vec<(i64, i64, i64, Option<String>, String, f64)> = stmt
            .query_map(params![MIN_VALID_TIME, now, floor as i64, CHUNK, sql_offset], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })?
            .collect::<Result<_, _>>()?;
        let exhausted = (rows.len() as u32) < CHUNK;
        sql_offset += rows.len() as u32;
        for (id, alloc, time, dir, name, score) in rows {
            let path = join(&dir.unwrap_or_default(), &name);
            if protected.is_protected(&path) {
                continue;
            }
            if accepted >= offset {
                if page.rows.len() as u32 == limit {
                    page.more = true;
                    return Ok(page);
                }
                page.rows.push(StaleRow {
                    id,
                    path,
                    is_dir: false,
                    alloc: alloc.max(0) as u64,
                    time,
                    age_days: ((now - time) / DAY).max(0) as u64,
                    score,
                });
            }
            accepted += 1;
        }
        if exhausted {
            return Ok(page);
        }
    }
}

/// Files and folders without a usable timestamp for `basis` (left out of the results).
pub fn invalid_time_count(conn: &Connection, basis: TimeBasis, now: i64) -> rusqlite::Result<u64> {
    let te = time_expr("e", basis);
    let n: i64 = conn.query_row(
        &format!(
            "SELECT count(*) FROM entries e WHERE e.id != 0 AND e.kind IN (0, 1) AND (e.flags & 2) = 0
               AND NOT ({te} >= ?1 AND {te} <= ?2 + 86400)"
        ),
        params![MIN_VALID_TIME, now],
        |r| r.get(0),
    )?;
    Ok(n.max(0) as u64)
}
