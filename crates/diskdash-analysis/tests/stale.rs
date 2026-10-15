#![allow(clippy::unwrap_used)]
//! Stale-data queries on hand-built snapshots. The analysis only receives a
//! database connection, so these tests run without any file system access
//! (none of the paths below exist).

use diskdash_analysis::eval::protected::Protected;
use diskdash_analysis::rules::MapEnv;
use diskdash_analysis::stale::{invalid_time_count, old_large, stale, StaleQuery, TimeBasis};
use rusqlite::Connection;

const NOW: i64 = 1_800_000_000;
const DAY: i64 = 86_400;
const GIB: i64 = 1024 * 1024 * 1024;

struct Db(Connection);

impl Db {
    fn new() -> Self {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(diskdash_core::store::SCHEMA).unwrap();
        let db = Db(c);
        db.dir(0, None, "C:\\", "C:\\", NOW - DAY, NOW - DAY);
        db
    }

    fn dir(&self, id: i64, parent: Option<i64>, name: &str, path: &str, max_mtime: i64, max_atime: i64) {
        self.0
            .execute(
                "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, mtime, atime, attrs, path, max_mtime, max_atime, last_desc_id)
                 VALUES (?1, ?2, ?3, lower(?3), 1, 1, 0, 0, ?4, ?5, 0, ?6, ?4, ?5, ?1)",
                rusqlite::params![id, parent, name, max_mtime, max_atime, path],
            )
            .unwrap();
    }

    fn file(&self, id: i64, parent: i64, name: &str, alloc: i64, mtime: i64, atime: i64) {
        self.0
            .execute(
                "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, mtime, atime, attrs)
                 VALUES (?1, ?2, ?3, lower(?3), 0, 2, ?4, ?4, ?5, ?6, 0)",
                rusqlite::params![id, parent, name, alloc, mtime, atime],
            )
            .unwrap();
        // Ancestors aggregate size and the newest descendant time, as the scanner does.
        let mut cur = Some(parent);
        while let Some(id) = cur {
            self.0
                .execute(
                    "UPDATE entries SET size = size + ?2, alloc = alloc + ?2, max_mtime = max(coalesce(max_mtime, 0), ?3), max_atime = max(coalesce(max_atime, 0), ?4) WHERE id = ?1",
                    rusqlite::params![id, alloc, mtime, atime],
                )
                .unwrap();
            cur = self.0.query_row("SELECT parent_id FROM entries WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        }
    }
}

fn protected() -> Protected {
    Protected::new(&MapEnv::default())
}

fn query<'a>(basis: TimeBasis, months: i64, limit: u32, offset: u32, p: &'a Protected) -> StaleQuery<'a> {
    StaleQuery { basis, cutoff: NOW - months * 30 * DAY, now: NOW, offset, limit, protected: p }
}

/// C:\data with a fully stale folder, a mixed folder and a file without a timestamp.
fn tree() -> Db {
    let db = Db::new();
    let old = NOW - 3 * 365 * DAY;
    db.dir(1, Some(0), "data", r"C:\data", 0, 0);
    db.dir(2, Some(1), "old", r"C:\data\old", 0, 0);
    db.dir(5, Some(1), "mixed", r"C:\data\mixed", 0, 0);
    db.file(3, 2, "a.bin", 10 * GIB, old, old);
    db.file(4, 2, "b.bin", GIB, old, old);
    db.file(6, 5, "stale.bin", 5 * GIB, old, old);
    db.file(7, 5, "fresh.txt", 1024, NOW - 7 * DAY, NOW - 7 * DAY);
    db.file(8, 1, "zero.bin", 1 << 20, 0, 0);
    // Propagate to `data` the way the scanner does.
    db.0.execute("UPDATE entries SET max_mtime = ?1, max_atime = ?1 WHERE id = 1", [NOW - 7 * DAY]).unwrap();
    db
}

#[test]
fn folder_is_stale_only_when_its_newest_descendant_is() {
    let db = tree();
    let p = protected();
    let page = stale(&db.0, &query(TimeBasis::Mtime, 12, 20, 0, &p)).unwrap();
    let paths: Vec<&str> = page.rows.iter().map(|r| r.path.as_str()).collect();
    // `old` collapses its two files; `mixed` is not stale but its stale file is; `data` has a recent file.
    assert_eq!(paths, [r"C:\data\old", r"C:\data\mixed\stale.bin"]);
    assert!(page.rows[0].is_dir && !page.rows[1].is_dir);
    assert_eq!(page.rows[0].alloc, 11 * GIB as u64);
    assert!(page.rows[0].age_days >= 3 * 365 - 1);
    assert!(!page.more);
}

#[test]
fn presets_requery_the_same_snapshot() {
    let db = tree();
    let p = protected();
    let n = |months| stale(&db.0, &query(TimeBasis::Mtime, months, 20, 0, &p)).unwrap().rows.len();
    assert_eq!(n(12), 2);
    assert_eq!(n(36), 2, "3 years old is older than 36 months (1080 days) only just: 1095 > 1080");
    assert_eq!(n(48), 0, "nothing is older than 4 years");
    assert_eq!(n(0), 1, "with no age limit the whole data folder is stale and collapses everything below it");
}

#[test]
fn entries_without_valid_timestamps_are_excluded_and_counted() {
    let db = tree();
    let p = protected();
    let page = stale(&db.0, &query(TimeBasis::Mtime, 12, 20, 0, &p)).unwrap();
    assert!(page.rows.iter().all(|r| !r.path.ends_with("zero.bin")));
    assert_eq!(invalid_time_count(&db.0, TimeBasis::Mtime, NOW).unwrap(), 1);
}

#[test]
fn time_basis_selects_the_column() {
    let db = Db::new();
    let old = NOW - 2 * 365 * DAY;
    db.dir(1, Some(0), "d", r"C:\d", 0, 0);
    // Modified long ago but read last week.
    db.file(2, 1, "read-often.bin", GIB, old, NOW - 7 * DAY);
    let p = protected();
    assert_eq!(stale(&db.0, &query(TimeBasis::Mtime, 12, 20, 0, &p)).unwrap().rows.len(), 1);
    assert_eq!(stale(&db.0, &query(TimeBasis::Atime, 12, 20, 0, &p)).unwrap().rows.len(), 0);
}

#[test]
fn paging_and_protected_paths() {
    let db = Db::new();
    let old = NOW - 3 * 365 * DAY;
    db.dir(1, Some(0), "Windows", r"C:\Windows", 0, 0);
    db.dir(2, Some(1), "System32", r"C:\Windows\System32", 0, 0);
    db.file(3, 2, "driver.sys", 50 * GIB, old, old);
    db.dir(4, Some(0), "docs", r"C:\docs", 0, 0);
    for i in 0..5 {
        db.file(10 + i, 4, &format!("f{i}.bin"), (5 - i) * GIB, old, old);
    }
    db.0.execute("UPDATE entries SET max_mtime = ?1, max_atime = ?1 WHERE id IN (1, 4)", [NOW - DAY]).unwrap();
    let p = protected();

    let first = stale(&db.0, &query(TimeBasis::Mtime, 12, 2, 0, &p)).unwrap();
    assert_eq!(first.rows.iter().map(|r| r.alloc / GIB as u64).collect::<Vec<_>>(), [5, 4]);
    assert!(first.more);
    let second = stale(&db.0, &query(TimeBasis::Mtime, 12, 2, 2, &p)).unwrap();
    assert_eq!(second.rows.iter().map(|r| r.alloc / GIB as u64).collect::<Vec<_>>(), [3, 2]);
    let last = stale(&db.0, &query(TimeBasis::Mtime, 12, 2, 4, &p)).unwrap();
    assert_eq!(last.rows.len(), 1);
    assert!(!last.more);
    // The 50 GiB driver in System32 never shows up.
    assert!(first.rows.iter().chain(&second.rows).all(|r| !r.path.contains("System32")));
}

#[test]
fn old_large_files_are_ranked_by_size_times_age() {
    let db = Db::new();
    db.dir(1, Some(0), "d", r"C:\d", 0, 0);
    db.file(2, 1, "A.iso", GIB, NOW - 400 * DAY, NOW - 400 * DAY);
    db.file(3, 1, "B.iso", 4 * GIB, NOW - 30 * DAY, NOW - 30 * DAY);
    db.file(4, 1, "small.bin", 10 << 20, NOW - 2000 * DAY, NOW - 2000 * DAY);
    db.file(5, 1, "today.iso", 9 * GIB, NOW - 3600, NOW - 3600);
    let p = protected();
    let page = old_large(&db.0, TimeBasis::Mtime, NOW, 100 << 20, 0, 20, &p).unwrap();
    let names: Vec<&str> = page.rows.iter().map(|r| r.path.rsplit('\\').next().unwrap()).collect();
    assert_eq!(names, ["A.iso", "B.iso"], "400 vs ~120 GiB-days; below the floor and younger than a day are left out");
    assert!(page.rows[0].score > page.rows[1].score);
    assert!((page.rows[0].score / GIB as f64 - 400.0).abs() < 0.01);
}
