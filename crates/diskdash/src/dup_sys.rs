//! Reading file contents for duplicate detection (the only code that does).
//!
//! Files are opened with a plain read-only `File::open`: read access only,
//! shared with every other reader, writer and deleter, so diskdash never blocks
//! or changes anything. Attributes are checked first, without opening, so
//! online-only cloud files are never opened.

use std::fs::File;
use std::io;
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::AsRawHandle;
use std::time::UNIX_EPOCH;

use diskdash_analysis::duplicates::{FileId, OpenError, OpenFile, Source};
use diskdash_core::paths;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};

pub struct FsSource;

fn map(e: &io::Error) -> OpenError {
    match e.raw_os_error() {
        Some(5) => OpenError::Denied,
        Some(32 | 33) => OpenError::Sharing,
        Some(2 | 3) => OpenError::NotFound,
        _ => OpenError::Other(e.to_string()),
    }
}

impl Source for FsSource {
    fn attributes(&self, path: &str) -> Result<u32, OpenError> {
        // Metadata only: no data access, so a cloud placeholder is not recalled.
        std::fs::symlink_metadata(paths::to_extended(path)).map(|m| m.file_attributes()).map_err(|e| map(&e))
    }

    fn open(&self, path: &str) -> Result<OpenFile, OpenError> {
        let file = File::open(paths::to_extended(path)).map_err(|e| map(&e))?;
        let meta = file.metadata().map_err(|e| map(&e))?;
        let mtime =
            meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle is valid for the lifetime of `file`; `info` is a valid out-pointer.
        let id = unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.ok().map(|()| FileId {
            serial: info.dwVolumeSerialNumber,
            index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        });
        Ok(OpenFile { size: meta.len(), mtime, attrs: meta.file_attributes(), id, read: Box::new(file) })
    }
}
