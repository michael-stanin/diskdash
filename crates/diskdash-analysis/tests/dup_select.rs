#![allow(clippy::unwrap_used)]
//! Stage 1 candidate selection on a hand-built snapshot (no disk access).

use diskdash_analysis::duplicates::{select, ScopeSnapshot};
use diskdash_analysis::eval::protected::Protected;
use diskdash_analysis::rules::MapEnv;
use rusqlite::Connection;

const MIB: i64 = 1 << 20;

type Row<'a> = (&'a str, &'a str, i64, i64, i64, Option<i64>);

fn db(rows: &[Row<'_>]) -> Connection {
    // (dir, name, size, attrs, flags, hardlink_of)
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(diskdash_core::store::SCHEMA).unwrap();
    c.execute(
        "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, attrs, path, last_desc_id) VALUES (0, NULL, 'C:', 'c:', 1, 0, 0, 0, 0, 'C:\\', 0)",
        [],
    )
    .unwrap();
    let mut dirs: Vec<&str> = rows.iter().map(|r| r.0).collect();
    dirs.sort_unstable();
    dirs.dedup();
    for (i, d) in dirs.iter().enumerate() {
        c.execute(
            "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, attrs, path, last_desc_id) VALUES (?1, 0, ?2, lower(?2), 1, 1, 0, 0, 0, ?3, ?1)",
            rusqlite::params![100 + i as i64, d.rsplit('\\').next().unwrap(), d],
        )
        .unwrap();
    }
    for (n, (dir, name, size, attrs, flags, link)) in rows.iter().enumerate() {
        let parent = 100 + dirs.iter().position(|d| d == dir).unwrap() as i64;
        c.execute(
            "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, mtime, attrs, flags, hardlink_of)
             VALUES (?1, ?2, ?3, lower(?3), 0, 2, ?4, ?4, 1000, ?5, ?6, ?7)",
            rusqlite::params![1000 + n as i64, parent, name, size, attrs, flags, link],
        )
        .unwrap();
    }
    c
}

#[test]
fn only_real_same_size_files_above_the_minimum_become_candidates() {
    let conn = db(&[
        (r"C:\a", "one.bin", 5 * MIB, 0, 0, None),
        (r"C:\b", "two.bin", 5 * MIB, 0, 0, None),
        (r"C:\b", "unique.bin", 7 * MIB, 0, 0, None),
        (r"C:\b", "cloud-flag.bin", 5 * MIB, 0, 2, None),
        (r"C:\b", "cloud-attr.bin", 5 * MIB, 0x400000, 0, None),
        (r"C:\b", "recall-open.bin", 5 * MIB, 0x40000, 0, None),
        (r"C:\b", "offline.bin", 5 * MIB, 0x1000, 0, None),
        (r"C:\b", "link.bin", 5 * MIB, 0x400, 0, None),
        (r"C:\b", "hardlink.bin", 5 * MIB, 0, 0, Some(1000)),
        (r"C:\b", "tiny1.bin", 1000, 0, 0, None),
        (r"C:\b", "tiny2.bin", 1000, 0, 0, None),
        (r"C:\b", "empty1.bin", 0, 0, 0, None),
        (r"C:\b", "empty2.bin", 0, 0, 0, None),
        (r"C:\Windows\System32", "dll1.bin", 5 * MIB, 0, 0, None),
        (r"C:\Windows\System32", "dll2.bin", 5 * MIB, 0, 0, None),
    ]);
    let scope = ScopeSnapshot {
        snapshot_id: 1,
        root: r"C:\".into(),
        volume_id: None,
        volume_serial: 7,
        disk: "disk-0".into(),
        hdd: false,
        conn: &conn,
    };
    let sel = select(&[scope], MIB as u64, &Protected::new(&MapEnv::default())).unwrap();
    let mut names: Vec<_> = sel.candidates.iter().map(|c| c.path.clone()).collect();
    names.sort();
    assert_eq!(names, [r"C:\a\one.bin", r"C:\b\two.bin"]);
    assert_eq!(sel.excluded.cloud_placeholder, 4, "flag, RECALL_ON_DATA_ACCESS, RECALL_ON_OPEN and OFFLINE");
    assert_eq!(sel.excluded.reparse_point, 1);
    assert_eq!(sel.excluded.hard_link, 1);
    assert_eq!(sel.excluded.protected, 2, "System32 copies are never offered");
    assert!(sel.candidates.iter().all(|c| c.volume_serial == 7 && !c.hdd));
}

#[test]
fn sizes_are_matched_across_snapshots() {
    let a = db(&[(r"C:\a", "x.bin", 3 * MIB, 0, 0, None)]);
    let b = db(&[(r"D:\b", "y.bin", 3 * MIB, 0, 0, None), (r"D:\b", "z.bin", 9 * MIB, 0, 0, None)]);
    let mk = |id, root: &str, conn| ScopeSnapshot {
        snapshot_id: id,
        root: root.into(),
        volume_id: None,
        volume_serial: id as u32,
        disk: format!("disk-{id}"),
        hdd: true,
        conn,
    };
    let sel = select(&[mk(1, r"C:\", &a), mk(2, r"D:\", &b)], MIB as u64, &Protected::new(&MapEnv::default())).unwrap();
    assert_eq!(sel.candidates.len(), 2, "x and y share a size; z is alone");
    assert_eq!(sel.candidates.iter().map(|c| c.snapshot_id).collect::<Vec<_>>(), [1, 2]);
}
