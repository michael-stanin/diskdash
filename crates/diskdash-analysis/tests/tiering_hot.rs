#![allow(clippy::unwrap_used)]
//! Hot-folder detection on a hand-built snapshot (no disk access).

use diskdash_analysis::eval::protected::Protected;
use diskdash_analysis::rules::MapEnv;
use diskdash_analysis::tiering::{hot_folders, HotParams};
use rusqlite::Connection;

const NOW: i64 = 1_800_000_000;
const DAY: i64 = 86_400;
const MIB: i64 = 1 << 20;

struct Snap {
    conn: Connection,
    next: i64,
}

impl Snap {
    fn new() -> Self {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(diskdash_core::store::SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, attrs, path, last_desc_id) VALUES (0, NULL, 'D:', 'd:', 1, 0, 0, 0, 0, 'D:\\', 0)",
            [],
        )
        .unwrap();
        Snap { conn, next: 1 }
    }

    /// A folder at `depth` holding files `(size_mib, age_days)`; returns nothing, sets totals.
    fn dir(&mut self, path: &str, depth: i64, files: &[(i64, i64)]) {
        let id = self.next;
        self.next += 1;
        let total: i64 = files.iter().map(|f| f.0 * MIB).sum();
        let last = id + files.len() as i64;
        self.conn
            .execute(
                "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, attrs, path, last_desc_id)
                 VALUES (?1, 0, ?2, lower(?2), 1, ?3, ?4, ?4, 0, ?5, ?6)",
                rusqlite::params![id, path.rsplit('\\').next().unwrap(), depth, total, path, last],
            )
            .unwrap();
        for (i, (mib, age)) in files.iter().enumerate() {
            self.conn
                .execute(
                    "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, mtime, attrs)
                     VALUES (?1, ?2, ?3, ?3, 0, ?4, ?5, ?5, ?6, 0)",
                    rusqlite::params![id + 1 + i as i64, id, format!("f{i}"), depth + 1, mib * MIB, NOW - age * DAY],
                )
                .unwrap();
        }
        self.next = last + 1;
    }
}

fn params() -> HotParams {
    HotParams { days: 30, share_percent: 20.0, min_files: 100, min_alloc: 100 * MIB as u64 }
}

fn protected() -> Protected {
    Protected::new(&MapEnv::default())
}

#[test]
fn folders_with_enough_recent_bytes_or_files_are_hot() {
    let mut s = Snap::new();
    // 30% of the bytes were modified 2 days ago.
    s.dir(r"D:\Projects", 1, &[(300, 2), (700, 400)]);
    // Nothing recent.
    s.dir(r"D:\Archive", 1, &[(500, 800), (500, 900)]);
    // 120 tiny recent files in a mostly old folder (under 20% of bytes).
    let mut many: Vec<(i64, i64)> = vec![(1, 3); 120];
    many.push((2000, 900));
    s.dir(r"D:\Logs", 1, &many);
    let hot = hot_folders(&s.conn, NOW, &params(), &protected()).unwrap();
    let names: Vec<&str> = hot.iter().map(|h| h.path.as_str()).collect();
    assert_eq!(names, [r"D:\Logs", r"D:\Projects"], "by size: Logs (2120 MiB) before Projects (1000 MiB)");
    let projects = hot.iter().find(|h| h.path.ends_with("Projects")).unwrap();
    assert_eq!(projects.share.round() as u64, 30);
    assert_eq!(projects.reason, "30% of it was modified in the last 30 days");
    let logs = hot.iter().find(|h| h.path.ends_with("Logs")).unwrap();
    assert_eq!(logs.recent_files, 120);
    assert!(logs.reason.starts_with("120 files in it were modified"), "{}", logs.reason);
}

#[test]
fn small_protected_and_nested_folders_are_left_out() {
    let mut s = Snap::new();
    s.dir(r"D:\Tiny", 1, &[(50, 1)]);
    s.dir(r"D:\Windows", 1, &[(500, 1)]);
    s.dir(r"D:\Big", 1, &[(500, 1)]);
    s.dir(r"D:\Big\Inner", 2, &[(500, 1)]);
    let hot = hot_folders(&s.conn, NOW, &params(), &protected()).unwrap();
    assert_eq!(hot.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(), [r"D:\Big"], "the outermost folder only");
}
