//! Game library detection from launcher manifests. Read-only: the manifests
//! are read through [`ManifestSource`] and never written.

use super::vdf::{self, Vdf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Launcher {
    Steam,
    Epic,
}

/// An installed game found in a launcher's manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryApp {
    pub launcher: Launcher,
    pub name: String,
    /// The game's install folder.
    pub path: String,
    /// Last played (Steam) or last updated, unix seconds, when the manifest says.
    pub last_active: Option<i64>,
    pub size_hint: Option<u64>,
}

/// Read access to manifest files (real disk, or fixtures in tests).
pub trait ManifestSource: Send + Sync {
    fn read_to_string(&self, path: &str) -> std::io::Result<String>;
    /// Full paths of files directly in `dir` whose names start with `prefix` and end with `ext`.
    fn list_files(&self, dir: &str, prefix: &str, ext: &str) -> Vec<String>;
}

fn join(a: &str, b: &str) -> String {
    format!("{}\\{}", a.trim_end_matches(['\\', '/']), b.trim_start_matches(['\\', '/']))
}

fn num(v: &Vdf, key: &str) -> Option<u64> {
    v.str(key).and_then(|s| s.trim().parse().ok())
}

/// Steam games in every library listed in `libraryfolders.vdf` (plus the Steam folder itself).
/// Problems (unreadable or malformed manifests) are added to `errors` and skipped.
pub fn steam_apps(src: &dyn ManifestSource, steam_root: &str, errors: &mut Vec<String>) -> Vec<LibraryApp> {
    let steam_root = steam_root.replace('/', "\\");
    let mut libs: Vec<String> = vec![steam_root.clone()];
    let vdf_path = join(&join(&steam_root, "steamapps"), "libraryfolders.vdf");
    match src.read_to_string(&vdf_path).map_err(|e| e.to_string()).and_then(|t| vdf::parse(&t)) {
        Ok(v) => {
            if let Some(root) = v.get("libraryfolders") {
                for (_, entry) in root.entries() {
                    // New format: an object with "path". Old format: the value is the path.
                    let path = match entry {
                        Vdf::Obj(_) => entry.str("path").map(str::to_string),
                        Vdf::Str(s) if s.contains(['\\', '/']) => Some(s.clone()),
                        Vdf::Str(_) => None,
                    };
                    if let Some(p) = path.map(|p| p.replace('/', "\\")) {
                        if !libs.iter().any(|l| l.eq_ignore_ascii_case(&p)) {
                            libs.push(p);
                        }
                    }
                }
            }
        }
        Err(e) => errors.push(format!("{vdf_path}: {e}")),
    }
    let mut apps = Vec::new();
    for lib in libs {
        let steamapps = join(&lib, "steamapps");
        for file in src.list_files(&steamapps, "appmanifest_", ".acf") {
            let parsed = src.read_to_string(&file).map_err(|e| e.to_string()).and_then(|t| vdf::parse(&t));
            let state = match parsed {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!("{file}: {e}"));
                    continue;
                }
            };
            let Some(app) = state.get("AppState") else {
                errors.push(format!("{file}: no AppState"));
                continue;
            };
            let (Some(name), Some(dir)) = (app.str("name"), app.str("installdir")) else {
                errors.push(format!("{file}: missing name or installdir"));
                continue;
            };
            let played = num(app, "LastPlayed").filter(|t| *t > 0);
            let updated = num(app, "LastUpdated").filter(|t| *t > 0);
            apps.push(LibraryApp {
                launcher: Launcher::Steam,
                name: name.to_string(),
                path: join(&join(&steamapps, "common"), dir),
                last_active: played.or(updated).map(|t| t as i64),
                size_hint: num(app, "SizeOnDisk"),
            });
        }
    }
    apps
}

/// Epic Games Launcher installs from `Manifests\*.item` (JSON).
pub fn epic_apps(src: &dyn ManifestSource, manifests_dir: &str, errors: &mut Vec<String>) -> Vec<LibraryApp> {
    let mut apps = Vec::new();
    for file in src.list_files(manifests_dir, "", ".item") {
        let parsed = src
            .read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string()));
        let v = match parsed {
            Ok(v) => v,
            Err(e) => {
                errors.push(format!("{file}: {e}"));
                continue;
            }
        };
        let Some(location) = v["InstallLocation"].as_str().filter(|s| !s.is_empty()) else {
            errors.push(format!("{file}: no InstallLocation"));
            continue;
        };
        let name = v["DisplayName"].as_str().or_else(|| v["AppName"].as_str()).unwrap_or("Epic game");
        apps.push(LibraryApp {
            launcher: Launcher::Epic,
            name: name.to_string(),
            path: location.replace('/', "\\"),
            last_active: None,
            size_hint: v["InstallSize"].as_u64(),
        });
    }
    apps
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake(HashMap<String, String>);

    impl ManifestSource for Fake {
        fn read_to_string(&self, path: &str) -> std::io::Result<String> {
            self.0
                .get(&path.to_lowercase())
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not found"))
        }

        fn list_files(&self, dir: &str, prefix: &str, ext: &str) -> Vec<String> {
            let d = format!("{}\\", dir.to_lowercase());
            let mut out: Vec<String> = self
                .0
                .keys()
                .filter(|k| {
                    k.strip_prefix(&d).is_some_and(|n| !n.contains('\\') && n.starts_with(prefix) && n.ends_with(ext))
                })
                .cloned()
                .collect();
            out.sort();
            out
        }
    }

    fn acf(name: &str, dir: &str, played: Option<i64>) -> String {
        let p = played.map(|t| format!("\"LastPlayed\"\t\"{t}\"\n")).unwrap_or_default();
        format!("\"AppState\"\n{{\n\"appid\" \"1\"\n\"name\" \"{name}\"\n\"installdir\" \"{dir}\"\n\"SizeOnDisk\" \"1000\"\n\"LastUpdated\" \"100\"\n{p}}}\n")
    }

    #[test]
    fn steam_games_from_every_library_with_a_malformed_manifest_skipped() {
        let mut f = Fake::default();
        f.0.insert(
            r"c:\steam\steamapps\libraryfolders.vdf".into(),
            "\"libraryfolders\" { \"0\" { \"path\" \"C:\\\\Steam\" } \"1\" { \"path\" \"D:\\\\SteamLibrary\" } }"
                .into(),
        );
        f.0.insert(r"c:\steam\steamapps\appmanifest_10.acf".into(), acf("Old Game", "OldGame", None));
        f.0.insert(
            r"d:\steamlibrary\steamapps\appmanifest_20.acf".into(),
            acf("New Game", "NewGame", Some(1_700_000_000)),
        );
        f.0.insert(r"d:\steamlibrary\steamapps\appmanifest_30.acf".into(), "\"AppState\" {".into());
        f.0.insert(r"d:\steamlibrary\steamapps\notes.txt".into(), "ignored".into());
        let mut errors = Vec::new();
        let apps = steam_apps(&f, "C:/Steam", &mut errors);
        assert_eq!(apps.len(), 2);
        let old = apps.iter().find(|a| a.name == "Old Game").unwrap();
        assert_eq!(old.path, r"C:\Steam\steamapps\common\OldGame");
        assert_eq!(old.last_active, Some(100), "falls back to LastUpdated");
        let new = apps.iter().find(|a| a.name == "New Game").unwrap();
        assert_eq!(
            (new.path.as_str(), new.last_active),
            (r"D:\SteamLibrary\steamapps\common\NewGame", Some(1_700_000_000))
        );
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("appmanifest_30.acf"));
    }

    #[test]
    fn old_style_library_list_and_missing_vdf_are_tolerated() {
        let mut f = Fake::default();
        f.0.insert(
            r"c:\steam\steamapps\libraryfolders.vdf".into(),
            "\"LibraryFolders\" { \"TimeNextStatsReport\" \"123\" \"1\" \"E:\\\\Games\\\\Steam\" }".into(),
        );
        f.0.insert(r"e:\games\steam\steamapps\appmanifest_5.acf".into(), acf("G", "G", Some(5)));
        let mut errors = Vec::new();
        assert_eq!(steam_apps(&f, r"C:\Steam", &mut errors).len(), 1);
        let none = steam_apps(&Fake::default(), r"C:\Steam", &mut errors);
        assert!(none.is_empty());
        assert!(errors.iter().any(|e| e.contains("libraryfolders.vdf")));
    }

    #[test]
    fn epic_items() {
        let mut f = Fake::default();
        f.0.insert(
            r"c:\pd\manifests\a.item".into(),
            r#"{"DisplayName":"Fortnite","InstallLocation":"D:/Epic/Fortnite","InstallSize":42}"#.into(),
        );
        f.0.insert(r"c:\pd\manifests\b.item".into(), "not json".into());
        f.0.insert(r"c:\pd\manifests\c.item".into(), r#"{"DisplayName":"No location"}"#.into());
        let mut errors = Vec::new();
        let apps = epic_apps(&f, r"C:\PD\Manifests", &mut errors);
        assert_eq!(
            apps,
            [LibraryApp {
                launcher: Launcher::Epic,
                name: "Fortnite".into(),
                path: r"D:\Epic\Fortnite".into(),
                last_active: None,
                size_hint: Some(42)
            }]
        );
        assert_eq!(errors.len(), 2);
    }
}
