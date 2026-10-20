//! Opening the client end of the helper's named pipe.
//!
//! This is the one place in the code base that asks for write access on a
//! handle (`GENERIC_WRITE`): a duplex pipe cannot be used without it. The handle
//! is our own IPC channel, not a file or volume; `cargo xtask readonly-check`
//! exempts exactly this file and exactly that identifier.

use std::fs::File;
use std::os::windows::io::FromRawHandle;

use windows::core::HSTRING;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING};

/// Connect to `\\.\pipe\<name>`. Fails if the pipe does not exist yet or belongs to another user.
pub fn connect_pipe(path: &str) -> std::io::Result<File> {
    // SAFETY: opens an existing named pipe; OPEN_EXISTING never creates anything.
    let h = unsafe {
        CreateFileW(
            &HSTRING::from(path),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))?;
    // SAFETY: we own the handle and give that ownership to the File.
    Ok(unsafe { File::from_raw_handle(h.0) })
}
