//! Windows pieces of the helper: the named pipe restricted to the current user,
//! and the backend that reads volumes through `diskdash-core`'s read-only NTFS module.

use std::ffi::c_void;
use std::fs::File;
use std::os::windows::io::FromRawHandle;
use std::time::UNIX_EPOCH;

use diskdash_core::ntfs::incremental::Changes;
use diskdash_core::ntfs::model::{MftEntry, MftInfo, MftSource, ScanTally};
use diskdash_core::ntfs::usn::{
    check_volume_of_changes, distinct_frns, plan, FullReason, JournalState, Plan, StoredUsn,
};
use diskdash_core::ntfs::volume::{Volume, VolumeMft};
use diskdash_inventory::assemble::{assemble, HealthInputs};
use diskdash_inventory::native;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, GENERIC_READ, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Ioctl::IOCTL_STORAGE_PREDICT_FAILURE;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::System::IO::DeviceIoControl;

use crate::proto::{HelperVolume, PrefetchRun, SmartCounters, UsnOutcome, BATCH_ENTRIES};
use crate::service::Backend;

const ERROR_PIPE_CONNECTED: u32 = 535;

/// The current user's SID as a string such as `S-1-5-21-...`.
pub fn current_user_sid() -> Result<String, String> {
    let mut token = HANDLE::default();
    // SAFETY: queries our own process token for reading.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(|e| e.to_string())?;
    let result = (|| {
        let mut needed = 0u32;
        // SAFETY: first call only asks for the required size.
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
        let mut buf = vec![0u8; needed as usize];
        // SAFETY: the buffer has the size the first call reported.
        unsafe { GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), needed, &mut needed) }
            .map_err(|e| e.to_string())?;
        // SAFETY: the buffer now holds a TOKEN_USER written by the system.
        let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
        let mut s = PWSTR::null();
        // SAFETY: the SID pointer is valid for the buffer's lifetime.
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut s) }.map_err(|e| e.to_string())?;
        // SAFETY: the system returned a NUL-terminated string that we free once.
        let text = unsafe { s.to_string() }.map_err(|e| e.to_string());
        let _ = unsafe { LocalFree(HLOCAL(s.0 as *mut c_void)) };
        text
    })();
    // SAFETY: token came from OpenProcessToken.
    let _ = unsafe { CloseHandle(token) };
    result
}

/// Create the single pipe instance, readable and writable only by `sid`, closed to
/// remote clients, and wait for the server to connect.
pub fn accept_pipe(path: &str, sid: &str) -> Result<File, String> {
    let sddl = HSTRING::from(format!("D:P(A;;GA;;;{sid})"));
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: converts a string we built into a self-relative descriptor, freed below.
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None) }
        .map_err(|e| e.to_string())?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    };
    // SAFETY: `sa` and the descriptor outlive the call.
    let handle = unsafe {
        CreateNamedPipeW(
            &HSTRING::from(path),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            1 << 20,
            1 << 20,
            0,
            Some(&sa),
        )
    };
    // SAFETY: the descriptor was allocated by the conversion call above.
    let _ = unsafe { LocalFree(HLOCAL(sd.0)) };
    if handle.is_invalid() {
        // SAFETY: reads the thread's last error.
        return Err(format!("cannot create the pipe (error {})", unsafe { GetLastError() }.0));
    }
    // SAFETY: blocks until the server connects to the instance we just created.
    if let Err(e) = unsafe { ConnectNamedPipe(handle, None) } {
        // SAFETY: reads the thread's last error.
        if unsafe { GetLastError() }.0 != ERROR_PIPE_CONNECTED {
            // SAFETY: handle from CreateNamedPipeW.
            let _ = unsafe { CloseHandle(handle) };
            return Err(format!("nobody connected: {e}"));
        }
    }
    // SAFETY: we own the handle and hand that ownership to the File, which closes it.
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

/// Reads real volumes. Volume ids come from the same enumeration the server uses.
pub struct SystemBackend {
    volumes: Vec<(HelperVolume, String)>,
}

impl SystemBackend {
    pub fn new() -> Self {
        Self { volumes: Self::enumerate() }
    }

    fn enumerate() -> Vec<(HelperVolume, String)> {
        let raw = native::collect();
        let inputs = HealthInputs { elevated: true, reliability: &[], smart: &[], windows_letter: None };
        let (drives, _) = assemble(&raw, &inputs);
        drives
            .into_iter()
            .map(|d| {
                let v = HelperVolume {
                    id: d.id.clone(),
                    letters: d.letters.clone(),
                    filesystem: d.filesystem.clone(),
                    disk_numbers: d.physical_disks.iter().map(|p| p.number as u32).collect(),
                };
                (v, d.volume_path.clone())
            })
            .collect()
    }

    fn path_of(&self, id: &str) -> Result<&str, String> {
        self.volumes
            .iter()
            .find(|(v, _)| v.id == id)
            .map(|(_, p)| p.as_str())
            .ok_or_else(|| "unknown volume".to_string())
    }

    fn open(&self, id: &str) -> Result<Volume, String> {
        Volume::open(self.path_of(id)?).map_err(|e| e.to_string())
    }
}

impl Default for SystemBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for SystemBackend {
    fn volumes(&self) -> Vec<HelperVolume> {
        self.volumes.iter().map(|(v, _)| v.clone()).collect()
    }

    fn mft_info(&self, volume_id: &str) -> Result<MftInfo, String> {
        let mft = VolumeMft::new(self.open(volume_id)?).map_err(|e| e.to_string())?;
        Ok(mft.info().clone())
    }

    fn mft_scan(
        &self,
        volume_id: &str,
        sink: &mut dyn FnMut(Vec<MftEntry>) -> bool,
    ) -> Result<(ScanTally, Option<JournalState>), String> {
        let mut mft = VolumeMft::new(self.open(volume_id)?).map_err(|e| e.to_string())?;
        // The position to resume from later is the journal's end when the scan starts: anything that
        // changes during the scan is simply seen again by the next quick rescan.
        let journal = mft.volume().query_usn().ok().filter(|j| j.active);
        let mut batch: Vec<MftEntry> = Vec::with_capacity(BATCH_ENTRIES);
        let mut open = true;
        let tally = mft
            .for_each(&mut |e| {
                batch.push(e);
                if batch.len() >= BATCH_ENTRIES {
                    open = sink(std::mem::take(&mut batch));
                }
                open
            })
            .map_err(|e| e.to_string())?;
        if open && !batch.is_empty() {
            sink(batch);
        }
        Ok((tally, journal))
    }

    fn usn_read(&self, volume_id: &str, stored: Option<StoredUsn>, total_records: u64) -> Result<UsnOutcome, String> {
        let vol = self.open(volume_id)?;
        let state = vol.query_usn().map_err(|e| e.to_string())?;
        let full =
            |reason: FullReason| UsnOutcome::Full { reason: reason.message(), state: state.active.then_some(state) };
        if let Plan::Full(r) = plan(stored, &state) {
            return Ok(full(r));
        }
        let Some(s) = stored else { return Ok(full(FullReason::NoPriorState)) };
        let Some((_, changes)) = vol.read_usn(&state, s.next_usn).map_err(|e| e.to_string())? else {
            return Ok(full(FullReason::Wrapped));
        };
        let frns = distinct_frns(&changes);
        if let Plan::Full(r) = check_volume_of_changes(frns.len() as u64, total_records) {
            return Ok(full(r));
        }
        let mut out = Changes::default();
        for frn in frns {
            match vol.entry_for(frn).map_err(|e| e.to_string())? {
                Some(e) => out.entries.push(e),
                None => out.gone.push(frn),
            }
        }
        Ok(UsnOutcome::Changes { changes: out, state })
    }

    fn smart(&self, disk_number: u32) -> Result<SmartCounters, String> {
        let path = HSTRING::from(format!(r"\\.\PhysicalDrive{disk_number}"));
        // SAFETY: read-only open of a physical drive for a query.
        let h = unsafe {
            CreateFileW(
                &path,
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                Default::default(),
                None,
            )
        }
        .map_err(|e| e.to_string())?;
        let mut out = [0u8; 516];
        let mut returned = 0u32;
        // SAFETY: the output buffer is valid for its length.
        let r = unsafe {
            DeviceIoControl(
                h,
                IOCTL_STORAGE_PREDICT_FAILURE,
                None,
                0,
                Some(out.as_mut_ptr() as *mut c_void),
                out.len() as u32,
                Some(&mut returned),
                None,
            )
        };
        // SAFETY: handle from CreateFileW.
        let _ = unsafe { CloseHandle(h) };
        Ok(SmartCounters {
            predict_failure: r.ok().map(|()| u32::from_le_bytes([out[0], out[1], out[2], out[3]]) != 0),
            temperature_c: None,
        })
    }

    fn prefetch(&self) -> Result<Vec<PrefetchRun>, String> {
        let root = std::env::var("SystemRoot").map_err(|e| e.to_string())?;
        let dir = std::path::Path::new(&root).join("Prefetch");
        let mut out = Vec::new();
        for e in std::fs::read_dir(&dir).map_err(|e| format!("cannot read Prefetch: {e}"))?.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".pf") else { continue };
            let exe = stem.rsplit_once('-').map_or(stem, |(a, _)| a).to_string();
            let last_run = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            out.push(PrefetchRun { exe, last_run });
        }
        Ok(out)
    }
}
