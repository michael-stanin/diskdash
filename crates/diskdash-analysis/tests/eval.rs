#![allow(clippy::unwrap_used)]
//! Rule evaluation on hand-built snapshots (no disk access).

use diskdash_analysis::eval::probe::{NullProbe, Probe, ProbeError, RecycleBin};
use diskdash_analysis::eval::{evaluate, EvalError, EvalOptions, EvalSummary};
use diskdash_analysis::rules::{load, load_all, LoadedRule, MapEnv, RuleSet};
use rusqlite::Connection;

const NOW: i64 = 1_800_000_000;
const DAY: i64 = 86_400;
const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

// ---------------------------------------------------------------------------
// A tiny snapshot builder: DFS pre-order ids, recursive totals, subtree ranges.
// ---------------------------------------------------------------------------

struct Item {
    path: String,
    dir: bool,
    size: u64,
    mtime: i64,
    flags: i64,
}

struct Snap {
    root: String,
    items: Vec<Item>,
}

impl Snap {
    fn new(root: &str) -> Self {
        Snap { root: root.into(), items: Vec::new() }
    }

    fn file(self, path: &str, size: u64, age_days: i64) -> Self {
        self.file_flags(path, size, age_days, 0)
    }

    fn file_flags(mut self, path: &str, size: u64, age_days: i64, flags: i64) -> Self {
        self.items.push(Item { path: path.into(), dir: false, size, mtime: NOW - age_days * DAY, flags });
        self
    }

    fn build(mut self) -> Connection {
        // Add every missing parent directory.
        let mut all: std::collections::BTreeMap<Vec<String>, Item> = Default::default();
        let root_parts = parts(&self.root);
        for it in self.items.drain(..) {
            let p = parts(&it.path);
            for n in root_parts.len() + 1..p.len() {
                all.entry(p[..n].to_vec()).or_insert_with(|| Item {
                    path: String::new(),
                    dir: true,
                    size: 0,
                    mtime: NOW - 5 * DAY,
                    flags: 0,
                });
            }
            all.insert(p, it);
        }
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(diskdash_core::store::SCHEMA).unwrap();

        // Sorted by lower-cased components: a directory precedes its subtree.
        let mut keys: Vec<(Vec<String>, Vec<String>)> =
            all.keys().map(|k| (k.iter().map(|s| s.to_lowercase()).collect(), k.clone())).collect();
        keys.sort();
        let mut ids = std::collections::HashMap::new();
        ids.insert(root_parts.clone(), 0i64);
        struct Out {
            id: i64,
            parent: i64,
            name: String,
            dir: bool,
            size: u64,
            mtime: i64,
            flags: i64,
            path: String,
            depth: i64,
            files: i64,
            last: i64,
            max_mtime: i64,
            total: u64,
            alloc: u64,
        }
        let mut outs: Vec<Out> = Vec::new();
        for (n, (_, orig)) in keys.iter().enumerate() {
            let it = &all[orig];
            let id = n as i64 + 1;
            ids.insert(orig.clone(), id);
            let parent = ids[&orig[..orig.len() - 1].to_vec()];
            outs.push(Out {
                id,
                parent,
                name: orig.last().unwrap().clone(),
                dir: it.dir,
                size: it.size,
                mtime: it.mtime,
                flags: it.flags,
                path: orig.join("\\"),
                depth: (orig.len() - root_parts.len()) as i64,
                files: i64::from(!it.dir),
                last: id,
                max_mtime: if it.dir { 0 } else { it.mtime },
                total: it.size,
                alloc: if it.dir { 0 } else { it.size.div_ceil(4096) * 4096 },
            });
        }
        // Roll totals up (children come after parents, so go backwards).
        let by_id: std::collections::HashMap<i64, usize> = outs.iter().enumerate().map(|(i, o)| (o.id, i)).collect();
        let mut root_total = (0u64, 0u64, 0i64, 0i64, outs.len() as i64);
        for i in (0..outs.len()).rev() {
            let (parent, total, alloc, files, mt, last) = {
                let o = &outs[i];
                (o.parent, o.total, o.alloc, o.files, o.max_mtime, o.last)
            };
            if parent == 0 {
                root_total.0 += total;
                root_total.1 += alloc;
                root_total.2 += files;
                root_total.3 = root_total.3.max(mt);
            } else {
                let p = &mut outs[by_id[&parent]];
                p.total += total;
                p.alloc += alloc;
                p.files += files;
                p.max_mtime = p.max_mtime.max(mt);
                p.last = p.last.max(last);
            }
        }
        let root_name = self.root.trim_end_matches('\\').rsplit('\\').next().unwrap_or("").to_string();
        conn.execute(
            "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, attrs, path, file_count, dir_count, max_mtime, last_desc_id)
             VALUES (0, NULL, ?1, ?2, 1, 0, ?3, ?4, 0, ?5, ?6, 0, ?7, ?8)",
            rusqlite::params![root_name, root_name.to_lowercase(), root_total.0 as i64, root_total.1 as i64, self.root, root_total.2, root_total.3, root_total.4],
        )
        .unwrap();
        for o in &outs {
            let (size, alloc) = if o.dir { (o.total, o.alloc) } else { (o.size, o.size.div_ceil(4096) * 4096) };
            conn.execute(
                "INSERT INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, mtime, attrs, flags, path, file_count, dir_count, max_mtime, last_desc_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, ?12, 0, ?13, ?14)",
                rusqlite::params![
                    o.id,
                    o.parent,
                    o.name,
                    o.name.to_lowercase(),
                    i64::from(o.dir),
                    o.depth,
                    size as i64,
                    alloc as i64,
                    if o.dir { None } else { Some(o.mtime) },
                    o.flags,
                    if o.dir { Some(o.path.clone()) } else { None },
                    o.files,
                    o.max_mtime,
                    o.last,
                ],
            )
            .unwrap();
        }
        conn
    }
}

fn parts(p: &str) -> Vec<String> {
    p.split('\\').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn env() -> MapEnv {
    MapEnv::default()
        .with_var("SystemDrive", "C:")
        .with_var("WINDIR", r"C:\Windows")
        .with_var("TEMP", r"C:\Users\Ann\AppData\Local\Temp")
        .with_var("USERPROFILE", r"C:\Users\Ann")
        .with_var("LOCALAPPDATA", r"C:\Users\Ann\AppData\Local")
        .with_var("APPDATA", r"C:\Users\Ann\AppData\Roaming")
        .with_var("PROGRAMDATA", r"C:\ProgramData")
        .with_folder("Downloads", r"C:\Users\Ann\Downloads")
}

fn subset<'a>(set: &'a RuleSet, ids: &[&str]) -> Vec<&'a LoadedRule> {
    let rules: Vec<_> = set.active().filter(|r| ids.contains(&r.def.id.as_str())).collect();
    assert_eq!(rules.len(), ids.len(), "all requested built-in rules exist");
    rules
}

fn run_with(conn: &mut Connection, rules: &[&LoadedRule], probe: &dyn Probe) -> EvalSummary {
    let env = env();
    let opts = EvalOptions { env: &env, probe, now: NOW, use_atime: false };
    evaluate(conn, rules, &opts, &mut |_, _, _| true).unwrap()
}

fn run(conn: &mut Connection, ids: &[&str]) -> EvalSummary {
    let set = load_all(None);
    run_with(conn, &subset(&set, ids), &NullProbe)
}

#[derive(Debug, PartialEq)]
struct M {
    rule: String,
    path: String,
    allocated: Option<i64>,
    counted: bool,
}

fn matches(conn: &Connection) -> Vec<M> {
    let mut stmt =
        conn.prepare("SELECT rule_id, path, allocated, counted FROM rule_matches ORDER BY rule_id, path").unwrap();
    stmt.query_map([], |r| {
        Ok(M { rule: r.get(0)?, path: r.get(1)?, allocated: r.get(2)?, counted: r.get::<_, i64>(3)? != 0 })
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

fn paths(conn: &Connection, rule: &str) -> Vec<String> {
    matches(conn).into_iter().filter(|m| m.rule == rule).map(|m| m.path).collect()
}

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

#[test]
fn node_modules_needs_a_sibling_package_json() {
    let mut conn = Snap::new(r"C:\")
        .file(r"C:\Dev\a\package.json", 100, 1)
        .file(r"C:\Dev\a\node_modules\x\index.js", 10_000, 1)
        .file(r"C:\Dev\a\node_modules\x\node_modules\deep\i.js", 5_000, 1)
        .file(r"C:\Dev\b\node_modules\y.js", 10_000, 1)
        .build();
    run(&mut conn, &["dev-node-modules"]);
    assert_eq!(
        paths(&conn, "dev-node-modules"),
        [r"C:\Dev\a\node_modules"],
        "b has no package.json; nested one is not listed twice"
    );
}

#[test]
fn venv_needs_pyvenv_cfg() {
    let mut conn = Snap::new(r"C:\")
        .file(r"C:\p\.venv\pyvenv.cfg", 100, 1)
        .file(r"C:\p\.venv\Lib\x.py", 9_000, 1)
        .file(r"C:\q\venv\Lib\x.py", 9_000, 1)
        .build();
    run(&mut conn, &["dev-python-venv"]);
    assert_eq!(paths(&conn, "dev-python-venv"), [r"C:\p\.venv"]);
}

#[test]
fn rust_target_and_dotnet_output_need_their_project_files() {
    let mut conn = Snap::new(r"C:\")
        .file(r"C:\r\Cargo.toml", 10, 1)
        .file(r"C:\r\target\debug\app.exe", 50_000, 1)
        .file(r"C:\s\target\x.bin", 50_000, 1)
        .file(r"C:\n\App.csproj", 10, 1)
        .file(r"C:\n\bin\Debug\App.dll", 50_000, 1)
        .file(r"C:\n\obj\project.assets.json", 50_000, 1)
        .file(r"C:\m\bin\tool.exe", 50_000, 1)
        .build();
    run(&mut conn, &["dev-rust-target", "dev-dotnet-build-output"]);
    assert_eq!(paths(&conn, "dev-rust-target"), [r"C:\r\target"]);
    assert_eq!(paths(&conn, "dev-dotnet-build-output"), [r"C:\n\bin", r"C:\n\obj"]);
}

#[test]
fn only_old_logs_match() {
    let mut conn = Snap::new(r"C:\")
        .file(r"C:\Logs\a.log", 1_000, 5)
        .file(r"C:\Logs\b.log", 1_000, 90)
        .file(r"C:\Logs\c.ETL", 1_000, 40)
        .file(r"C:\Logs\notes.txt", 1_000, 400)
        .build();
    run(&mut conn, &["logs-old-logs"]);
    assert_eq!(paths(&conn, "logs-old-logs"), [r"C:\Logs\b.log", r"C:\Logs\c.ETL"]);
}

#[test]
fn old_temp_files_use_prefix_and_suffix_filters() {
    let mut conn = Snap::new(r"C:\")
        .file(r"C:\w\x.tmp", 1_000, 60)
        .file(r"C:\w\~$report.docx", 162, 60)
        .file(r"C:\w\fresh.tmp", 1_000, 2)
        .file(r"C:\w\keep.docx", 1_000, 60)
        .build();
    run(&mut conn, &["logs-old-temp-files"]);
    assert_eq!(paths(&conn, "logs-old-temp-files"), [r"C:\w\x.tmp", r"C:\w\~$report.docx"]);
}

#[test]
fn extracted_archive_in_downloads() {
    let mut conn = Snap::new(r"C:\")
        .file(r"C:\Users\Ann\Downloads\tool.zip", 100 * MIB, 3)
        .file(r"C:\Users\Ann\Downloads\tool\bin\tool.exe", 60 * MIB, 3)
        .file(r"C:\Users\Ann\Downloads\other.zip", 100 * MIB, 3)
        .file(r"C:\Users\Ann\Downloads\thin.zip", 100 * MIB, 3)
        .file(r"C:\Users\Ann\Downloads\thin\readme.txt", MIB, 3)
        .file(r"C:\Users\Ann\Downloads\sub\data.tar.gz", 80 * MIB, 3)
        .file(r"C:\Users\Ann\Downloads\sub\data\a.bin", 80 * MIB, 3)
        .build();
    run(&mut conn, &["downloads-extracted-archives"]);
    let found = paths(&conn, "downloads-extracted-archives");
    assert_eq!(found, [r"C:\Users\Ann\Downloads\sub\data.tar.gz", r"C:\Users\Ann\Downloads\tool.zip"]);
    let risk: String =
        conn.query_row("SELECT risk FROM rule_matches WHERE path LIKE '%tool.zip'", [], |r| r.get(0)).unwrap();
    assert_eq!(risk, "Review");
}

#[test]
fn old_installers_only() {
    let mut conn = Snap::new(r"C:\")
        .file(r"C:\Users\Ann\Downloads\setup-old.exe", 50 * MIB, 90)
        .file(r"C:\Users\Ann\Downloads\setup-new.exe", 50 * MIB, 2)
        .file(r"C:\Users\Ann\Documents\tool.exe", 50 * MIB, 90)
        .build();
    run(&mut conn, &["downloads-installers"]);
    assert_eq!(paths(&conn, "downloads-installers"), [r"C:\Users\Ann\Downloads\setup-old.exe"]);
}

#[test]
fn path_with_globs_is_scoped_to_the_path() {
    let mut conn = Snap::new(r"C:\")
        .file(r"C:\Users\Ann\AppData\Local\Google\Chrome\User Data\Default\Cache\Cache_Data\f_1", 5 * MIB, 1)
        .file(r"C:\Users\Ann\AppData\Local\Google\Chrome\User Data\Default\Code Cache\js\a", MIB, 1)
        .file(r"C:\Users\Ann\AppData\Local\Google\Chrome\User Data\Default\Bookmarks", 4_000, 1)
        .file(r"C:\Elsewhere\Default\Cache\x", MIB, 1)
        .build();
    run(&mut conn, &["browser-chrome-cache"]);
    assert_eq!(
        paths(&conn, "browser-chrome-cache"),
        [
            r"C:\Users\Ann\AppData\Local\Google\Chrome\User Data\Default\Cache",
            r"C:\Users\Ann\AppData\Local\Google\Chrome\User Data\Default\Code Cache"
        ]
    );
}

// ---------------------------------------------------------------------------
// Overlap and savings
// ---------------------------------------------------------------------------

#[test]
fn overlapping_matches_are_not_counted_twice() {
    let temp = r"C:\Users\Ann\AppData\Local\Temp";
    let mut conn = Snap::new(r"C:\")
        .file(&format!(r"{temp}\big.dat"), 900 * MIB, 1)
        .file(&format!(r"{temp}\old.tmp"), 100 * MIB, 90)
        .build();
    let s = run(&mut conn, &["windows-user-temp", "logs-old-temp-files"]);
    let all = matches(&conn);
    assert_eq!(all.len(), 2);
    let temp_m = all.iter().find(|m| m.rule == "windows-user-temp").unwrap();
    let tmp_m = all.iter().find(|m| m.rule == "logs-old-temp-files").unwrap();
    assert!(temp_m.counted && !tmp_m.counted, "the outer folder wins, the file is listed under it");
    let parent: Option<i64> =
        conn.query_row("SELECT parent_match FROM rule_matches WHERE counted = 0", [], |r| r.get(0)).unwrap();
    assert!(parent.is_some());

    let safe = s.savings.iter().find(|x| x.risk.as_str() == "Safe").unwrap();
    assert_eq!(safe.allocated as i64, temp_m.allocated.unwrap(), "1000 MiB, not 1100 MiB");
    assert_eq!(safe.matches, 1);
    assert_eq!((s.matches, s.counted), (2, 1));
}

#[test]
fn savings_are_split_by_risk_and_kind() {
    let mut conn = Snap::new(r"C:\")
        .file(r"C:\hiberfil.sys", 12 * GIB, 1)
        .file(r"C:\Users\Ann\AppData\Local\Temp\a", 10 * MIB, 1)
        .file(r"C:\Windows.old\Windows\x.dll", 2 * GIB, 400)
        .build();
    let s = run(&mut conn, &["system-hiberfil", "windows-user-temp", "upgrade-windows-old"]);
    let find = |risk: &str, kind: &str| {
        s.savings.iter().find(|x| x.risk.as_str() == risk && x.kind.as_str() == kind).map(|x| x.allocated)
    };
    assert_eq!(find("Safe", "manual-cleanup"), Some(10 * MIB));
    assert_eq!(find("Review", "manual-cleanup"), Some(2 * GIB), "Windows.old");
    assert_eq!(find("Review", "system-setting"), Some(12 * GIB), "hiberfil is a system setting, totalled separately");
}

// ---------------------------------------------------------------------------
// Virtual candidates, system files and the probe
// ---------------------------------------------------------------------------

struct FakeProbe {
    recycle: Result<RecycleBin, ProbeError>,
    shadow: Result<u64, ProbeError>,
    file: Result<u64, ProbeError>,
}

impl Probe for FakeProbe {
    fn recycle_bin(&self, _: &str) -> Result<RecycleBin, ProbeError> {
        self.recycle.clone()
    }

    fn shadow_storage(&self, _: &str) -> Result<u64, ProbeError> {
        self.shadow.clone()
    }

    fn file_size(&self, _: &str) -> Result<u64, ProbeError> {
        self.file.clone()
    }
}

#[test]
fn recycle_bin_and_shadow_storage_are_virtual_candidates() {
    let mut conn = Snap::new(r"C:\").file(r"C:\x", 10, 1).build();
    let probe = FakeProbe {
        recycle: Ok(RecycleBin { bytes: 3 * GIB, items: 42 }),
        shadow: Err(ProbeError::Unavailable("size unavailable without admin".into())),
        file: Err(ProbeError::Missing),
    };
    let set = load_all(None);
    let s = run_with(&mut conn, &subset(&set, &["recycle-bin", "system-shadow-copies"]), &probe);
    let rb: (Option<i64>, String, Option<String>) = conn
        .query_row("SELECT allocated, path, virtual_key FROM rule_matches WHERE rule_id = 'recycle-bin'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map(|(a, p, k): (Option<i64>, String, Option<String>)| (a, p, k))
        .unwrap();
    assert_eq!(rb.0, Some(3 * GIB as i64));
    assert_eq!(rb.1, "Recycle Bin (C:)");
    assert_eq!(rb.2.as_deref(), Some("recycle-bin"));
    let (alloc, note): (Option<i64>, String) = conn
        .query_row("SELECT allocated, note FROM rule_matches WHERE rule_id = 'system-shadow-copies'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(alloc, None);
    assert!(note.contains("without admin"));
    let shadow = s.savings.iter().find(|x| x.kind.as_str() == "system-setting").unwrap();
    assert_eq!((shadow.allocated, shadow.unknown), (0, 1), "unknown sizes are counted, not summed");
}

#[test]
fn virtual_candidates_need_a_drive_root_snapshot() {
    let mut conn = Snap::new(r"C:\Users\Ann").file(r"C:\Users\Ann\x", 10, 1).build();
    let probe = FakeProbe {
        recycle: Ok(RecycleBin { bytes: GIB, items: 1 }),
        shadow: Err(ProbeError::Unavailable("n/a".into())),
        file: Err(ProbeError::Missing),
    };
    let set = load_all(None);
    run_with(&mut conn, &subset(&set, &["recycle-bin", "system-shadow-copies"]), &probe);
    assert!(matches(&conn).is_empty());
}

#[test]
fn shadow_storage_only_on_the_system_drive() {
    let mut conn = Snap::new(r"D:\").file(r"D:\x", 10, 1).build();
    let probe = FakeProbe { recycle: Err(ProbeError::Missing), shadow: Ok(GIB), file: Err(ProbeError::Missing) };
    let set = load_all(None);
    run_with(&mut conn, &subset(&set, &["system-shadow-copies"]), &probe);
    assert!(matches(&conn).is_empty(), "SystemDrive is C:");
}

#[test]
fn hiberfil_comes_from_the_snapshot() {
    let mut conn = Snap::new(r"C:\").file(r"C:\hiberfil.sys", 12 * GIB, 1).build();
    run(&mut conn, &["system-hiberfil"]);
    let m = matches(&conn);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].allocated, Some(12 * GIB as i64));
    let (kind, instructions): (String, String) = conn
        .query_row("SELECT kind, instructions FROM rule_defs WHERE rule_id = 'system-hiberfil'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(kind, "system-setting");
    assert!(instructions.contains("powercfg /h off"));
}

#[test]
fn pagefile_with_denied_metadata_shows_unknown_size() {
    let mut conn = Snap::new(r"C:\").file(r"C:\other.txt", 10, 1).build();
    let probe = FakeProbe {
        recycle: Err(ProbeError::Missing),
        shadow: Err(ProbeError::Missing),
        file: Err(ProbeError::Denied("access denied".into())),
    };
    let set = load_all(None);
    run_with(&mut conn, &subset(&set, &["system-pagefile"]), &probe);
    let (alloc, note): (Option<i64>, Option<String>) = conn
        .query_row("SELECT allocated, note FROM rule_matches WHERE rule_id = 'system-pagefile'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(alloc, None);
    assert!(note.unwrap().contains("access denied"));
}

#[test]
fn a_missing_system_file_is_not_a_candidate() {
    let mut conn = Snap::new(r"C:\").file(r"C:\other.txt", 10, 1).build();
    run(&mut conn, &["system-pagefile", "system-hiberfil"]);
    assert!(matches(&conn).is_empty());
}

// ---------------------------------------------------------------------------
// Safety
// ---------------------------------------------------------------------------

#[test]
fn protected_locations_are_never_manual_cleanup_candidates() {
    let rules = r#"
rules:
  - id: bad-system32
    category: windows
    title: x
    explanation: x
    risk: Safe
    kind: manual-cleanup
    paths: ["%WINDIR%\\System32\\drivers"]
    instructions: x
  - id: bad-profile
    category: windows
    title: x
    explanation: x
    risk: Safe
    kind: manual-cleanup
    paths: ["%USERPROFILE%"]
    instructions: x
  - id: bad-dlls
    category: windows
    title: x
    explanation: x
    risk: Safe
    kind: manual-cleanup
    globs: ["*.dll"]
    instructions: x
"#;
    let set = load(&[("t.yaml", rules)], None);
    assert!(set.issues.is_empty(), "{:?}", set.issues);
    let mut conn = Snap::new(r"C:\")
        .file(r"C:\Windows\System32\drivers\a.sys", 1000, 1)
        .file(r"C:\Windows\System32\kernel.dll", 1000, 1)
        .file(r"C:\Users\Ann\doc.txt", 1000, 1)
        .file(r"C:\Apps\mine.dll", 1000, 1)
        .build();
    let all: Vec<_> = set.active().collect();
    run_with(&mut conn, &all, &NullProbe);
    assert_eq!(paths(&conn, "bad-dlls"), [r"C:\Apps\mine.dll"], "System32 is off limits");
    assert!(paths(&conn, "bad-system32").is_empty());
    assert!(paths(&conn, "bad-profile").is_empty());
}

#[test]
fn cloud_placeholders_are_skipped() {
    let mut conn =
        Snap::new(r"C:\").file_flags(r"C:\Logs\cloud.log", 5_000, 90, 2).file(r"C:\Logs\local.log", 5_000, 90).build();
    run(&mut conn, &["logs-old-logs"]);
    assert_eq!(paths(&conn, "logs-old-logs"), [r"C:\Logs\local.log"]);
}

#[test]
fn cancelling_writes_nothing() {
    let mut conn = Snap::new(r"C:\").file(r"C:\Logs\old.log", 5_000, 90).build();
    let set = load_all(None);
    let rules = subset(&set, &["logs-old-logs", "logs-old-temp-files"]);
    let env = env();
    let opts = EvalOptions { env: &env, probe: &NullProbe, now: NOW, use_atime: false };
    let mut calls = 0;
    let r = evaluate(&mut conn, &rules, &opts, &mut |_, _, _| {
        calls += 1;
        calls < 2
    });
    assert!(matches!(r, Err(EvalError::Cancelled)));
    assert!(matches(&conn).is_empty());
}

#[test]
fn rerunning_replaces_earlier_results() {
    let mut conn = Snap::new(r"C:\").file(r"C:\Logs\old.log", 5_000, 90).build();
    run(&mut conn, &["logs-old-logs"]);
    run(&mut conn, &["logs-old-logs"]);
    assert_eq!(matches(&conn).len(), 1);
    let defs: i64 = conn.query_row("SELECT count(*) FROM rule_defs", [], |r| r.get(0)).unwrap();
    assert_eq!(defs, 1);
}

#[test]
fn one_failing_rule_does_not_stop_the_others() {
    // A glob that passes validation but a path that cannot expand is skipped, not fatal.
    let rules = r#"
rules:
  - id: needs-unset-var
    category: windows
    title: x
    explanation: x
    risk: Safe
    kind: manual-cleanup
    paths: ["%NOT_SET%\\x"]
    instructions: x
  - id: old-logs
    category: logs
    title: Logs
    explanation: x
    risk: Safe
    kind: manual-cleanup
    globs: ["*.log"]
    min_age_days: 30
    instructions: x
"#;
    let set = load(&[("t.yaml", rules)], None);
    let mut conn = Snap::new(r"C:\").file(r"C:\Logs\old.log", 5_000, 90).build();
    let all: Vec<_> = set.active().collect();
    let s = run_with(&mut conn, &all, &NullProbe);
    assert_eq!(paths(&conn, "old-logs").len(), 1);
    assert!(s.rule_failures.is_empty());
}

// ---------------------------------------------------------------------------
// Built-in rule set
// ---------------------------------------------------------------------------

#[test]
fn all_builtin_rules_are_valid_and_have_instructions() {
    let set = load_all(None);
    assert!(set.issues.is_empty(), "{:#?}", set.issues);
    assert!(set.rules.len() >= 40, "{} rules", set.rules.len());
    let mut ids = std::collections::HashSet::new();
    for r in &set.rules {
        assert!(ids.insert(r.def.id.clone()), "duplicate id {}", r.def.id);
        assert!(!r.def.instructions.is_empty(), "{} has no instructions", r.def.id);
        assert!(!r.def.explanation.is_empty());
        assert!(!r.disabled);
    }
}

#[test]
fn builtin_rules_cover_the_required_categories() {
    let set = load_all(None);
    let ids: std::collections::HashSet<&str> = set.rules.iter().map(|r| r.def.id.as_str()).collect();
    for needed in [
        "windows-user-temp",
        "windows-system-temp",
        "windows-update-download",
        "windows-delivery-optimization",
        "windows-error-reports",
        "windows-crash-dumps",
        "windows-thumbnail-cache",
        "windows-prefetch",
        "upgrade-windows-old",
        "upgrade-windows-bt",
        "upgrade-windows-ws",
        "recycle-bin",
        "browser-chrome-cache",
        "browser-edge-cache",
        "browser-firefox-cache",
        "browser-brave-cache",
        "dev-node-modules",
        "dev-python-venv",
        "dev-python-pycache",
        "dev-rust-target",
        "dev-dotnet-build-output",
        "dev-gradle-cache",
        "dev-maven-repository",
        "dev-js-package-caches",
        "dev-pip-cache",
        "dev-nuget-cache",
        "dev-cargo-cache",
        "dev-android-system-images",
        "dev-visual-studio-cache",
        "dev-jetbrains-cache",
        "apps-steam-shader-cache",
        "apps-steam-downloading",
        "apps-teams-cache",
        "apps-slack-cache",
        "apps-discord-cache",
        "apps-spotify-cache",
        "apps-adobe-media-cache",
        "downloads-extracted-archives",
        "downloads-installers",
        "downloads-disk-images",
        "logs-old-logs",
        "logs-old-temp-files",
        "system-hiberfil",
        "system-pagefile",
        "system-swapfile",
        "system-shadow-copies",
        "system-winsxs",
        "system-vhdx",
    ] {
        assert!(ids.contains(needed), "missing built-in rule {needed}");
    }
    let prefetch = set.rules.iter().find(|r| r.def.id == "windows-prefetch").unwrap();
    assert_eq!(prefetch.def.risk.as_str(), "Expert");
    for sys in [
        "system-hiberfil",
        "system-pagefile",
        "system-swapfile",
        "system-shadow-copies",
        "system-winsxs",
        "system-vhdx",
    ] {
        let r = set.rules.iter().find(|r| r.def.id == sys).unwrap();
        assert_eq!(r.def.kind.as_str(), "system-setting", "{sys}");
    }
}

#[test]
fn full_builtin_set_over_a_realistic_tree() {
    let mut b = Snap::new(r"C:\");
    let local = r"C:\Users\Ann\AppData\Local";
    for (p, size, age) in [
        (format!(r"{local}\Temp\a.dat"), 20 * MIB, 1),
        (format!(r"{local}\Google\Chrome\User Data\Default\Cache\Cache_Data\f"), 50 * MIB, 1),
        (r"C:\Dev\app\package.json".to_string(), 1_000, 1),
        (r"C:\Dev\app\node_modules\lib\index.js".to_string(), 30 * MIB, 1),
        (r"C:\Users\Ann\Downloads\tool.zip".to_string(), 100 * MIB, 3),
        (r"C:\Users\Ann\Downloads\tool\tool.exe".to_string(), 90 * MIB, 3),
        (r"C:\ProgramData\app\old.log".to_string(), 3 * MIB, 200),
        (r"C:\hiberfil.sys".to_string(), 8 * GIB, 1),
        (r"C:\Windows\WinSxS\x\a.dll".to_string(), 4 * GIB, 100),
        (r"C:\Windows\System32\kernel32.dll".to_string(), MIB, 100),
    ] {
        b = b.file(&p, size, age);
    }
    let mut conn = b.build();
    let set = load_all(None);
    let all: Vec<_> = set.active().collect();
    let s = run_with(&mut conn, &all, &NullProbe);
    assert!(s.rule_failures.is_empty(), "{:?}", s.rule_failures);
    let m = matches(&conn);
    let found: std::collections::BTreeSet<&str> = m.iter().map(|m| m.rule.as_str()).collect();
    for expected in [
        "windows-user-temp",
        "browser-chrome-cache",
        "dev-node-modules",
        "downloads-extracted-archives",
        "logs-old-logs",
        "system-hiberfil",
        "system-winsxs",
    ] {
        assert!(found.contains(expected), "{expected} should match, got {found:?}");
    }
    assert!(!found.contains("upgrade-windows-old"));
    assert!(m.iter().all(|m| !m.path.to_lowercase().contains("system32")));
}
