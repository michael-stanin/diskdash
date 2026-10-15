//! Path protection (safety rule 4): places that are never suggested, in any
//! suggestion source, and that cannot be added to a plan.
//!
//! The check is a pure function over normalised, case-folded full paths with
//! matching at path-component boundaries. System-setting suggestions (which
//! only carry instructions for Windows tools) are exempt where the caller says
//! so; they never propose removing the folder itself.

use crate::rules::expand::normalize;
use crate::rules::Env;

/// Folders that must not be suggested as a whole (their contents may be).
const EXACT: &[(&str, &str)] = &[
    ("", "protected:drive_root"),
    (r"\program files", "protected:program_files"),
    (r"\program files (x86)", "protected:program_files"),
    (r"\programdata", "protected:program_data"),
    (r"\users", "protected:profile_root"),
];

/// Subtrees that must never be suggested.
const SUBTREES: &[(&str, &str)] = &[
    (r"\windows", "protected:windows"),
    (r"\program files", "protected:program_files"),
    (r"\program files (x86)", "protected:program_files"),
    (r"\system volume information", "protected:system_volume_information"),
    (r"\$recycle.bin", "protected:recycle_bin"),
    (r"\boot", "protected:boot"),
    (r"\efi", "protected:boot"),
    (r"\recovery", "protected:recovery"),
];

/// Documented temp and cache locations inside the protected trees that stay suggestable.
const ALLOWED: &[&str] = &[
    r"\windows\temp",
    r"\windows\softwaredistribution\download",
    r"\windows\prefetch",
    r"\windows\minidump",
    r"\windows\memory.dmp",
    r"\windows\livekernelreports",
    r"\windows\serviceprofiles\networkservice\appdata\local\microsoft\windows\deliveryoptimization\cache",
    r"\program files (x86)\steam\steamapps\shadercache",
    r"\program files (x86)\steam\steamapps\downloading",
    r"\program files\steam\steamapps\shadercache",
    r"\program files\steam\steamapps\downloading",
];

/// The active paging, hibernation and swap files live at a drive root.
const SYSTEM_FILES: &[&str] = &[r"\pagefile.sys", r"\hiberfil.sys", r"\swapfile.sys"];

#[derive(Debug, Clone, Default)]
pub struct Protected {
    /// User profile and its personal folders (lower-case, absolute).
    exact_abs: Vec<String>,
    /// Paths the user excluded (lower-case, absolute, no trailing separator).
    excluded: Vec<String>,
    /// Paths the user marked protected.
    user_protected: Vec<String>,
}

fn strip_drive(p: &str) -> &str {
    p.get(2..).unwrap_or("")
}

fn under(path: &str, base: &str) -> bool {
    path == base || path.strip_prefix(base).is_some_and(|rest| rest.starts_with('\\'))
}

fn clean(list: &[String]) -> Vec<String> {
    list.iter().filter_map(|p| normalize(p)).map(|p| p.trim_end_matches('\\').to_lowercase()).collect()
}

impl Protected {
    pub fn new(env: &dyn Env) -> Self {
        let mut exact_abs = Vec::new();
        if let Some(p) = env.var("USERPROFILE").and_then(|p| normalize(&p)) {
            exact_abs.push(p.to_lowercase());
        }
        for f in ["Documents", "Desktop", "Pictures", "Videos", "Music", "Downloads"] {
            if let Some(p) = env.known_folder(f).and_then(|p| normalize(&p)) {
                exact_abs.push(p.to_lowercase());
            }
        }
        Self { exact_abs, ..Self::default() }
    }

    /// Add the user's excluded and protected paths (from Settings).
    pub fn with_user(mut self, excluded: &[String], protected: &[String]) -> Self {
        self.excluded = clean(excluded);
        self.user_protected = clean(protected);
        self
    }

    /// Why `path` must not be suggested, or `None` when it may be.
    pub fn check(&self, path: &str) -> Option<String> {
        let Some(norm) = normalize(path) else { return Some("protected:invalid_path".into()) };
        let lower = norm.to_lowercase();
        let lower = lower.trim_end_matches('\\');
        let lower = if lower.len() == 2 { format!("{lower}\\") } else { lower.to_string() };
        let lower = lower.as_str();
        if self.excluded.iter().any(|e| under(lower, e)) {
            return Some("excluded:user".into());
        }
        if self.user_protected.iter().any(|e| under(lower, e)) {
            return Some("protected:user".into());
        }
        if self.exact_abs.iter().any(|p| p == lower) {
            return Some("protected:user_folder".into());
        }
        let rel = strip_drive(lower).trim_end_matches('\\');
        if SYSTEM_FILES.contains(&rel) {
            return Some("protected:system_file".into());
        }
        if let Some((_, reason)) = EXACT.iter().find(|(p, _)| *p == rel) {
            return Some((*reason).into());
        }
        if ALLOWED.iter().any(|a| under(rel, a)) {
            return None;
        }
        SUBTREES.iter().find(|(s, _)| under(rel, s)).map(|(_, reason)| {
            let sub = match rel.strip_prefix(r"\windows\") {
                Some(r) if r.starts_with("system32") || r.starts_with("syswow64") => "protected:system32",
                _ => reason,
            };
            sub.to_string()
        })
    }

    pub fn is_protected(&self, path: &str) -> bool {
        self.check(path).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::MapEnv;

    fn p() -> Protected {
        Protected::new(
            &MapEnv::default()
                .with_var("USERPROFILE", r"C:\Users\Ann")
                .with_folder("Documents", r"C:\Users\Ann\Documents"),
        )
    }

    #[test]
    fn whole_system_folders_and_profile_are_protected() {
        let p = p();
        for path in [
            r"C:\",
            r"D:\",
            r"C:\Windows",
            r"C:\Windows\",
            r"c:\program files",
            r"C:\Users",
            r"C:\Users\Ann",
            r"C:\Users\Ann\Documents",
            r"C:\ProgramData",
        ] {
            assert!(p.is_protected(path), "{path}");
        }
    }

    #[test]
    fn trees_are_protected_but_documented_cache_paths_are_not() {
        let p = p();
        assert_eq!(p.check(r"C:\Windows\System32\drivers\etc").as_deref(), Some("protected:system32"));
        assert_eq!(p.check(r"C:\Windows\WinSxS").as_deref(), Some("protected:windows"));
        assert_eq!(p.check(r"C:\Windows\Logs\CBS\CBS.log").as_deref(), Some("protected:windows"));
        assert_eq!(p.check(r"C:\Program Files\Foo\bar.dll").as_deref(), Some("protected:program_files"));
        assert_eq!(p.check(r"C:\Program Files (x86)").as_deref(), Some("protected:program_files"));
        assert_eq!(p.check(r"D:\System Volume Information").as_deref(), Some("protected:system_volume_information"));
        for ok in [
            r"C:\Windows\Temp",
            r"C:\Windows\Temp\x.tmp",
            r"C:\Windows\SoftwareDistribution\Download",
            r"C:\Users\Ann\AppData\Local\Temp",
            r"C:\Users\Ann\Documents\old.log",
            r"C:\Program Files (x86)\Steam\steamapps\shadercache",
            r"C:\Windows.old",
            r"D:\Projects\app\node_modules",
            r"C:\ProgramData\Microsoft\Windows\WER",
        ] {
            assert_eq!(p.check(ok), None, "{ok}");
        }
    }

    #[test]
    fn matching_is_case_insensitive_and_on_component_boundaries() {
        let p = p();
        assert!(p.is_protected(r"c:\PROGRAM FILES\x"));
        assert!(!p.is_protected(r"C:\Program Files Backup\x"), "a different folder that only shares a prefix");
        assert!(!p.is_protected(r"C:\Windowsish\x"));
    }

    #[test]
    fn active_system_files_are_never_manual_candidates() {
        let p = p();
        for f in [r"C:\pagefile.sys", r"D:\hiberfil.sys", r"C:\swapfile.sys"] {
            assert_eq!(p.check(f).as_deref(), Some("protected:system_file"), "{f}");
        }
        assert_eq!(p.check(r"C:\data\pagefile.sys"), None, "only the ones at a drive root");
    }

    #[test]
    fn user_exclusions_and_protected_paths() {
        let p = p().with_user(&[r"D:\Photos".into(), "not a path".into()], &[r"E:\Keep\".into()]);
        assert_eq!(p.check(r"D:\Photos").as_deref(), Some("excluded:user"));
        assert_eq!(p.check(r"d:\photos\2020\a.jpg").as_deref(), Some("excluded:user"));
        assert_eq!(p.check(r"E:\Keep\x").as_deref(), Some("protected:user"));
        assert_eq!(p.check(r"D:\Photos2\a.jpg"), None);
    }

    #[test]
    fn unparsable_paths_fail_closed() {
        assert!(p().is_protected("not a path"));
    }
}
