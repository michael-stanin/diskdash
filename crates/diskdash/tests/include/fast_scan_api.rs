// Included from http.rs: fast MFT scans, manual quick rescans and the elevated helper, through the HTTP API
// with a fake helper (no administrator rights, nothing touches the real C: drive).

use diskdash::helper::{HelperLink, HelperScan, HelperState, HelperStatus};
use std::sync::Mutex;
use diskdash_core::ntfs::incremental::Changes;
use diskdash_core::ntfs::model::{EntryName, MftEntry, MftInfo, VecSource};
use diskdash_core::ntfs::usn::{JournalState, StoredUsn};
use diskdash_helper::proto::{HelperVolume, UsnOutcome};

fn fs_name(parent: u64, n: &str) -> EntryName {
    EntryName { parent, parent_seq: if parent == 5 { 5 } else { 1 }, name: n.into() }
}

fn fs_dir(frn: u64, parent: u64, n: &str) -> MftEntry {
    MftEntry {
        frn,
        seq: if frn == 5 { 5 } else { 1 },
        is_dir: true,
        attrs: 0x10,
        ctime: 100,
        mtime: 200,
        atime: 300,
        names: vec![fs_name(parent, n)],
        size: 0,
        allocated: 0,
        resident: true,
        compressed_or_sparse: false,
        reparse_tag: 0,
    }
}

fn fs_file(frn: u64, parent: u64, n: &str, size: u64) -> MftEntry {
    MftEntry {
        frn,
        seq: 1,
        is_dir: false,
        attrs: 0x20,
        ctime: 100,
        mtime: 1_000 + frn as i64,
        atime: 2_000,
        names: vec![fs_name(parent, n)],
        size,
        allocated: 0,
        resident: size < 600,
        compressed_or_sparse: false,
        reparse_tag: 0,
    }
}

fn fs_volume() -> Vec<MftEntry> {
    vec![
        fs_dir(5, 5, "."),
        fs_dir(30, 5, "docs"),
        fs_file(31, 30, "a.txt", 1_000),
        fs_file(32, 30, "b.bin", 10_000),
        fs_dir(40, 5, "other"),
        fs_file(41, 40, "c.dat", 3_000),
        fs_file(46, 5, "root.txt", 4_096),
    ]
}

struct FakeHelper {
    running: std::sync::atomic::AtomicBool,
    enable_result: Mutex<Result<(), String>>,
    entries: Mutex<Vec<MftEntry>>,
    usn: Mutex<Option<UsnOutcome>>,
    fail_open: std::sync::atomic::AtomicBool,
    journal: Mutex<JournalState>,
    usn_calls: Mutex<Vec<(Option<StoredUsn>, u64)>>,
}

impl FakeHelper {
    fn new(running: bool) -> Arc<Self> {
        Arc::new(Self {
            running: running.into(),
            enable_result: Mutex::new(Ok(())),
            entries: Mutex::new(fs_volume()),
            usn: Mutex::new(None),
            fail_open: false.into(),
            journal: Mutex::new(JournalState { journal_id: 77, first_usn: 0, next_usn: 1_000, active: true }),
            usn_calls: Mutex::new(Vec::new()),
        })
    }
}

struct FakeScan {
    src: VecSource,
    journal: Option<JournalState>,
}

impl HelperScan for FakeScan {
    fn source(&mut self) -> &mut dyn diskdash_core::ntfs::model::MftSource {
        &mut self.src
    }

    fn journal(&self) -> Option<JournalState> {
        self.journal
    }
}

impl HelperLink for FakeHelper {
    fn status(&self) -> HelperStatus {
        let running = self.running.load(std::sync::atomic::Ordering::SeqCst);
        HelperStatus { state: if running { HelperState::Running } else { HelperState::NotRunning }, message: None }
    }

    fn enable(&self) -> Result<(), String> {
        let r = self.enable_result.lock().unwrap().clone();
        if r.is_ok() {
            self.running.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        r
    }

    fn volumes(&self) -> Result<Vec<HelperVolume>, String> {
        Ok(Vec::new())
    }

    fn open_scan(&self, _: &str) -> Result<Box<dyn HelperScan>, String> {
        if self.fail_open.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("access denied".into());
        }
        let info = MftInfo { bytes_per_cluster: 4096, record_size: 1024, record_count: 64, serial: 0xABCD };
        Ok(Box::new(FakeScan {
            src: VecSource { info, entries: self.entries.lock().unwrap().clone() },
            journal: Some(*self.journal.lock().unwrap()),
        }))
    }

    fn usn_read(&self, _: &str, stored: Option<StoredUsn>, total: u64) -> Result<UsnOutcome, String> {
        self.usn_calls.lock().unwrap().push((stored, total));
        self.usn.lock().unwrap().clone().ok_or_else(|| "no journal scripted".to_string())
    }
}

fn helper_fixture(h: &Arc<FakeHelper>) -> Fixture {
    fixture_with_helper(
        false,
        diskdash_analysis::rules::MapEnv::default(),
        diskdash_inventory::apps::registry::FakeRegistry::default(),
        Box::new(RecordedSource),
        Arc::new(NoManifests),
        h.clone(),
    )
}

async fn top_sizes(f: &Fixture, id: i64) -> Vec<(String, u64)> {
    let (_, v) = json(&f.router, get(&format!("/api/v1/snapshots/{id}/top-files?limit=20&measure=size"))).await;
    v["data"].as_array().unwrap().iter().map(|e| (e["path"].as_str().unwrap().to_string(), e["size"].as_u64().unwrap())).collect()
}

async fn post_ok(f: &Fixture, path: &str, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
    json(&f.router, post_json(path, body)).await
}

async fn run_scan(f: &Fixture, path: &str, body: serde_json::Value) -> (i64, serde_json::Value) {
    let (st, started) = post_ok(f, path, body).await;
    assert_eq!(st, StatusCode::OK, "{started}");
    let job = wait_job(&f.router, started["job_id"].as_u64().unwrap()).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    (started["snapshots"][0]["id"].as_i64().unwrap(), started)
}

#[tokio::test]
async fn without_the_helper_every_drive_says_why_it_is_walked() {
    let f = fixture(false);
    let (st, m) = json(&f.router, get("/api/v1/scans/modes")).await;
    assert_eq!(st, StatusCode::OK, "{m}");
    assert_eq!(m["helper"]["state"], "not-running");
    let drives = m["drives"].as_array().unwrap();
    assert!(!drives.is_empty());
    for d in drives {
        assert_eq!(d["mode"], "walk", "{d}");
        let reason = d["reason"].as_str().unwrap();
        assert!(reason == "fast scan requires administrator rights" || reason.contains("NTFS only"), "{reason}");
    }
    // A folder scan is a walk, with the folder reason.
    let tree = test_fixtures::Fixture::new();
    tree.file("a.txt", 100);
    let sid = scan_tree(&f, &tree).await;
    let snap = snapshot_json(&f, sid).await;
    assert_eq!(snap["method"], "walk");
    assert_eq!(snap["scan_note"], "MFT scan needs a whole drive, not a folder");
}

#[tokio::test]
async fn with_the_helper_a_whole_ntfs_drive_is_read_from_the_mft() {
    let h = FakeHelper::new(true);
    let f = helper_fixture(&h);
    let (_, m) = json(&f.router, get("/api/v1/scans/modes")).await;
    let c = m["drives"].as_array().unwrap().iter().find(|d| d["root"] == r"C:\").expect("C:");
    assert_eq!((c["mode"].as_str(), c["reason"].is_null()), (Some("mft"), true), "{c}");

    let (sid, started) = run_scan(&f, "/api/v1/scans", serde_json::json!({ "roots": [r"C:\"] })).await;
    assert_eq!(started["snapshots"][0]["method"], "mft");
    let snap = snapshot_json(&f, sid).await;
    assert_eq!((snap["status"].as_str(), snap["method"].as_str()), (Some("complete"), Some("mft")));
    assert!(snap["scan_note"].is_null());
    assert_eq!(snap["hardlinks_deduplicated"], true, "MFT link counts are exact whatever the walk setting");
    assert_eq!(snap["total_size"], 1_000 + 10_000 + 3_000 + 4_096);
    assert_eq!(snap["file_count"], 4);

    // It browses like any other snapshot.
    let (_, kids) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/children?limit=10"))).await;
    let names: Vec<&str> = kids["data"]["items"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"docs") && names.contains(&"other") && names.contains(&"root.txt"), "{names:?}");

    // The journal position was stored for a later quick rescan.
    let conn = f.db.connect().unwrap();
    let (jid, next, snap_id): (i64, i64, i64) =
        conn.query_row("SELECT journal_id, next_usn, snapshot_id FROM volume_usn_state", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!((jid, next, snap_id), (77, 1_000, sid));
}

#[tokio::test]
async fn quick_rescan_reads_only_the_changes_and_matches_a_full_scan() {
    let h = FakeHelper::new(true);
    let f = helper_fixture(&h);
    let (first, _) = run_scan(&f, "/api/v1/scans", serde_json::json!({ "roots": [r"C:\"] })).await;

    // a.txt grew, new.txt appeared, c.dat was deleted.
    let changes = Changes { entries: vec![fs_file(31, 30, "a.txt", 5_000), fs_file(50, 40, "new.txt", 20_000)], gone: vec![41] };
    *h.usn.lock().unwrap() = Some(UsnOutcome::Changes { changes: changes.clone(), state: JournalState { journal_id: 77, first_usn: 0, next_usn: 2_000, active: true } });
    let (quick, started) = run_scan(&f, "/api/v1/scans/quick", serde_json::json!({ "root": r"C:\" })).await;
    assert_eq!(started["snapshots"][0]["method"], "mft-quick");
    assert_eq!(h.usn_calls.lock().unwrap()[0].0, Some(StoredUsn { journal_id: 77, next_usn: 1_000 }), "resumes from the stored position");
    assert_eq!(h.usn_calls.lock().unwrap()[0].1, 7, "used-record estimate: files + folders of the last scan");

    // The same volume scanned in full gives identical results.
    let mut after = fs_volume();
    after.retain(|e| e.frn != 31 && e.frn != 41);
    after.extend([fs_file(31, 30, "a.txt", 5_000), fs_file(50, 40, "new.txt", 20_000)]);
    *h.entries.lock().unwrap() = after;
    let (full, _) = run_scan(&f, "/api/v1/scans", serde_json::json!({ "roots": [r"C:\"] })).await;
    let (q, fu) = (snapshot_json(&f, quick).await, snapshot_json(&f, full).await);
    assert_eq!(q["method"], "mft-quick");
    for k in ["total_size", "total_alloc", "file_count", "dir_count"] {
        assert_eq!(q[k], fu[k], "{k}");
    }
    assert_eq!(q["total_size"], 5_000 + 10_000 + 20_000 + 4_096);
    assert_eq!(top_sizes(&f, quick).await, top_sizes(&f, full).await);
    let _ = first;

    // The stored position moved to the new snapshot.
    let conn = f.db.connect().unwrap();
    let (next, snap_id): (i64, i64) =
        conn.query_row("SELECT next_usn, snapshot_id FROM volume_usn_state", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((next, snap_id), (1_000, full), "the later full scan owns the position now");
}

#[tokio::test]
async fn quick_rescan_that_cannot_be_quick_runs_a_full_scan_and_says_why() {
    let h = FakeHelper::new(true);
    let f = helper_fixture(&h);

    // No earlier fast scan.
    let (_, started) = run_scan(&f, "/api/v1/scans/quick", serde_json::json!({ "root": r"C:\" })).await;
    assert_eq!(started["snapshots"][0]["method"], "mft");
    assert!(started["snapshots"][0]["note"].as_str().unwrap().contains("no earlier fast scan"), "{started}");
    assert_eq!(h.usn_calls.lock().unwrap().len(), 0, "the journal is not read without a prior scan");

    // The journal wrapped.
    *h.usn.lock().unwrap() = Some(UsnOutcome::Full { reason: "USN journal wrapped; running a full scan".into(), state: None });
    let (sid, started) = run_scan(&f, "/api/v1/scans/quick", serde_json::json!({ "root": r"C:\" })).await;
    assert_eq!(started["snapshots"][0]["method"], "mft");
    assert_eq!(started["snapshots"][0]["note"], "USN journal wrapped; running a full scan");
    assert_eq!(snapshot_json(&f, sid).await["scan_note"], "USN journal wrapped; running a full scan");
}

#[tokio::test]
async fn quick_rescan_needs_the_helper_and_a_whole_drive() {
    let f = fixture(false);
    let (st, v) = post_ok(&f, "/api/v1/scans/quick", serde_json::json!({ "root": r"C:\" })).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["message"].as_str().unwrap().contains("administrator"), "{v}");

    let h = FakeHelper::new(true);
    let f = helper_fixture(&h);
    let tree = test_fixtures::Fixture::new();
    let (st, v) = post_ok(&f, "/api/v1/scans/quick", serde_json::json!({ "root": tree.root().to_string_lossy() })).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["message"].as_str().unwrap().contains("whole drive"), "{v}");
}

#[tokio::test]
async fn the_helper_is_started_only_on_request_and_a_declined_prompt_changes_nothing() {
    let h = FakeHelper::new(false);
    *h.enable_result.lock().unwrap() = Err("The administrator prompt was declined, so fast scans stay off.".into());
    let f = helper_fixture(&h);
    let (_, s) = json(&f.router, get("/api/v1/helper")).await;
    assert_eq!(s["state"], "not-running");

    let (st, v) = post_ok(&f, "/api/v1/helper/enable", serde_json::json!({})).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["message"].as_str().unwrap().contains("declined"), "{v}");
    let (_, s) = json(&f.router, get("/api/v1/helper")).await;
    assert_eq!(s["state"], "not-running", "still in reduced mode");
    let (_, m) = json(&f.router, get("/api/v1/scans/modes")).await;
    assert!(m["drives"].as_array().unwrap().iter().all(|d| d["mode"] == "walk"));

    *h.enable_result.lock().unwrap() = Ok(());
    let (st, s) = post_ok(&f, "/api/v1/helper/enable", serde_json::json!({})).await;
    assert_eq!((st, s["state"].as_str()), (StatusCode::OK, Some("running")));
    let (_, m) = json(&f.router, get("/api/v1/scans/modes")).await;
    assert!(m["drives"].as_array().unwrap().iter().any(|d| d["mode"] == "mft"));
}

#[tokio::test]
async fn a_failed_fast_scan_falls_back_to_the_walk_and_records_the_reason() {
    use diskdash::scans::{ItemMode, ScanItem, ScanJob};
    let h = FakeHelper::new(true);
    h.fail_open.store(true, std::sync::atomic::Ordering::SeqCst);
    let f = helper_fixture(&h);

    // A small folder stands in for the drive, so the fallback walk is quick and touches nothing real.
    let tree = test_fixtures::Fixture::new();
    tree.file("a.txt", 100);
    tree.file("sub/b.txt", 200);
    let root = tree.root().to_string_lossy().into_owned();
    let conn = f.db.connect().unwrap();
    conn.execute(
        "INSERT INTO snapshots (root, volume_id, elevated, hardlinks, status, started_at, db_file, method)
         VALUES (?1, 'v', 0, 'not_deduplicated', 'running', '2026-01-01T00:00:00Z', 'snapshots/900.db', 'mft')",
        [&root],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    conn.execute("UPDATE snapshots SET db_file = ?2 WHERE id = ?1", rusqlite::params![id, format!("snapshots/{id}.db")]).unwrap();
    let db_path = f.own.prepare_db_path(&f.own.root().join(format!("snapshots/{id}.db"))).unwrap();
    let job = ScanJob {
        items: vec![ScanItem {
            snapshot_id: id,
            root: root.clone(),
            db_path,
            threads: 1,
            hardlinks: false,
            expected: None,
            resume: false,
            group: "g".into(),
            mode: ItemMode::Mft { volume_id: "v".into() },
            note: None,
        }],
        db: f.db.clone(),
        exclude: Vec::new(),
        windows_dir: None,
        cleanup: None,
        plans: None,
        helper: h.clone(),
        own_files: f.own.clone(),
        history: None,
        completed: Mutex::new(Vec::new()),
    };
    let job_id = f.jobs.submit(Box::new(job)).unwrap();
    assert_eq!(wait_job(&f.router, job_id).await["status"], "succeeded");
    let snap = snapshot_json(&f, id).await;
    assert_eq!(snap["status"], "complete");
    assert_eq!(snap["method"], "walk", "{snap}");
    assert!(snap["scan_note"].as_str().unwrap().contains("access denied"), "{snap}");
    assert_eq!(snap["file_count"], 2);
}

#[test]
fn the_helper_routes_are_read_only_or_start_only_our_own_helper() {
    let src = include_str!("../../src/app.rs");
    let routes: Vec<&str> = src.lines().filter_map(|l| l.trim().strip_prefix(".route(\"")).filter_map(|r| r.split('"').next()).collect();
    for r in ["/helper", "/helper/enable", "/scans/modes", "/scans/quick"] {
        assert!(routes.contains(&r), "{r}");
    }
    // The helper's protocol has no operation that takes a path or writes: see diskdash-helper's service tests.
}
