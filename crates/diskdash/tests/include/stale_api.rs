// Included from http.rs: stale data and old large files through the HTTP API.

fn stale_tree() -> test_fixtures::Fixture {
    let tree = test_fixtures::Fixture::new();
    tree.file("archive/2019/a.bin", 600 << 10);
    tree.file("archive/2019/b.bin", 300 << 10);
    tree.file("mixed/old.bin", 400 << 10);
    tree.file("mixed/new.txt", 1_000);
    tree.file("recent/c.bin", 200 << 10);
    for f in ["archive/2019/a.bin", "archive/2019/b.bin"] {
        tree.age_days(f, 1200);
    }
    tree.age_days("mixed/old.bin", 800);
    tree
}

#[tokio::test]
async fn stale_lists_highest_stale_folder_and_reports_the_time_basis() {
    let tree = stale_tree();
    let f = fixture(false);
    let sid = scan_tree(&f, &tree).await;

    let (status, v) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/stale?months=12"))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let data = &v["data"];
    assert_eq!(data["time_basis"], "mtime", "the fixture reports last-access updates as disabled");
    assert_eq!(data["time_basis_reason"], "disabled");
    assert_eq!(data["months"], 12);
    let names: Vec<&str> = data["items"].as_array().unwrap().iter().map(|i| i["entry"]["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["archive", "old.bin"], "folder collapses its files; mixed is not stale but its old file is");
    assert!(data["items"][0]["age_days"].as_u64().unwrap() >= 1199);
    assert_eq!(data["items"][0]["entry"]["kind"], "dir");

    // A shorter-than-everything preset lists more, a longer one nothing, from the same snapshot.
    let (_, none) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/stale?months=60"))).await;
    assert_eq!(none["data"]["items"].as_array().unwrap().len(), 0);

    let (status, bad) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/stale?months=0"))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    let (status, _) = json(&f.router, get("/api/v1/snapshots/9999/stale")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn old_large_files_are_ranked_by_score_above_the_floor() {
    let tree = stale_tree();
    let f = fixture(false);
    let sid = scan_tree(&f, &tree).await;
    let (status, v) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/old-large?floor_mib=0"))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let items = v["data"]["items"].as_array().unwrap();
    let names: Vec<&str> = items.iter().map(|i| i["entry"]["name"].as_str().unwrap()).collect();
    // 600x1200 > 300x1200 > 400x800 (KiB x days); files from today or yesterday are not "old".
    assert_eq!(names[..3], ["a.bin", "b.bin", "old.bin"]);
    assert!(items[0]["score"].as_f64().unwrap() > items[1]["score"].as_f64().unwrap());

    let (_, floor) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/old-large?floor_mib=1"))).await;
    assert_eq!(floor["data"]["items"].as_array().unwrap().len(), 0, "every file is under 1 MiB");
}

#[tokio::test]
async fn the_time_basis_is_stored_with_the_scan() {
    let tree = stale_tree();
    let f = fixture(false);
    let sid = scan_tree(&f, &tree).await;
    let (_, snap) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}"))).await;
    assert_eq!(snap["time_basis"], "mtime");
    assert_eq!(snap["time_basis_reason"], "disabled");
}

fn put_settings(body: serde_json::Value) -> Request<Body> {
    req(Method::PUT, "/api/v1/settings")
        .header("x-diskdash-token", TOKEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(body))
        .unwrap()
}

#[tokio::test]
async fn excluded_folders_disappear_from_stale_and_duplicates() {
    let tree = dup_tree();
    let f = fixture(false);
    let (_, mut settings) = json(&f.router, get("/api/v1/settings")).await;
    settings["excluded_paths"] = serde_json::json!([tree.path("old"), tree.path("backup")]);
    assert_eq!(json(&f.router, put_settings(settings)).await.0, StatusCode::OK);
    let sid = scan_tree(&f, &tree).await;

    let (_, stale) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/stale?months=12"))).await;
    let paths: Vec<&str> = stale["data"]["items"].as_array().unwrap().iter().map(|i| i["entry"]["path"].as_str().unwrap()).collect();
    assert!(!paths.iter().any(|p| p.contains(r"\old")), "{paths:?}");

    let run = find_dups(&f, serde_json::json!({ "snapshot_ids": [sid] })).await;
    assert_eq!(run["group_count"], 0, "two of the three copies are excluded, so no group is left: {run}");
    assert!(run["skips"]["protected"].as_u64().unwrap() >= 2);
}
