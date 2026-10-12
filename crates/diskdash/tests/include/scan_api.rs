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
