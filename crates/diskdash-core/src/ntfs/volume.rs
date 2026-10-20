//! Windows access to an NTFS volume. Everything here only reads: the handle is
//! opened with `GENERIC_READ` (sharing read, write and delete), and the only
//! control codes sent are queries (`FSCTL_GET_NTFS_VOLUME_DATA`,
//! `FSCTL_GET_NTFS_FILE_RECORD`, `FSCTL_QUERY_USN_JOURNAL`, `FSCTL_READ_USN_JOURNAL`).
//! Needs administrator rights; the elevated helper is the only caller.

use std::ffi::c_void;

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, GetLastError, GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, SetFilePointerEx, FILE_BEGIN, FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::{
    FSCTL_GET_NTFS_FILE_RECORD, FSCTL_GET_NTFS_VOLUME_DATA, FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_USN_JOURNAL,
};
use windows::Win32::System::IO::DeviceIoControl;

use super::assemble::{to_entry, Assembler};
use super::model::{MftEntry, MftError, MftInfo, MftSource, ScanTally};
use super::record::{parse_record, ParseError, RawRecord};
use super::runs::Run;
use super::usn::{parse_read_buffer, JournalState, UsnChange};

const ERROR_JOURNAL_NOT_ACTIVE: u32 = 1179;
const ERROR_JOURNAL_DELETE_IN_PROGRESS: u32 = 1178;
const ERROR_JOURNAL_ENTRY_DELETED: u32 = 1181;
const CHUNK_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct NtfsData {
    pub serial: u64,
    pub bytes_per_sector: u32,
    pub bytes_per_cluster: u32,
    pub bytes_per_record: u32,
    pub mft_valid_length: u64,
    pub mft_start_lcn: u64,
}

pub struct Volume {
    handle: HANDLE,
    pub data: NtfsData,
}

impl Drop for Volume {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW and is closed exactly once.
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

fn last_err() -> u32 {
    // SAFETY: reads the calling thread's last-error value.
    unsafe { GetLastError() }.0
}

fn rd32(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn rd64(b: &[u8], o: usize) -> u64 {
    b.get(o..o + 8).map_or(0, |s| {
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        u64::from_le_bytes(a)
    })
}

impl Volume {
    /// `path` is a volume GUID path such as `\\?\Volume{...}` without a trailing backslash.
    pub fn open(path: &str) -> Result<Self, MftError> {
        // SAFETY: opens an existing volume for reading only; OPEN_EXISTING never creates or truncates.
        let handle = unsafe {
            CreateFileW(
                &HSTRING::from(path.trim_end_matches('\\')),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_SEQUENTIAL_SCAN,
                None,
            )
        }
        .map_err(|e| MftError::Open(format!("{e}")))?;
        let mut v = Volume {
            handle,
            data: NtfsData {
                serial: 0,
                bytes_per_sector: 0,
                bytes_per_cluster: 0,
                bytes_per_record: 0,
                mft_valid_length: 0,
                mft_start_lcn: 0,
            },
        };
        let mut out = [0u8; 128];
        let n = v
            .ioctl(FSCTL_GET_NTFS_VOLUME_DATA, &[], &mut out)
            .map_err(|code| MftError::Open(format!("not an NTFS volume (error {code})")))?;
        if n < 72 {
            return Err(MftError::Open("short NTFS volume data".into()));
        }
        v.data = NtfsData {
            serial: rd64(&out, 0),
            bytes_per_sector: rd32(&out, 40),
            bytes_per_cluster: rd32(&out, 44),
            bytes_per_record: rd32(&out, 48),
            mft_valid_length: rd64(&out, 56),
            mft_start_lcn: rd64(&out, 64),
        };
        let d = v.data;
        if d.bytes_per_sector == 0
            || d.bytes_per_cluster == 0
            || d.bytes_per_record < 1024
            || d.bytes_per_record > 65_536
        {
            return Err(MftError::Open("implausible NTFS geometry".into()));
        }
        Ok(v)
    }

    /// Send a query control code. Returns bytes written to `out`, or the Win32 error code.
    fn ioctl(&self, code: u32, input: &[u8], out: &mut [u8]) -> Result<usize, u32> {
        let mut returned = 0u32;
        // SAFETY: both buffers are valid for their stated lengths for the duration of the call.
        let r = unsafe {
            DeviceIoControl(
                self.handle,
                code,
                if input.is_empty() { None } else { Some(input.as_ptr() as *const c_void) },
                input.len() as u32,
                Some(out.as_mut_ptr() as *mut c_void),
                out.len() as u32,
                Some(&mut returned),
                None,
            )
        };
        match r {
            Ok(()) => Ok(returned as usize),
            Err(_) => Err(last_err()),
        }
    }

    /// Read exactly `buf.len()` bytes at byte offset `offset` (both multiples of the sector size).
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), MftError> {
        // SAFETY: plain seek on our own handle.
        unsafe { SetFilePointerEx(self.handle, offset as i64, None, FILE_BEGIN) }
            .map_err(|e| MftError::Read(e.to_string()))?;
        let mut done = 0usize;
        while done < buf.len() {
            let mut n = 0u32;
            // SAFETY: the slice is valid and writable for its length.
            unsafe { ReadFile(self.handle, Some(&mut buf[done..]), Some(&mut n), None) }
                .map_err(|e| MftError::Read(e.to_string()))?;
            if n == 0 {
                return Err(MftError::Read("unexpected end of volume".into()));
            }
            done += n as usize;
        }
        Ok(())
    }

    pub fn query_usn(&self) -> Result<JournalState, MftError> {
        let mut out = [0u8; 80];
        match self.ioctl(FSCTL_QUERY_USN_JOURNAL, &[], &mut out) {
            Ok(n) if n >= 24 => Ok(JournalState {
                journal_id: rd64(&out, 0),
                first_usn: rd64(&out, 8) as i64,
                next_usn: rd64(&out, 16) as i64,
                active: true,
            }),
            Ok(_) => Err(MftError::Read("short journal data".into())),
            Err(ERROR_JOURNAL_NOT_ACTIVE) | Err(ERROR_JOURNAL_DELETE_IN_PROGRESS) => {
                Ok(JournalState { journal_id: 0, first_usn: 0, next_usn: 0, active: false })
            }
            Err(code) => Err(MftError::Read(format!("cannot query the change journal (error {code})"))),
        }
    }

    /// Changes from `from_usn` up to `state.next_usn`. `Err(Wrapped)` semantics are returned as `Ok(None)`.
    pub fn read_usn(&self, state: &JournalState, from_usn: i64) -> Result<Option<(i64, Vec<UsnChange>)>, MftError> {
        let mut start = from_usn;
        let mut all = Vec::new();
        let mut buf = vec![0u8; 256 * 1024];
        while start < state.next_usn {
            let mut input = Vec::with_capacity(40);
            input.extend_from_slice(&start.to_le_bytes());
            input.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // every reason
            input.extend_from_slice(&0u32.to_le_bytes()); // not only on close
            input.extend_from_slice(&0u64.to_le_bytes()); // do not wait
            input.extend_from_slice(&0u64.to_le_bytes());
            input.extend_from_slice(&state.journal_id.to_le_bytes());
            let n = match self.ioctl(FSCTL_READ_USN_JOURNAL, &input, &mut buf) {
                Ok(n) => n,
                Err(ERROR_JOURNAL_ENTRY_DELETED) => return Ok(None),
                Err(code) => return Err(MftError::Read(format!("cannot read the change journal (error {code})"))),
            };
            let (next, mut changes) = parse_read_buffer(&buf[..n]).map_err(MftError::Read)?;
            all.append(&mut changes);
            if next <= start {
                break;
            }
            start = next;
        }
        Ok(Some((start, all)))
    }

    /// The raw FILE record in `frn`, or `None` when that slot is not in use.
    pub fn file_record(&self, frn: u64) -> Result<Option<Vec<u8>>, MftError> {
        let mut out = vec![0u8; 16 + self.data.bytes_per_record as usize];
        let n = match self.ioctl(FSCTL_GET_NTFS_FILE_RECORD, &(frn as i64).to_le_bytes(), &mut out) {
            Ok(n) => n,
            // Slots past the end of the MFT.
            Err(_) => return Ok(None),
        };
        // The system returns the nearest in-use record at or below the one asked for.
        if n < 12 || rd64(&out, 0) & 0x0000_FFFF_FFFF_FFFF != frn {
            return Ok(None);
        }
        let len = (rd32(&out, 8) as usize).min(out.len().saturating_sub(12));
        Ok(Some(out[12..12 + len].to_vec()))
    }

    /// Current entry for one changed record, joined with its extension records. `None` when it is gone.
    pub fn entry_for(&self, frn: u64) -> Result<Option<MftEntry>, MftError> {
        let Some(mut bytes) = self.file_record(frn)? else { return Ok(None) };
        let Ok(base) = parse_record(&mut bytes) else { return Ok(None) };
        if !base.in_use() || base.is_extension() {
            return Ok(None);
        }
        let mut exts: Vec<RawRecord> = Vec::new();
        for e in &base.attr_list {
            if e.record != frn {
                if let Some(mut b) = self.file_record(e.record)? {
                    if let Ok(r) = parse_record(&mut b) {
                        if r.is_extension() && r.base == frn {
                            exts.push(r);
                        }
                    }
                }
            }
        }
        Ok(to_entry(frn, &base, &exts))
    }

    /// Run list of `$MFT`'s data and its real size, from MFT record 0.
    fn mft_layout(&self) -> Result<(Vec<Run>, u64), MftError> {
        let d = self.data;
        let mut rec = vec![0u8; d.bytes_per_record as usize];
        self.read_at(d.mft_start_lcn * u64::from(d.bytes_per_cluster), &mut rec)?;
        let raw = parse_record(&mut rec).map_err(|e| MftError::Read(format!("$MFT record is damaged: {e}")))?;
        let size = raw.data.first().map_or(d.mft_valid_length, |x| x.size);
        if raw.data_runs.is_empty() {
            return Err(MftError::Read("$MFT has no data runs".into()));
        }
        Ok((raw.data_runs, size))
    }
}

/// Streams every in-use record of a volume's MFT.
pub struct VolumeMft {
    vol: Volume,
    info: MftInfo,
    runs: Vec<Run>,
    mft_size: u64,
}

impl VolumeMft {
    pub fn new(vol: Volume) -> Result<Self, MftError> {
        let (runs, mft_size) = vol.mft_layout()?;
        let info = MftInfo {
            bytes_per_cluster: u64::from(vol.data.bytes_per_cluster),
            record_size: vol.data.bytes_per_record,
            record_count: mft_size / u64::from(vol.data.bytes_per_record),
            serial: vol.data.serial as u32,
        };
        Ok(Self { vol, info, runs, mft_size })
    }

    pub fn volume(&self) -> &Volume {
        &self.vol
    }
}

impl MftSource for VolumeMft {
    fn info(&self) -> &MftInfo {
        &self.info
    }

    fn for_each(&mut self, f: &mut dyn FnMut(MftEntry) -> bool) -> Result<ScanTally, MftError> {
        let rs = u64::from(self.info.record_size);
        let cluster = self.info.bytes_per_cluster;
        let mut tally = ScanTally::default();
        let mut asm = Assembler::new();
        let mut index = 0u64;
        let mut keep_going = true;
        let mut buf = vec![0u8; CHUNK_BYTES as usize];
        'runs: for run in self.runs.clone() {
            let run_bytes = run.clusters * cluster;
            let Some(lcn) = run.lcn else {
                index += run_bytes / rs; // sparse: no records
                continue;
            };
            let mut done = 0u64;
            while done < run_bytes {
                // Whole records only, and nothing past the end of the MFT's real size.
                if index * rs >= self.mft_size {
                    break 'runs;
                }
                let want = ((run_bytes - done).min(CHUNK_BYTES) / rs) * rs;
                if want == 0 {
                    break 'runs;
                }
                let chunk = &mut buf[..want as usize];
                self.vol.read_at(lcn * cluster + done, chunk)?;
                for rec in chunk.chunks_exact_mut(rs as usize) {
                    if index * rs >= self.mft_size {
                        break;
                    }
                    tally.records += 1;
                    if rec[0..4] == [0, 0, 0, 0] {
                        index += 1; // never-used slot
                        continue;
                    }
                    match parse_record(rec) {
                        Ok(raw) => asm.push(index, raw, &mut |e| {
                            if keep_going && !f(e) {
                                keep_going = false;
                            }
                        }),
                        Err(ParseError::BadSignature) if rec[0..4] == *b"BAAD" => tally.bad_records += 1,
                        Err(e) => {
                            tracing::debug!(record = index, error = %e, "unreadable MFT record");
                            tally.bad_records += 1;
                        }
                    }
                    index += 1;
                }
                if !keep_going {
                    break 'runs;
                }
                done += want;
            }
        }
        if keep_going {
            tally.bad_records += asm.incomplete;
            asm.finish(&mut |e| {
                if keep_going && !f(e) {
                    keep_going = false;
                }
            });
        }
        Ok(tally)
    }
}
