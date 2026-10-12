#![allow(clippy::unwrap_used)]
//! End-to-end scanner tests on generated fixture trees.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use diskdash_core::scan::{scan, scan_blocking, ScanError, ScanOptions, ScanStatus};
use rusqlite::Connection;
use test_fixtures::{assert_tree_unchanged, cluster_size, manifest, round_up, Fixture};

const KIB: u64 = 1024;
const MIB: u64 = 1024 * 1024;

fn opts(root: &Path, db: &Path, hardlinks: bool) -> ScanOptions {
    ScanOptions {
        root: root.to_path_buf(),
        db_path: db.to_path_buf(),
        threads: 2,
        detect_hardlinks: hardlinks,
        exclude: Vec::new(),
        resume: false,
        windows_dir: Some(r"C:\Windows".into()),
    }
}

fn db_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

struct Entry {
    id: i64,
    parent: Option<i64>,
    name: String,
    kind: i64,
    size: i64,
    alloc: i64,
    flags: i64,
    hardlink_of: Option<i64>,
    path: Option<String>,
    last_desc: Option<i64>,
    max_mtime: Option<i64>,
    mtime: Option<i64>,
    link_target: Option<String>,
}

fn entries(conn: &Connection) -> Vec<Entry> {
    let mut stmt = conn
        .prepare(
            "SELECT id, parent_id, name, kind, size, alloc, flags, hardlink_of, path, last_desc_id, max_mtime, mtime, link_target
             FROM entries ORDER BY id",
        )
        .unwrap();
    stmt.query_map([], |r| {
        Ok(Entry {
            id: r.get(0)?,
            parent: r.get(1)?,
            name: r.get(2)?,
            kind: r.get(3)?,
            size: r.get(4)?,
            alloc: r.get(5)?,
            flags: r.get(6)?,
            hardlink_of: r.get(7)?,
            path: r.get(8)?,
            last_desc: r.get(9)?,
            max_mtime: r.get(10)?,
            mtime: r.get(11)?,
            link_target: r.get(12)?,
        })
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

fn by_name<'a>(es: &'a [Entry], name: &str) -> &'a Entry {
    es.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("no entry {name}"))
}

/// Every directory's totals equal its direct counted files plus child dirs,
/// and its id range contains exactly its descendants.
fn check_invariants(es: &[Entry]) {
    let by_id: HashMap<i64, &Entry> = es.iter().map(|e| (e.id, e)).collect();
    let mut children: HashMap<i64, Vec<&Entry>> = HashMap::new();
    for e in es {
        if let Some(p) = e.parent {
            if e.id != 0 {
                children.entry(p).or_default().push(e);
            }
        }
    }
    fn descendants(id: i64, children: &HashMap<i64, Vec<&Entry>>, out: &mut Vec<i64>) {
        for c in children.get(&id).into_iter().flatten() {
            out.push(c.id);
            descendants(c.id, children, out);
        }
    }
    for d in es.iter().filter(|e| e.kind == 1) {
        let kids = children.get(&d.id).cloned().unwrap_or_default();
        let sum: i64 = kids.iter().filter(|k| k.kind != 2 && k.hardlink_of.is_none()).map(|k| k.size).sum();
        assert_eq!(d.size, sum, "size of {} = sum of children", d.name);
        let asum: i64 = kids.iter().filter(|k| k.kind != 2 && k.hardlink_of.is_none()).map(|k| k.alloc).sum();
        assert_eq!(d.alloc, asum, "alloc of {}", d.name);
        let mut desc = Vec::new();
        descendants(d.id, &children, &mut desc);
        let last = d.last_desc.unwrap();
        let in_range: Vec<i64> = by_id.keys().copied().filter(|id| *id > d.id && *id <= last).collect();
        let (mut a, mut b) = (desc.clone(), in_range);
        a.sort();
        b.sort();
        assert_eq!(a, b, "id range of {} holds exactly its descendants", d.name);
    }
}

#[test]
fn exact_totals_on_the_full_fixture() {
    let mut f = Fixture::new();
    let cluster = cluster_size(f.root());
    f.file("docs/a.txt", 1);
    f.file("docs/b.pdf", 3 * KIB);
    f.file("media/big.mkv", 2 * MIB);
    let target = f.dir("media");
    f.junction("media-link", &target);
    f.junction("media/loop", f.root());
    f.file("x/data.bin", 512 * KIB);
    f.hard_link("x/data.bin", "y/data.bin");
    f.compressed("z/zeros.bin", 4 * MIB);
    f.sparse("z/sparse.bin", 64 * MIB, 64 * KIB);
    let long = f.long_path_file(600, 100);
    f.dir("locked/inner");
    f.file("locked/inner/secret.bin", 10 * KIB);
    f.deny("locked");
    f.file("root.txt", 5000);

    let before = manifest(f.root());
    let tmp = db_dir();
    let db = tmp.path().join("snap.db");
    let summary = scan_blocking(opts(f.root(), &db, true)).unwrap();
    assert_tree_unchanged(f.root(), &before);

    assert_eq!(summary.status, ScanStatus::Complete);
    assert!(summary.hardlinks_deduplicated);
    let conn = Connection::open(&db).unwrap();
    let es = entries(&conn);
    check_invariants(&es);

    // Junctions are zero-size link entries with their target; never traversed.
    let link = by_name(&es, "media-link");
    assert_eq!((link.kind, link.size), (2, 0));
    assert_eq!(link.link_target.as_deref().map(str::to_lowercase), Some(target.to_string_lossy().to_lowercase()));
    assert_eq!(by_name(&es, "loop").kind, 2);
    assert_eq!(es.iter().filter(|e| e.name == "big.mkv").count(), 1, "junction not double counted");

    // Hard links counted once.
    let copies: Vec<_> = es.iter().filter(|e| e.name == "data.bin").collect();
    assert_eq!(copies.len(), 2);
    assert_eq!(copies.iter().filter(|e| e.hardlink_of.is_some()).count(), 1);

    // Allocated sizes.
    assert_eq!(by_name(&es, "a.txt").alloc as u64, round_up(1, cluster), "cluster slack (or 0 if MFT-resident)");
    assert!((by_name(&es, "zeros.bin").alloc as u64) < 4 * MIB, "compressed allocates less");
    assert_eq!(by_name(&es, "zeros.bin").size as u64, 4 * MIB);
    let sparse = by_name(&es, "sparse.bin");
    assert_eq!(sparse.size as u64, 64 * MIB);
    assert!((sparse.alloc as u64) < MIB, "sparse allocates only written ranges: {}", sparse.alloc);

    // Denied folder: recorded, flagged, error logged, scan continued.
    let locked = by_name(&es, "locked");
    assert_eq!(locked.flags & 1, 1);
    assert_eq!(locked.size, 0);
    let denied_errors: i64 =
        conn.query_row("SELECT count(*) FROM errors WHERE code = 5 AND path LIKE '%locked'", [], |r| r.get(0)).unwrap();
    assert_eq!(denied_errors, 1);
    assert_eq!(summary.stats.access_denied, 1);

    // Long path present, stored without the \\?\ prefix.
    let leaf_dir: &Entry = es.iter().find(|e| e.id == by_name(&es, "leaf.bin").parent.unwrap()).unwrap();
    let full = format!("{}\\leaf.bin", leaf_dir.path.as_deref().unwrap());
    assert!(full.len() >= 600 && !full.starts_with(r"\\?\"));
    assert_eq!(PathBuf::from(&full), long);

    // Root totals = unique logical bytes.
    let unique = 1 + 3 * KIB + 2 * MIB + 512 * KIB + 4 * MIB + 64 * MIB + 100 + 5000;
    let root = es.iter().find(|e| e.id == 0).unwrap();
    assert_eq!(root.size as u64, unique);
    assert_eq!(summary.stats.size, unique);

    // Category totals add up to the root.
    let cat_sum: i64 = conn.query_row("SELECT sum(size) FROM category_totals", [], |r| r.get(0)).unwrap();
    assert_eq!(cat_sum as u64, unique);
    let video: i64 = conn.query_row("SELECT size FROM category_totals WHERE category = 0", [], |r| r.get(0)).unwrap();
    assert_eq!(video as u64, 2 * MIB);

    // Newest descendant time on a directory.
    let docs = by_name(&es, "docs");
    let newest = es.iter().filter(|e| e.parent == Some(docs.id)).filter_map(|e| e.mtime).max();
    assert_eq!(docs.max_mtime, newest);
}

#[test]
fn hardlink_detection_off_double_counts_and_is_flagged() {
    let f = Fixture::new();
    f.file("x/data.bin", 100 * KIB);
    f.hard_link("x/data.bin", "y/data.bin");
    let tmp = db_dir();
    let db = tmp.path().join("s.db");
    let s = scan_blocking(opts(f.root(), &db, false)).unwrap();
    assert!(!s.hardlinks_deduplicated);
    assert_eq!(s.stats.size, 200 * KIB);
    let conn = Connection::open(&db).unwrap();
    let mode: String = conn.query_row("SELECT value FROM meta WHERE key = 'hardlinks'", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "not_deduplicated");
}

#[test]
fn excluded_directory_is_skipped() {
    let f = Fixture::new();
    f.file("keep/a.bin", 1000);
    f.file("diskdash-data/snap.db", 5000);
    let tmp = db_dir();
    let db = tmp.path().join("s.db");
    let mut o = opts(f.root(), &db, true);
    o.exclude = vec![f.path("diskdash-data").to_string_lossy().into_owned()];
    let s = scan_blocking(o).unwrap();
    assert_eq!(s.stats.size, 1000);
}

#[test]
fn unreadable_root_fails_the_scan() {
    let tmp = db_dir();
    let err =
        scan_blocking(opts(Path::new(r"C:\definitely-not-here-diskdash"), &tmp.path().join("s.db"), true)).unwrap_err();
    assert!(matches!(err, ScanError::RootUnreadable { .. } | ScanError::Volume { .. }), "{err}");
}

fn wide_tree(f: &Fixture, dirs: usize, files: usize) {
    for d in 0..dirs {
        for i in 0..files {
            f.file(&format!("d{d:03}/f{i:04}.dat"), (i as u64 % 7) * 100 + 1);
        }
    }
}

#[test]
fn cancel_then_resume_matches_a_fresh_scan() {
    let f = Fixture::new();
    wide_tree(&f, 40, 150);
    let tmp = db_dir();

    let fresh = scan_blocking(opts(f.root(), &tmp.path().join("fresh.db"), true)).unwrap();

    let db = tmp.path().join("resumable.db");
    let mut o = opts(f.root(), &db, true);
    o.threads = 1;
    let handle = scan(o.clone()).unwrap();
    while handle.progress().units_done < 5 {
        std::thread::sleep(Duration::from_millis(1));
    }
    let start = Instant::now();
    handle.cancel();
    let partial = handle.join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(2), "cancel within 2 s");
    assert_eq!(partial.status, ScanStatus::Partial);

    let conn = Connection::open(&db).unwrap();
    let done: i64 = conn.query_row("SELECT count(*) FROM units WHERE status = 'done'", [], |r| r.get(0)).unwrap();
    assert!((5..41).contains(&done), "some units done, not all: {done}");
    drop(conn);

    o.resume = true;
    let resumed = scan_blocking(o).unwrap();
    assert_eq!(resumed.status, ScanStatus::Complete);
    assert_eq!(resumed.stats, fresh.stats);
    let conn = Connection::open(&db).unwrap();
    check_invariants(&entries(&conn));
    let resumed_flag: String =
        conn.query_row("SELECT value FROM meta WHERE key = 'resumed'", [], |r| r.get(0)).unwrap();
    assert_eq!(resumed_flag, "1");
}

#[test]
fn resume_refuses_a_different_volume() {
    let f = Fixture::new();
    wide_tree(&f, 3, 5);
    let tmp = db_dir();
    let db = tmp.path().join("s.db");
    scan_blocking(opts(f.root(), &db, true)).unwrap();
    let conn = Connection::open(&db).unwrap();
    conn.execute("UPDATE meta SET value = '12345' WHERE key = 'volume_serial'", []).unwrap();
    conn.execute("UPDATE units SET status = 'pending' WHERE block = 1", []).unwrap();
    drop(conn);
    let mut o = opts(f.root(), &db, true);
    o.resume = true;
    assert!(matches!(scan_blocking(o), Err(ScanError::SerialMismatch { expected: 12345, .. })));
}

/// 1 M-file benchmark (opt-in): `cargo test -p diskdash-core --release -- --ignored bench --nocapture`
#[test]
#[ignore]
fn bench_one_million_files() {
    let f = Fixture::new();
    let t0 = Instant::now();
    for d in 0..1000 {
        f.empty_files(&format!("d{d:04}"), 1000);
    }
    println!("created 1M files in {:?}", t0.elapsed());
    let tmp = db_dir();
    for hardlinks in [true, false] {
        let t = Instant::now();
        let s = scan_blocking(ScanOptions {
            threads: 4,
            ..opts(f.root(), &tmp.path().join(format!("b{hardlinks}.db")), hardlinks)
        })
        .unwrap();
        println!("hardlinks={hardlinks}: {} files in {:?}", s.stats.files, t.elapsed());
    }
}
