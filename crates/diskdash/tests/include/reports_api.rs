// Included from http.rs: report export (HTML, CSV in a ZIP, JSON) and the safe "save to folder".

use std::io::Read as _;

async fn raw_post(f: &Fixture, path: &str, body: serde_json::Value) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let res = f.router.clone().oneshot(post_json(path, body)).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, headers, bytes)
}

async fn scanned_fixture() -> (Fixture, test_fixtures::Fixture, i64) {
    let f = fixture(false);
    let tree = test_fixtures::Fixture::new();
    tree.file("videos/movie.mkv", 3 << 20);
    tree.file("docs/report.pdf", 200 << 10);
    tree.file("docs/notes.txt", 100);
    tree.file("big/disk.bin", 5 << 20);
    let sid = scan_tree(&f, &tree).await;
    (f, tree, sid)
}

fn json_report(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).expect("a JSON report")
}

#[tokio::test]
async fn sections_say_what_has_data_and_why_not() {
    let f = fixture(false);
    let (st, v) = json(&f.router, get("/api/v1/reports/sections")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let by = |id: &str| v.as_array().unwrap().iter().find(|s| s["id"] == id).cloned().unwrap_or_else(|| panic!("section {id}: {v}"));
    assert_eq!(by("drives")["available"], true);
    for id in ["top", "stale", "duplicates", "plans", "cleanup"] {
        assert_eq!(by(id)["available"], false, "{id}");
        assert!(by(id)["reason"].as_str().unwrap().len() > 10, "{id} says why");
    }
    assert_eq!(by("top")["title"], "Largest folders and files");
    assert_eq!(json(&f.router, get("/api/v1/reports/sections?drive=ZZ")).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_json_report_has_a_schema_version_byte_sizes_and_marks_missing_sections() {
    let (f, tree, sid) = scanned_fixture().await;
    let (st, headers, bytes) = raw_post(&f, "/api/v1/reports", serde_json::json!({ "format": "json", "sections": ["drives", "top", "duplicates"], "top_n": 3 })).await;
    assert_eq!(st, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    assert_eq!(headers["content-type"], "application/json");
    let disp = headers["content-disposition"].to_str().unwrap();
    assert!(disp.starts_with("attachment; filename=\"diskdash-report-") && disp.ends_with(".json\""), "{disp}");
    assert_eq!(headers["x-content-type-options"], "nosniff");

    let r = json_report(&bytes);
    assert_eq!(r["schema_version"], 1);
    assert!(r["generated_at"].as_str().unwrap().ends_with('Z'), "UTC RFC 3339");
    assert_eq!(r["top_n"], 3);
    let sections = r["sections"].as_object().unwrap();
    assert_eq!(sections.keys().map(String::as_str).collect::<Vec<_>>(), ["drives", "duplicates", "top"], "only the selected sections");
    assert_eq!(sections["drives"]["status"], "ok");
    assert!(sections["drives"]["data"][0]["capacity"].is_u64(), "sizes are integer bytes");
    let top = &sections["top"]["data"][0];
    assert_eq!(top["snapshot_id"], sid);
    let files = top["files"].as_array().unwrap();
    assert_eq!(files.len(), 3, "top_n applies");
    assert!(files[0]["path"].as_str().unwrap().ends_with(r"big\disk.bin"), "{files:?}");
    assert_eq!(files[0]["logical"], 5 << 20);
    // No duplicate analysis has run: the section is there and says so, and the export still succeeded.
    assert_eq!(sections["duplicates"]["status"], "not_available");
    assert!(sections["duplicates"]["reason"].as_str().unwrap().contains("duplicate analysis"));
    assert!(sections["duplicates"].get("data").is_none());
    let _ = tree;
}

#[tokio::test]
async fn the_drive_filter_and_the_limits_are_validated() {
    let (f, _tree, _) = scanned_fixture().await;
    let (st, _, bytes) = raw_post(&f, "/api/v1/reports", serde_json::json!({ "format": "json", "sections": ["drives"], "drive": "D" })).await;
    assert_eq!(st, StatusCode::OK);
    let rows = json_report(&bytes)["sections"]["drives"]["data"].as_array().unwrap().clone();
    assert!(rows.iter().all(|d| d["drive"] == "D:"), "{rows:?}");
    assert_eq!(json_report(&bytes)["scope"]["drive"], "D:");
    for body in [
        serde_json::json!({ "format": "json", "drive": "not-a-drive" }),
        serde_json::json!({ "format": "json", "top_n": 0 }),
        serde_json::json!({ "format": "json", "top_n": 101 }),
    ] {
        let (st, _, b) = raw_post(&f, "/api/v1/reports", body.clone()).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body}: {}", String::from_utf8_lossy(&b));
    }
    let (st, _, _) = raw_post(&f, "/api/v1/reports", serde_json::json!({ "format": "exe" })).await;
    assert!(st.is_client_error(), "unknown formats are refused");
}

#[tokio::test]
async fn a_csv_export_is_a_zip_with_one_file_per_section() {
    let (f, _tree, _) = scanned_fixture().await;
    let (st, headers, bytes) = raw_post(&f, "/api/v1/reports", serde_json::json!({ "format": "csv", "sections": ["drives", "top", "stale", "plans"], "top_n": 5 })).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(headers["content-type"], "application/zip");
    assert!(headers["content-disposition"].to_str().unwrap().ends_with(".zip\""));
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a zip");
    let names: Vec<String> = zip.file_names().map(str::to_string).collect();
    for expected in ["drives.csv", "top-folders.csv", "top-files.csv", "stale.csv", "manifest.json"] {
        assert!(names.contains(&expected.to_string()), "{expected} in {names:?}");
    }
    assert!(!names.iter().any(|n| n.ends_with(".ps1") || n.ends_with(".bat") || n.ends_with(".cmd")), "{names:?}");
    assert!(!names.contains(&"plans.csv".to_string()), "a section without data has no file");

    let mut read = |name: &str| {
        let mut s = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut s).unwrap();
        s
    };
    let files_csv = read("top-files.csv");
    assert_eq!(&files_csv[..3], b"\xEF\xBB\xBF", "a byte-order mark for Excel");
    let text = String::from_utf8(files_csv[3..].to_vec()).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next().unwrap(), "root,snapshot_id,scanned_at,rank,path,allocated_bytes,logical_bytes");
    let first: Vec<&str> = lines.next().unwrap().split(',').collect();
    assert!(first.iter().any(|c| c.ends_with(r"big\disk.bin")), "{first:?}");
    assert!(first.last().unwrap().parse::<u64>().is_ok(), "logical size is an integer");
    assert!(first[2].ends_with('Z'), "UTC timestamps");
    let manifest = json_report(&read("manifest.json"));
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["sections"]["plans"]["status"], "not_available");
}

#[tokio::test]
async fn the_html_report_is_self_contained_and_has_no_script() {
    let (f, _tree, _) = scanned_fixture().await;
    let (st, headers, bytes) = raw_post(&f, "/api/v1/reports", serde_json::json!({ "format": "html" })).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(headers["content-type"], "text/html; charset=utf-8");
    assert!(headers["content-disposition"].to_str().unwrap().ends_with(".html\""));
    let html = String::from_utf8(bytes).unwrap();
    assert!(html.starts_with("<!doctype html>"));
    for needle in ["http://", "https://", "//cdn", "<script", "<link", "<iframe", "<img", " src=", "@import", "url(", "javascript:", "onload=", "onclick="] {
        assert!(!html.contains(needle), "the report must not contain {needle}");
    }
    assert!(html.contains("<style>") && html.contains("<svg "), "inline CSS and inline charts");
    assert!(html.contains("Drive summary") && html.contains("Largest folders and files"));
    assert!(html.contains("Not available:"), "sections without data say so");
    assert!(html.contains("big\\disk.bin") || html.contains("disk.bin"), "the largest file is listed");
    // Every link is an in-page anchor.
    for part in html.split("href=\"").skip(1) {
        assert!(part.starts_with('#'), "external link: {}", &part[..part.len().min(40)]);
    }
}

#[tokio::test]
async fn saving_writes_a_new_file_into_the_chosen_folder_and_never_replaces_one() {
    let (f, _tree, _) = scanned_fixture().await;
    let out = test_fixtures::Fixture::new();
    let folder = out.root().to_string_lossy().to_string();
    let (st, a) = json(&f.router, post_json("/api/v1/reports/save", serde_json::json!({ "format": "json", "sections": ["drives"], "folder": folder }))).await;
    assert_eq!(st, StatusCode::OK, "{a}");
    let path = std::path::PathBuf::from(a["path"].as_str().unwrap());
    assert!(path.starts_with(out.root()) && path.extension().unwrap() == "json", "{path:?}");
    assert_eq!(a["bytes"].as_u64().unwrap(), std::fs::metadata(&path).unwrap().len());
    assert_eq!(json_report(&std::fs::read(&path).unwrap())["schema_version"], 1);

    // Saved again in the same second: a new file, the first is untouched.
    let before = std::fs::read(&path).unwrap();
    let (_, b) = json(&f.router, post_json("/api/v1/reports/save", serde_json::json!({ "format": "json", "sections": ["drives"], "folder": folder }))).await;
    if a["file_name"] == b["file_name"] {
        panic!("a second report must not reuse the first name");
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let names: Vec<String> = std::fs::read_dir(out.root()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert!(names.iter().all(|n| n.ends_with(".json") && !n.starts_with('.')), "{names:?}");
}

#[tokio::test]
async fn saving_refuses_bad_folders_with_a_field_message() {
    let (f, _tree, _) = scanned_fixture().await;
    for (folder, expect) in [
        (serde_json::json!(""), "choose a folder"),
        (serde_json::json!("relative\\dir"), "full folder path"),
        (serde_json::json!(r"\\server\share\reports"), "full folder path"),
        (serde_json::json!(r"C:\definitely\not\here"), "not a folder that exists"),
    ] {
        let (st, v) = json(&f.router, post_json("/api/v1/reports/save", serde_json::json!({ "format": "html", "folder": folder }))).await;
        assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{folder}: {v}");
        assert!(v["fields"]["folder"].as_str().unwrap().contains(expect), "{v}");
    }
    let (st, _) = json(&f.router, post_json("/api/v1/reports/save", serde_json::json!({ "format": "html" }))).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "no folder given");
}

#[tokio::test]
async fn exports_read_stored_data_only_and_leave_user_files_alone() {
    let (f, tree, _) = scanned_fixture().await;
    let before = test_fixtures::manifest(tree.root());
    let out = test_fixtures::Fixture::new();
    for format in ["json", "csv", "html"] {
        let (st, _, _) = raw_post(&f, "/api/v1/reports", serde_json::json!({ "format": format })).await;
        assert_eq!(st, StatusCode::OK, "{format}");
        let (st, v) = json(&f.router, post_json("/api/v1/reports/save", serde_json::json!({ "format": format, "folder": out.root().to_string_lossy() }))).await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    test_fixtures::assert_contents_unchanged(tree.root(), &before);
}

#[test]
fn report_routes_are_not_actions() {
    let src = include_str!("../../src/app.rs");
    let routes: Vec<&str> = src.lines().filter_map(|l| l.trim().strip_prefix(".route(\"")).filter_map(|r| r.split('"').next()).collect();
    for r in ["/reports", "/reports/save", "/reports/sections", "/alerts", "/alerts/{id}/dismiss"] {
        assert!(routes.contains(&r), "{r}");
    }
}
