//! The single allow-listed writer (safety rule 3).
//!
//! Every file the app creates, appends to or deletes goes through [`OwnFiles`],
//! which only accepts paths inside the app data directory. Nothing else in the
//! workspace may call `std::fs` mutators (enforced by clippy, see `clippy.toml`).
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::DataDir;

#[derive(Debug, thiserror::Error)]
pub enum OwnFilesError {
    #[error("refusing to write outside the diskdash data directory: {0}")]
    OutsideDataDir(PathBuf),
    #[error("path must not contain `..`, prefixes or be relative: {0}")]
    InvalidPath(PathBuf),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Write access to the app's own data directory, and nothing else.
#[derive(Debug, Clone)]
pub struct OwnFiles {
    root: PathBuf,
}

impl OwnFiles {
    /// Create the data directory if needed and take ownership of it.
    pub fn new(data_dir: &DataDir) -> Result<Self, OwnFilesError> {
        std::fs::create_dir_all(data_dir.root())?;
        let root = data_dir.root().canonicalize()?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve `path` and check it lies strictly inside the data directory.
    /// Relative paths are taken relative to the data directory.
    pub fn check(&self, path: &Path) -> Result<PathBuf, OwnFilesError> {
        // Check raw segments: under a `\\?\` root, `Path::components` treats
        // `..` as an ordinary name, so it would not be caught after joining.
        let raw = path.as_os_str().to_string_lossy();
        if raw.split(['\\', '/']).any(|seg| seg == ".." || seg == ".")
            || path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(OwnFilesError::InvalidPath(path.to_path_buf()));
        }
        let joined = if path.is_absolute() { path.to_path_buf() } else { self.root.join(path) };
        // Canonicalise the deepest existing ancestor, so junctions inside the
        // data dir cannot redirect writes elsewhere.
        let mut existing = joined.as_path();
        let mut rest = Vec::new();
        while !existing.exists() {
            match (existing.parent(), existing.file_name()) {
                (Some(parent), Some(name)) => {
                    rest.push(name.to_os_string());
                    existing = parent;
                }
                _ => return Err(OwnFilesError::InvalidPath(path.to_path_buf())),
            }
        }
        let mut resolved = existing.canonicalize()?;
        for part in rest.into_iter().rev() {
            resolved.push(part);
        }
        if resolved == self.root || !resolved.starts_with(&self.root) {
            return Err(OwnFilesError::OutsideDataDir(path.to_path_buf()));
        }
        Ok(resolved)
    }

    /// Create a directory (and parents) inside the data dir.
    pub fn create_dir_all(&self, path: &Path) -> Result<PathBuf, OwnFilesError> {
        let p = self.check(path)?;
        std::fs::create_dir_all(&p)?;
        Ok(p)
    }

    /// Open a file for appending, creating it (and its parent) if missing.
    pub fn open_append(&self, path: &Path) -> Result<File, OwnFilesError> {
        let p = self.check(path)?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(OpenOptions::new().create(true).append(true).open(p)?)
    }

    /// Rename within the data dir (used for log rotation).
    pub fn rename(&self, from: &Path, to: &Path) -> Result<(), OwnFilesError> {
        let (from, to) = (self.check(from)?, self.check(to)?);
        std::fs::rename(from, to)?;
        Ok(())
    }

    /// Delete a file inside the data dir. Missing files are not an error.
    pub fn remove_file(&self, path: &Path) -> Result<(), OwnFilesError> {
        let p = self.check(path)?;
        match std::fs::remove_file(p) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// Ensure the parent of a database file exists and return its checked path.
    /// SQLite then creates the file itself.
    pub fn prepare_db_path(&self, path: &Path) -> Result<PathBuf, OwnFilesError> {
        let p = self.check(path)?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn setup() -> (tempfile::TempDir, OwnFiles) {
        let tmp = tempfile::tempdir().unwrap();
        let dd = DataDir::at(tmp.path().join("data"));
        let own = OwnFiles::new(&dd).unwrap();
        (tmp, own)
    }

    #[test]
    fn writes_inside_data_dir() {
        let (_tmp, own) = setup();
        let mut f = own.open_append(Path::new("logs/diskdash.log")).unwrap();
        writeln!(f, "hello").unwrap();
        assert!(own.root().join("logs/diskdash.log").exists());
    }

    #[test]
    fn refuses_paths_outside_data_dir() {
        let (tmp, own) = setup();
        let outside = tmp.path().join("victim.txt");
        assert!(matches!(own.open_append(&outside), Err(OwnFilesError::OutsideDataDir(_))));
        assert!(!outside.exists());
        assert!(matches!(own.remove_file(Path::new(r"C:\Windows\win.ini")), Err(OwnFilesError::OutsideDataDir(_))));
    }

    #[test]
    fn refuses_parent_dir_escape() {
        let (_tmp, own) = setup();
        assert!(matches!(own.open_append(Path::new(r"..\escape.txt")), Err(OwnFilesError::InvalidPath(_))));
        assert!(matches!(own.check(Path::new(r"logs\..\..\x")), Err(OwnFilesError::InvalidPath(_))));
    }

    #[test]
    fn refuses_the_root_itself() {
        let (_tmp, own) = setup();
        let root = own.root().to_path_buf();
        assert!(matches!(own.remove_file(&root), Err(OwnFilesError::OutsideDataDir(_))));
    }
}
