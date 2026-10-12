//! Thin, read-only Win32 wrappers used by the walker. Every path passed here is
//! already in extended (`\\?\`) form. Handles are opened with
//! `FILE_READ_ATTRIBUTES` only: metadata, never contents.

use std::ffi::c_void;
use std::mem::size_of;

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FileIdInfo, FileStandardInfo, FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW,
    FindNextFileW, GetCompressedFileSizeW, GetDiskFreeSpaceW, GetFileInformationByHandleEx, GetVolumeInformationW,
    GetVolumePathNameW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO, FIND_FIRST_EX_LARGE_FETCH, OPEN_EXISTING,
    WIN32_FIND_DATAW,
};
use windows::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
use windows::Win32::System::IO::DeviceIoControl;

pub const ERROR_FILE_NOT_FOUND: u32 = 2;
pub const ERROR_PATH_NOT_FOUND: u32 = 3;
pub const ERROR_ACCESS_DENIED: u32 = 5;
pub const ERROR_NO_MORE_FILES: u32 = 18;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Windows error {code}: {message}")]
pub struct WinError {
    pub code: u32,
    pub message: String,
}

impl From<windows::core::Error> for WinError {
    fn from(e: windows::core::Error) -> Self {
        let hr = e.code().0 as u32;
        // HRESULT_FROM_WIN32 → Win32 code
        let code = if hr & 0xFFFF_0000 == 0x8007_0000 { hr & 0xFFFF } else { hr };
        Self { code, message: e.message().trim().to_string() }
    }
}

fn last_error() -> WinError {
    // SAFETY: no preconditions.
    let code = unsafe { GetLastError() }.0;
    WinError { code, message: windows::core::Error::from_win32().message().trim().to_string() }
}

/// One directory entry from `FindFirstFileExW`.
#[derive(Debug, Clone)]
pub struct FindEntry {
    pub name: String,
    pub attrs: u32,
    pub size: u64,
    pub ctime: i64,
    pub atime: i64,
    pub mtime: i64,
    /// Reparse tag when `attrs` has REPARSE_POINT, else 0.
    pub reparse_tag: u32,
}

/// FILETIME (100 ns since 1601) → Unix seconds; 0 stays 0 ("unknown").
pub fn filetime_to_unix(low: u32, high: u32) -> i64 {
    let ticks = (u64::from(high) << 32) | u64::from(low);
    if ticks == 0 {
        return 0;
    }
    (ticks as i64 - 116_444_736_000_000_000) / 10_000_000
}

fn entry_from(d: &WIN32_FIND_DATAW) -> FindEntry {
    let end = d.cFileName.iter().position(|&c| c == 0).unwrap_or(d.cFileName.len());
    let attrs = d.dwFileAttributes;
    FindEntry {
        name: String::from_utf16_lossy(&d.cFileName[..end]),
        attrs,
        size: (u64::from(d.nFileSizeHigh) << 32) | u64::from(d.nFileSizeLow),
        ctime: filetime_to_unix(d.ftCreationTime.dwLowDateTime, d.ftCreationTime.dwHighDateTime),
        atime: filetime_to_unix(d.ftLastAccessTime.dwLowDateTime, d.ftLastAccessTime.dwHighDateTime),
        mtime: filetime_to_unix(d.ftLastWriteTime.dwLowDateTime, d.ftLastWriteTime.dwHighDateTime),
        reparse_tag: if attrs & crate::alloc::FILE_ATTRIBUTE_REPARSE_POINT != 0 { d.dwReserved0 } else { 0 },
    }
}

/// Enumerates one directory (no `.`/`..`).
pub struct DirIter {
    handle: Option<HANDLE>,
    first: Option<FindEntry>,
}

impl DirIter {
    /// `dir` is an extended path to a directory.
    pub fn open(dir: &str) -> Result<Self, WinError> {
        let pattern = HSTRING::from(format!("{}\\*", dir.trim_end_matches('\\')));
        let mut data = WIN32_FIND_DATAW::default();
        // SAFETY: `data` is a valid out buffer; flags request basic info only.
        let handle = unsafe {
            FindFirstFileExW(
                &pattern,
                FindExInfoBasic,
                &mut data as *mut _ as *mut c_void,
                FindExSearchNameMatch,
                None,
                FIND_FIRST_EX_LARGE_FETCH,
            )
        }
        .map_err(WinError::from)?;
        Ok(Self { handle: Some(handle), first: Some(entry_from(&data)) })
    }
}

impl Iterator for DirIter {
    type Item = Result<FindEntry, WinError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = if let Some(e) = self.first.take() {
                e
            } else {
                let h = self.handle?;
                let mut data = WIN32_FIND_DATAW::default();
                // SAFETY: valid find handle and out buffer.
                match unsafe { FindNextFileW(h, &mut data) } {
                    Ok(()) => entry_from(&data),
                    Err(e) => {
                        let err = WinError::from(e);
                        self.close();
                        return (err.code != ERROR_NO_MORE_FILES).then_some(Err(err));
                    }
                }
            };
            if entry.name != "." && entry.name != ".." {
                return Some(Ok(entry));
            }
        }
    }
}

impl DirIter {
    fn close(&mut self) {
        if let Some(h) = self.handle.take() {
            // SAFETY: handle from FindFirstFileExW, closed once.
            let _ = unsafe { FindClose(h) };
        }
    }
}

impl Drop for DirIter {
    fn drop(&mut self) {
        self.close();
    }
}

/// A metadata-only handle (`FILE_READ_ATTRIBUTES`, all sharing, no reparse follow).
pub struct MetaHandle(HANDLE);

impl Drop for MetaHandle {
    fn drop(&mut self) {
        // SAFETY: handle from CreateFileW, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

pub struct StandardInfo {
    pub allocation: u64,
    pub links: u32,
}

impl MetaHandle {
    pub fn open(path: &str) -> Result<Self, WinError> {
        // SAFETY: attribute-only access; OPEN_EXISTING never creates; reparse
        // points are opened themselves rather than followed.
        let h = unsafe {
            CreateFileW(
                &HSTRING::from(path),
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                None,
            )
        }
        .map_err(WinError::from)?;
        Ok(Self(h))
    }

    pub fn standard_info(&self) -> Result<StandardInfo, WinError> {
        let mut info = FILE_STANDARD_INFO::default();
        // SAFETY: buffer sized for FILE_STANDARD_INFO.
        unsafe {
            GetFileInformationByHandleEx(
                self.0,
                FileStandardInfo,
                &mut info as *mut _ as *mut c_void,
                size_of::<FILE_STANDARD_INFO>() as u32,
            )
        }
        .map_err(WinError::from)?;
        Ok(StandardInfo { allocation: info.AllocationSize.max(0) as u64, links: info.NumberOfLinks })
    }

    /// (volume serial, 128-bit file id)
    pub fn file_id(&self) -> Result<(u64, u128), WinError> {
        let mut info = FILE_ID_INFO::default();
        // SAFETY: buffer sized for FILE_ID_INFO.
        unsafe {
            GetFileInformationByHandleEx(
                self.0,
                FileIdInfo,
                &mut info as *mut _ as *mut c_void,
                size_of::<FILE_ID_INFO>() as u32,
            )
        }
        .map_err(WinError::from)?;
        Ok((info.VolumeSerialNumber, u128::from_le_bytes(info.FileId.Identifier)))
    }

    /// Print name of a junction or symlink target.
    pub fn reparse_target(&self) -> Option<String> {
        let mut buf = vec![0u8; 16 * 1024];
        let mut returned = 0u32;
        // SAFETY: output buffer is writable and its length is passed.
        unsafe {
            DeviceIoControl(
                self.0,
                FSCTL_GET_REPARSE_POINT,
                None,
                0,
                Some(buf.as_mut_ptr() as *mut c_void),
                buf.len() as u32,
                Some(&mut returned),
                None,
            )
        }
        .ok()?;
        parse_reparse_target(&buf[..returned as usize])
    }
}

/// Parse a REPARSE_DATA_BUFFER for mount points and symlinks (print name).
pub fn parse_reparse_target(b: &[u8]) -> Option<String> {
    let u16_at = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize);
    let tag = u32::from_le_bytes(b.get(0..4)?.try_into().ok()?);
    let (path_buffer, print_off, print_len) = match tag {
        crate::reparse::IO_REPARSE_TAG_MOUNT_POINT => (16, u16_at(12)?, u16_at(14)?),
        crate::reparse::IO_REPARSE_TAG_SYMLINK => (20, u16_at(12)?, u16_at(14)?),
        _ => return None,
    };
    let start = path_buffer + print_off;
    let bytes = b.get(start..start + print_len)?;
    let wide: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let s = String::from_utf16_lossy(&wide);
    Some(crate::paths::from_extended(s.trim_start_matches(r"\??\")))
}

/// Real allocation for compressed/sparse/deduplicated files.
pub fn compressed_size(path: &str) -> Option<u64> {
    let mut high = 0u32;
    // SAFETY: valid path; out pointer is a local.
    let low = unsafe { GetCompressedFileSizeW(&HSTRING::from(path), Some(&mut high)) };
    if low == u32::MAX {
        // INVALID_FILE_SIZE is also a valid low word; check the error.
        let e = last_error();
        if e.code != 0 {
            return None;
        }
    }
    Some((u64::from(high) << 32) | u64::from(low))
}

pub struct VolumeInfo {
    /// Root of the volume holding the path, e.g. `C:\`.
    pub root: String,
    pub serial: u32,
    pub filesystem: String,
    pub cluster_size: u64,
}

/// Volume facts for any path on it (non-extended path is fine).
pub fn volume_info(path: &str) -> Result<VolumeInfo, WinError> {
    let mut vol = [0u16; 300];
    // SAFETY: writable buffer.
    unsafe { GetVolumePathNameW(&HSTRING::from(path), &mut vol) }.map_err(WinError::from)?;
    let end = vol.iter().position(|&c| c == 0).unwrap_or(vol.len());
    let root = String::from_utf16_lossy(&vol[..end]);
    let root_h = HSTRING::from(root.as_str());
    let (mut serial, mut fs) = (0u32, [0u16; 64]);
    // SAFETY: writable buffers / out pointers.
    unsafe { GetVolumeInformationW(&root_h, None, Some(&mut serial), None, None, Some(&mut fs)) }
        .map_err(WinError::from)?;
    let (mut spc, mut bps) = (0u32, 0u32);
    // SAFETY: out pointers are locals.
    unsafe { GetDiskFreeSpaceW(&root_h, Some(&mut spc), Some(&mut bps), None, None) }.map_err(WinError::from)?;
    let fs_end = fs.iter().position(|&c| c == 0).unwrap_or(fs.len());
    Ok(VolumeInfo {
        root,
        serial,
        filesystem: String::from_utf16_lossy(&fs[..fs_end]),
        cluster_size: u64::from(spc) * u64::from(bps),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::to_extended;
    use test_fixtures::Fixture;

    #[test]
    fn enumerates_a_fixture() {
        let f = Fixture::new();
        f.file("a.bin", 1234);
        f.dir("sub");
        let root = to_extended(&f.root().to_string_lossy());
        let mut names: Vec<_> = DirIter::open(&root).unwrap().map(|e| e.unwrap()).map(|e| (e.name, e.size)).collect();
        names.sort();
        assert_eq!(names, vec![("a.bin".to_string(), 1234), ("sub".to_string(), 0)]);
    }

    #[test]
    fn missing_dir_reports_win32_code() {
        let err = DirIter::open(r"\\?\C:\definitely\not\here").err().unwrap();
        assert!(err.code == ERROR_PATH_NOT_FOUND || err.code == ERROR_FILE_NOT_FOUND, "{err:?}");
    }

    #[test]
    fn hard_links_share_an_id_and_count() {
        let f = Fixture::new();
        f.file("x.bin", 10);
        f.hard_link("x.bin", "y.bin");
        let a = MetaHandle::open(&to_extended(&f.path("x.bin").to_string_lossy())).unwrap();
        let b = MetaHandle::open(&to_extended(&f.path("y.bin").to_string_lossy())).unwrap();
        assert_eq!(a.standard_info().unwrap().links, 2);
        assert_eq!(a.file_id().unwrap(), b.file_id().unwrap());
    }

    #[test]
    fn junction_target_is_read() {
        let f = Fixture::new();
        let target = f.dir("target");
        let j = f.junction("link", &target);
        let h = MetaHandle::open(&to_extended(&j.to_string_lossy())).unwrap();
        assert_eq!(h.reparse_target().unwrap().to_lowercase(), target.to_string_lossy().to_lowercase());
    }

    #[test]
    fn compressed_file_allocates_less() {
        let f = Fixture::new();
        let p = f.compressed("z.bin", 10 << 20);
        let alloc = compressed_size(&to_extended(&p.to_string_lossy())).unwrap();
        assert!(alloc < 10 << 20, "{alloc}");
    }

    #[test]
    fn volume_info_for_temp() {
        let f = Fixture::new();
        let v = volume_info(&f.root().to_string_lossy()).unwrap();
        assert!(v.cluster_size >= 512);
        assert_eq!(v.filesystem, "NTFS");
        assert!(v.root.ends_with('\\'));
    }

    #[test]
    fn filetime_conversion() {
        // 2020-01-01T00:00:00Z
        let ticks: u64 = 132_223_104_000_000_000;
        assert_eq!(filetime_to_unix(ticks as u32, (ticks >> 32) as u32), 1_577_836_800);
        assert_eq!(filetime_to_unix(0, 0), 0);
    }
}
