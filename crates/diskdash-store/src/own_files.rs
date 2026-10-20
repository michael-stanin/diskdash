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
    #[error("refusing to write through a link or junction inside the data directory: {0}")]
    ThroughLink(PathBuf),
    #[error("path must not contain `..`, prefixes or be relative: {0}")]
    InvalidPath(PathBuf),
    #[error(transparent)]
    Io(#[from] io::Error),
}

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// Write access to the app's own data directory, and nothing else.
///
/// Paths are compared lexically (case-insensitive) rather than by resolving
/// them: packaged-app filesystem virtualisation can make the same folder
/// resolve to different physical locations. Junctions and symlinks below the
/// data directory are refused instead, which is what resolving guarded against.
#[derive(Debug, Clone)]
pub struct OwnFiles {
    root: PathBuf,
}

/// Path segments without any `\\?\` verbatim prefix (original case).
fn segments(p: &Path) -> Vec<String> {
    let s = p.as_os_str().to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.split(['\\', '/']).filter(|c| !c.is_empty()).map(str::to_string).collect()
}

fn same_segments(a: &[String], b: &[String]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y) || x.to_lowercase() == y.to_lowercase())
}

fn is_link(p: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

impl OwnFiles {
    /// Create the data directory if needed and take ownership of it.
    pub fn new(data_dir: &DataDir) -> Result<Self, OwnFilesError> {
        std::fs::create_dir_all(data_dir.root())?;
        let root = std::path::absolute(data_dir.root())?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Check `path` lies strictly inside the data directory, with no link or
    /// junction on the way. Relative paths are taken relative to the data dir.
    pub fn check(&self, path: &Path) -> Result<PathBuf, OwnFilesError> {
        // Raw segments: under a `\\?\` prefix, `Path::components` treats `..`
        // as an ordinary name.
        let raw = path.as_os_str().to_string_lossy();
        if raw.split(['\\', '/']).any(|seg| seg == ".." || seg == ".")
            || path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(OwnFilesError::InvalidPath(path.to_path_buf()));
        }
        let joined = if path.is_absolute() { path.to_path_buf() } else { self.root.join(path) };
        let (root_seg, seg) = (segments(&self.root), segments(&joined));
        if seg.len() <= root_seg.len() || !same_segments(&seg[..root_seg.len()], &root_seg) {
            return Err(OwnFilesError::OutsideDataDir(path.to_path_buf()));
        }
        // Every existing component below the root (and the target) must be a
        // plain directory/file, never a junction or symlink pointing elsewhere.
        let mut cur = self.root.clone();
        for part in &seg[root_seg.len()..] {
            cur.push(part);
            if is_link(&cur) {
                return Err(OwnFilesError::ThroughLink(cur));
            }
        }
        Ok(cur)
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

/// Extensions a report may have. Never anything executable.
pub const REPORT_EXTENSIONS: &[&str] = &["html", "csv", "zip", "json"];

#[derive(Debug, thiserror::Error)]
pub enum ReportWriteError {
    #[error("choose a full folder path on a local drive, such as D:\\Reports ({0})")]
    NotAFolderPath(String),
    #[error("{0} is not a folder that exists")]
    NoSuchFolder(PathBuf),
    #[error("reports can only be .html, .csv, .zip or .json files, not .{0}")]
    BadExtension(String),
    #[error("cannot write to {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
}

/// Letters, digits, dot, dash and underscore only; anything else would let a name steer the write.
fn safe_stem(stem: &str) -> bool {
    !stem.is_empty()
        && stem.len() <= 80
        && !stem.starts_with('.')
        && stem.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Write a finished report into a folder the user picked.
///
/// This is the only place outside the data directory that diskdash writes, and it
/// can only create a **new** file there: the name is reserved with `create_new`
/// (an existing file is never replaced; a counter is added instead), the content
/// goes to a temporary file in the same folder, and that is renamed over the
/// reserved name. On any failure the temporary and reserved files are removed, so a
/// failed export leaves nothing behind.
pub fn write_report_file(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf, ReportWriteError> {
    let ext = ext.to_ascii_lowercase();
    if !REPORT_EXTENSIONS.contains(&ext.as_str()) {
        return Err(ReportWriteError::BadExtension(ext));
    }
    let raw = dir.as_os_str().to_string_lossy().to_string();
    let drive_part = raw.strip_prefix(r"\\?\").unwrap_or(&raw);
    let local_drive = drive_part.as_bytes().get(1) == Some(&b':') && drive_part.as_bytes()[0].is_ascii_alphabetic();
    let has_dots = raw.split(['\\', '/']).any(|seg| seg == ".." || seg == ".");
    if !local_drive || has_dots || !dir.is_absolute() {
        return Err(ReportWriteError::NotAFolderPath(raw));
    }
    if !std::fs::metadata(dir).is_ok_and(|m| m.is_dir()) {
        return Err(ReportWriteError::NoSuchFolder(dir.to_path_buf()));
    }
    if !safe_stem(stem) {
        return Err(ReportWriteError::NotAFolderPath(format!("bad file name {stem}")));
    }
    let werr = |path: &Path, source: io::Error| ReportWriteError::Write { path: path.to_path_buf(), source };

    // Reserve a name that does not exist yet.
    let mut final_path = dir.join(format!("{stem}.{ext}"));
    let mut n = 1;
    loop {
        match OpenOptions::new().write(true).create_new(true).open(&final_path) {
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < 1000 => {
                n += 1;
                final_path = dir.join(format!("{stem}-{n}.{ext}"));
            }
            Err(e) => return Err(werr(&final_path, e)),
        }
    }
    let tmp = dir.join(format!(".{stem}.{}.tmp", std::process::id()));
    let result = (|| -> io::Result<()> {
        use std::io::Write;
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &final_path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&final_path);
        return Err(werr(&final_path, e));
    }
    Ok(final_path)
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
    fn reopens_existing_file_by_absolute_path() {
        let tmp = tempfile::tempdir().unwrap();
        let dd = DataDir::at(tmp.path().join("data"));
        let own = OwnFiles::new(&dd).unwrap();
        let log = dd.logs_dir().join("diskdash.log");
        drop(own.open_append(&log).unwrap());
        // Second start: the file now exists.
        let own2 = OwnFiles::new(&dd).unwrap();
        own2.open_append(&log).expect("existing file inside the data dir must be accepted");
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
    fn refuses_writes_through_a_junction() {
        let (tmp, own) = setup();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let link = own.root().join("sneaky");
        let ok = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "mklink /J works without admin");
        let err = own.open_append(&link.join("x.log")).unwrap_err();
        assert!(matches!(err, OwnFilesError::ThroughLink(_)), "{err}");
        assert!(!outside.join("x.log").exists());
    }

    #[test]
    fn accepts_case_and_prefix_variants_of_the_root() {
        let (_tmp, own) = setup();
        let upper = std::path::PathBuf::from(own.root().to_string_lossy().to_uppercase()).join("a.txt");
        assert!(own.check(&upper).is_ok());
        let verbatim = std::path::PathBuf::from(format!(r"\\?\{}", own.root().display())).join("b.txt");
        assert!(own.check(&verbatim).is_ok());
    }

    #[test]
    fn refuses_sibling_with_common_prefix() {
        let (_tmp, own) = setup();
        let sibling = std::path::PathBuf::from(format!("{}-evil", own.root().display())).join("x");
        assert!(matches!(own.check(&sibling), Err(OwnFilesError::OutsideDataDir(_))));
    }

    #[test]
    fn refuses_the_root_itself() {
        let (_tmp, own) = setup();
        let root = own.root().to_path_buf();
        assert!(matches!(own.remove_file(&root), Err(OwnFilesError::OutsideDataDir(_))));
    }

    #[test]
    fn report_files_are_new_atomic_and_only_the_allowed_kinds() {
        let tmp = tempfile::tempdir().unwrap();
        let p = write_report_file(tmp.path(), "diskdash-report-1", "html", b"<p>hi</p>").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"<p>hi</p>");
        // The same name again gets a counter; nothing is replaced.
        let q = write_report_file(tmp.path(), "diskdash-report-1", "html", b"second").unwrap();
        assert_ne!(p, q);
        assert_eq!(std::fs::read(&p).unwrap(), b"<p>hi</p>");
        let names: Vec<String> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "no temporary file is left: {names:?}");
        for bad in ["ps1", "bat", "cmd", "exe", "js", "html.ps1", ""] {
            assert!(
                matches!(write_report_file(tmp.path(), "r", bad, b"x"), Err(ReportWriteError::BadExtension(_))),
                "{bad}"
            );
        }
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 2);
    }

    #[test]
    fn report_folders_and_names_are_validated() {
        let tmp = tempfile::tempdir().unwrap();
        for bad in ["relative\\dir", r"\\server\share", r"C:\..\x", "", r"C:\Windows\.\x"] {
            assert!(
                matches!(write_report_file(Path::new(bad), "r", "csv", b"x"), Err(ReportWriteError::NotAFolderPath(_))),
                "{bad}"
            );
        }
        assert!(matches!(
            write_report_file(&tmp.path().join("missing"), "r", "csv", b"x"),
            Err(ReportWriteError::NoSuchFolder(_))
        ));
        // A file is not a folder.
        let f = tmp.path().join("a.txt");
        std::fs::write(&f, b"x").unwrap();
        assert!(matches!(write_report_file(&f, "r", "csv", b"x"), Err(ReportWriteError::NoSuchFolder(_))));
        for stem in ["../evil", "a b", "x/y", ".hidden", "", "a:b"] {
            assert!(write_report_file(tmp.path(), stem, "csv", b"x").is_err(), "{stem}");
        }
    }

    #[test]
    fn an_unwritable_folder_fails_cleanly_and_leaves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locked");
        std::fs::create_dir(&dir).unwrap();
        let user = std::env::var("USERNAME").unwrap();
        let denied = std::process::Command::new("icacls")
            .arg(&dir)
            .args(["/deny", &format!("{user}:(OI)(CI)(W,DC)")])
            .output()
            .unwrap()
            .status
            .success();
        assert!(denied, "icacls /deny");
        struct Restore(PathBuf, String);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::process::Command::new("icacls").arg(&self.0).args(["/remove:d", &self.1]).output();
            }
        }
        let restore = Restore(dir.clone(), user);
        let err = write_report_file(&dir, "report", "json", b"{}").unwrap_err();
        assert!(matches!(err, ReportWriteError::Write { .. }), "{err}");
        drop(restore);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "no partial or reserved file remains");
    }
}
