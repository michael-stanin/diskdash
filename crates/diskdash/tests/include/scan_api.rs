// Included from http.rs: end-to-end scan tests through the HTTP API.

async fn json(router: &Router, r: Request<Body>) -> (StatusCode, serde_json::Value) {
    let (status, body, _) = send(router, r).await;
    (status, serde_json::from_str(&body).unwrap_or(serde_json::Value::Null))
}

async fn wait_job(router: &Router, id: u64) -> serde_json::Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        let (_, v) = json(router, req(Method::GET, &format!("/api/v1/jobs/{id}")).body(Body::empty()).unwrap()).await;
        if ["succeeded", "failed", "cancelled"].contains(&v["status"].as_str().unwrap_or("")) {
            return v;
        }
        assert!(std::time::Instant::now() < deadline, "job did not finish: {v}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn get(path: &str) -> Request<Body> {
    req(Method::GET, path).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn scan_a_folder_end_to_end_then_browse_and_remove() {
    let f = fixture(false);
    let tree = test_fixtures::Fixture::new();
    tree.file("videos/movie.mkv", 3 << 20);
    tree.file("videos/clip.mp4", 1 << 20);
    tree.file("docs/report.pdf", 200 << 10);
    tree.file("notes.txt", 100);
    let root = tree.root().to_string_lossy().into_owned();

    let r = req(Method::POST, "/api/v1/scans")
        .header("x-diskdash-token", TOKEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(serde_json::json!({ "roots": [root] })))
        .unwrap();
    let (status, started) = json(&f.router, r).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let job = wait_job(&f.router, started["job_id"].as_u64().unwrap()).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert_eq!(job["kind"], "scan");
    let sid = started["snapshots"][0]["id"].as_i64().unwrap();

    let (_, snap) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}"))).await;
    assert_eq!(snap["status"], "complete", "{snap}");
    assert_eq!(snap["file_count"], 4);
    assert_eq!(snap["total_size"], (3 << 20) + (1 << 20) + (200 << 10) + 100);

    let (_, top) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/top-files?limit=2&measure=size"))).await;
    assert_eq!(top["snapshot_id"], sid);
    assert!(top["as_of"].is_string());
    assert_eq!(top["data"][0]["name"], "movie.mkv");
    assert_eq!(top["data"][0]["category"], "video");
    assert!(top["data"][0]["path"].as_str().unwrap().ends_with(r"videos\movie.mkv"));
    assert_eq!(top["data"].as_array().unwrap().len(), 2);

    let (_, kids) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/children?limit=1"))).await;
    assert_eq!(kids["data"]["items"][0]["name"], "videos");
    assert_eq!(kids["data"]["items"][0]["kind"], "dir");
    assert!(kids["data"]["next_cursor"].is_number());

    let videos = kids["data"]["items"][0]["path"].as_str().unwrap().to_string();
    let uri = format!("/api/v1/snapshots/{sid}/children?path={}", videos.replace('\\', "%5C").replace(':', "%3A"));
    let (status, sub) = json(&f.router, get(&uri)).await;
    assert_eq!(status, StatusCode::OK, "{sub}");
    assert_eq!(sub["data"]["items"].as_array().unwrap().len(), 2);

    let (status, _) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/children?path=C%3A%5Cnope"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, cats) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/categories"))).await;
    let total: u64 = cats["data"].as_array().unwrap().iter().map(|c| c["size"].as_u64().unwrap()).sum();
    assert_eq!(total, snap["total_size"].as_u64().unwrap());

    let (_, errs) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/errors"))).await;
    assert_eq!(errs["data"]["items"].as_array().unwrap().len(), 0);

    let (_, latest) = json(&f.router, get("/api/v1/snapshots/latest")).await;
    assert_eq!(latest[0]["id"], sid);

    let removal =
        req(Method::DELETE, &format!("/api/v1/snapshots/{sid}")).header("x-diskdash-token", TOKEN).body(Body::empty()).unwrap();
    assert_eq!(json(&f.router, removal).await.0, StatusCode::OK);
    let (status, _) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Scan a fixture tree through the API; returns the snapshot id.
async fn scan_tree(f: &Fixture, tree: &test_fixtures::Fixture) -> i64 {
    let r = req(Method::POST, "/api/v1/scans")
        .header("x-diskdash-token", TOKEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(serde_json::json!({ "roots": [tree.root().to_string_lossy()] })))
        .unwrap();
    let (status, started) = json(&f.router, r).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let job = wait_job(&f.router, started["job_id"].as_u64().unwrap()).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    started["snapshots"][0]["id"].as_i64().unwrap()
}

#[tokio::test]
async fn tree_caps_levels_and_other_sums_to_parent() {
    let f = fixture(false);
    let tree = test_fixtures::Fixture::new();
    for i in 0..30u64 {
        tree.file(&format!("many/f{i:02}.bin"), 1000 + i * 100);
    }
    tree.file("big/movie.mkv", 2 << 20);
    tree.file("big/sub/doc.pdf", 10_000);
    let sid = scan_tree(&f, &tree).await;

    let (status, t) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/tree?depth=2&limit=5&measure=size"))).await;
    assert_eq!(status, StatusCode::OK, "{t}");
    let node = &t["data"]["node"];
    let kids = node["children"].as_array().unwrap();
    assert_eq!(kids[0]["entry"]["name"], "big", "largest first");
    assert_eq!(kids[0]["entry"]["top_category"], "video");
    let many = kids.iter().find(|k| k["entry"]["name"] == "many").unwrap();
    let grand = many["children"].as_array().unwrap();
    assert_eq!(grand.len(), 5, "capped at 5");
    assert_eq!(many["other"]["count"], 25);
    let shown: u64 = grand.iter().map(|g| g["entry"]["size"].as_u64().unwrap()).sum();
    let other = many["other"]["size"].as_u64().unwrap();
    assert_eq!(shown + other, many["entry"]["size"].as_u64().unwrap(), "level sums to the parent");
    assert_eq!(t["data"]["ancestors"].as_array().unwrap().len(), 1, "root only");

    // Re-root at `many` by id: breadcrumb grows.
    let many_id = many["entry"]["id"].as_i64().unwrap();
    let (_, t2) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/tree?node={many_id}&depth=1&limit=5"))).await;
    assert_eq!(t2["data"]["ancestors"].as_array().unwrap().len(), 2);
    assert!(t2["data"]["node"]["children"][0]["children"].is_null(), "depth 1 loads one level");

    // "Other" is explorable: page through the rest of the children.
    let (_, page) =
        json(&f.router, get(&format!("/api/v1/snapshots/{sid}/children?node={many_id}&measure=size&limit=20&cursor=5"))).await;
    assert_eq!(page["data"]["items"].as_array().unwrap().len(), 20);

    let (status, _) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/tree?node=987654321"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = json(&f.router, get("/api/v1/snapshots/424242/tree")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Category filter and paging on top files.
    let (_, vids) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/top-files?category=video"))).await;
    assert_eq!(vids["data"].as_array().unwrap().len(), 1);
    let (_, p2) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/top-files?limit=20&offset=20"))).await;
    assert_eq!(p2["data"].as_array().unwrap().len(), 12, "32 files: second page holds 12");
    let (status, _) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/top-files?category=bogus"))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn reveal_in_explorer_resolves_from_snapshot() {
    let f = fixture(false);
    let tree = test_fixtures::Fixture::new();
    tree.file("keep.bin", 5000);
    tree.file("gone.bin", 4000);
    let sid = scan_tree(&f, &tree).await;
    let (_, top) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/top-files"))).await;
    let id_of = |name: &str| top["data"].as_array().unwrap().iter().find(|e| e["name"] == name).unwrap()["id"].as_i64().unwrap();

    let reveal = |entry: i64, token: bool| {
        let mut b = req(Method::POST, "/api/v1/shell/reveal").header(header::CONTENT_TYPE, "application/json");
        if token {
            b = b.header("x-diskdash-token", TOKEN);
        }
        b.body(json_body(serde_json::json!({ "snapshot_id": sid, "entry_id": entry }))).unwrap()
    };

    assert_eq!(send(&f.router, reveal(id_of("keep.bin"), true)).await.0, StatusCode::NO_CONTENT);
    let launched = f.launcher.0.lock().unwrap().clone();
    assert_eq!(launched.len(), 1);
    assert!(launched[0].ends_with("keep.bin"));

    tree.remove("gone.bin");
    let (status, body, _) = send(&f.router, reveal(id_of("gone.bin"), true)).await;
    assert_eq!(status, StatusCode::GONE, "{body}");
    assert!(body.contains("no longer exists"));

    assert_eq!(send(&f.router, reveal(id_of("keep.bin"), false)).await.0, StatusCode::FORBIDDEN);
    assert_eq!(f.launcher.0.lock().unwrap().len(), 1, "nothing else was launched");
}

#[test]
fn top_n_queries_use_the_indexes() {
    let tree = test_fixtures::Fixture::new();
    tree.file("a.bin", 10);
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("s.db");
    diskdash_core::scan::scan_blocking(diskdash_core::scan::ScanOptions {
        root: tree.root().to_path_buf(),
        db_path: db.clone(),
        threads: 1,
        detect_hardlinks: false,
        exclude: vec![],
        resume: false,
        windows_dir: None,
    })
    .unwrap();
    let conn = rusqlite::Connection::open(&db).unwrap();
    let plan = |sql: &str| -> String {
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(3)).unwrap().map(Result::unwrap).collect::<Vec<_>>().join(" | ")
    };
    let files = plan("SELECT id FROM entries WHERE kind = 0 ORDER BY alloc DESC LIMIT 20");
    assert!(files.contains("entries_kind_alloc"), "{files}");
    let kids = plan("SELECT id FROM entries WHERE parent_id = 0 ORDER BY size DESC LIMIT 20");
    assert!(kids.contains("entries_children_size"), "{kids}");
}

#[tokio::test]
async fn scan_request_validation() {
    let f = fixture(false);
    let r = req(Method::POST, "/api/v1/scans")
        .header("x-diskdash-token", TOKEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(serde_json::json!({ "roots": ["Z:\\", "\\\\nas\\media"] })))
        .unwrap();
    let (status, v) = json(&f.router, r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(v["fields"]["roots[0]"].as_str().unwrap().contains("fixed internal"));
    assert!(v["fields"]["roots[1]"].as_str().unwrap().contains("network"));
}
