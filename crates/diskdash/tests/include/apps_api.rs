// Included from http.rs: installed applications through the HTTP API (fake registry).

use diskdash_inventory::apps::registry::{FakeRegistry, Hive, RegValue, View};

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const USERASSIST: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist";

fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a') + 13) % 26 + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A') + 13) % 26 + b'A') as char,
            o => o,
        })
        .collect()
}

fn launch_block(unix: i64) -> Vec<u8> {
    let mut d = vec![0u8; 72];
    d[4..8].copy_from_slice(&3u32.to_le_bytes());
    d[60..68].copy_from_slice(&(((unix + 11_644_473_600) as u64) * 10_000_000).to_le_bytes());
    d
}

fn s(v: &str) -> RegValue {
    RegValue::Str(v.into())
}

#[tokio::test]
async fn installed_apps_with_sizes_and_last_used_sources() {
    let tree = test_fixtures::Fixture::new();
    tree.file("Games/Foo/foo.exe", 300 << 10);
    tree.file("Games/Foo/data.bin", 700 << 10);
    let foo_dir = tree.root().join("Games").join("Foo").to_string_lossy().into_owned();
    let foo_exe = format!(r"{foo_dir}\foo.exe");

    let registry = FakeRegistry::default()
        .key(Hive::Hklm, View::V64, &format!(r"{UNINSTALL}\Foo"), &[
            ("DisplayName", s("Foo")),
            ("InstallLocation", s(&foo_dir)),
            ("DisplayIcon", s(&format!("\"{foo_exe}\",0"))),
        ])
        .key(Hive::Hklm, View::V64, &format!(r"{UNINSTALL}\Bar"), &[
            ("DisplayName", s("Bar")),
            ("EstimatedSize", RegValue::Dword(2 * 1024 * 1024)),
            ("InstallDate", s("20230115")),
        ])
        .key(Hive::Hklm, View::V64, &format!(r"{UNINSTALL}\Baz"), &[("DisplayName", s("Baz"))])
        .key(Hive::Hkcu, View::V64, &format!(r"{USERASSIST}\{{G}}\Count"), &[(
            rot13(&foo_exe).as_str(),
            RegValue::Binary(launch_block(1_700_000_000)),
        )]);
    let f = fixture_full(false, diskdash_analysis::rules::MapEnv::default(), registry);

    // Before any scan, Foo's size is unknown.
    let (status, v) = json(&f.router, get("/api/v1/apps")).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let foo = v["apps"].as_array().unwrap().iter().find(|a| a["name"] == "Foo").unwrap().clone();
    assert_eq!(foo["size_source"], "unknown");
    assert_eq!(foo["last_used_source"], "userassist");
    assert!(foo["last_used"].as_str().unwrap().starts_with("2023-11-14"));
    assert_eq!(v["prefetch_available"], false, "no elevated helper: Prefetch is never read");

    scan_tree(&f, &tree).await;
    let (_, v) = json(&f.router, get("/api/v1/apps")).await;
    let by = |n: &str| v["apps"].as_array().unwrap().iter().find(|a| a["name"] == n).unwrap().clone();
    assert_eq!(by("Foo")["size_source"], "scan");
    assert!(by("Foo")["size"].as_u64().unwrap() >= 1000 << 10);
    assert_eq!(by("Bar")["size_source"], "estimated");
    assert_eq!(by("Bar")["size"], 2u64 * 1024 * 1024 * 1024);
    assert_eq!(by("Bar")["install_date"], "2023-01-15");
    assert_eq!(by("Bar")["last_used_source"], "unknown");
    assert_eq!(by("Baz")["size_source"], "unknown");
    assert_eq!(v["apps"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn unreadable_registry_is_an_error_with_the_reason() {
    let registry = FakeRegistry::default()
        .denied(Hive::Hklm, View::V64, UNINSTALL)
        .denied(Hive::Hklm, View::V32, UNINSTALL)
        .denied(Hive::Hkcu, View::V64, r"Software\Microsoft\Windows\CurrentVersion\Uninstall");
    let f = fixture_full(false, diskdash_analysis::rules::MapEnv::default(), registry);
    let (status, v) = json(&f.router, get("/api/v1/apps")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(v["message"].as_str().unwrap().contains("access denied"), "{v}");
}

#[tokio::test]
async fn no_apps_is_an_empty_list_not_an_error() {
    let f = fixture(false);
    let (status, v) = json(&f.router, get("/api/v1/apps")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["apps"].as_array().unwrap().len(), 0);
}
