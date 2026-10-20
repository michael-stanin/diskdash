//! A throw-away NTFS volume on a virtual disk, for the opt-in administrator tests.
//!
//! Creates a VHDX in its own temporary directory, attaches it, formats it NTFS and
//! gives it a drive letter. Dropping it detaches the disk and deletes the file.
//! Needs an elevated process; nothing here ever touches a real disk.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub struct Vhdx {
    file: PathBuf,
    letter: char,
    dir: Option<tempfile::TempDir>,
}

/// `DISKDASH_ADMIN_TESTS=1` opts in to the tests that need administrator rights.
pub fn admin_tests_enabled() -> bool {
    std::env::var("DISKDASH_ADMIN_TESTS").is_ok_and(|v| v == "1")
}

/// Whether this process is elevated (`fltmc` only works for administrators).
pub fn is_elevated() -> bool {
    Command::new("fltmc").output().is_ok_and(|o| o.status.success())
}

fn diskpart(script: &str, dir: &Path) -> Result<String, String> {
    let path = dir.join(format!("script-{}.txt", std::process::id()));
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    let out = Command::new("diskpart").arg("/s").arg(&path).output().map_err(|e| format!("diskpart: {e}"))?;
    let _ = std::fs::remove_file(&path);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(format!("diskpart failed ({}): {text}", out.status))
    }
}

fn free_letter() -> Option<char> {
    ('K'..='Z').rev().find(|c| !Path::new(&format!("{c}:\\")).exists())
}

impl Vhdx {
    /// A formatted NTFS volume of `size_mb` megabytes.
    pub fn create(size_mb: u32) -> Result<Self, String> {
        if !is_elevated() {
            return Err("creating a VHDX needs an elevated process".into());
        }
        let letter = free_letter().ok_or("no free drive letter")?;
        let dir = tempfile::Builder::new().prefix("diskdash-vhdx-").tempdir().map_err(|e| e.to_string())?;
        let file = dir.path().join("fixture.vhdx");
        let f = file.display();
        let mut vhdx = Self { file: file.clone(), letter, dir: Some(dir) };
        let dir_path = vhdx.dir.as_ref().map(|d| d.path().to_path_buf()).ok_or("no temp dir")?;
        let script = format!(
            "create vdisk file=\"{f}\" maximum={size_mb} type=expandable\nselect vdisk file=\"{f}\"\nattach vdisk\ncreate partition primary\nformat fs=ntfs quick label=DDTEST\nassign letter={letter}\n"
        );
        if let Err(e) = diskpart(&script, &dir_path) {
            vhdx.detach();
            return Err(e);
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while !Path::new(&vhdx.root()).exists() {
            if Instant::now() > deadline {
                vhdx.detach();
                return Err("the new volume did not appear".into());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Ok(vhdx)
    }

    /// `X:\`
    pub fn root(&self) -> String {
        format!("{}:\\", self.letter)
    }

    /// Volume path for opening the volume itself: `\\?\X:`
    pub fn volume_path(&self) -> String {
        format!(r"\\?\{}:", self.letter)
    }

    /// Turn the change journal on, as Windows Search or backup software would.
    pub fn create_usn_journal(&self) -> Result<(), String> {
        let out = Command::new("fsutil")
            .args(["usn", "createjournal", "m=33554432", "a=4194304", &format!("{}:", self.letter)])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stdout).to_string())
        }
    }

    fn detach(&mut self) {
        if let Some(dir) = self.dir.as_ref().map(|d| d.path().to_path_buf()) {
            let f = self.file.display();
            let _ = diskpart(&format!("select vdisk file=\"{f}\"\ndetach vdisk\n"), &dir);
        }
        // The temporary directory (and the VHDX inside it) is removed when it drops.
        self.dir = None;
    }
}

impl Drop for Vhdx {
    fn drop(&mut self) {
        self.detach();
    }
}
