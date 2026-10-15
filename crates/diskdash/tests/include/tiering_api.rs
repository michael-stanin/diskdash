// Included from http.rs: storage tiering suggestions through the HTTP API, on planted scan snapshots.

use diskdash_analysis::tiering::ManifestSource;
use diskdash_inventory::apps::registry::{FakeRegistry as TierRegistry, Hive as TierHive, RegValue as TierValue, View as TierView};

const TGIB: i64 = 1 << 30;

/// The recorded machine (C: NVMe, D: HDD, G: SATA SSD) with D: given `d_free_gib` free, or without D:.
struct TierSource {
    d_free_gib: u64,
    drop_hdd: bool,
}

impl InventorySource for TierSource {
    fn collect(&self) -> Inventory {
        let mut inv = RecordedSource.collect();
        inv.drives.retain(|d| !(self.drop_hdd && d.letters.first().is_some_and(|l| l == "D")));
        for d in &mut inv.drives {
            if d.letters.first().is_some_and(|l| l == "D") {
                d.free = self.d_free_gib << 30;
                d.used = d.capacity - d.free;
                d.used_fraction = d.used as f64 / d.capacity as f64;
            }
        }
        inv
    }
}

#[derive(Default)]
struct Manifests(std::collections::HashMap<String, String>);

impl ManifestSource for Manifests {
    fn read_to_string(&self, path: &str) -> std::io::Result<String> {
        self.0.get(&path.to_lowercase()).cloned().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))
    }

    fn list_files(&self, dir: &str, prefix: &str, ext: &str) -> Vec<String> {
        let d = format!("{}\\", dir.to_lowercase());
        let mut v: Vec<String> =
            self.0.keys().filter(|k| k.strip_prefix(&d).is_some_and(|n| !n.contains('\\') && n.starts_with(prefix) && n.ends_with(ext))).cloned().collect();
        v.sort();
        v
    }
}

fn tier_fixture(d_free_gib: u64, drop_hdd: bool, registry: TierRegistry, manifests: Manifests) -> Fixture {
    fixture_parts(
        false,
        diskdash_analysis::rules::MapEnv::default().with_var("SystemDrive", "C:"),
        registry,
        Box::new(TierSource { d_free_gib, drop_hdd }),
        Arc::new(manifests),
    )
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

async fn drive_id(f: &Fixture, letter: &str) -> String {
    let (_, v) = json(&f.router, get("/api/v1/drives")).await;
    v["drives"].as_array().unwrap().iter().find(|d| d["letters"][0] == letter).map(|d| d["id"].as_str().unwrap().to_string()).unwrap()
}

/// Plant a complete scan of a drive root: items are (path, is_dir, size in bytes, age in days).
async fn plant(f: &Fixture, letter: &str, items: &[(&str, bool, i64, i64)]) -> i64 {
    let root = format!("{letter}:\\");
    let vol = drive_id(f, letter).await;
    let conn = f.db.connect().unwrap();
    conn.execute(
        "INSERT INTO snapshots (root, volume_id, hardlinks, status, started_at, finished_at, db_file, time_basis, time_basis_reason)
         VALUES (?1, ?2, 'detected', 'complete', '2026-10-15T10:00:00Z', '2026-10-15T10:05:00Z', '', 'mtime', 'disabled')",
        rusqlite::params![root, vol],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    let rel = format!("snapshots/{id}.db");
    conn.execute("UPDATE snapshots SET db_file = ?2 WHERE id = ?1", rusqlite::params![id, rel]).unwrap();
    let path = f.own.prepare_db_path(&f.own.root().join(&rel)).unwrap();
    let s = rusqlite::Connection::open(path).unwrap();
    s.execute_batch(diskdash_core::store::SCHEMA).unwrap();

    // Parent folders, DFS order, ids, recursive totals.
    let now = now_secs();
    let parts = |p: &str| -> Vec<String> { p.split('\\').filter(|x| !x.is_empty()).map(str::to_string).collect() };
    let root_parts = parts(&root);
    let mut all: std::collections::BTreeMap<Vec<String>, (bool, i64, i64)> = Default::default();
    for (p, dir, size, age) in items {
        let v = parts(p);
        for n in root_parts.len() + 1..v.len() {
            all.entry(v[..n].to_vec()).or_insert((true, 0, now));
        }
        all.insert(v, (*dir, *size, now - age * 86_400));
    }
    let mut keys: Vec<(Vec<String>, Vec<String>)> = all.keys().map(|k| (k.iter().map(|x| x.to_lowercase()).collect(), k.clone())).collect();
    keys.sort();
    let mut ids = std::collections::HashMap::new();
    ids.insert(root_parts.clone(), 0i64);
    struct R {
        id: i64,
        parent: i64,
        name: String,
        dir: bool,
        mtime: i64,
        path: String,
        depth: i64,
        total: i64,
        files: i64,
        last: i64,
        max_mtime: i64,
    }
    let mut rows: Vec<R> = Vec::new();
    for (n, (_, orig)) in keys.iter().enumerate() {
        let (dir, size, mtime) = all[orig];
        let id = n as i64 + 1;
        ids.insert(orig.clone(), id);
        rows.push(R {
            id,
            parent: ids[&orig[..orig.len() - 1].to_vec()],
            name: orig.last().unwrap().clone(),
            dir,
            mtime,
            path: orig.join("\\"),
            depth: (orig.len() - root_parts.len()) as i64,
            total: size,
            files: i64::from(!dir),
            last: id,
            max_mtime: if dir { 0 } else { mtime },
        });
    }
    let index: std::collections::HashMap<i64, usize> = rows.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    let mut root_total = 0;
    for i in (0..rows.len()).rev() {
        let (parent, total, files, mt, last) = (rows[i].parent, rows[i].total, rows[i].files, rows[i].max_mtime, rows[i].last);
        if parent == 0 {
            root_total += total;
        } else {
            let p = &mut rows[index[&parent]];
            p.total += total;
            p.files += files;
            p.max_mtime = p.max_mtime.max(mt);
            p.last = p.last.max(last);
        }
    }
    s.execute(
        "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, attrs, path, last_desc_id) VALUES (0, NULL, ?1, lower(?1), 1, 0, ?2, ?2, 0, ?3, ?4)",
        rusqlite::params![root.trim_end_matches('\\'), root_total, root, rows.len() as i64],
    )
    .unwrap();
    for r in &rows {
        s.execute(
            "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, mtime, attrs, path, file_count, dir_count, max_mtime, last_desc_id)
             VALUES (?1, ?2, ?3, lower(?3), ?4, ?5, ?6, ?6, ?7, 0, ?8, ?9, 0, ?10, ?11)",
            rusqlite::params![r.id, r.parent, r.name, i64::from(r.dir), r.depth, r.total, r.mtime, r.dir.then(|| r.path.clone()), r.files, r.max_mtime, r.last],
        )
        .unwrap();
    }
    conn.execute("UPDATE snapshots SET total_alloc = ?2, total_size = ?2 WHERE id = ?1", rusqlite::params![id, root_total]).unwrap();
    id
}

fn find<'a>(v: &'a serde_json::Value, path_end: &str) -> Option<&'a serde_json::Value> {
    v["suggestions"].as_array().unwrap().iter().find(|s| s["path"].as_str().unwrap().to_lowercase().ends_with(&path_end.to_lowercase()))
}

fn drive<'a>(v: &'a serde_json::Value, letter: &str) -> &'a serde_json::Value {
    v["drives"].as_array().unwrap().iter().find(|d| d["letter"] == letter).unwrap()
}

#[tokio::test]
async fn cold_data_on_an_ssd_is_suggested_for_an_hdd_with_instructions() {
    let f = tier_fixture(700, false, TierRegistry::default(), Manifests::default());
    plant(
        &f,
        "C",
        &[
            (r"C:\Videos\2019\trip.mkv", false, 3 * TGIB, 1100),
            (r"C:\Videos\2024\new.mkv", false, 3 * TGIB, 10),
            (r"C:\Windows\Cold\big.bin", false, 5 * TGIB, 1200),
            (r"C:\Stuff\small.bin", false, 100 << 20, 1200),
        ],
    )
    .await;
    let (status, v) = json(&f.router, get("/api/v1/tiering")).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let s = find(&v, r"\2019").unwrap();
    assert_eq!(s["direction"], "cold-to-hdd");
    assert_eq!(s["kind"], "data");
    assert_eq!(s["allocated"], 3 * TGIB);
    assert!(s["reason"].as_str().unwrap().contains("not modified for 3.0 years"), "{}", s["reason"]);
    assert_eq!(s["target"]["letter"], "D");
    assert_eq!(s["destination"], r"D:\Videos\2019");
    assert!(s["instructions"].as_array().unwrap().iter().any(|l| l == r#"robocopy "C:\Videos\2019" "D:\Videos\2019" /E /COPY:DAT /DCOPY:T"#), "{}", s["instructions"]);
    assert!(find(&v, r"\2024").is_none(), "recent");
    assert!(find(&v, "Cold").is_none() && find(&v, "Windows").is_none(), "protected Windows tree");
    assert!(find(&v, "Stuff").is_none(), "under the 1 GiB minimum");
    assert_eq!(v["thresholds"]["cold_months"], 12);
    assert!(v["as_of"].is_string());
}

#[tokio::test]
async fn a_target_without_room_gives_no_target_with_space() {
    let f = tier_fixture(35, false, TierRegistry::default(), Manifests::default());
    plant(&f, "C", &[(r"C:\Videos\2019\trip.mkv", false, 3 * TGIB, 1100), (r"C:\Videos\new.mkv", false, 1 << 20, 1)]).await;
    let (_, v) = json(&f.router, get("/api/v1/tiering")).await;
    let s = find(&v, r"\2019").unwrap();
    assert!(s["target"].is_null());
    assert_eq!(s["no_target_reason"], "no_target_with_space");
    assert!(s["destination"].is_null());
    assert!(s["instructions"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().contains("<destination>")));
}

#[tokio::test]
async fn without_an_internal_hdd_there_are_no_cold_suggestions_and_the_page_says_why() {
    let f = tier_fixture(700, true, TierRegistry::default(), Manifests::default());
    plant(&f, "C", &[(r"C:\Videos\2019\trip.mkv", false, 3 * TGIB, 1100)]).await;
    let (_, v) = json(&f.router, get("/api/v1/tiering")).await;
    assert_eq!(v["suggestions"].as_array().unwrap().len(), 0);
    assert!(v["notes"].as_array().unwrap().iter().any(|n| n["code"] == "no_hdd_tier"), "{}", v["notes"]);
}

#[tokio::test]
async fn hot_folders_and_steam_games_on_an_hdd_go_to_an_ssd() {
    let mut m = Manifests::default();
    m.0.insert(r"d:\steam\steamapps\libraryfolders.vdf".into(), "\"libraryfolders\" { \"0\" { \"path\" \"D:\\\\Steam\" } }".into());
    m.0.insert(
        r"d:\steam\steamapps\appmanifest_1.acf".into(),
        format!("\"AppState\" {{ \"name\" \"Big Game\" \"installdir\" \"BigGame\" \"LastUpdated\" \"100\" \"LastPlayed\" \"{}\" }}", now_secs() - 2 * 86_400),
    );
    m.0.insert(r"d:\steam\steamapps\appmanifest_2.acf".into(), "\"AppState\" {".into());
    let reg = TierRegistry::default().key(TierHive::Hkcu, TierView::V64, r"Software\Valve\Steam", &[("SteamPath", TierValue::Str("d:/steam".into()))]);
    let f = tier_fixture(700, false, reg, m);
    plant(
        &f,
        "D",
        &[
            (r"D:\Steam\steamapps\common\BigGame\game.pak", false, 4 * TGIB, 400),
            (r"D:\Work\active\render.bin", false, 2 * TGIB, 1),
            (r"D:\Old\archive.bin", false, 3 * TGIB, 900),
        ],
    )
    .await;
    let (_, v) = json(&f.router, get("/api/v1/tiering")).await;
    let game = find(&v, "BigGame").unwrap();
    assert_eq!((game["direction"].as_str(), game["kind"].as_str()), (Some("hot-to-ssd"), Some("app-library")));
    assert_eq!(game["app"], "Big Game");
    assert!(game["reason"].as_str().unwrap().contains("Steam game last played 2 days ago"), "{}", game["reason"]);
    assert!(game["instructions"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().contains("Move install folder")));
    assert!(!game["instructions"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().contains("robocopy")), "launcher games are never copied by hand");
    assert!(matches!(game["target"]["letter"].as_str(), Some("C" | "G")), "an SSD");
    let work = find(&v, r"\Work\active").map(|w| w["path"].clone());
    assert!(work.is_none(), "depth: the hot folder is D:\\Work (outermost)");
    let hot = find(&v, r"\Work").unwrap();
    assert!(hot["reason"].as_str().unwrap().contains("modified in the last 30 days"));
    assert!(find(&v, "Old").is_none(), "cold data on an HDD stays");
    assert_eq!(v["manifest_errors"], 1, "the broken manifest is skipped and counted");
}

#[tokio::test]
async fn registered_program_folders_are_flagged_for_reinstall() {
    let reg = TierRegistry::default().key(
        TierHive::Hklm,
        TierView::V64,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\BigApp",
        &[("DisplayName", TierValue::Str("Big App".into())), ("InstallLocation", TierValue::Str(r"C:\Apps\BigApp".into()))],
    );
    let f = tier_fixture(700, false, reg, Manifests::default());
    plant(&f, "C", &[(r"C:\Apps\BigApp\data.bin", false, 3 * TGIB, 1000), (r"C:\Apps\Other\x.bin", false, 1 << 20, 1)]).await;
    let (_, v) = json(&f.router, get("/api/v1/tiering")).await;
    let s = find(&v, "BigApp").unwrap();
    assert_eq!(s["kind"], "app-install");
    assert_eq!(s["app"], "Big App");
    let text = s["instructions"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("Reinstall") && text.contains("junction") && !text.contains("robocopy"), "{text}");
}

#[tokio::test]
async fn planning_moves_updates_the_projected_free_space() {
    let f = tier_fixture(700, false, TierRegistry::default(), Manifests::default());
    plant(
        &f,
        "C",
        &[
            (r"C:\Videos\2019\a.mkv", false, 3 * TGIB, 1100),
            (r"C:\Videos\new.mkv", false, 1 << 20, 1),
            (r"C:\Backups\old\b.bin", false, 2 * TGIB, 1500),
            (r"C:\Backups\recent.bin", false, 1 << 20, 1),
        ],
    )
    .await;
    let (_, before) = json(&f.router, get("/api/v1/tiering")).await;
    let c0 = drive(&before, "C")["free"].as_u64().unwrap();
    let d0 = drive(&before, "D")["free"].as_u64().unwrap();
    assert_eq!(drive(&before, "C")["projected_free"], drive(&before, "C")["free"], "nothing planned yet");

    let plan = |path: &str, planned: bool| post_json("/api/v1/tiering/planned", serde_json::json!({ "path": path, "planned": planned }));
    let a = find(&before, r"\2019").unwrap()["path"].as_str().unwrap().to_string();
    let b = find(&before, r"\old").unwrap()["path"].as_str().unwrap().to_string();
    let (status, after) = json(&f.router, plan(&a, true)).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(drive(&after, "C")["projected_free"].as_u64().unwrap(), c0 + 3 * TGIB as u64);
    assert_eq!(drive(&after, "D")["projected_free"].as_u64().unwrap(), d0 - 3 * TGIB as u64);
    assert_eq!(find(&after, r"\2019").unwrap()["planned"], true);
    let (_, both) = json(&f.router, plan(&b, true)).await;
    assert_eq!(drive(&both, "C")["projected_free"].as_u64().unwrap(), c0 + 5 * TGIB as u64);

    // It persists across requests, and unplanning restores the numbers.
    let (_, again) = json(&f.router, get("/api/v1/tiering")).await;
    assert_eq!(drive(&again, "C")["projected_free"], drive(&both, "C")["projected_free"]);
    let (_, none) = json(&f.router, plan(&a, false)).await;
    assert_eq!(drive(&none, "C")["projected_free"].as_u64().unwrap(), c0 + 2 * TGIB as u64);
    assert_eq!(json(&f.router, plan(r"C:\not\a\suggestion", true)).await.0, StatusCode::NOT_FOUND);
    let no_token = req(Method::POST, "/api/v1/tiering/planned").header(header::CONTENT_TYPE, "application/json").body(json_body(serde_json::json!({ "path": a, "planned": true }))).unwrap();
    assert_eq!(json(&f.router, no_token).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_suggestion_can_join_a_plan_as_a_manual_move() {
    let f = tier_fixture(700, false, TierRegistry::default(), Manifests::default());
    let sid = plant(&f, "C", &[(r"C:\Videos\2019\a.mkv", false, 3 * TGIB, 1100), (r"C:\Videos\new.mkv", false, 1 << 20, 1)]).await;
    let (_, v) = json(&f.router, get("/api/v1/tiering")).await;
    let s = find(&v, r"\2019").unwrap().clone();
    let plan = new_plan(&f, "tiers").await;
    let body = serde_json::json!({ "source": "tiering", "snapshot_id": sid, "entry_id": s["entry_id"], "destination": s["destination"] });
    let (status, r) = json(&f.router, post_json(&format!("/api/v1/plans/{plan}/items"), body)).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["item"]["kind"], "manual-move");
    assert_eq!(r["item"]["destination"], r"D:\Videos\2019");
    assert!(r["item"]["instructions"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().starts_with("robocopy")));
}

#[tokio::test]
async fn excluded_folders_and_unscanned_drives() {
    let f = tier_fixture(700, false, TierRegistry::default(), Manifests::default());
    let (_, empty) = json(&f.router, get("/api/v1/tiering")).await;
    assert!(empty["notes"].as_array().unwrap().iter().any(|n| n["code"] == "unscanned"));

    plant(&f, "C", &[(r"C:\Videos\2019\a.mkv", false, 3 * TGIB, 1100)]).await;
    let (_, settings) = json(&f.router, get("/api/v1/settings")).await;
    let mut s2 = settings.clone();
    s2["excluded_paths"] = serde_json::json!([r"C:\Videos"]);
    assert_eq!(json(&f.router, put_settings(s2)).await.0, StatusCode::OK);
    let (_, v) = json(&f.router, get("/api/v1/tiering")).await;
    assert_eq!(v["suggestions"].as_array().unwrap().len(), 0);
    assert!(v["notes"].as_array().unwrap().iter().any(|n| n["code"] == "nothing_found"), "{}", v["notes"]);

    // The thresholds are settings.
    let mut s3 = settings;
    s3["tiering_min_gib"] = serde_json::json!(5);
    assert_eq!(json(&f.router, put_settings(s3)).await.0, StatusCode::OK);
    let (_, v) = json(&f.router, get("/api/v1/tiering")).await;
    assert_eq!(v["thresholds"]["min_gib"], 5);
    assert_eq!(v["suggestions"].as_array().unwrap().len(), 0, "3 GiB is below the new 5 GiB minimum");
}
