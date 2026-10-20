//! Opt-in, administrator-only: the MFT scanner against the directory walk on a throw-away VHDX.
//!
//! Run elevated with:
//!   `$env:DISKDASH_ADMIN_TESTS = "1"; cargo test -p diskdash-core --test admin_ntfs -- --ignored --nocapture`
//!
//! The test creates its own VHDX, formats it NTFS, fills it with a tree that has hard links,
//! a junction, sparse and compressed files and a long path, scans it both ways, changes it,
//! quick-rescans through the USN journal, and finally detaches and deletes the VHDX.

#![cfg(windows)]
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use diskdash_core::ntfs::import::{import, ImportOptions};
use diskdash_core::ntfs::incremental::{quick_rescan, Changes};
use diskdash_core::ntfs::model::MftSource;
use diskdash_core::ntfs::usn::distinct_frns;
use diskdash_core::ntfs::volume::{Volume, VolumeMft};
use diskdash_core::scan::{scan_blocking, ScanOptions};
use rusqlite::Connection;
use test_fixtures::vhdx::{admin_tests_enabled, Vhdx};
use test_fixtures::Fixture;

/// (size, alloc, files, dirs) of the folder at `path` in a snapshot database.
fn folder_totals(db: &Path, path: &str) -> (i64, i64, i64, i64) {
    let c = Connection::open(db).unwrap();
    c.query_row(
        "SELECT size, alloc, coalesce(file_count, 0), coalesce(dir_count, 0) FROM entries WHERE lower(path) = lower(?1)",
        [path],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .unwrap_or_else(|e| panic!("no folder {path} in {}: {e}", db.display()))
}

/// Every entry below `prefix` as `path → (kind, size, alloc)`, ignoring ids and timestamps.
fn tree(db: &Path, prefix: &str) -> BTreeMap<String, (i64, i64, i64)> {
    let c = Connection::open(db).unwrap();
    let mut out = BTreeMap::new();
    let mut stmt = c
        .prepare(
            "SELECT coalesce(e.path, p.path || '\\' || e.name), e.kind, e.size, e.alloc
             FROM entries e LEFT JOIN entries p ON p.id = e.parent_id WHERE e.hardlink_of IS NULL",
        )
        .unwrap();
    for row in stmt
        .query_map([], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
        })
        .unwrap()
    {
        let (path, kind, size, alloc) = row.unwrap();
        if let Some(p) = path.filter(|p| p.to_lowercase().starts_with(&prefix.to_lowercase())) {
            out.insert(p.to_lowercase(), (kind, size, alloc));
        }
    }
    out
}

fn peak_working_set_mb() -> u64 {
    use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS::default();
    // SAFETY: the structure is local and its size is passed.
    let _ = unsafe {
        K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32)
    };
    c.PeakWorkingSetSize as u64 / (1 << 20)
}

fn mft_import(volume: &str, db: &Path, root: &str) -> diskdash_core::scan::ScanSummary {
    let mut src = VolumeMft::new(Volume::open(volume).expect("open the volume")).expect("read the MFT");
    let opts =
        ImportOptions { root: root.into(), db_path: db.to_path_buf(), windows_dir: None, max_bad_fraction: 0.01 };
    import(&mut src, &opts, &AtomicBool::new(false), &mut |_, _| {}).expect("MFT import")
}

#[test]
#[ignore = "needs an elevated process and DISKDASH_ADMIN_TESTS=1 (creates and removes its own VHDX)"]
fn mft_scan_equals_the_directory_walk_and_quick_rescan_equals_a_full_scan() {
    if !admin_tests_enabled() {
        eprintln!("skipped: set DISKDASH_ADMIN_TESTS=1 and run elevated");
        return;
    }
    let vhdx = Vhdx::create(512).expect("create the VHDX");
    vhdx.create_usn_journal().expect("enable the change journal");
    let tmp = tempfile::tempdir().unwrap();

    // ---- A tree with the awkward cases ---------------------------------------------------
    let fx = Fixture::in_dir(Path::new(&vhdx.root()));
    fx.file("docs/a.txt", 1_000);
    fx.file("docs/big.bin", 3 << 20);
    fx.file("docs/tiny.txt", 10);
    fx.hard_link("docs/big.bin", "other/big-link.bin");
    fx.compressed("other/packed.dat", 1 << 20);
    fx.sparse("other/sparse.dat", 8 << 20, 64 << 10);
    fx.junction("junction-to-docs", &fx.path("docs"));
    fx.long_path_file(280, 4_096);
    fx.empty_files("empty", 20);
    let fixture_path = fx.root().to_string_lossy().trim_end_matches('\\').to_string();

    // ---- Walk vs MFT ------------------------------------------------------------------
    let walk_db = tmp.path().join("walk.db");
    scan_blocking(ScanOptions {
        root: fx.root().to_path_buf(),
        db_path: walk_db.clone(),
        threads: 2,
        detect_hardlinks: true,
        exclude: Vec::new(),
        resume: false,
        windows_dir: None,
    })
    .expect("walk");
    let journal0 = Volume::open(&vhdx.volume_path()).unwrap().query_usn().unwrap();
    assert!(journal0.active, "the test enabled the journal");

    let started = Instant::now();
    let mft_db = tmp.path().join("mft.db");
    mft_import(&vhdx.volume_path(), &mft_db, &vhdx.root());
    let took = started.elapsed();
    eprintln!("MFT import of the fixture volume: {took:?}, peak working set {} MB", peak_working_set_mb());
    assert!(peak_working_set_mb() < 500, "memory bound");

    let walk_root = folder_totals(&walk_db, &fixture_path);
    let mft_dir = folder_totals(&mft_db, &fixture_path);
    assert_eq!(walk_root, mft_dir, "(size, alloc, files, dirs) of the fixture folder: walk vs MFT");
    assert_eq!(tree(&walk_db, &fixture_path), tree(&mft_db, &fixture_path), "entry by entry");

    // ---- Quick rescan vs full scan ----------------------------------------------------------
    let state0 = Volume::open(&vhdx.volume_path()).unwrap().query_usn().unwrap();
    fx.file("docs/a.txt", 50_000);
    fx.file("docs/new.txt", 123_456);
    fx.remove("docs/tiny.txt");
    fx.file("new-dir/inside.bin", 70_000);

    let vol = Volume::open(&vhdx.volume_path()).unwrap();
    let state1 = vol.query_usn().unwrap();
    let (_, changes) = vol.read_usn(&state1, state0.next_usn).unwrap().expect("journal did not wrap");
    let mut ch = Changes::default();
    for frn in distinct_frns(&changes) {
        match vol.entry_for(frn).unwrap() {
            Some(e) => ch.entries.push(e),
            None => ch.gone.push(frn),
        }
    }
    assert!(ch.entries.len() + ch.gone.len() < 200, "only a handful of records changed");

    let quick_db = tmp.path().join("quick.db");
    let opts =
        ImportOptions { root: vhdx.root(), db_path: quick_db.clone(), windows_dir: None, max_bad_fraction: 0.01 };
    quick_rescan(&mft_db, ch, &opts, &AtomicBool::new(false), &mut |_, _| {}).expect("quick rescan");
    let full_db = tmp.path().join("full.db");
    mft_import(&vhdx.volume_path(), &full_db, &vhdx.root());
    assert_eq!(folder_totals(&quick_db, &fixture_path), folder_totals(&full_db, &fixture_path));
    assert_eq!(tree(&quick_db, &fixture_path), tree(&full_db, &fixture_path), "quick rescan equals a full scan");

    // The helper-facing source reports the volume it read.
    let src = VolumeMft::new(Volume::open(&vhdx.volume_path()).unwrap()).unwrap();
    assert!(src.info().record_count > 0);
}
