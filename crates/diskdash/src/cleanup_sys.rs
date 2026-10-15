//! Windows side of the cleanup rules: the interactive user's environment and
//! the metadata probes. Everything here only reads (known folders, attributes
//! of a few system files, the size of the Recycle Bin). Nothing opens a user
//! file and nothing is executed.

use diskdash_analysis::eval::probe::{Probe, ProbeError, RecycleBin};
use diskdash_analysis::rules::Env;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, HANDLE};
use windows::Win32::Storage::FileSystem::{GetFileAttributesExW, GetFileExInfoStandard, WIN32_FILE_ATTRIBUTE_DATA};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos,
    SHGetKnownFolderPath, SHQueryRecycleBinW, KF_FLAG_DEFAULT, SHQUERYRBINFO,
};

/// Environment of the user running diskdash.
pub struct SystemEnv;

impl Env for SystemEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    }

    fn known_folder(&self, name: &str) -> Option<String> {
        let id = match name.to_ascii_lowercase().as_str() {
            "downloads" => &FOLDERID_Downloads,
            "documents" => &FOLDERID_Documents,
            "desktop" => &FOLDERID_Desktop,
            "pictures" => &FOLDERID_Pictures,
            "videos" => &FOLDERID_Videos,
            "music" => &FOLDERID_Music,
            _ => return None,
        };
        // SAFETY: `id` points to a static GUID; on success the returned string
        // is owned by us, copied out, and released with `CoTaskMemFree`.
        unsafe {
            let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, HANDLE::default()).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        }
    }
}

pub struct WindowsProbe;

fn map_error(e: &windows::core::Error) -> ProbeError {
    let code = e.code();
    let is = |win32: windows::Win32::Foundation::WIN32_ERROR| code == win32.to_hresult();
    if is(ERROR_FILE_NOT_FOUND) || is(ERROR_PATH_NOT_FOUND) {
        ProbeError::Missing
    } else if is(ERROR_ACCESS_DENIED) || code == windows::Win32::Foundation::E_ACCESSDENIED {
        ProbeError::Denied("access denied".into())
    } else {
        ProbeError::Unavailable(e.message())
    }
}

impl Probe for WindowsProbe {
    fn recycle_bin(&self, drive_root: &str) -> Result<RecycleBin, ProbeError> {
        let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
        let root = HSTRING::from(drive_root);
        // SAFETY: `info` is a valid, correctly sized SHQUERYRBINFO and `root`
        // is a NUL-terminated string that outlives the call.
        unsafe { SHQueryRecycleBinW(PCWSTR(root.as_ptr()), &mut info) }.map_err(|e| map_error(&e))?;
        Ok(RecycleBin { bytes: info.i64Size.max(0) as u64, items: info.i64NumItems.max(0) as u64 })
    }

    fn shadow_storage(&self, _drive_root: &str) -> Result<u64, ProbeError> {
        // Needs the elevated read-only helper (a later change). Until then the
        // candidate is shown with manual instructions.
        Err(ProbeError::Unavailable("Size unavailable without admin. Check it with the command below.".into()))
    }

    fn file_size(&self, path: &str) -> Result<u64, ProbeError> {
        let wide = HSTRING::from(diskdash_core::paths::to_extended(path));
        let mut data = WIN32_FILE_ATTRIBUTE_DATA::default();
        // SAFETY: `data` is a valid WIN32_FILE_ATTRIBUTE_DATA matching
        // GetFileExInfoStandard; `wide` is NUL-terminated. Only attributes are read.
        unsafe {
            GetFileAttributesExW(PCWSTR(wide.as_ptr()), GetFileExInfoStandard, &mut data as *mut _ as *mut _)
                .map_err(|e| map_error(&e))?;
        }
        Ok((u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow))
    }
}
