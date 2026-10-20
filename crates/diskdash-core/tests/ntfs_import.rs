//! MFT import and quick rescan on synthetic volumes (no administrator rights needed).

#![allow(clippy::unwrap_used)]

use std::sync::atomic::AtomicBool;

use diskdash_core::ntfs::import::{import, ImportError, ImportOptions, Stage};
use diskdash_core::ntfs::incremental::{quick_rescan, Changes};
use diskdash_core::ntfs::model::{EntryName, MftEntry, MftError, MftInfo, MftSource, ScanTally, VecSource};
use diskdash_core::scan::ScanSummary;
use rusqlite::Connection;

const DIR: u32 = 0x10;

fn name(parent: u64, n: &str) -> EntryName {
    EntryName { parent, parent_seq: parent_seq(parent), name: n.into() }
}

/// Sequence numbers: the root is 5, everything else 1.
fn parent_seq(parent: u64) -> u16 {
    if parent == 5 {
        5
    } else {
        1
    }
}

fn dir(frn: u64, parent: u64, n: &str) -> MftEntry {
    MftEntry {
        frn,
        seq: if frn == 5 { 5 } else { 1 },
        is_dir: true,
        attrs: DIR,
        ctime: 100,
        mtime: 200,
        atime: 300,
        names: vec![name(parent, n)],
        size: 0,
        allocated: 0,
        resident: true,
        compressed_or_sparse: false,
        reparse_tag: 0,
    }
}

fn file(frn: u64, parent: u64, n: &str, size: u64) -> MftEntry {
    MftEntry {
        frn,
        seq: 1,
        is_dir: false,
        attrs: 0x20,
        ctime: 100,
        mtime: 1_000 + frn as i64,
        atime: 2_000 + frn as i64,
        names: vec![name(parent, n)],
        size,
        allocated: 0,
        resident: size < 600,
        compressed_or_sparse: false,
        reparse_tag: 0,
    }
}

fn info() -> MftInfo {
    MftInfo { bytes_per_cluster: 4096, record_size: 1024, record_count: 100, serial: 0xABCD }
}

fn volume() -> Vec<MftEntry> {
    let mut b = file(32, 30, "b.bin", 10_000);
    b.names.push(name(40, "b-link.bin"));
    let mut c = file(41, 40, "c.dat", 1 << 20);
    c.attrs |= 0x800;
    c.allocated = 70_000;
    c.compressed_or_sparse = true;
    let mut junction = dir(42, 5, "junction");
    junction.attrs |= 0x400;
    junction.reparse_tag = 0xA000_0003;
    let mut stale = file(45, 30, "stale.txt", 77);
    stale.names[0].parent_seq = 9; // parent was recreated since this name was written
    vec![
        dir(5, 5, "."),
        file(0, 5, "$MFT", 1 << 20),
        dir(11, 5, "$Extend"),
        file(26, 11, "$UsnJrnl", 100),
        dir(30, 5, "docs"),
        file(31, 30, "a.txt", 1_000),
        b,
        dir(40, 5, "other"),
        c,
        junction,
        file(44, 99, "orphan.txt", 5),
        stale,
        file(46, 5, "root.txt", 4_096),
    ]
}

fn opts(db: &std::path::Path) -> ImportOptions {
    ImportOptions {
        root: r"C:\".into(),
        db_path: db.to_path_buf(),
        windows_dir: Some(r"C:\Windows".into()),
        max_bad_fraction: 0.01,
    }
}

fn run(entries: Vec<MftEntry>, db: &std::path::Path) -> ScanSummary {
    let mut src = VecSource { info: info(), entries };
    import(&mut src, &opts(db), &AtomicBool::new(false), &mut |_, _| {}).unwrap()
}

type Row =
    (i64, Option<i64>, String, i64, i64, i64, i64, Option<String>, Option<i64>, Option<i64>, Option<i64>, Option<i64>);

fn rows(db: &std::path::Path) -> Vec<Row> {
    let c = Connection::open(db).unwrap();
    let mut s = c
        .prepare(
            "SELECT id, parent_id, name, kind, depth, size, alloc, path, hardlink_of, nlink, last_desc_id, file_count
             FROM entries ORDER BY id",
        )
        .unwrap();
    s.query_map([], |r| {
        Ok((
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            r.get(5)?,
            r.get(6)?,
            r.get(7)?,
            r.get(8)?,
            r.get(9)?,
            r.get(10)?,
            r.get(11)?,
        ))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

fn by_path(rows: &[Row], path: &str) -> Row {
    rows.iter().find(|r| r.7.as_deref() == Some(path)).cloned().unwrap_or_else(|| panic!("no folder {path}"))
}

fn by_name<'a>(rows: &'a [Row], n: &str) -> Vec<&'a Row> {
    rows.iter().filter(|r| r.2 == n).collect()
}

#[test]
fn imports_the_tree_with_walk_compatible_totals() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("m.db");
    let sum = run(volume(), &db);
    let r = rows(&db);

    // Folder totals: a.txt 1000 (4096 on disk) + b.bin 10000 (12288) under docs; the stale and orphan names are dropped.
    let docs = by_path(&r, r"C:\docs");
    assert_eq!((docs.5, docs.6), (11_000, 4_096 + 12_288), "docs");
    assert_eq!(docs.11, Some(2));
    // other: the hard link is counted under docs only; c.dat is compressed so it counts its real allocation.
    let other = by_path(&r, r"C:\other");
    assert_eq!((other.5, other.6), (1 << 20, 73_728), "compressed file counts its on-disk size");

    let root = &r[0];
    assert_eq!((root.2.as_str(), root.7.as_deref(), root.1), (r"C:\", Some(r"C:\"), None));
    assert_eq!(by_name(&r, "orphan.txt").len() + by_name(&r, "stale.txt").len(), 0);
    assert_eq!(sum.cluster_size, 4096);
    assert_eq!(sum.filesystem, "NTFS");
    assert!(sum.hardlinks_deduplicated);

    // Metafiles live under a synthetic node.
    let meta = by_path(&r, r"C:\<NTFS metadata>");
    let usn = by_name(&r, "$UsnJrnl")[0];
    let extend = by_name(&r, "$Extend")[0];
    assert_eq!(extend.1, Some(meta.0), "$Extend is placed under the metadata node");
    assert_eq!(usn.1, Some(extend.0), "its children stay under it");
    assert!(by_name(&r, "$MFT")[0].1 == Some(meta.0));

    // The junction is a link: recorded, size 0, not entered.
    let j = by_name(&r, "junction")[0];
    assert_eq!((j.3, j.5, j.6), (2, 0, 0));

    // Totals excluding the metadata node equal what a walk would count: a, b, c, root.txt.
    let walk_files: i64 = ["a.txt", "b.bin", "c.dat", "root.txt"].iter().map(|n| by_name(&r, n)[0].5).sum();
    assert_eq!(walk_files, 1_000 + 10_000 + (1 << 20) + 4_096);
    assert_eq!(root.5 - meta.5, walk_files);
    assert_eq!(root.6 - meta.6, 4_096 + 12_288 + 73_728 + 4_096);
}

#[test]
fn ids_are_depth_first_with_subtrees_as_ranges() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("m.db");
    run(volume(), &db);
    let r = rows(&db);
    let ids: Vec<i64> = r.iter().map(|x| x.0).collect();
    assert_eq!(ids, (0..r.len() as i64).collect::<Vec<_>>(), "ids are dense from 0");
    for d in r.iter().filter(|x| x.3 == 1) {
        let last = d.10.expect("folder has last_desc_id");
        let inside: Vec<&Row> = r.iter().filter(|x| x.0 > d.0 && x.0 <= last).collect();
        // Everything inside the range is below the folder, everything below is inside the range.
        let below = r
            .iter()
            .filter(|x| {
                let mut p = x.1;
                while let Some(pid) = p {
                    if pid == d.0 {
                        return true;
                    }
                    p = r[pid as usize].1;
                }
                false
            })
            .count();
        assert_eq!(inside.len(), below, "subtree of {} is its id range", d.2);
    }
}

#[test]
fn hard_links_count_once_and_point_at_the_first_name() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("m.db");
    run(volume(), &db);
    let r = rows(&db);
    let names = [by_name(&r, "b.bin")[0], by_name(&r, "b-link.bin")[0]];
    let first = names.iter().min_by_key(|x| x.0).unwrap();
    let second = names.iter().max_by_key(|x| x.0).unwrap();
    assert_eq!(first.8, None, "the first name counts");
    assert_eq!(second.8, Some(first.0), "the other name points at it");
    assert_eq!((first.9, second.9), (Some(2), Some(2)));
    let c = Connection::open(&db).unwrap();
    let counted: i64 = c
        .query_row(
            "SELECT count(*) FROM entries WHERE name IN ('b.bin','b-link.bin') AND hardlink_of IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(counted, 1);
    // The category totals (built from counted files) include the file once.
    let n: i64 = c.query_row("SELECT coalesce(sum(file_count), 0) FROM category_totals", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 6, "a, b once, c, root.txt, $MFT, $UsnJrnl");
}

#[test]
fn the_stream_order_does_not_change_the_result() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("a.db"), tmp.path().join("b.db"));
    run(volume(), &a);
    let mut rev = volume();
    rev.reverse();
    run(rev, &b);
    assert_eq!(rows(&a), rows(&b));
}

struct BadSource {
    inner: VecSource,
    bad: u64,
}

impl MftSource for BadSource {
    fn info(&self) -> &MftInfo {
        self.inner.info()
    }

    fn for_each(&mut self, f: &mut dyn FnMut(MftEntry) -> bool) -> Result<ScanTally, MftError> {
        let mut t = self.inner.for_each(f)?;
        t.bad_records = self.bad;
        Ok(t)
    }
}

#[test]
fn too_many_unreadable_records_fall_back_to_the_walk() {
    let tmp = tempfile::tempdir().unwrap();
    let mut src = BadSource { inner: VecSource { info: info(), entries: volume() }, bad: 3 };
    let e = import(&mut src, &opts(&tmp.path().join("x.db")), &AtomicBool::new(false), &mut |_, _| {}).unwrap_err();
    assert!(matches!(e, ImportError::Mft(MftError::TooManyBadRecords { bad: 3, .. })), "{e}");
    // One bad record in a thousand is tolerated.
    let mut src = BadSource { inner: VecSource { info: info(), entries: volume() }, bad: 0 };
    assert!(import(&mut src, &opts(&tmp.path().join("y.db")), &AtomicBool::new(false), &mut |_, _| {}).is_ok());
}

#[test]
fn a_volume_without_a_root_record_is_refused_and_cancel_is_honoured() {
    let tmp = tempfile::tempdir().unwrap();
    let mut entries = volume();
    entries.retain(|e| e.frn != 5);
    let mut src = VecSource { info: info(), entries };
    assert!(import(&mut src, &opts(&tmp.path().join("x.db")), &AtomicBool::new(false), &mut |_, _| {}).is_err());

    // Cancel is polled every few thousand records.
    let many: Vec<MftEntry> =
        std::iter::once(dir(5, 5, ".")).chain((100..30_000u64).map(|i| file(i, 5, &format!("f{i}"), 1))).collect();
    let mut src = VecSource { info: info(), entries: many };
    let cancel = AtomicBool::new(false);
    let e = import(&mut src, &opts(&tmp.path().join("z.db")), &cancel, &mut |stage, _| {
        if stage == Stage::Indexing {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    })
    .unwrap_err();
    assert!(matches!(e, ImportError::Cancelled));
}

#[test]
fn quick_rescan_equals_a_full_scan_of_the_changed_volume() {
    let tmp = tempfile::tempdir().unwrap();
    let (full0, quick, full1) = (tmp.path().join("0.db"), tmp.path().join("q.db"), tmp.path().join("1.db"));
    run(volume(), &full0);

    // What changed: a.txt grew, a new file appeared, c.dat was deleted, "docs" was renamed,
    // and a new hard link to a.txt was created in "other".
    let mut a = file(31, 30, "a.txt", 5_000);
    a.names.push(name(40, "a-link.txt"));
    let new_file = file(50, 40, "new.txt", 20_000);
    let docs = dir(30, 5, "documents");
    let changes = Changes { entries: vec![a.clone(), new_file.clone(), docs.clone()], gone: vec![41] };

    let mut changed = volume();
    changed.retain(|e| e.frn != 41 && e.frn != 31 && e.frn != 30);
    changed.extend([a, new_file, docs]);
    run(changed, &full1);

    let sum = quick_rescan(&full0, changes, &opts(&quick), &AtomicBool::new(false), &mut |_, _| {}).unwrap();
    assert_eq!(rows(&quick), rows(&full1), "same ids, totals, links and paths as a full scan");
    let q = rows(&quick);
    assert!(by_path(&q, r"C:\documents").5 >= 15_000);
    assert!(by_name(&q, "c.dat").is_empty());
    assert_eq!(sum.stats.files, 6, "a (counted once despite two names), b, root.txt, new.txt, $MFT, $UsnJrnl");
}

#[test]
fn quick_rescan_refuses_a_walk_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("walk.db");
    let c = diskdash_core::store::open(&db).unwrap();
    c.execute("INSERT INTO meta (key, value) VALUES ('cluster_size', '4096')", []).unwrap();
    drop(c);
    let e =
        quick_rescan(&db, Changes::default(), &opts(&tmp.path().join("n.db")), &AtomicBool::new(false), &mut |_, _| {})
            .unwrap_err();
    assert!(e.to_string().contains("not made by an MFT scan"), "{e}");
}
