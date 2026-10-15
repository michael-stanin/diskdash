//! Builds temporary fixture trees for integration tests.
//!
//! This is the only crate allowed to create files, links and ACLs, and only
//! inside temporary directories it creates itself. Denied ACLs are always
//! restored before the temporary directory is removed.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

pub use tempfile::TempDir;

/// A fixture tree under a fresh temporary directory.
pub struct Fixture {
    denied: Vec<PathBuf>,
    // Declared last so ACLs are restored (in `Drop`) before it is deleted.
    dir: TempDir,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("diskdash-fixture-").tempdir().expect("temp dir");
        Self { denied: Vec::new(), dir }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        // cmd tools (mklink, icacls) need Windows separators.
        self.root().join(rel.replace('/', "\\"))
    }

    pub fn dir(&self, rel: &str) -> PathBuf {
        let p = self.path(rel);
        fs::create_dir_all(&p).expect("create dir");
        p
    }

    /// A file with `size` bytes of deterministic content.
    pub fn file(&self, rel: &str, size: u64) -> PathBuf {
        let p = self.path(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        let mut f = fs::File::create(&p).expect("create file");
        let chunk: Vec<u8> = (0..65536u32).map(|i| (i % 251) as u8).collect();
        let mut left = size;
        while left > 0 {
            let n = left.min(chunk.len() as u64) as usize;
            f.write_all(&chunk[..n]).expect("write");
            left -= n as u64;
        }
        p
    }

    /// Like [`Fixture::file`], but one byte in the middle (at `offset`) differs.
    pub fn tweaked(&self, rel: &str, size: u64, offset: u64) -> PathBuf {
        let p = self.file(rel, size);
        let mut f = OpenOptions::new().read(true).write(true).open(&p).expect("open fixture file");
        f.seek(SeekFrom::Start(offset)).expect("seek");
        f.write_all(&[0xFF]).expect("write");
        p
    }

    /// Hold a fixture file open without any sharing (simulates a locked file). Drop to release.
    pub fn lock_exclusive(&self, rel: &str) -> fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        OpenOptions::new().read(true).share_mode(0).open(self.path(rel)).expect("lock fixture file")
    }

    /// A file of zeros (compresses well).
    pub fn zeros(&self, rel: &str, size: u64) -> PathBuf {
        let p = self.path(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        let mut f = fs::File::create(&p).expect("create file");
        let zero = vec![0u8; 65536];
        let mut left = size;
        while left > 0 {
            let n = left.min(zero.len() as u64) as usize;
            f.write_all(&zero[..n]).expect("write");
            left -= n as u64;
        }
        p
    }

    /// Delete a fixture file (simulates a file removed after a scan).
    pub fn remove(&self, rel: &str) {
        fs::remove_file(self.path(rel)).expect("remove fixture file");
    }

    /// Set a fixture file's last-modified time to `days` days ago.
    pub fn age_days(&self, rel: &str, days: u64) {
        let file = OpenOptions::new().write(true).open(self.path(rel)).expect("open fixture file");
        let when = SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
        file.set_modified(when).expect("set modified time");
    }

    /// `count` empty files named f0000… in `rel` (bulk trees for benchmarks).
    pub fn empty_files(&self, rel: &str, count: usize) {
        let dir = self.dir(rel);
        for i in 0..count {
            fs::File::create(dir.join(format!("f{i:04}"))).expect("create empty file");
        }
    }

    /// NTFS-compressed file of zeros (`compact /C`).
    pub fn compressed(&self, rel: &str, size: u64) -> PathBuf {
        let p = self.zeros(rel, size);
        let ok =
            Command::new("compact").args(["/C", "/Q"]).arg(&p).output().map(|o| o.status.success()).unwrap_or(false);
        assert!(ok, "compact /C failed");
        p
    }

    /// Sparse file: `logical` bytes long with only `written` bytes of data.
    pub fn sparse(&self, rel: &str, logical: u64, written: u64) -> PathBuf {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Ioctl::FSCTL_SET_SPARSE;
        use windows::Win32::System::IO::DeviceIoControl;

        let p = self.path(rel);
        let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(&p).expect("create sparse");
        let mut returned = 0u32;
        // SAFETY: valid open handle; FSCTL_SET_SPARSE takes no buffers.
        unsafe {
            DeviceIoControl(HANDLE(f.as_raw_handle()), FSCTL_SET_SPARSE, None, 0, None, 0, Some(&mut returned), None)
        }
        .expect("FSCTL_SET_SPARSE");
        f.write_all(&vec![7u8; written as usize]).expect("write head");
        f.set_len(logical).expect("set_len");
        f.seek(SeekFrom::Start(0)).expect("seek");
        p
    }

    /// Hard link `rel` → existing `target`.
    pub fn hard_link(&self, target: &str, rel: &str) -> PathBuf {
        let p = self.path(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::hard_link(self.path(target), &p).expect("hard link");
        p
    }

    /// Directory junction `rel` → `target` (absolute or fixture-relative). No admin needed.
    pub fn junction(&self, rel: &str, target: &Path) -> PathBuf {
        let p = self.path(rel);
        let target = if target.is_absolute() { target.to_path_buf() } else { self.path(&target.to_string_lossy()) };
        let ok = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&p)
            .arg(&target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "mklink /J failed");
        p
    }

    /// File symlink; `None` when the process lacks the privilege (no Developer Mode / admin).
    pub fn symlink_file(&self, rel: &str, target: &str) -> Option<PathBuf> {
        let p = self.path(rel);
        std::os::windows::fs::symlink_file(self.path(target), &p).ok().map(|()| p)
    }

    /// File whose full path is at least `min_len` characters (beyond MAX_PATH).
    pub fn long_path_file(&self, min_len: usize, size: u64) -> PathBuf {
        let mut rel = PathBuf::new();
        let mut i = 0;
        while self.root().join(&rel).as_os_str().len() < min_len {
            rel.push(format!("deep-folder-segment-{i:03}-abcdefghijklmnopqrstuvwxyz"));
            i += 1;
        }
        let dir = extended(&self.root().join(&rel));
        fs::create_dir_all(&dir).expect("create long dir");
        let file = dir.join("leaf.bin");
        let mut f = fs::File::create(&file).expect("create long file");
        f.write_all(&vec![1u8; size as usize]).expect("write long file");
        self.root().join(rel).join("leaf.bin")
    }

    /// Deny the current user read/list on `rel` (restored on drop).
    pub fn deny(&mut self, rel: &str) -> PathBuf {
        let p = self.path(rel);
        let user = std::env::var("USERNAME").expect("USERNAME");
        let ok = Command::new("icacls")
            .arg(&p)
            .args(["/deny", &format!("{user}:(OI)(CI)(RX)")])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "icacls /deny failed");
        self.denied.push(p.clone());
        p
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let user = std::env::var("USERNAME").unwrap_or_default();
        for p in &self.denied {
            let _ = Command::new("icacls").arg(p).args(["/remove:d", &user]).output();
        }
    }
}

fn extended(p: &Path) -> PathBuf {
    PathBuf::from(format!(r"\\?\{}", p.display()))
}

/// Metadata of one fixture entry, for "the scan changed nothing" checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryState {
    pub path: PathBuf,
    pub len: u64,
    pub attributes: u32,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
}

/// Metadata of every entry under `root` (links are not followed). Reads no
/// file contents, so taking the manifest cannot itself change access times.
pub fn manifest(root: &Path) -> Vec<EntryState> {
    let mut out = Vec::new();
    let mut stack = vec![extended(root)];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(m) = fs::symlink_metadata(&p) else { continue };
            out.push(EntryState {
                path: p.clone(),
                len: m.len(),
                attributes: m.file_attributes(),
                modified: m.modified().ok(),
                created: m.created().ok(),
                accessed: m.accessed().ok(),
            });
            if m.is_dir() && m.file_attributes() & 0x400 == 0 {
                stack.push(p);
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Panic with a diff if anything under `root` changed since `before`.
///
/// Directory last-access times are excluded: NTFS may update them whenever a
/// directory is listed (by anyone, including this manifest), and no read-only
/// API can prevent that. Everything else, including file access times, must
/// be identical.
pub fn assert_tree_unchanged(root: &Path, before: &[EntryState]) {
    let after = manifest(root);
    assert_eq!(before.len(), after.len(), "number of entries changed");
    for (b, a) in before.iter().zip(&after) {
        let is_dir = b.attributes & 0x10 != 0;
        if is_dir {
            let strip = |e: &EntryState| EntryState { accessed: None, ..e.clone() };
            assert_eq!(strip(b), strip(a), "directory changed: {}", b.path.display());
        } else {
            assert_eq!(b, a, "file changed: {}", b.path.display());
        }
    }
}

/// Like [`assert_tree_unchanged`] but also ignores file last-access times.
///
/// For tests of features that read file contents: where Windows keeps last-access
/// times up to date, reading a file updates it (accepted, see the duplicate
/// finder design). Names, sizes, attributes, creation and modification times
/// must still be identical.
pub fn assert_contents_unchanged(root: &Path, before: &[EntryState]) {
    let after = manifest(root);
    assert_eq!(before.len(), after.len(), "number of entries changed");
    let strip = |e: &EntryState| EntryState { accessed: None, ..e.clone() };
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(strip(b), strip(a), "changed: {}", b.path.display());
    }
}

/// Cluster size of the volume holding `path` (for expected allocated sizes).
pub fn cluster_size(path: &Path) -> u64 {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceW, GetVolumePathNameW};
    let mut vol = [0u16; 300];
    // SAFETY: buffers are writable; paths are valid.
    unsafe {
        GetVolumePathNameW(&HSTRING::from(path.as_os_str()), &mut vol).expect("volume path");
        let end = vol.iter().position(|&c| c == 0).unwrap_or(vol.len());
        let root = HSTRING::from_wide(&vol[..end]).expect("hstring");
        let (mut spc, mut bps) = (0u32, 0u32);
        GetDiskFreeSpaceW(&root, Some(&mut spc), Some(&mut bps), None, None).expect("cluster size");
        u64::from(spc) * u64::from(bps)
    }
}

/// `size` rounded up to whole clusters.
pub fn round_up(size: u64, cluster: u64) -> u64 {
    size.div_ceil(cluster) * cluster
}

/// Write a small text file (creating parent folders). For tests that need rule or config files.
pub fn write_text(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent folder");
    }
    fs::write(path, content).expect("write text file");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_cleans_up_including_denied_dirs() {
        let root;
        {
            let mut f = Fixture::new();
            root = f.root().to_path_buf();
            f.file("a/b.bin", 1000);
            f.dir("locked/inner");
            f.file("locked/inner/secret.bin", 10);
            f.deny("locked");
            assert!(fs::read_dir(f.path("locked")).is_err(), "deny took effect");
        }
        assert!(!root.exists(), "temp dir removed after ACL restore");
    }

    #[test]
    fn manifest_sees_long_paths() {
        let f = Fixture::new();
        let p = f.long_path_file(600, 10);
        assert!(p.as_os_str().len() >= 600);
        assert!(manifest(f.root()).iter().any(|e| e.path.ends_with("leaf.bin")));
    }
}
