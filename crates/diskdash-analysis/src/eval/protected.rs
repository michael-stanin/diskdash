//! Built-in protected-path list (safety rule 4). Until `add-cleanup-plans`
//! brings user-managed protection, manual-cleanup candidates are never offered
//! for these places. System-setting candidates are exempt: they only carry
//! instructions for Windows tools and never suggest removing the folder itself.

use crate::rules::expand::normalize;
use crate::rules::Env;

/// Folders that must not be suggested as a whole (their contents may be).
const EXACT: &[&str] = &["", r"\windows", r"\program files", r"\program files (x86)", r"\programdata", r"\users"];

/// Subtrees that must never be suggested or entered by a rule.
const SUBTREES: &[&str] = &[
    r"\windows\system32",
    r"\windows\syswow64",
    r"\windows\winsxs",
    r"\windows\servicing",
    r"\windows\boot",
    r"\system volume information",
    r"\$recycle.bin",
    r"\boot",
    r"\efi",
    r"\recovery",
];

#[derive(Debug, Clone, Default)]
pub struct Protected {
    /// User profile and its personal folders (lower-case, absolute).
    exact_abs: Vec<String>,
}

fn strip_drive(p: &str) -> &str {
    p.get(2..).unwrap_or("")
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
        Self { exact_abs }
    }

    pub fn is_protected(&self, path: &str) -> bool {
        let Some(norm) = normalize(path) else { return true };
        let lower = norm.to_lowercase();
        if self.exact_abs.contains(&lower) {
            return true;
        }
        let rel = strip_drive(&lower).trim_end_matches('\\');
        if EXACT.contains(&rel) {
            return true;
        }
        SUBTREES.iter().any(|s| rel == *s || rel.starts_with(&format!("{s}\\")))
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
        ] {
            assert!(p.is_protected(path), "{path}");
        }
    }

    #[test]
    fn deny_subtrees_are_protected_but_normal_cache_paths_are_not() {
        let p = p();
        assert!(p.is_protected(r"C:\Windows\System32\drivers\etc"));
        assert!(p.is_protected(r"C:\Windows\WinSxS"));
        assert!(p.is_protected(r"D:\System Volume Information"));
        for ok in [
            r"C:\Windows\Temp",
            r"C:\Windows\SoftwareDistribution\Download",
            r"C:\Users\Ann\AppData\Local\Temp",
            r"C:\Users\Ann\Documents\old.log",
            r"C:\Program Files (x86)\Steam\steamapps\shadercache",
            r"D:\Projects\app\node_modules",
        ] {
            assert!(!p.is_protected(ok), "{ok}");
        }
    }

    #[test]
    fn unparsable_paths_fail_closed() {
        assert!(p().is_protected("not a path"));
    }
}
