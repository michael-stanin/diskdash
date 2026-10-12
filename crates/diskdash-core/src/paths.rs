//! Extended-length (`\\?\`) path helpers. All filesystem calls use the
//! extended form so paths longer than MAX_PATH work; stored and displayed
//! paths never carry the prefix.

use std::path::{Path, PathBuf};

const VERBATIM: &str = r"\\?\";
const VERBATIM_UNC: &str = r"\\?\UNC\";

/// `C:\a` → `\\?\C:\a`; `\\server\share` → `\\?\UNC\server\share`.
pub fn to_extended(p: &str) -> String {
    if p.starts_with(VERBATIM) {
        p.to_string()
    } else if let Some(rest) = p.strip_prefix(r"\\") {
        format!("{VERBATIM_UNC}{rest}")
    } else {
        format!("{VERBATIM}{p}")
    }
}

/// Inverse of [`to_extended`].
pub fn from_extended(p: &str) -> String {
    if let Some(rest) = p.strip_prefix(VERBATIM_UNC) {
        format!(r"\\{rest}")
    } else if let Some(rest) = p.strip_prefix(VERBATIM) {
        rest.to_string()
    } else {
        p.to_string()
    }
}

/// Absolute, separator-normalised root without the prefix. Drive roots keep
/// their trailing backslash (`C:\`); folders lose it (`C:\Data`).
pub fn normalize_root(p: &Path) -> std::io::Result<String> {
    let abs = std::path::absolute(p)?;
    let s = from_extended(&abs.to_string_lossy()).replace('/', "\\");
    let trimmed = s.trim_end_matches('\\');
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        Ok(format!("{}\\", trimmed.to_ascii_uppercase()))
    } else {
        let mut out = trimmed.to_string();
        if out.as_bytes().get(1) == Some(&b':') {
            out.replace_range(0..1, &out[0..1].to_ascii_uppercase());
        }
        Ok(out)
    }
}

/// Join a child name onto a directory path (no prefix).
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('\\') {
        format!("{dir}{name}")
    } else {
        format!("{dir}\\{name}")
    }
}

/// Case-insensitive "is `path` equal to or inside `dir`".
pub fn is_within(path: &str, dir: &str) -> bool {
    let (p, d) = (path.to_lowercase(), dir.trim_end_matches('\\').to_lowercase());
    p == d || (p.starts_with(&d) && p.as_bytes().get(d.len()) == Some(&b'\\'))
}

pub fn to_path(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for p in [r"C:\", r"C:\a\b", r"\\server\share\x"] {
            assert_eq!(from_extended(&to_extended(p)), p);
        }
        assert_eq!(to_extended(r"C:\a"), r"\\?\C:\a");
        assert_eq!(to_extended(r"\\?\C:\a"), r"\\?\C:\a");
    }

    #[test]
    fn long_path_round_trips() {
        let long = format!(r"C:\{}", "x".repeat(600));
        assert_eq!(from_extended(&to_extended(&long)), long);
    }

    #[test]
    fn roots() {
        assert_eq!(normalize_root(Path::new("c:")).unwrap(), r"C:\");
        assert_eq!(normalize_root(Path::new(r"d:\Data\")).unwrap(), r"D:\Data");
        assert_eq!(normalize_root(Path::new(r"\\?\D:\Data")).unwrap(), r"D:\Data");
        assert_eq!(normalize_root(Path::new("D:/Data/Photos")).unwrap(), r"D:\Data\Photos");
    }

    #[test]
    fn joining_and_containment() {
        assert_eq!(join(r"C:\", "Users"), r"C:\Users");
        assert_eq!(join(r"C:\Users", "me"), r"C:\Users\me");
        assert!(is_within(r"c:\users\me\x", r"C:\Users"));
        assert!(is_within(r"C:\Users", r"C:\Users\"));
        assert!(!is_within(r"C:\Users2", r"C:\Users"));
    }
}
