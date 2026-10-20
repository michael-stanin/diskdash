// Included from http.rs: history, trends and old-scan clean-up through the HTTP API.

use diskdash::history::{retain, Stop};

fn hist_settings(keep: u32) -> Request<Body> {
    let mut s = default_settings();
    s["retention_keep"] = serde_json::json!(keep);
    put_settings(s)
}

fn snap_file(f: &Fixture, snapshot: &serde_json::Value) -> std::path::PathBuf {
    let conn = f.db.connect().unwrap();
    let file: String = conn.query_row("SELECT db_file FROM snapshots WHERE id = ?1", [snapshot["id"].as_i64().unwrap()], |r| r.get(0)).unwrap();
    f.own.root().join(file)
}

async fn snapshot_json(f: &Fixture, id: i64) -> serde_json::Value {
    json(&f.router, get(&format!("/api/v1/snapshots/{id}"))).await.1
}

#[tokio::test]
async fn diff_names_the_folders_where_the_change_is() {
    let f = fixture(false);
    let tree = test_fixtures::Fixture::new();
    tree.file("docs/a.txt", 1 << 20);
    tree.file("media/old.mkv", 4 << 20);
    tree.file("keep/k.bin", 1 << 20);
    let first = scan_tree(&f, &tree).await;

    tree.file("docs/b.txt", 3 << 20);
    tree.remove("media/old.mkv");
    tree.file("big/new.bin", 101 << 20);
    let second = scan_tree(&f, &tree).await;

    let (status, d) = json(&f.router, get(&format!("/api/v1/history/diff?old={first}&new={second}"))).await;
    assert_eq!(status, StatusCode::OK, "{d}");
    let paths = |k: &str| d[k].as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert!(paths("growing").iter().any(|p| p.ends_with("docs")), "{d}");
    assert!(paths("shrinking").iter().any(|p| p.ends_with("media")), "{d}");
    assert!(paths("appeared").iter().any(|p| p.ends_with("big")), "{d}");
    assert!(d["disappeared"].as_array().unwrap().is_empty());
    assert_eq!(d["new_large_available"], true);
    let large = d["new_large_files"].as_array().unwrap();
    assert_eq!(large.len(), 1, "{d}");
    assert!(large[0]["path"].as_str().unwrap().ends_with(r"big\new.bin"));
    assert!(d["total_delta"].as_i64().unwrap() > 90 << 20);
    assert!(!d["categories"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn diff_needs_two_scans_of_the_same_root_in_order() {
    let f = fixture(false);
    let (a, b) = (test_fixtures::Fixture::new(), test_fixtures::Fixture::new());
    a.file("x.txt", 10);
    b.file("y.txt", 10);
    let (sa, sb) = (scan_tree(&f, &a).await, scan_tree(&f, &b).await);
    let (st, v) = json(&f.router, get(&format!("/api/v1/history/diff?old={sa}&new={sb}"))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["message"].as_str().unwrap().contains("same root"), "{v}");

    let sa2 = scan_tree(&f, &a).await;
    let (st, v) = json(&f.router, get(&format!("/api/v1/history/diff?old={sa2}&new={sa}"))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let (st, _) = json(&f.router, get("/api/v1/history/diff?old=99999&new=99998")).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn volume_trends_say_when_there_is_not_enough_history() {
    let f = fixture(false);
    let (st, v) = json(&f.router, get("/api/v1/history/volumes")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // The drive list is refreshed on start, so the recorded machine's volumes have a first sample.
    let vols = v["volumes"].as_array().unwrap();
    assert!(!vols.is_empty(), "{v}");
    for vol in vols {
        assert_eq!(vol["forecast"]["state"], "not-enough-history", "{vol}");
        assert!(!vol["samples"].as_array().unwrap().is_empty());
    }
}

#[tokio::test]
async fn old_scans_shrink_to_folder_totals_and_pinned_ones_stay() {
    let f = fixture(false);
    assert_eq!(json(&f.router, hist_settings(2)).await.0, StatusCode::OK);
    let tree = test_fixtures::Fixture::new();
    tree.file("a/one.txt", 1 << 20);
    tree.file("a/deep/er/est/est2/x.txt", 2 << 20);
    let s1 = scan_tree(&f, &tree).await;
    let s1_json = snapshot_json(&f, s1).await;
    let s1_file = snap_file(&f, &s1_json);
    let (st, pinned) = json(
        &f.router,
        req(Method::POST, &format!("/api/v1/snapshots/{s1}/pin"))
            .header("x-diskdash-token", TOKEN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(json_body(serde_json::json!({ "pinned": true })))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{pinned}");
    assert_eq!(pinned["pinned"], true);

    tree.file("a/two.txt", 1 << 20);
    let s2 = scan_tree(&f, &tree).await;
    let s2_file = snap_file(&f, &snapshot_json(&f, s2).await);
    tree.file("a/three.txt", 1 << 20);
    let s3 = scan_tree(&f, &tree).await;
    tree.file("a/four.txt", 1 << 20);
    let s4 = scan_tree(&f, &tree).await;

    // keep = 2 unpinned: s4 and s3 stay full, s2 is shrunk, s1 is pinned so it stays.
    assert!(s1_file.exists(), "pinned scan keeps its file");
    assert_eq!(snapshot_json(&f, s1).await["summarised"], false);
    assert!(!s2_file.exists(), "shrunk scan's file is removed");
    let s2_now = snapshot_json(&f, s2).await;
    assert_eq!(s2_now["summarised"], true);
    assert_eq!(s2_now["summary_depth"], 4);
    assert_eq!(snapshot_json(&f, s3).await["summarised"], false);

    // A shrunk scan cannot be browsed, but it can still be compared.
    let (st, v) = json(&f.router, get(&format!("/api/v1/snapshots/{s2}/children"))).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let (st, d) = json(&f.router, get(&format!("/api/v1/history/diff?old={s2}&new={s4}"))).await;
    assert_eq!(st, StatusCode::OK, "{d}");
    assert_eq!(d["new_large_available"], false);
    assert_eq!(d["depth_limit"], 4);
    let growing: Vec<&str> = d["growing"].as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap()).collect();
    assert!(growing.iter().any(|p| p.ends_with(r"\a")), "{d}");
    assert!(growing.iter().all(|p| !p.ends_with("est2")), "folders deeper than the kept depth are not compared: {d}");

    // Snapshot metadata is still listed after shrinking.
    let (_, list) = json(&f.router, get("/api/v1/snapshots")).await;
    assert!(list.to_string().contains(&format!("\"id\":{s2}")), "{list}");
}

#[tokio::test]
async fn an_interrupted_cleanup_loses_nothing_and_finishes_next_time() {
    let f = fixture(false);
    let tree = test_fixtures::Fixture::new();
    tree.file("a/one.txt", 1 << 20);
    let s1 = scan_tree(&f, &tree).await;
    let s1_file = snap_file(&f, &snapshot_json(&f, s1).await);
    tree.file("a/two.txt", 1 << 20);
    let s2 = scan_tree(&f, &tree).await;
    assert!(s1_file.exists());
    let rows = |sql: &str| -> i64 { f.db.connect().unwrap().query_row(sql, [], |r| r.get(0)).unwrap() };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;

    // Stop before the catalog switches: everything is rolled back, the scan is intact.
    retain(&f.db, &f.own, 1, 4, now, Stop::BeforeSwitch).unwrap();
    assert_eq!(snapshot_json(&f, s1).await["summarised"], false);
    assert_eq!(rows("SELECT count(*) FROM snapshot_summaries"), 0);
    assert!(s1_file.exists());
    let (st, _) = json(&f.router, get(&format!("/api/v1/snapshots/{s1}/children"))).await;
    assert_eq!(st, StatusCode::OK);

    // Stop after the switch but before the file is removed: summary is complete, file lingers.
    retain(&f.db, &f.own, 1, 4, now, Stop::BeforeFileRemoval).unwrap();
    assert_eq!(snapshot_json(&f, s1).await["summarised"], true);
    assert!(rows("SELECT count(*) FROM snapshot_summaries") > 0);
    assert!(s1_file.exists());
    let (st, d) = json(&f.router, get(&format!("/api/v1/history/diff?old={s1}&new={s2}"))).await;
    assert_eq!(st, StatusCode::OK, "{d}");

    // Startup reconciliation removes the leftover file without touching the summary.
    let snaps_dir = s1_file.parent().unwrap().to_path_buf();
    diskdash::snapshots::reconcile(&f.db, &f.own, &snaps_dir).unwrap();
    assert!(!s1_file.exists());
    assert_eq!(snapshot_json(&f, s1).await["status"], "complete", "a shrunk scan is not marked failed");

    // Running it again is a no-op.
    let again = retain(&f.db, &f.own, 1, 4, now, Stop::Never).unwrap();
    assert_eq!(again.summarised, 0);
    assert!(snapshot_json(&f, s2).await["summarised"] == false);
}

#[tokio::test]
async fn retention_settings_are_validated() {
    let f = fixture(false);
    for (field, value) in [("retention_keep", 1), ("retention_keep", 51), ("retention_depth", 0), ("retention_depth", 9)] {
        let mut s = default_settings();
        s[field] = serde_json::json!(value);
        let (st, v) = json(&f.router, put_settings(s)).await;
        assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{field}={value}: {v}");
    }
}
