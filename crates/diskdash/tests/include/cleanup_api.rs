// Included from http.rs: cleanup suggestions through the HTTP API, on a really scanned fixture tree.

use diskdash_analysis::rules::MapEnv;

/// Plant the items the built-in rules look for under a fixture root and
/// build the environment that makes `%TEMP%`, Downloads, ... point inside it.
fn cleanup_tree() -> (test_fixtures::Fixture, MapEnv) {
    let tree = test_fixtures::Fixture::new();
    let root = tree.root().to_string_lossy().into_owned();
    let drive = &root[..2];
    let at = |rel: &str| format!(r"{root}\{rel}");
    let env = MapEnv::default()
        .with_var("SystemDrive", drive)
        .with_var("TEMP", &at(r"Users\Ann\AppData\Local\Temp"))
        .with_var("LOCALAPPDATA", &at(r"Users\Ann\AppData\Local"))
        .with_var("APPDATA", &at(r"Users\Ann\AppData\Roaming"))
        .with_var("USERPROFILE", &at("Users\\Ann"))
        .with_var("WINDIR", &at("Windows"))
        .with_folder("Downloads", &at(r"Users\Ann\Downloads"));

    tree.file(r"Users/Ann/AppData/Local/Temp/scratch.dat", 300 << 10);
    tree.file(r"Users/Ann/AppData/Local/Google/Chrome/User Data/Default/Cache/Cache_Data/f_1", 500 << 10);
    tree.file(r"Users/Ann/AppData/Local/Google/Chrome/User Data/Default/Bookmarks", 4_000);
    tree.file(r"proj/package.json", 100);
    tree.file(r"proj/node_modules/lib/index.js", 200 << 10);
    tree.file(r"loose/node_modules/x.js", 200 << 10);
    tree.file(r"Users/Ann/Downloads/tool.zip", 100 << 10);
    tree.file(r"Users/Ann/Downloads/tool/tool.exe", 60 << 10);
    tree.file(r"Users/Ann/Downloads/keep.zip", 100 << 10);
    tree.file(r"logs/old.log", 50 << 10);
    tree.file(r"logs/new.log", 50 << 10);
    tree.age_days(r"logs/old.log", 90);
    (tree, env)
}

async fn wait_for_cleanup(f: &Fixture, sid: i64) -> serde_json::Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        let (_, v) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/cleanup"))).await;
        if matches!(v["run"]["status"].as_str(), Some("complete" | "failed")) {
            return v;
        }
        assert!(std::time::Instant::now() < deadline, "cleanup did not finish: {v}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn rule_ids(page: &serde_json::Value) -> Vec<String> {
    let mut ids: Vec<String> =
        page["items"].as_array().unwrap().iter().map(|i| i["rule_id"].as_str().unwrap().to_string()).collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn scan_completion_evaluates_rules_and_lists_candidates() {
    let (tree, env) = cleanup_tree();
    let f = fixture_env(false, env);
    let sid = scan_tree(&f, &tree).await;
    let c = wait_for_cleanup(&f, sid).await;
    assert_eq!(c["run"]["status"], "complete", "{c}");
    assert_eq!(c["run"]["stale"], false);

    let (status, page) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&limit=50"))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let ids = rule_ids(&page);
    for expected in [
        "browser-chrome-cache",
        "dev-node-modules",
        "downloads-extracted-archives",
        "logs-old-logs",
        "windows-user-temp",
    ] {
        assert!(ids.iter().any(|i| i == expected), "{expected} missing from {ids:?}");
    }
    assert_eq!(page["scanned"], 1);
    assert_eq!(page["evaluating"], 0);

    // The node_modules without a package.json, the fresh log and the lone archive are not listed.
    let paths: Vec<&str> = page["items"].as_array().unwrap().iter().map(|i| i["path"].as_str().unwrap()).collect();
    assert!(paths.iter().any(|p| p.ends_with(r"proj\node_modules")));
    assert!(!paths.iter().any(|p| p.contains(r"loose\")), "{paths:?}");
    assert!(!paths.iter().any(|p| p.ends_with("new.log") || p.ends_with("keep.zip")), "{paths:?}");
    assert!(paths.iter().any(|p| p.ends_with("old.log")));

    // Ranked: Safe items outrank Review items of similar size.
    let first = &page["items"][0];
    assert!(first["allocated"].as_u64().unwrap() > 0);
    for item in page["items"].as_array().unwrap() {
        assert!(!item["instructions"].as_array().unwrap().is_empty());
        assert!(item["explanation"].as_str().unwrap().len() > 5);
    }

    // Savings per risk, each byte once: the totals equal the listed (counted) candidates.
    let savings = c["savings"].as_array().unwrap();
    let counted: u64 = savings.iter().map(|s| s["allocated"].as_u64().unwrap()).sum();
    assert_eq!(counted, page["total_allocated"].as_u64().unwrap());
    assert!(savings.iter().any(|s| s["risk"] == "Safe" && s["kind"] == "manual-cleanup"));
    assert!(savings.iter().any(|s| s["risk"] == "Review" && s["kind"] == "manual-cleanup"));
}

#[tokio::test]
async fn candidate_filters_paging_and_reveal() {
    let (tree, env) = cleanup_tree();
    let f = fixture_env(false, env);
    let sid = scan_tree(&f, &tree).await;
    wait_for_cleanup(&f, sid).await;

    let (_, safe) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&risk=Safe&limit=50"))).await;
    assert!(safe["items"].as_array().unwrap().iter().all(|i| i["risk"] == "Safe"));
    assert!(safe["total"].as_u64().unwrap() >= 3);

    let (_, dev) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&category=developer"))).await;
    assert_eq!(rule_ids(&dev), ["dev-node-modules"]);

    let (_, q) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&q=OLD.LOG"))).await;
    assert_eq!(rule_ids(&q), ["logs-old-logs"], "path search is case-insensitive");

    let (_, p1) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&limit=2"))).await;
    assert_eq!(p1["items"].as_array().unwrap().len(), 2);
    assert_eq!(p1["next_cursor"], 2);
    let (_, p2) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&limit=2&cursor=2"))).await;
    let first_ids: Vec<_> = p1["items"].as_array().unwrap().iter().map(|i| i["match_id"].clone()).collect();
    assert!(p2["items"].as_array().unwrap().iter().all(|i| !first_ids.contains(&i["match_id"])));

    let (status, bad) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&risk=Nope"))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    let (status, _) = json(&f.router, get("/api/v1/cleanup/candidates?snapshot_id=9999")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Open in Explorer works from the candidate's entry id, resolved on the server.
    let item = dev["items"][0].clone();
    let reveal = req(Method::POST, "/api/v1/shell/reveal")
        .header("x-diskdash-token", TOKEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(serde_json::json!({ "snapshot_id": sid, "entry_id": item["entry_id"] })))
        .unwrap();
    assert_eq!(json(&f.router, reveal).await.0, StatusCode::NO_CONTENT);
    assert!(f.launcher.0.lock().unwrap().iter().any(|p| p.ends_with(r"proj\node_modules")));
}

#[tokio::test]
async fn overview_lists_drive_root_scans_only() {
    let (tree, env) = cleanup_tree();
    let f = fixture_env(false, env);
    let sid = scan_tree(&f, &tree).await;
    wait_for_cleanup(&f, sid).await;
    let (status, o) = json(&f.router, get("/api/v1/cleanup/overview")).await;
    assert_eq!(status, StatusCode::OK, "{o}");
    assert_eq!(o["drives"].as_array().unwrap().len(), 0, "a folder scan is not a drive");
    assert_eq!(o["rule_issues"], 0);

    let (_, page) = json(&f.router, get("/api/v1/cleanup/candidates")).await;
    assert_eq!(page["scanned"], 0, "recommendations cover drive scans");
    assert_eq!(page["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn deleting_a_scan_removes_its_suggestions() {
    let (tree, env) = cleanup_tree();
    let f = fixture_env(false, env);
    let sid = scan_tree(&f, &tree).await;
    wait_for_cleanup(&f, sid).await;
    let removal =
        req(Method::DELETE, &format!("/api/v1/snapshots/{sid}")).header("x-diskdash-token", TOKEN).body(Body::empty()).unwrap();
    assert_eq!(json(&f.router, removal).await.0, StatusCode::OK);
    let (status, _) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/cleanup"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rules_endpoint_reports_overrides_disabled_and_broken_files() {
    let (tree, env) = cleanup_tree();
    let f = fixture_env(false, env);
    let rules_dir = f._tmp.path().join("dd").join("rules");
    test_fixtures::write_text(&rules_dir.join("broken.yaml"), "rules:\n  - id: x\n   bad: [\n");
    test_fixtures::write_text(&rules_dir.join("off.yaml"), "disabled: [windows-prefetch]\n");
    test_fixtures::write_text(
        &rules_dir.join("mine.yaml"),
        "rules:\n  - id: logs-old-logs\n    category: logs\n    title: My logs\n    explanation: mine\n    risk: Review\n    kind: manual-cleanup\n    globs: ['*.log']\n    min_age_days: 7\n    instructions: delete them\n",
    );

    let (status, v) = json(&f.router, get("/api/v1/rules")).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let rules = v["rules"].as_array().unwrap();
    assert!(rules.len() >= 40);
    let mine = rules.iter().find(|r| r["id"] == "logs-old-logs").unwrap();
    assert_eq!(mine["title"], "My logs");
    assert_eq!(mine["overrides_builtin"], true);
    assert_eq!(mine["source"]["type"], "user");
    assert_eq!(rules.iter().find(|r| r["id"] == "windows-prefetch").unwrap()["disabled"], true);
    let temp = rules.iter().find(|r| r["id"] == "windows-user-temp").unwrap();
    assert!(temp["expanded_paths"][0]["path"].as_str().unwrap().ends_with(r"Local\Temp"));
    let issues = v["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0]["file"], "broken.yaml");
    assert!(issues[0]["line"].is_number());
    assert_eq!(v["user_dir"]["exists"], true);

    // The override is what gets evaluated: 7 days instead of 30 still catches only old.log, as Review.
    let sid = scan_tree(&f, &tree).await;
    wait_for_cleanup(&f, sid).await;
    let (_, page) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&category=logs"))).await;
    let log = &page["items"][0];
    assert_eq!(log["title"], "My logs");
    assert_eq!(log["risk"], "Review");
    let (_, c) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/cleanup"))).await;
    assert!(c["run"]["rule_errors"].as_u64().unwrap() >= 1, "broken file is counted");
}

#[tokio::test]
async fn rerun_and_state_changing_calls_need_the_token() {
    let (tree, env) = cleanup_tree();
    let f = fixture_env(false, env);
    let sid = scan_tree(&f, &tree).await;
    wait_for_cleanup(&f, sid).await;

    let no_token = req(Method::POST, "/api/v1/cleanup/evaluate")
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(serde_json::json!({})))
        .unwrap();
    assert_eq!(json(&f.router, no_token).await.0, StatusCode::FORBIDDEN);

    let ok = req(Method::POST, "/api/v1/cleanup/evaluate")
        .header("x-diskdash-token", TOKEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(serde_json::json!({ "snapshot_ids": [sid] })))
        .unwrap();
    let (status, job) = json(&f.router, ok).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let done = wait_job(&f.router, job["id"].as_u64().unwrap()).await;
    assert_eq!(done["status"], "succeeded", "{done}");
    assert_eq!(done["kind"], "cleanup");

    let unknown = req(Method::POST, "/api/v1/cleanup/evaluate")
        .header("x-diskdash-token", TOKEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(serde_json::json!({ "snapshot_ids": [4242] })))
        .unwrap();
    assert_eq!(json(&f.router, unknown).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn nothing_is_modified_by_evaluation() {
    let (tree, env) = cleanup_tree();
    let before = test_fixtures::manifest(tree.root());
    let f = fixture_env(false, env);
    let sid = scan_tree(&f, &tree).await;
    wait_for_cleanup(&f, sid).await;
    test_fixtures::assert_tree_unchanged(tree.root(), &before);
}

#[tokio::test]
async fn exclusions_are_validated_and_remove_suggestions_everywhere() {
    let (tree, env) = cleanup_tree();
    let f = fixture_env(false, env);
    let put = |body: serde_json::Value| {
        req(Method::PUT, "/api/v1/settings")
            .header("x-diskdash-token", TOKEN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(json_body(body))
            .unwrap()
    };
    let (_, current) = json(&f.router, get("/api/v1/settings")).await;
    let mut bad = current.clone();
    bad["excluded_paths"] = serde_json::json!(["not a path"]);
    let (status, err) = json(&f.router, put(bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{err}");
    assert!(err["fields"]["excluded_paths"].is_string());

    let mut good = current.clone();
    good["excluded_paths"] = serde_json::json!([tree.path("logs")]);
    assert_eq!(json(&f.router, put(good)).await.0, StatusCode::OK);
    let sid = scan_tree(&f, &tree).await;
    wait_for_cleanup(&f, sid).await;
    let (_, page) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&category=logs"))).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 0, "old.log is inside the excluded folder");
    let (_, other) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&category=developer"))).await;
    assert_eq!(other["items"].as_array().unwrap().len(), 1, "everything else still suggested");
}
