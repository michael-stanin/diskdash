//! Path-scoped job locks. Two locks conflict when the paths are equal or one is
//! an ancestor of the other (case-insensitive, Windows separators).

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockPath(String);

impl LockPath {
    pub fn new(path: &Path) -> Self {
        let s = path.as_os_str().to_string_lossy();
        let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
        let mut norm = s.replace('/', "\\").to_lowercase();
        while norm.ends_with('\\') {
            norm.pop();
        }
        Self(norm)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn conflicts(&self, other: &LockPath) -> bool {
        let (a, b) = (&self.0, &other.0);
        a == b || is_ancestor(a, b) || is_ancestor(b, a)
    }
}

fn is_ancestor(ancestor: &str, path: &str) -> bool {
    path.len() > ancestor.len() && path.starts_with(ancestor) && path.as_bytes()[ancestor.len()] == b'\\'
}

pub fn any_conflict(a: &[LockPath], b: &[LockPath]) -> bool {
    a.iter().any(|x| b.iter().any(|y| x.conflicts(y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(s: &str) -> LockPath {
        LockPath::new(Path::new(s))
    }

    #[test]
    fn overlapping_paths_conflict() {
        assert!(l(r"D:\Data").conflicts(&l(r"d:\data\Photos")));
        assert!(l(r"d:\data\photos").conflicts(&l(r"D:\Data\")));
        assert!(l(r"D:\").conflicts(&l(r"D:\anything")));
        assert!(l(r"\\?\D:\Data").conflicts(&l("D:/Data")));
    }

    #[test]
    fn disjoint_paths_do_not_conflict() {
        assert!(!l(r"D:\Data").conflicts(&l(r"E:\Media")));
        assert!(!l(r"D:\Data").conflicts(&l(r"D:\Database")));
        assert!(!l(r"C:\a\b").conflicts(&l(r"C:\a\c")));
    }
}
