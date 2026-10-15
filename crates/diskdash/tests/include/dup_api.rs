// Included from http.rs: duplicate detection through the HTTP API, on really scanned and read files.

const MIB: u64 = 1 << 20;

fn post_json(path: &str, body: serde_json::Value) -> Request<Body> {
    req(Method::POST, path)
        .header("x-diskdash-token", TOKEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(body))
        .unwrap()
}

async fn find_dups(f: &Fixture, body: serde_json::Value) -> serde_json::Value {
    let (status, job) = json(&f.router, post_json("/api/v1/duplicates/runs", body)).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let done = wait_job(&f.router, job["id"].as_u64().unwrap()).await;
    assert_eq!(done["status"], "succeeded", "{done}");
    let (_, run) = json(&f.router, get("/api/v1/duplicates/status")).await;
    run
}

fn dup_tree() -> test_fixtures::Fixture {
    let t = test_fixtures::Fixture::new();
    t.file("photos/a.jpg", 3 * MIB);
    t.file("backup/a-copy.jpg", 3 * MIB);
    t.file("old/a-old.jpg", 3 * MIB);
    t.hard_link("photos/a.jpg", "linked/a-link.jpg");
    t.zeros("other/b.jpg", 3 * MIB); // same size, different content
    t.tweaked("tweak/t.jpg", 3 * MIB, 1_500_000); // same ends, different middle
    t.file("small/s1.bin", 100 << 10);
    t.file("small/s2.bin", 100 << 10);
    t.age_days("old/a-old.jpg", 400);
    t
}

#[tokio::test]
async fn finds_true_duplicates_only_and_changes_nothing() {
    let tree = dup_tree();
    let before = test_fixtures::manifest(tree.root());
    let f = fixture(false);
    let sid = scan_tree(&f, &tree).await;
    let run = find_dups(&f, serde_json::json!({ "snapshot_ids": [sid] })).await;
    assert_eq!(run["status"], "complete", "{run}");
    assert_eq!(run["algo"], "blake3");
    assert_eq!(run["group_count"], 1, "{run}");

    let (_, page) = json(&f.router, get("/api/v1/duplicates/groups")).await;
    let g = &page["groups"][0];
    let paths: Vec<&str> = g["members"].as_array().unwrap().iter().map(|m| m["path"].as_str().unwrap()).collect();
    assert_eq!(paths.len(), 3, "{paths:?}");
    assert!(paths.iter().any(|p| p.ends_with(r"backup\a-copy.jpg")));
    assert!(paths.iter().any(|p| p.ends_with(r"old\a-old.jpg")));
    assert!(paths.iter().filter(|p| p.ends_with("a.jpg") && !p.ends_with("copy.jpg")).count() <= 1, "a hard link is not a second copy: {paths:?}");
    assert!(!paths.iter().any(|p| p.contains("small") || p.contains("other") || p.contains("tweak")));
    assert_eq!(g["wasted"], 2 * g["alloc"].as_u64().unwrap(), "(members - 1) x allocated size");
    assert_eq!(page["total_wasted"], g["wasted"]);
    assert!(g["members"][0]["category"].is_string());

    test_fixtures::assert_contents_unchanged(tree.root(), &before);
}

#[tokio::test]
async fn a_rerun_uses_the_hash_cache() {
    let tree = dup_tree();
    let f = fixture(false);
    let sid = scan_tree(&f, &tree).await;
    let first = find_dups(&f, serde_json::json!({ "snapshot_ids": [sid] })).await;
    assert!(first["files_hashed"].as_u64().unwrap() > 0);
    let second = find_dups(&f, serde_json::json!({ "snapshot_ids": [sid] })).await;
    assert_eq!(second["files_hashed"], 0, "{second}");
    assert!(second["cache_hits"].as_u64().unwrap() > 0);
    assert_eq!(second["group_count"], first["group_count"]);
    assert_ne!(second["id"], first["id"]);
}

#[tokio::test]
async fn locked_and_changed_files_are_skipped_and_listed() {
    let tree = dup_tree();
    let f = fixture(false);
    let sid = scan_tree(&f, &tree).await;
    let _guard = tree.lock_exclusive("backup/a-copy.jpg");
    tree.age_days("old/a-old.jpg", 10); // changed since the scan
    let run = find_dups(&f, serde_json::json!({ "snapshot_ids": [sid] })).await;
    assert_eq!(run["skips"]["sharing-violation"], 1, "{run}");
    assert_eq!(run["skips"]["changed-since-scan"], 1, "{run}");
    let (_, skips) = json(&f.router, get("/api/v1/duplicates/skips?reason=sharing-violation")).await;
    assert!(skips[0]["path"].as_str().unwrap().ends_with("a-copy.jpg"));
    let (_, page) = json(&f.router, get("/api/v1/duplicates/groups")).await;
    assert_eq!(page["total"], 0, "only one readable copy is left");
}

#[tokio::test]
async fn keep_rules_store_suggestions_and_never_touch_files() {
    let tree = dup_tree();
    let before = test_fixtures::manifest(tree.root());
    let f = fixture(false);
    let sid = scan_tree(&f, &tree).await;
    find_dups(&f, serde_json::json!({ "snapshot_ids": [sid] })).await;
    let (_, page) = json(&f.router, get("/api/v1/duplicates/groups")).await;
    let gid = page["groups"][0]["id"].as_i64().unwrap();
    assert!(page["groups"][0]["members"][0]["suggested"].is_null());

    let (status, r) = json(&f.router, post_json("/api/v1/duplicates/suggestions", serde_json::json!({ "rule": "oldest" }))).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["applied"], 1);
    let (_, page) = json(&f.router, get("/api/v1/duplicates/groups")).await;
    let members = page["groups"][0]["members"].as_array().unwrap();
    let keep: Vec<_> = members.iter().filter(|m| m["suggested"] == "keep").collect();
    assert_eq!(keep.len(), 1);
    assert!(keep[0]["path"].as_str().unwrap().ends_with("a-old.jpg"), "the 400-day-old copy");
    assert_eq!(members.iter().filter(|m| m["suggested"] == "remove").count(), 2);
    assert_eq!(page["groups"][0]["rule"], "oldest");

    let (_, r) = json(&f.router, post_json("/api/v1/duplicates/suggestions", serde_json::json!({ "rule": "prefer-folder", "value": tree.path("backup"), "group_id": gid }))).await;
    assert_eq!(r["applied"], 1);
    let (_, page) = json(&f.router, get("/api/v1/duplicates/groups")).await;
    let kept = page["groups"][0]["members"].as_array().unwrap().iter().find(|m| m["suggested"] == "keep").unwrap().clone();
    assert!(kept["path"].as_str().unwrap().ends_with("a-copy.jpg"));

    for bad in [serde_json::json!({ "rule": "delete-all" }), serde_json::json!({ "rule": "prefer-drive" })] {
        assert_eq!(json(&f.router, post_json("/api/v1/duplicates/suggestions", bad)).await.0, StatusCode::BAD_REQUEST);
    }
    assert_eq!(json(&f.router, post_json("/api/v1/duplicates/suggestions", serde_json::json!({ "rule": "newest", "group_id": 9999 }))).await.0, StatusCode::NOT_FOUND);

    let clear = req(Method::DELETE, "/api/v1/duplicates/suggestions").header("x-diskdash-token", TOKEN).body(Body::empty()).unwrap();
    assert_eq!(json(&f.router, clear).await.0, StatusCode::NO_CONTENT);
    let (_, page) = json(&f.router, get("/api/v1/duplicates/groups")).await;
    assert!(page["groups"][0]["members"][0]["suggested"].is_null());
    test_fixtures::assert_contents_unchanged(tree.root(), &before);
}

#[tokio::test]
async fn groups_filter_by_drive_folder_and_category() {
    let tree = dup_tree();
    let f = fixture(false);
    let sid = scan_tree(&f, &tree).await;
    find_dups(&f, serde_json::json!({ "snapshot_ids": [sid] })).await;
    let drive = tree.root().to_string_lossy()[..1].to_uppercase();
    let total = |q: String| {
        let router = f.router.clone();
        async move { json(&router, get(&format!("/api/v1/duplicates/groups?{q}"))).await.1["total"].as_u64().unwrap() }
    };
    assert_eq!(total(format!("drive={drive}")).await, 1);
    assert_eq!(total("drive=Z".into()).await, 0);
    let folder = tree.path("backup").to_string_lossy().replace('\\', "%5C").replace(':', "%3A");
    assert_eq!(total(format!("folder={folder}")).await, 1);
    let other = tree.path("other").to_string_lossy().replace('\\', "%5C").replace(':', "%3A");
    assert_eq!(total(format!("folder={other}")).await, 0);
    assert_eq!(total("category=images".into()).await, 1);
    assert_eq!(total("category=video".into()).await, 0);
}

#[tokio::test]
async fn near_duplicate_folders_are_reported_when_enabled() {
    let t = test_fixtures::Fixture::new();
    for i in 0..12u64 {
        let size = MIB + i * 1000;
        t.file(&format!("proj/f{i}.bin"), size);
        if i != 4 {
            t.file(&format!("backup/proj/f{i}.bin"), size);
        }
    }
    let f = fixture(false);
    let sid = scan_tree(&f, &t).await;
    let run = find_dups(&f, serde_json::json!({ "snapshot_ids": [sid], "near_duplicates": true, "near_threshold": 0.9 })).await;
    assert_eq!(run["near_enabled"], true);
    let (_, near) = json(&f.router, get("/api/v1/duplicates/near")).await;
    assert_eq!(near.as_array().unwrap().len(), 1, "{near}");
    assert!(near[0]["similarity"].as_f64().unwrap() >= 0.9);
    assert_eq!(near[0]["shared_files"], 11);

    let off = find_dups(&f, serde_json::json!({ "snapshot_ids": [sid] })).await;
    assert_eq!(off["near_enabled"], false);
    let (_, near) = json(&f.router, get("/api/v1/duplicates/near")).await;
    assert_eq!(near.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn start_validation_and_no_scan() {
    let f = fixture(false);
    let (status, v) = json(&f.router, post_json("/api/v1/duplicates/runs", serde_json::json!({}))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    let tree = dup_tree();
    let sid = scan_tree(&f, &tree).await;
    assert_eq!(json(&f.router, post_json("/api/v1/duplicates/runs", serde_json::json!({ "snapshot_ids": [sid], "min_size_mib": 0 }))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(json(&f.router, post_json("/api/v1/duplicates/runs", serde_json::json!({ "snapshot_ids": [sid], "near_threshold": 0.1 }))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(json(&f.router, post_json("/api/v1/duplicates/runs", serde_json::json!({ "snapshot_ids": [4242] }))).await.0, StatusCode::NOT_FOUND);
    let no_token = req(Method::POST, "/api/v1/duplicates/runs").header(header::CONTENT_TYPE, "application/json").body(json_body(serde_json::json!({}))).unwrap();
    assert_eq!(json(&f.router, no_token).await.0, StatusCode::FORBIDDEN);
    let (_, none) = json(&f.router, get("/api/v1/duplicates/status")).await;
    assert!(none.is_null());
}
