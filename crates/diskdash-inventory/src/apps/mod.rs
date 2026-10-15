//! Installed applications from the Uninstall registry keys. Read-only: the
//! registry is only opened for reading, the uninstall commands stored there
//! are never used and nothing is ever uninstalled by diskdash.

pub mod registry;
pub mod userassist;

use std::collections::HashMap;

use registry::{Hive, RegError, RegValue, Registry, View};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use userassist::Launch;

pub const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_KEY_HKCU: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AppSource {
    /// HKLM, 64-bit view.
    Hklm64,
    /// HKLM WOW6432Node, 32-bit view.
    Hklm32,
    /// HKCU, installed for the current user only.
    Hkcu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AppEntry {
    pub name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    /// `YYYY-MM-DD` when the registry holds a valid `YYYYMMDD`.
    pub install_date: Option<String>,
    pub install_location: Option<String>,
    pub display_icon: Option<String>,
    /// `EstimatedSize` converted to bytes.
    #[ts(type = "number | null")]
    pub estimated_size: Option<u64>,
    pub sources: Vec<AppSource>,
}

#[derive(Debug, Clone, Default)]
pub struct InstalledApps {
    pub apps: Vec<AppEntry>,
    /// Uninstall entries that could not be read and were skipped.
    pub skipped: u32,
}

fn text(v: &HashMap<String, RegValue>, key: &str) -> Option<String> {
    v.get(key).and_then(RegValue::as_str).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn iso_date(raw: &str) -> Option<String> {
    let d = raw.trim();
    if d.len() != 8 || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (y, m, day): (u32, u32, u32) = (d[..4].parse().ok()?, d[4..6].parse().ok()?, d[6..].parse().ok()?);
    ((1980..=2200).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&day))
        .then(|| format!("{y:04}-{m:02}-{day:02}"))
}

fn entry(values: &HashMap<String, RegValue>, source: AppSource) -> Option<AppEntry> {
    let name = text(values, "DisplayName")?;
    if values.get("SystemComponent").and_then(RegValue::as_dword) == Some(1) {
        return None;
    }
    // Patches and updates hang off their parent product.
    if values.contains_key("ParentKeyName") || values.contains_key("ParentDisplayName") {
        return None;
    }
    Some(AppEntry {
        name,
        publisher: text(values, "Publisher"),
        version: text(values, "DisplayVersion"),
        install_date: text(values, "InstallDate").and_then(|d| iso_date(&d)),
        install_location: text(values, "InstallLocation").map(|p| p.trim_matches('"').to_string()),
        display_icon: text(values, "DisplayIcon"),
        estimated_size: values
            .get("EstimatedSize")
            .and_then(RegValue::as_dword)
            .filter(|k| *k > 0)
            .map(|k| u64::from(k) * 1024),
        sources: vec![source],
    })
}

fn merge_into(m: &mut AppEntry, e: AppEntry, source: AppSource) {
    if !m.sources.contains(&source) {
        m.sources.push(source);
    }
    m.publisher = m.publisher.take().or(e.publisher);
    m.install_date = m.install_date.take().or(e.install_date);
    m.install_location = m.install_location.take().or(e.install_location);
    m.display_icon = m.display_icon.take().or(e.display_icon);
    m.estimated_size = m.estimated_size.or(e.estimated_size);
}

/// Read the three Uninstall roots. Unreadable entries are skipped and counted;
/// when no root can be read at all the error is returned.
pub fn read_installed(reg: &dyn Registry) -> Result<InstalledApps, String> {
    let roots = [
        (Hive::Hklm, View::V64, UNINSTALL_KEY, AppSource::Hklm64),
        (Hive::Hklm, View::V32, UNINSTALL_KEY, AppSource::Hklm32),
        (Hive::Hkcu, View::V64, UNINSTALL_KEY_HKCU, AppSource::Hkcu),
    ];
    let mut out = InstalledApps::default();
    let mut readable = 0;
    let mut first_error: Option<RegError> = None;
    let mut merged: Vec<AppEntry> = Vec::new();
    for (hive, view, path, source) in roots {
        let subkeys = match reg.subkeys(hive, view, path) {
            Ok(k) => k,
            Err(RegError::NotFound) => {
                readable += 1; // for example no per-user installs
                continue;
            }
            Err(e) => {
                tracing::warn!(?source, error = %e, "cannot read an Uninstall root");
                first_error.get_or_insert(e);
                continue;
            }
        };
        readable += 1;
        for sub in subkeys {
            match reg.values(hive, view, &format!(r"{path}\{sub}")) {
                Ok(values) => {
                    let Some(e) = entry(&values, source) else { continue };
                    let same = |m: &&mut AppEntry| {
                        m.name.eq_ignore_ascii_case(&e.name)
                            && m.version
                                .as_deref()
                                .unwrap_or("")
                                .eq_ignore_ascii_case(e.version.as_deref().unwrap_or(""))
                    };
                    match merged.iter_mut().find(same) {
                        Some(m) => merge_into(m, e, source),
                        None => merged.push(e),
                    }
                }
                Err(e) => {
                    tracing::warn!(key = %sub, error = %e, "skipping an unreadable Uninstall entry");
                    out.skipped += 1;
                }
            }
        }
    }
    if readable == 0 {
        return Err(first_error.map_or_else(|| "the registry could not be read".into(), |e| e.to_string()));
    }
    merged.sort_by_key(|a| a.name.to_lowercase());
    out.apps = merged;
    Ok(out)
}

/// Lower-case file name of the program in a `DisplayIcon` value such as `"C:\x\app.exe",0`.
pub fn exe_name(display_icon: &str) -> Option<String> {
    let first = display_icon.split(',').next()?.trim().trim_matches('"');
    let name = first.rsplit('\\').next()?.to_lowercase();
    name.ends_with(".exe").then_some(name)
}

fn under(path: &str, dir: &str) -> bool {
    let (p, d) = (path.to_lowercase(), dir.trim_end_matches('\\').to_lowercase());
    !d.is_empty() && (p == d || p.starts_with(&format!("{d}\\")))
}

/// Latest recorded launch of an application: a launch from its install folder,
/// or one whose program name matches the one in its `DisplayIcon`.
pub fn last_launch(app: &AppEntry, launches: &[Launch]) -> Option<i64> {
    let icon_exe = app.display_icon.as_deref().and_then(exe_name);
    launches
        .iter()
        .filter(|l| {
            app.install_location.as_deref().is_some_and(|dir| under(&l.path, dir))
                || icon_exe.as_deref().is_some_and(|exe| l.path.to_lowercase().ends_with(&format!("\\{exe}")))
        })
        .map(|l| l.last_run)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry::FakeRegistry;

    fn s(v: &str) -> RegValue {
        RegValue::Str(v.into())
    }

    fn fake() -> FakeRegistry {
        let u = UNINSTALL_KEY;
        FakeRegistry::default()
            .key(
                Hive::Hklm,
                View::V64,
                &format!(r"{u}\Foo"),
                &[
                    ("DisplayName", s("Foo")),
                    ("DisplayVersion", s("1.0")),
                    ("Publisher", s("Acme")),
                    ("InstallDate", s("20240131")),
                    ("EstimatedSize", RegValue::Dword(2048)),
                    ("InstallLocation", s(r"D:\Games\Foo")),
                ],
            )
            .key(
                Hive::Hklm,
                View::V32,
                &format!(r"{u}\Foo"),
                &[
                    ("DisplayName", s("FOO")),
                    ("DisplayVersion", s("1.0")),
                    ("DisplayIcon", s(r#""D:\Games\Foo\foo.exe",0"#)),
                ],
            )
            .key(
                Hive::Hklm,
                View::V64,
                &format!(r"{u}\Hidden"),
                &[("DisplayName", s("Hidden")), ("SystemComponent", RegValue::Dword(1))],
            )
            .key(Hive::Hklm, View::V64, &format!(r"{u}\NoName"), &[("Publisher", s("X"))])
            .key(
                Hive::Hklm,
                View::V64,
                &format!(r"{u}\KB1"),
                &[("DisplayName", s("Update KB1")), ("ParentKeyName", s("OS"))],
            )
            .denied(Hive::Hklm, View::V64, &format!(r"{u}\Locked"))
            .key(
                Hive::Hkcu,
                View::V64,
                &format!(r"{UNINSTALL_KEY_HKCU}\Mine"),
                &[("DisplayName", s("Mine")), ("InstallDate", s("not a date"))],
            )
    }

    #[test]
    fn lists_visible_apps_once_with_all_sources_and_skips_unreadable() {
        let r = read_installed(&fake()).unwrap();
        let names: Vec<&str> = r.apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Foo", "Mine"]);
        let foo = &r.apps[0];
        assert_eq!(foo.sources, [AppSource::Hklm64, AppSource::Hklm32], "same name and version in both views");
        assert_eq!(foo.estimated_size, Some(2048 * 1024));
        assert_eq!(foo.install_date.as_deref(), Some("2024-01-31"));
        assert_eq!(foo.display_icon.as_deref(), Some(r#""D:\Games\Foo\foo.exe",0"#), "merged from the 32-bit entry");
        assert_eq!(r.apps[1].install_date, None);
        assert_eq!(r.skipped, 1, "the locked entry");
    }

    #[test]
    fn fails_only_when_no_root_can_be_read() {
        let reg = FakeRegistry::default()
            .denied(Hive::Hklm, View::V64, UNINSTALL_KEY)
            .denied(Hive::Hklm, View::V32, UNINSTALL_KEY)
            .denied(Hive::Hkcu, View::V64, UNINSTALL_KEY_HKCU);
        let e = read_installed(&reg).unwrap_err();
        assert!(e.contains("access denied"), "{e}");
        // One readable root is enough.
        let ok = FakeRegistry::default().denied(Hive::Hklm, View::V64, UNINSTALL_KEY);
        assert!(read_installed(&ok).is_ok());
    }

    #[test]
    fn matches_launches_by_install_folder_or_icon_program() {
        let app = read_installed(&fake()).unwrap().apps.remove(0);
        let l = |p: &str, t| Launch { path: p.into(), run_count: 1, last_run: t };
        let launches =
            [l(r"d:\games\foo\bin\launcher.exe", 100), l(r"C:\Other\foo.exe", 300), l(r"C:\Else\bar.exe", 999)];
        assert_eq!(last_launch(&app, &launches), Some(300), "install-folder match (100) and program-name match (300)");
        assert_eq!(last_launch(&app, &launches[2..]), None);
        assert_eq!(exe_name(r#""D:\Games\Foo\foo.exe",0"#).as_deref(), Some("foo.exe"));
        assert_eq!(exe_name(r"C:\x\icon.ico"), None);
    }
}
