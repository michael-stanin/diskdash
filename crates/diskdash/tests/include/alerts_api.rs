// Included from http.rs: in-app alerts (low space, fast growth), de-duplication, cooldown and dismiss.

use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

/// The recorded machine with D:'s free space under the test's control.
struct FreeSource {
    d_free_gib: Arc<AtomicU64>,
}

impl InventorySource for FreeSource {
    fn collect(&self) -> Inventory {
        let mut inv = RecordedSource.collect();
        let free = self.d_free_gib.load(AtomicOrdering::SeqCst) << 30;
        for d in &mut inv.drives {
            if d.letters.first().is_some_and(|l| l == "D") {
                d.free = free;
                d.used = d.capacity - free;
                d.used_fraction = d.used as f64 / d.capacity as f64;
            }
        }
        inv
    }
}

fn alerts_fixture(free_gib: u64) -> (Fixture, Arc<AtomicU64>) {
    let free = Arc::new(AtomicU64::new(free_gib));
    let f = fixture_parts(
        false,
        diskdash_analysis::rules::MapEnv::default(),
        diskdash_inventory::apps::registry::FakeRegistry::default(),
        Box::new(FreeSource { d_free_gib: free.clone() }),
        Arc::new(NoManifests),
    );
    (f, free)
}

async fn refresh(f: &Fixture) {
    let (st, v) = json(&f.router, post_json("/api/v1/drives/refresh", serde_json::json!({}))).await;
    assert!(st == StatusCode::OK || st == StatusCode::ACCEPTED, "{st} {v}");
    // Refresh runs in the background; give it a moment to finish and run the alert hook.
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(40)).await;
        if f.events.subscriber_count() == 0 {
            break;
        }
    }
}

async fn alerts(f: &Fixture, status: &str) -> serde_json::Value {
    let (st, v) = json(&f.router, get(&format!("/api/v1/alerts?status={status}"))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}

/// Wait until the active alert count equals `n`.
async fn wait_active(f: &Fixture, n: u64) -> serde_json::Value {
    for _ in 0..100 {
        let v = alerts(f, "active").await;
        if v["active"] == n {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    panic!("active alerts never became {n}: {}", alerts(f, "all").await);
}

#[tokio::test]
async fn a_drive_below_the_limit_raises_one_alert_and_repeats_stay_quiet() {
    let (f, _free) = alerts_fixture(40); // D: is 931 GiB, so 4.3% free
    let mut rx = f.events.subscribe(None).1;
    refresh(&f).await;
    let v = wait_active(&f, 1).await;
    let a = &v["alerts"][0];
    assert_eq!((a["kind"].as_str(), a["drive"].as_str()), (Some("low_space"), Some("D:")), "{a}");
    assert_eq!(a["title"], "D: is low on space");
    assert!(a["message"].as_str().unwrap().contains("40.0 GiB free"), "{a}");
    assert_eq!(v["banners"], 1);
    assert_eq!(v["thresholds"]["low_percent"], 10.0);
    assert_eq!(v["thresholds"]["cooldown_hours"], 24);
    assert!(v["running_since"].is_string());

    // The bell updates live: an SSE event carries the new count.
    let mut seen = None;
    while let Ok(ev) = rx.try_recv() {
        if ev.kind == "alert.changed" {
            seen = Some(ev.data);
        }
    }
    assert_eq!(seen.expect("alert.changed was published")["active"], 1);

    // Three more refreshes while still low: still one alert.
    for _ in 0..3 {
        refresh(&f).await;
    }
    assert_eq!(alerts(&f, "all").await["alerts"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn dismissing_hides_the_banner_until_the_alert_is_raised_again() {
    let (f, _free) = alerts_fixture(40);
    refresh(&f).await;
    let id = wait_active(&f, 1).await["alerts"][0]["id"].as_i64().unwrap();

    let (st, d) = json(&f.router, post_json(&format!("/api/v1/alerts/{id}/dismiss"), serde_json::json!({}))).await;
    assert_eq!(st, StatusCode::OK, "{d}");
    assert_eq!(d["dismissed"], true);
    let v = alerts(&f, "active").await;
    assert_eq!((v["active"].as_u64(), v["banners"].as_u64()), (Some(1), Some(0)), "still active, banner hidden");

    // Still low, inside the cooldown: not shown again.
    refresh(&f).await;
    assert_eq!(alerts(&f, "active").await["banners"], 0);

    // After the cooldown a still-low drive is raised again, and the banner returns.
    let conn = f.db.connect().unwrap();
    conn.execute("UPDATE alert_state SET last_raised_at = last_raised_at - 90000", []).unwrap();
    drop(conn);
    refresh(&f).await;
    let v = wait_active(&f, 1).await;
    assert_eq!(v["banners"], 1, "{v}");
    let all = alerts(&f, "all").await;
    assert_eq!(all["alerts"].as_array().unwrap().len(), 2, "the renewed alert replaced the dismissed one");
    assert_eq!(alerts(&f, "resolved").await["alerts"].as_array().unwrap().len(), 1);

    assert_eq!(json(&f.router, post_json("/api/v1/alerts/9999/dismiss", serde_json::json!({}))).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_alert_clears_only_when_free_space_is_well_past_the_limit_and_can_recur() {
    let (f, free) = alerts_fixture(40);
    refresh(&f).await;
    wait_active(&f, 1).await;

    // 10% of 931 GiB is 93 GiB: 100 GiB is above the limit but inside the 2% margin (112 GiB).
    free.store(100, AtomicOrdering::SeqCst);
    refresh(&f).await;
    assert_eq!(alerts(&f, "active").await["active"], 1, "hysteresis keeps it");

    free.store(300, AtomicOrdering::SeqCst);
    refresh(&f).await;
    wait_active(&f, 0).await;
    let resolved = alerts(&f, "resolved").await;
    assert_eq!(resolved["alerts"].as_array().unwrap().len(), 1);
    assert!(resolved["alerts"][0]["resolved_at"].is_string());

    // It dips again: a fresh alert, not a repeat of the old one.
    free.store(30, AtomicOrdering::SeqCst);
    refresh(&f).await;
    wait_active(&f, 1).await;
    assert_eq!(alerts(&f, "all").await["alerts"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn healthy_drives_raise_nothing_and_bad_queries_are_refused() {
    let (f, _free) = alerts_fixture(400);
    refresh(&f).await;
    let v = alerts(&f, "all").await;
    assert_eq!((v["active"].as_u64(), v["alerts"].as_array().unwrap().len()), (Some(0), 0), "{v}");
    assert_eq!(json(&f.router, get("/api/v1/alerts?status=bogus")).await.0, StatusCode::BAD_REQUEST);
}

fn set_growth_limit(gib_per_day: f64) -> Request<Body> {
    let mut s = default_settings();
    s["alert_growth_gib_per_day"] = serde_json::json!(gib_per_day);
    put_settings(s)
}

#[tokio::test]
async fn a_folder_that_balloons_between_scans_a_day_apart_raises_a_growth_alert() {
    let f = fixture(false);
    assert_eq!(json(&f.router, set_growth_limit(0.05)).await.0, StatusCode::OK); // 51 MiB per day
    let tree = test_fixtures::Fixture::new();
    tree.file("docs/a.txt", 1 << 20);
    let first = scan_tree(&f, &tree).await;

    // Only one scan: nothing to compare, no growth alert.
    assert_eq!(alerts(&f, "all").await["alerts"].as_array().unwrap().iter().filter(|a| a["kind"] == "growth").count(), 0);

    // The first scan happened two days ago; since then 200 MiB landed in a new folder.
    let conn = f.db.connect().unwrap();
    conn.execute(
        "UPDATE snapshots SET started_at = '2026-01-01T00:00:00Z', finished_at = datetime('now', '-2 days') WHERE id = ?1",
        [first],
    )
    .unwrap();
    // The catalog stores RFC 3339 text.
    conn.execute(
        "UPDATE snapshots SET finished_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-2 days') WHERE id = ?1",
        [first],
    )
    .unwrap();
    drop(conn);
    tree.file("vms/disk.vhdx", 200 << 20);
    let second = scan_tree(&f, &tree).await;

    let v = wait_for_alert(&f, "growth").await;
    let a = v["alerts"].as_array().unwrap().iter().find(|a| a["kind"] == "growth").unwrap();
    assert!(a["path"].as_str().unwrap().ends_with("vms"), "{a}");
    assert!(a["title"].as_str().unwrap().contains("growing fast"));
    assert_eq!(a["history"], serde_json::json!({ "old": first, "new": second }), "links to the history diff");
    let rate = a["value"].as_f64().unwrap();
    assert!((rate / f64::from(1 << 20) - 100.0).abs() < 5.0, "about 100 MiB per day: {rate}");

    // A third scan an hour later: less than a day after the second, so it compares with the first and
    // finds the same folder: still one alert.
    let third = scan_tree(&f, &tree).await;
    let _ = third;
    let all = alerts(&f, "all").await;
    let growth_count = all["alerts"].as_array().unwrap().iter().filter(|a| a["kind"] == "growth").count();
    assert_eq!(growth_count, 1, "the cooldown keeps it from repeating: {all}");
}

async fn wait_for_alert(f: &Fixture, kind: &str) -> serde_json::Value {
    for _ in 0..100 {
        let v = alerts(f, "active").await;
        if v["alerts"].as_array().unwrap().iter().any(|a| a["kind"] == kind) {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    panic!("no {kind} alert: {}", alerts(f, "all").await);
}

#[tokio::test]
async fn scans_less_than_a_day_apart_are_not_compared() {
    let f = fixture(false);
    assert_eq!(json(&f.router, set_growth_limit(0.05)).await.0, StatusCode::OK);
    let tree = test_fixtures::Fixture::new();
    tree.file("docs/a.txt", 1 << 20);
    scan_tree(&f, &tree).await;
    tree.file("vms/disk.vhdx", 200 << 20);
    scan_tree(&f, &tree).await;
    let v = alerts(&f, "all").await;
    assert_eq!(v["alerts"].as_array().unwrap().iter().filter(|a| a["kind"] == "growth").count(), 0, "{v}");
}

#[tokio::test]
async fn alert_settings_are_validated() {
    let f = fixture(false);
    for (field, value) in [("alert_growth_gib_per_day", serde_json::json!(0.0)), ("alert_growth_gib_per_day", serde_json::json!(20_000.0)), ("alert_cooldown_hours", serde_json::json!(0)), ("alert_cooldown_hours", serde_json::json!(1000))] {
        let mut s = default_settings();
        s[field] = value.clone();
        let (st, v) = json(&f.router, put_settings(s)).await;
        assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{field}={value}: {v}");
    }
}
