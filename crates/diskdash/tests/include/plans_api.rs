// Included from http.rs: cleanup plans through the HTTP API.

fn patch_json(path: &str, body: serde_json::Value) -> Request<Body> {
    req(Method::PATCH, path)
        .header("x-diskdash-token", TOKEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(body))
        .unwrap()
}

fn delete_req(path: &str) -> Request<Body> {
    req(Method::DELETE, path).header("x-diskdash-token", TOKEN).body(Body::empty()).unwrap()
}

async fn new_plan(f: &Fixture, name: &str) -> i64 {
    let (status, p) = json(&f.router, post_json("/api/v1/plans", serde_json::json!({ "name": name }))).await;
    assert_eq!(status, StatusCode::OK, "{p}");
    p["id"].as_i64().unwrap()
}

/// Scan the cleanup tree, wait for its suggestions and return (snapshot, node_modules candidate).
async fn candidate_setup(f: &Fixture, tree: &test_fixtures::Fixture) -> (i64, serde_json::Value) {
    let sid = scan_tree(f, tree).await;
    wait_for_cleanup(f, sid).await;
    let (_, page) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&category=developer"))).await;
    (sid, page["items"][0].clone())
}

fn add_cleanup(sid: i64, c: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "source": "cleanup", "snapshot_id": sid, "match_id": c["match_id"] })
}

#[tokio::test]
async fn plans_can_be_created_renamed_and_deleted() {
    let f = fixture(false);
    let (_, none) = json(&f.router, get("/api/v1/plans")).await;
    assert_eq!(none.as_array().unwrap().len(), 0);
    let id = new_plan(&f, "  Spring clean  ").await;
    let (_, list) = json(&f.router, get("/api/v1/plans")).await;
    assert_eq!(list[0]["name"], "Spring clean");
    assert_eq!(list[0]["totals"]["overall"]["count"], 0);

    let (status, p) = json(&f.router, patch_json(&format!("/api/v1/plans/{id}"), serde_json::json!({ "name": "Renamed" }))).await;
    assert_eq!(status, StatusCode::OK, "{p}");
    assert_eq!(p["name"], "Renamed");
    assert_eq!(json(&f.router, post_json("/api/v1/plans", serde_json::json!({ "name": "   " }))).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json(&f.router, patch_json("/api/v1/plans/999", serde_json::json!({ "name": "x" }))).await.0, StatusCode::NOT_FOUND);
    assert_eq!(json(&f.router, get("/api/v1/plans/999")).await.0, StatusCode::NOT_FOUND);

    assert_eq!(json(&f.router, delete_req(&format!("/api/v1/plans/{id}"))).await.0, StatusCode::NO_CONTENT);
    assert_eq!(json(&f.router, get(&format!("/api/v1/plans/{id}"))).await.0, StatusCode::NOT_FOUND);
    let no_token = req(Method::POST, "/api/v1/plans").header(header::CONTENT_TYPE, "application/json").body(json_body(serde_json::json!({ "name": "x" }))).unwrap();
    assert_eq!(json(&f.router, no_token).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn adding_a_candidate_records_a_baseline_and_guards_against_duplicates() {
    let (tree, env) = cleanup_tree();
    let f = fixture_env(false, env);
    let (sid, c) = candidate_setup(&f, &tree).await;
    let plan = new_plan(&f, "p").await;

    let (status, r) = json(&f.router, post_json(&format!("/api/v1/plans/{plan}/items"), add_cleanup(sid, &c))).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["added"], true);
    let item = &r["item"];
    assert!(item["path"].as_str().unwrap().ends_with(r"proj\node_modules"));
    assert_eq!(item["source"], "cleanup-rules");
    assert_eq!(item["risk"], "Review");
    assert_eq!(item["status"], "todo");
    assert_eq!(item["allocated"], c["allocated"]);
    assert!(item["instructions"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().contains("npm install")));

    let (_, again) = json(&f.router, post_json(&format!("/api/v1/plans/{plan}/items"), add_cleanup(sid, &c))).await;
    assert_eq!(again["added"], false);
    assert_eq!(again["reason"], "already-planned");
    assert_eq!(again["item"]["id"], item["id"]);
    let (_, view) = json(&f.router, get(&format!("/api/v1/plans/{plan}"))).await;
    assert_eq!(view["items"].as_array().unwrap().len(), 1);
    assert_eq!(view["plan"]["totals"]["overall"]["allocated"], c["allocated"]);

    let missing = serde_json::json!({ "source": "cleanup", "snapshot_id": sid, "match_id": 999_999 });
    assert_eq!(json(&f.router, post_json(&format!("/api/v1/plans/{plan}/items"), missing)).await.0, StatusCode::NOT_FOUND);
    assert_eq!(json(&f.router, post_json("/api/v1/plans/999/items", add_cleanup(sid, &c))).await.0, StatusCode::NOT_FOUND);
    assert_eq!(json(&f.router, post_json(&format!("/api/v1/plans/{plan}/items"), serde_json::json!({ "source": "path", "path": "C:\\Windows" }))).await.0, StatusCode::BAD_REQUEST, "free-form paths are not accepted");
    let (_, view) = json(&f.router, get(&format!("/api/v1/plans/{plan}"))).await;
    assert_eq!(view["items"].as_array().unwrap().len(), 1, "unchanged by the failed requests");
}

#[tokio::test]
async fn stale_duplicate_and_tiering_items_get_their_instructions() {
    let tree = dup_tree();
    let f = fixture(false);
    let sid = scan_tree(&f, &tree).await;
    find_dups(&f, serde_json::json!({ "snapshot_ids": [sid] })).await;
    let plan = new_plan(&f, "mixed").await;
    let url = format!("/api/v1/plans/{plan}/items");

    let (_, stale) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/stale?months=12"))).await;
    let entry = stale["data"]["items"][0]["entry"]["id"].clone();
    let (status, r) = json(&f.router, post_json(&url, serde_json::json!({ "source": "stale", "snapshot_id": sid, "entry_id": entry }))).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["item"]["kind"], "stale");
    assert!(r["item"]["instructions"].as_array().unwrap().len() >= 2);

    let (_, groups) = json(&f.router, get("/api/v1/duplicates/groups")).await;
    let member = groups["groups"][0]["members"][1]["id"].clone();
    let (_, r) = json(&f.router, post_json(&url, serde_json::json!({ "source": "duplicate", "member_id": member }))).await;
    assert_eq!(r["item"]["kind"], "duplicate");
    assert!(r["item"]["title"].as_str().unwrap().starts_with("Duplicate copy of"));
    assert!(r["item"]["allocated"].as_u64().unwrap() > 0);

    let tier = |dest: Option<&str>| serde_json::json!({ "source": "tiering", "snapshot_id": sid, "entry_id": entry, "destination": dest });
    assert_eq!(json(&f.router, post_json(&url, tier(None))).await.0, StatusCode::BAD_REQUEST);
    let other = serde_json::json!({ "source": "tiering", "snapshot_id": sid, "entry_id": groups["groups"][0]["members"][0]["entry_id"], "destination": r"E:\Archive" });
    let (status, r) = json(&f.router, post_json(&url, other)).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["item"]["kind"], "manual-move");
    assert_eq!(r["item"]["destination"], r"E:\Archive");
    assert!(r["item"]["instructions"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().starts_with("robocopy \"") && l.as_str().unwrap().contains(r#""E:\Archive""#)));
    assert_eq!(json(&f.router, post_json("/api/v1/plans/999/items", tier(Some("E:\\x")))).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn protected_and_excluded_paths_are_rejected_and_flagged() {
    let (tree, env) = cleanup_tree();
    let f = fixture_env(false, env);
    let (sid, c) = candidate_setup(&f, &tree).await;
    let plan = new_plan(&f, "p").await;
    let url = format!("/api/v1/plans/{plan}/items");

    // A folder the user protected is refused, with the reason.
    let (_, mut settings) = json(&f.router, get("/api/v1/settings")).await;
    settings["protected_paths"] = serde_json::json!([tree.path("proj")]);
    assert_eq!(json(&f.router, put_settings(settings.clone())).await.0, StatusCode::OK);
    let (status, err) = json(&f.router, post_json(&url, add_cleanup(sid, &c))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{err}");
    assert_eq!(err["fields"]["path"], "protected:user");
    let (_, view) = json(&f.router, get(&format!("/api/v1/plans/{plan}"))).await;
    assert_eq!(view["items"].as_array().unwrap().len(), 0);

    // An exclusion added later flags the item and drops it from the totals.
    settings["protected_paths"] = serde_json::json!([]);
    assert_eq!(json(&f.router, put_settings(settings.clone())).await.0, StatusCode::OK);
    assert_eq!(json(&f.router, post_json(&url, add_cleanup(sid, &c))).await.1["added"], true);
    settings["excluded_paths"] = serde_json::json!([tree.path("proj")]);
    assert_eq!(json(&f.router, put_settings(settings)).await.0, StatusCode::OK);
    let (_, view) = json(&f.router, get(&format!("/api/v1/plans/{plan}"))).await;
    assert_eq!(view["items"][0]["blocked"], "excluded:user");
    assert_eq!(view["plan"]["blocked"], 1);
    assert_eq!(view["plan"]["totals"]["overall"]["count"], 0, "now-excluded items are left out of the totals");
}

#[tokio::test]
async fn status_notes_and_totals() {
    let (tree, env) = cleanup_tree();
    let f = fixture_env(false, env);
    let (sid, c) = candidate_setup(&f, &tree).await;
    let plan = new_plan(&f, "p").await;
    let (_, r) = json(&f.router, post_json(&format!("/api/v1/plans/{plan}/items"), add_cleanup(sid, &c))).await;
    let item = r["item"]["id"].as_i64().unwrap();
    let url = format!("/api/v1/plans/{plan}/items/{item}");

    let (status, v) = json(&f.router, patch_json(&url, serde_json::json!({ "status": "done", "note": "deleted on Friday" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!((v["status"].as_str(), v["note"].as_str()), (Some("done"), Some("deleted on Friday")));
    let (_, view) = json(&f.router, get(&format!("/api/v1/plans/{plan}"))).await;
    assert_eq!(view["plan"]["totals"]["done"]["allocated"], c["allocated"]);
    assert_eq!(view["plan"]["totals"]["todo"]["count"], 0);
    assert_eq!(json(&f.router, patch_json(&url, serde_json::json!({ "status": "deleted" }))).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json(&f.router, patch_json(&format!("/api/v1/plans/{plan}/items/999"), serde_json::json!({ "status": "done" }))).await.0, StatusCode::NOT_FOUND);

    let (_, v) = json(&f.router, patch_json(&url, serde_json::json!({ "status": "skipped", "note": "" }))).await;
    assert_eq!(v["note"], serde_json::Value::Null);
    let (_, view) = json(&f.router, get(&format!("/api/v1/plans/{plan}"))).await;
    assert_eq!(view["plan"]["totals"]["skipped"]["count"], 1);
    assert_eq!(json(&f.router, delete_req(&url)).await.0, StatusCode::NO_CONTENT);
    assert_eq!(json(&f.router, delete_req(&url)).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn export_is_csv_or_txt_with_paths_and_instructions_only() {
    let t = test_fixtures::Fixture::new();
    t.file("old/a, b.bin", 300 << 10);
    t.file("old/c.bin", 200 << 10);
    t.age_days("old/a, b.bin", 900);
    t.age_days("old/c.bin", 900);
    let f = fixture(false);
    let sid = scan_tree(&f, &t).await;
    let (_, stale) = json(&f.router, get(&format!("/api/v1/snapshots/{sid}/stale?months=12"))).await;
    let entry = stale["data"]["items"][0]["entry"]["id"].clone();
    let plan = new_plan(&f, "My: plan?").await;
    json(&f.router, post_json(&format!("/api/v1/plans/{plan}/items"), serde_json::json!({ "source": "stale", "snapshot_id": sid, "entry_id": entry }))).await;

    let r = req(Method::GET, &format!("/api/v1/plans/{plan}/export?format=csv")).body(Body::empty()).unwrap();
    let (status, body, headers) = send(&f.router, r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get(header::CONTENT_TYPE).unwrap().to_str().unwrap().starts_with("text/csv"));
    let disp = headers.get(header::CONTENT_DISPOSITION).unwrap().to_str().unwrap();
    assert!(disp.contains("attachment") && disp.contains("diskdash-plan-my-plan.csv"), "{disp}");
    assert!(body.trim_start_matches('\u{FEFF}').starts_with("status,risk,kind,path,logical_bytes,allocated_bytes,instructions,note"));
    assert!(body.contains("todo,Review,stale,"));

    let r = req(Method::GET, &format!("/api/v1/plans/{plan}/export?format=txt")).body(Body::empty()).unwrap();
    let (_, txt, headers) = send(&f.router, r).await;
    assert!(headers.get(header::CONTENT_DISPOSITION).unwrap().to_str().unwrap().ends_with(".txt\""));
    assert!(txt.contains("[ ] ") && txt.contains("Cleanup plan: My: plan?"));
    for bad in ["@echo", "#!", "Remove-Item", "del /"] {
        assert!(!txt.contains(bad) && !body.contains(bad), "{bad}");
    }
    let (status, _, _) = send(&f.router, req(Method::GET, &format!("/api/v1/plans/{plan}/export?format=ps1")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(send(&f.router, req(Method::GET, "/api/v1/plans/999/export").body(Body::empty()).unwrap()).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_manual_rescan_verifies_what_was_freed() {
    let (tree, env) = cleanup_tree();
    let f = fixture_env(false, env);
    let (sid, c) = candidate_setup(&f, &tree).await;
    let plan = new_plan(&f, "verify me").await;
    let url = format!("/api/v1/plans/{plan}/items");
    let (_, a) = json(&f.router, post_json(&url, add_cleanup(sid, &c))).await;
    let baseline = a["item"]["allocated"].as_u64().unwrap();
    let (_, logs) = json(&f.router, get(&format!("/api/v1/cleanup/candidates?snapshot_id={sid}&category=logs"))).await;
    let (_, b) = json(&f.router, post_json(&url, add_cleanup(sid, &logs["items"][0]))).await;
    let item_a = a["item"]["id"].as_i64().unwrap();
    let item_b = b["item"]["id"].as_i64().unwrap();
    json(&f.router, patch_json(&format!("{url}/{item_a}"), serde_json::json!({ "status": "done" }))).await;
    let (_, before) = json(&f.router, get(&format!("/api/v1/plans/{plan}"))).await;
    assert_eq!(before["items"][0]["verify"]["status"], "unverified");

    // The user deletes the node_modules folder by hand, then rescans manually.
    tree.remove_dir("proj/node_modules");
    scan_tree(&f, &tree).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let view = loop {
        let (_, v) = json(&f.router, get(&format!("/api/v1/plans/{plan}"))).await;
        if v["items"][0]["verify"]["status"] != "unverified" {
            break v;
        }
        assert!(std::time::Instant::now() < deadline, "verification did not run: {v}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let by_id = |id: i64| view["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap().clone();
    assert_eq!(by_id(item_a)["verify"]["status"], "removed");
    assert_eq!(by_id(item_a)["verify"]["freed"], baseline);
    assert_eq!(by_id(item_b)["verify"]["status"], "present", "the old log is still there: still present, 0 B freed");
    assert_eq!(by_id(item_b)["verify"]["freed"], 0);
    assert_eq!(view["freed_total"], baseline);
    let v = &view["verifications"][0];
    assert_eq!(v["items"], 2);
    assert_eq!(v["freed"], baseline);
    assert!(v["scanned_change"].as_i64().unwrap() >= baseline as i64, "the scanned size dropped at least by the folder");
    assert!(view["baseline_as_of"].is_string());

    // Verification can also be asked for explicitly and never opens files (snapshot data only).
    let (status, job) = json(&f.router, post_json(&format!("/api/v1/plans/{plan}/verify"), serde_json::json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(wait_job(&f.router, job["id"].as_u64().unwrap()).await["status"], "succeeded");
    assert_eq!(json(&f.router, post_json("/api/v1/plans/999/verify", serde_json::json!({}))).await.0, StatusCode::NOT_FOUND);
}

#[test]
fn the_api_has_no_endpoint_that_acts_on_files_or_runs_commands() {
    let src = include_str!("../../src/app.rs");
    let routes: Vec<&str> = src.lines().filter_map(|l| l.trim().strip_prefix(".route(\"")).filter_map(|r| r.split('"').next()).collect();
    assert!(routes.len() > 40, "found {} routes", routes.len());
    for r in &routes {
        for verb in ["delete-file", "remove-file", "recycle", "execute", "exec", "run-command", "run", "shell/run", "move", "rename", "compress", "archive", "hardlink", "link", "script", "uninstall", "apply", "clean"] {
            // `/shell/reveal` is the one allowed shell route; `/cleanup/...` is read-only suggestions.
            let seg_hit = r.split('/').any(|s| s == verb);
            assert!(!seg_hit, "route {r} looks like an action ({verb})");
        }
    }
    // The only route under /shell shows an item in Explorer.
    assert_eq!(routes.iter().copied().filter(|r| r.starts_with("/shell")).collect::<Vec<&str>>(), ["/shell/reveal"]);
    // The only DELETE handlers remove our own catalog rows (plans, plan items, snapshots, duplicate suggestions).
    for line in src.lines().filter(|l| l.trim().starts_with(".route(") && l.contains("delete(")) {
        let l = line.trim();
        assert!(
            l.contains("/plans") || l.contains("/snapshots") || l.contains("/duplicates/suggestions") || l.contains("delete_snapshot") || l.contains("delete_plan") || l.contains("remove_plan_item") || l.contains("duplicates_clear"),
            "unexpected DELETE route: {l}"
        );
    }
}
