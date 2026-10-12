use std::path::{Path, PathBuf};

/// Environment variable that overrides the data directory (tests, portable use).
pub const DATA_DIR_ENV: &str = "DISKDASH_DATA_DIR";

/// The app's own data directory: `%LOCALAPPDATA%\diskdash` by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// Resolve from `DISKDASH_DATA_DIR`, else `%LOCALAPPDATA%\diskdash`.
    pub fn resolve() -> Option<Self> {
        Self::resolve_from(
            std::env::var_os(DATA_DIR_ENV).map(PathBuf::from),
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        )
    }

    fn resolve_from(override_dir: Option<PathBuf>, local_app_data: Option<PathBuf>) -> Option<Self> {
        let root = match override_dir {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => local_app_data?.join("diskdash"),
        };
        let root = std::path::absolute(&root).unwrap_or(root);
        Some(Self { root })
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn catalog_db(&self) -> PathBuf {
        self.root.join("diskdash.db")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn snapshots_dir(&self) -> PathBuf {
        self.root.join("snapshots")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins() {
        let d =
            DataDir::resolve_from(Some(PathBuf::from(r"C:\tmp\dd")), Some(PathBuf::from(r"C:\Users\x\AppData\Local")))
                .unwrap();
        assert_eq!(d.root(), Path::new(r"C:\tmp\dd"));
    }

    #[test]
    fn default_is_localappdata_diskdash() {
        let d = DataDir::resolve_from(None, Some(PathBuf::from(r"C:\Users\x\AppData\Local"))).unwrap();
        assert_eq!(d.root(), Path::new(r"C:\Users\x\AppData\Local\diskdash"));
        assert_eq!(d.catalog_db(), Path::new(r"C:\Users\x\AppData\Local\diskdash\diskdash.db"));
    }

    #[test]
    fn empty_override_is_ignored() {
        let d = DataDir::resolve_from(Some(PathBuf::new()), Some(PathBuf::from(r"C:\L"))).unwrap();
        assert_eq!(d.root(), Path::new(r"C:\L\diskdash"));
    }
}
