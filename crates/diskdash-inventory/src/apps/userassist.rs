//! UserAssist: Explorer records programs the current user launched, with the
//! value names ROT13-encoded and a 72-byte data block (Windows 7 and later).
//! Best effort: it misses programs started in other ways.

use super::registry::{Hive, RegValue, Registry, View};

pub const USERASSIST_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist";

pub fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a') + 13) % 26 + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A') + 13) % 26 + b'A') as char,
            other => other,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// Decoded program path (may start with a known-folder GUID).
    pub path: String,
    pub run_count: u32,
    /// Last run, unix seconds.
    pub last_run: i64,
}

const FILETIME_UNIX_SECS: i64 = 11_644_473_600;

/// FILETIME (100 ns ticks since 1601) to unix seconds; zero is "never".
pub fn filetime_to_unix(ft: u64) -> Option<i64> {
    (ft != 0).then(|| (ft / 10_000_000) as i64 - FILETIME_UNIX_SECS).filter(|t| *t > 0)
}

/// Decode one value. `None` for counters without a usable record.
pub fn parse_entry(encoded_name: &str, data: &[u8]) -> Option<Launch> {
    let path = rot13(encoded_name);
    if path.starts_with("UEME_") || data.len() < 68 {
        return None;
    }
    let run_count = u32::from_le_bytes(data[4..8].try_into().ok()?);
    let ft = u64::from_le_bytes(data[60..68].try_into().ok()?);
    Some(Launch { path, run_count, last_run: filetime_to_unix(ft)? })
}

/// Known-folder GUIDs Explorer stores instead of a drive path, and the variable that holds the folder.
const KNOWN_FOLDERS: &[(&str, &str)] = &[
    ("{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}", "ProgramFiles(x86)"),
    ("{905E63B6-C1BF-494E-B29C-65B732D3D21A}", "ProgramFiles"),
    ("{6D809377-6AF0-444B-8957-A3773F02200E}", "ProgramW6432"),
    ("{F38BF404-1D43-42F2-9305-67DE0B28FC23}", "SystemRoot"),
    ("{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}", "SystemRoot"),
    ("{F1B32785-6FBA-4FCF-9D55-7B8E7F157091}", "LOCALAPPDATA"),
    ("{3EB685DB-65F9-4CF6-A03A-E3EF65729F3D}", "APPDATA"),
    ("{62AB5D82-FDC1-4DC3-A9DD-070D1D495D97}", "ProgramData"),
];

/// Replace a leading known-folder GUID with the real folder, using `lookup` for the variables.
pub fn expand_known_folder(path: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    for (guid, var) in KNOWN_FOLDERS {
        if path.len() >= guid.len()
            && path.is_char_boundary(guid.len())
            && path[..guid.len()].eq_ignore_ascii_case(guid)
        {
            if let Some(dir) = lookup(var) {
                let rest = &path[guid.len()..];
                let dir = dir.trim_end_matches('\\');
                // {System} is System32 below the Windows folder.
                let sys32 = if *guid == "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}" { "\\System32" } else { "" };
                return format!("{dir}{sys32}{rest}");
            }
        }
    }
    path.to_string()
}

/// All launches recorded for the current user.
pub fn read(reg: &dyn Registry) -> Vec<Launch> {
    let Ok(guids) = reg.subkeys(Hive::Hkcu, View::V64, USERASSIST_KEY) else { return Vec::new() };
    let mut out = Vec::new();
    for g in guids {
        let Ok(values) = reg.values(Hive::Hkcu, View::V64, &format!(r"{USERASSIST_KEY}\{g}\Count")) else { continue };
        for (name, v) in values {
            if let RegValue::Binary(data) = v {
                if let Some(mut l) = parse_entry(&name, &data) {
                    l.path = expand_known_folder(&l.path, &|v| std::env::var(v).ok());
                    out.push(l);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::registry::FakeRegistry;

    fn block(count: u32, unix: i64) -> Vec<u8> {
        let mut d = vec![0u8; 72];
        d[4..8].copy_from_slice(&count.to_le_bytes());
        let ft = ((unix + FILETIME_UNIX_SECS) as u64) * 10_000_000;
        d[60..68].copy_from_slice(&ft.to_le_bytes());
        d
    }

    #[test]
    fn rot13_round_trips_and_keeps_other_characters() {
        assert_eq!(rot13("P:\\Cebtenz Svyrf\\Ncc\\ncc.rkr"), r"C:\Program Files\App\app.exe");
        assert_eq!(rot13(&rot13("Hello, World 1")), "Hello, World 1");
    }

    #[test]
    fn parses_run_count_and_filetime() {
        let l = parse_entry(r"P:\Nccf\Gbby.rkr", &block(7, 1_700_000_000)).unwrap();
        assert_eq!(l, Launch { path: r"C:\Apps\Tool.exe".into(), run_count: 7, last_run: 1_700_000_000 });
    }

    #[test]
    fn skips_counters_short_blocks_and_never_run() {
        assert!(parse_entry("HRZR_THVFRFFVBA", &block(1, 1_700_000_000)).is_none());
        assert!(parse_entry(r"P:\k.rkr", &[0u8; 16]).is_none());
        assert!(parse_entry(r"P:\k.rkr", &[0u8; 72]).is_none(), "FILETIME 0 means never");
    }

    #[test]
    fn expands_known_folder_guids() {
        let env = |v: &str| match v {
            "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".to_string()),
            "SystemRoot" => Some(r"C:\Windows".to_string()),
            _ => None,
        };
        assert_eq!(
            expand_known_folder(r"{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}\Steam\steam.exe", &env),
            r"C:\Program Files (x86)\Steam\steam.exe"
        );
        assert_eq!(
            expand_known_folder(r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\notepad.exe", &env),
            r"C:\Windows\System32\notepad.exe"
        );
        assert_eq!(expand_known_folder(r"D:\x.exe", &env), r"D:\x.exe");
        let unset = r"{905E63B6-C1BF-494E-B29C-65B732D3D21A}\a.exe";
        assert_eq!(expand_known_folder(unset, &env), unset, "an unset variable leaves the path alone");
    }

    #[test]
    fn reads_every_guid_key_of_the_current_user() {
        let reg = FakeRegistry::default()
            .key(
                Hive::Hkcu,
                View::V64,
                &format!(r"{USERASSIST_KEY}\{{A}}\Count"),
                &[(r"P:\Nccf\Bar.rkr", RegValue::Binary(block(2, 1_650_000_000)))],
            )
            .key(
                Hive::Hkcu,
                View::V64,
                &format!(r"{USERASSIST_KEY}\{{B}}\Count"),
                &[("HRZR_PGYPAG", RegValue::Binary(block(9, 1_650_000_000)))],
            );
        let launches = read(&reg);
        assert_eq!(launches.len(), 1);
        assert_eq!(launches[0].path, r"C:\Apps\One.exe");
    }
}
