#![allow(clippy::unwrap_used)]
//! HTTP-level tests of the security boundary, settings, jobs, SSE and the SPA.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use diskdash::app::{build_router, AppState};
use diskdash::assets::MemoryAssets;
use diskdash::events::EventBus;
use diskdash::inventory::{InventoryService, InventorySource};
use diskdash::jobs::JobRunner;
use diskdash::security::Security;
use diskdash::system::ElevationDetector;
use diskdash_inventory::model::{HealthSources, Inventory, TrimState};
use diskdash_store::{open_catalog, DataDir, OwnFiles};
use http_body_util::BodyExt;
use tower::ServiceExt;

const PORT: u16 = 8765;
const TOKEN: &str = "test-token";
const HOST: &str = "127.0.0.1:8765";

struct NotElevated;
impl ElevationDetector for NotElevated {
    fn is_elevated(&self) -> bool {
        false
    }
}

/// This machine's recorded inventory (C: NVMe, D: HDD, G: SATA SSD, E: optical).
struct RecordedSource;
impl InventorySource for RecordedSource {
    fn collect(&self) -> Inventory {
        let raw = diskdash_inventory::raw::RawInventory::from_json(include_str!(
            "../../diskdash-inventory/tests/fixtures/this_machine.json"
        ))
        .unwrap();
        let inputs = diskdash_inventory::assemble::HealthInputs {
            elevated: false,
            reliability: &[],
            smart: &[],
            windows_letter: Some("C".into()),
        };
        let (drives, excluded) = diskdash_inventory::assemble::assemble(&raw, &inputs);
        Inventory {
            drives,
            excluded,
            trim: TrimState { ntfs: Some(true), refs: Some(true), reason: None },
            sources: HealthSources { elevated: false, smartctl: None },
            fallback: false,
        }
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    router: Router,
    events: Arc<EventBus>,
}

fn fixture(dev: bool) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let dd = DataDir::at(tmp.path().join("dd"));
    let own = OwnFiles::new(&dd).unwrap();
    let db = open_catalog(&dd, &own).unwrap();
    let events = Arc::new(EventBus::new());
    let jobs = JobRunner::start(db.clone(), events.clone(), 2).unwrap();
    let inventory = Arc::new(InventoryService::new(Box::new(RecordedSource), db.clone(), events.clone()));
    let mut files = HashMap::new();
    files.insert(
        "index.html".to_string(),
        br#"<html><head><meta name="diskdash-token" content="__DISKDASH_TOKEN__"></head></html>"#.to_vec(),
    );
    files.insert("assets/app-abc123.js".to_string(), b"console.log(1)".to_vec());
    let state = AppState {
        security: Arc::new(Security::with_token(PORT, TOKEN)),
        db,
        jobs,
        events: events.clone(),
        inventory,
        assets: Arc::new(MemoryAssets(files)),
        elevation: Arc::new(NotElevated),
        data_dir: dd.root().to_path_buf(),
        dev,
    };
    Fixture { _tmp: tmp, router: build_router(state), events }
}

fn req(method: Method, path: &str) -> axum::http::request::Builder {
    Request::builder().method(method).uri(path).header(header::HOST, HOST)
}

async fn send(router: &Router, r: Request<Body>) -> (StatusCode, String, axum::http::HeaderMap) {
    let res = router.clone().oneshot(r).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned(), headers)
}

fn json_body(v: serde_json::Value) -> Body {
    Body::from(v.to_string())
}

fn default_settings() -> serde_json::Value {
    serde_json::json!({
        "low_space_percent": 10.0, "low_space_floor_gib": 20, "stale_months": 12,
        "hdd_scan_concurrency": 1, "ssd_scan_concurrency": 4, "chart_children_per_level": 20,
        "hardlink_detection": "system-volume", "hardlink_overrides": {}
    })
}

#[tokio::test]
async fn foreign_host_is_rejected_with_421() {
    let f = fixture(false);
    let r =
        Request::builder().uri("/api/v1/health").header(header::HOST, "evil.example:8765").body(Body::empty()).unwrap();
    let (status, _, _) = send(&f.router, r).await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
}

#[tokio::test]
async fn localhost_host_is_accepted() {
    let f = fixture(false);
    let r =
        Request::builder().uri("/api/v1/health").header(header::HOST, "localhost:8765").body(Body::empty()).unwrap();
    let (status, body, _) = send(&f.router, r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"status\":\"ok\""));
}

#[tokio::test]
async fn state_changing_request_without_token_is_forbidden() {
    let f = fixture(false);
    let r = req(Method::PUT, "/api/v1/settings")
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(default_settings()))
        .unwrap();
    let (status, body, _) = send(&f.router, r).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("missing token"));
}

#[tokio::test]
async fn wrong_token_and_cross_origin_are_forbidden() {
    let f = fixture(false);
    let wrong = req(Method::PUT, "/api/v1/settings")
        .header("x-diskdash-token", "nope")
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(default_settings()))
        .unwrap();
    assert_eq!(send(&f.router, wrong).await.0, StatusCode::FORBIDDEN);

    let cross = req(Method::PUT, "/api/v1/settings")
        .header("x-diskdash-token", TOKEN)
        .header(header::ORIGIN, "https://other.site")
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(default_settings()))
        .unwrap();
    let (status, body, _) = send(&f.router, cross).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("cross-origin"));
}

#[tokio::test]
async fn settings_defaults_validation_and_save() {
    let f = fixture(false);
    let (status, body, _) = send(&f.router, req(Method::GET, "/api/v1/settings").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    let got: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(got, default_settings());

    let mut bad = default_settings();
    bad["low_space_percent"] = serde_json::json!(150.0);
    let r = req(Method::PUT, "/api/v1/settings")
        .header("x-diskdash-token", TOKEN)
        .header(header::ORIGIN, "http://127.0.0.1:8765")
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(bad))
        .unwrap();
    let (status, body, _) = send(&f.router, r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let err: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(err["fields"]["low_space_percent"].is_string());

    let mut good = default_settings();
    good["stale_months"] = serde_json::json!(24);
    let r = req(Method::PUT, "/api/v1/settings")
        .header("x-diskdash-token", TOKEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(good.clone()))
        .unwrap();
    assert_eq!(send(&f.router, r).await.0, StatusCode::OK);
    let (_, body, _) = send(&f.router, req(Method::GET, "/api/v1/settings").body(Body::empty()).unwrap()).await;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap(), good);
}

#[tokio::test]
async fn system_reports_reduced_mode() {
    let f = fixture(false);
    let (status, body, _) = send(&f.router, req(Method::GET, "/api/v1/system").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["elevated"], false);
    assert!(!v["reduced_features"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn spa_fallback_injects_token_and_serves_assets() {
    let f = fixture(false);
    let (status, body, headers) = send(&f.router, req(Method::GET, "/drives/c").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(&format!("content=\"{TOKEN}\"")));
    assert!(!body.contains("__DISKDASH_TOKEN__"));
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(headers[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"));

    let (status, _, headers) =
        send(&f.router, req(Method::GET, "/assets/app-abc123.js").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers[header::CONTENT_TYPE].to_str().unwrap().contains("javascript"));
    assert!(headers[header::CACHE_CONTROL].to_str().unwrap().contains("immutable"));

    let (status, _, _) = send(&f.router, req(Method::GET, "/missing.png").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_api_route_is_json_404() {
    let f = fixture(false);
    let (status, body, _) = send(&f.router, req(Method::GET, "/api/v1/nope").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("not_found"));
}

#[tokio::test]
async fn dev_only_routes_are_absent_in_normal_mode() {
    let f = fixture(false);
    let (status, _, _) = send(&f.router, req(Method::GET, "/api/v1/session").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn job_submit_poll_and_cancel_over_http() {
    let f = fixture(true);
    let start = std::time::Instant::now();
    let r = req(Method::POST, "/api/v1/jobs")
        .header("x-diskdash-token", TOKEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json_body(serde_json::json!({"kind": "noop-sleep", "duration_ms": 60000})))
        .unwrap();
    let (status, body, _) = send(&f.router, r).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(start.elapsed() < Duration::from_millis(500));
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"].as_u64().unwrap();

    let r = req(Method::POST, &format!("/api/v1/jobs/{id}/cancel"))
        .header("x-diskdash-token", TOKEN)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&f.router, r).await.0, StatusCode::OK);
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let (_, body, _) =
            send(&f.router, req(Method::GET, &format!("/api/v1/jobs/{id}")).body(Body::empty()).unwrap()).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        if v["status"] == "cancelled" {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "not cancelled within 2 s: {v}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let (status, _, _) = send(&f.router, req(Method::GET, "/api/v1/jobs/99999").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn event_stream_replays_after_last_event_id() {
    let f = fixture(false);
    for i in 0..3 {
        f.events.publish("job.progress", serde_json::json!({ "n": i }));
    }
    let r = req(Method::GET, "/api/v1/events").header("last-event-id", "1").body(Body::empty()).unwrap();
    let res = f.router.clone().oneshot(r).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "text/event-stream");
    let mut body = res.into_body();
    let mut text = String::new();
    while !(text.contains("id: 2") && text.contains("id: 3")) {
        let frame = tokio::time::timeout(Duration::from_secs(2), body.frame()).await.unwrap().unwrap().unwrap();
        if let Ok(data) = frame.into_data() {
            text.push_str(&String::from_utf8_lossy(&data));
        }
    }
    assert!(!text.contains("id: 1\n"), "event 1 was already seen");
    assert!(text.find("id: 2").unwrap() < text.find("id: 3").unwrap());
}

#[tokio::test]
async fn drives_endpoint_lists_in_scope_drives_with_low_space() {
    let f = fixture(false);
    let (status, body, _) = send(&f.router, req(Method::GET, "/api/v1/drives").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let letters: Vec<String> =
        v["drives"].as_array().unwrap().iter().map(|d| d["letters"][0].as_str().unwrap().to_string()).collect();
    assert_eq!(letters, ["C", "D", "G"]);
    assert_eq!(v["excluded"]["optical"], 1);
    let d = &v["drives"][1];
    assert_eq!(d["media"], "HDD");
    assert_eq!(d["low_space"], true);
    assert_eq!(d["low_space_reason"], "percent");
    assert!(v["refreshed_at"].is_string());

    let id = v["drives"][0]["id"].as_str().unwrap().to_string();
    let (status, body, _) =
        send(&f.router, req(Method::GET, &format!("/api/v1/drives/{id}")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Lexar 512GB SSD"));

    let (status, _, _) =
        send(&f.router, req(Method::GET, "/api/v1/drives/not-a-volume").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn drives_refresh_is_a_job() {
    let f = fixture(false);
    let r = req(Method::POST, "/api/v1/drives/refresh").header("x-diskdash-token", TOKEN).body(Body::empty()).unwrap();
    let (status, body, _) = send(&f.router, r).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"].as_u64().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let (_, body, _) =
            send(&f.router, req(Method::GET, &format!("/api/v1/jobs/{id}")).body(Body::empty()).unwrap()).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        if v["status"] == "succeeded" {
            assert_eq!(v["kind"], "inventory-refresh");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{v}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
