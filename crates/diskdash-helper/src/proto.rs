//! The helper's wire protocol: length-prefixed `postcard` frames over a named
//! pipe. One serde enum per direction, shared by both ends, so the set of
//! things the helper can be asked to do is exactly the [`Request`] enum.
//!
//! Nothing in it names a path, a command, or a write: volumes are identified by
//! the ids the helper itself listed, disks by number.

use std::io::{Read, Write};

use diskdash_core::ntfs::incremental::Changes;
use diskdash_core::ntfs::model::{MftEntry, MftInfo, ScanTally};
use diskdash_core::ntfs::usn::{JournalState, StoredUsn};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
/// Largest frame either side accepts. A batch of entries is far smaller.
pub const MAX_FRAME: usize = 32 * 1024 * 1024;
/// Entries per `MftBatch` frame.
pub const BATCH_ENTRIES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    /// Must be the first frame. `secret` is the per-launch secret.
    Hello {
        secret: Vec<u8>,
        version: u32,
    },
    /// Fixed internal volumes the helper will talk about.
    Volumes,
    /// Geometry of a volume's MFT (opens the volume read-only).
    MftInfo {
        volume_id: String,
    },
    /// Stream every in-use entry as `MftBatch` frames, then `MftDone`.
    MftScan {
        volume_id: String,
    },
    /// Changes since the stored position, or why a full scan is needed.
    UsnRead {
        volume_id: String,
        stored: Option<StoredUsn>,
        total_records: u64,
    },
    SmartCounters {
        disk_number: u32,
    },
    PrefetchLastRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperVolume {
    pub id: String,
    pub letters: Vec<String>,
    pub filesystem: String,
    pub disk_numbers: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UsnOutcome {
    Changes {
        changes: Changes,
        state: JournalState,
    },
    /// A full scan is needed; `state` lets the server store the position after it.
    Full {
        reason: String,
        state: Option<JournalState>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartCounters {
    pub predict_failure: Option<bool>,
    pub temperature_c: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefetchRun {
    /// Executable name from the prefetch file name, e.g. `CHROME.EXE`.
    pub exe: String,
    /// Unix seconds of the prefetch file's last write (when the program last ran).
    pub last_run: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    NotAuthenticated,
    BadVersion,
    /// A frame that is not in [`Request`], or one that cannot be decoded.
    UnknownOperation,
    /// The id is not one of the helper's own fixed internal volumes.
    UnknownVolume,
    NotNtfs,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    Welcome { version: u32 },
    Volumes(Vec<HelperVolume>),
    MftInfo(MftInfo),
    MftBatch(Vec<MftEntry>),
    MftDone { tally: ScanTally, journal: Option<JournalState> },
    Usn(UsnOutcome),
    Smart(SmartCounters),
    Prefetch(Vec<PrefetchRun>),
    Error { code: ErrorCode, message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("connection closed")]
    Closed,
    #[error("frame of {0} bytes is over the limit")]
    TooLarge(usize),
    #[error("cannot decode frame: {0}")]
    Decode(String),
    #[error("i/o error: {0}")]
    Io(std::io::Error),
}

pub fn write_frame<T: Serialize>(w: &mut impl Write, v: &T) -> Result<(), FrameError> {
    let bytes = postcard::to_stdvec(v).map_err(|e| FrameError::Decode(e.to_string()))?;
    if bytes.len() > MAX_FRAME {
        return Err(FrameError::TooLarge(bytes.len()));
    }
    w.write_all(&(bytes.len() as u32).to_le_bytes()).map_err(FrameError::Io)?;
    w.write_all(&bytes).map_err(FrameError::Io)?;
    w.flush().map_err(FrameError::Io)
}

/// Read one frame's bytes (not yet decoded, so a bad payload does not desynchronise the stream).
pub fn read_frame_bytes(r: &mut impl Read) -> Result<Vec<u8>, FrameError> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Err(FrameError::Closed),
        Err(e) => return Err(FrameError::Io(e)),
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(FrameError::TooLarge(n));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            FrameError::Closed
        } else {
            FrameError::Io(e)
        }
    })?;
    Ok(buf)
}

pub fn decode<T: for<'a> Deserialize<'a>>(bytes: &[u8]) -> Result<T, FrameError> {
    postcard::from_bytes(bytes).map_err(|e| FrameError::Decode(e.to_string()))
}

pub fn read_frame<T: for<'a> Deserialize<'a>>(r: &mut impl Read) -> Result<T, FrameError> {
    decode(&read_frame_bytes(r)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frames_round_trip_and_reject_oversize_and_garbage() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Request::MftScan { volume_id: "abc".into() }).unwrap();
        let back: Request = read_frame(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(back, Request::MftScan { volume_id: "abc".into() });

        let mut huge = (MAX_FRAME as u32 + 1).to_le_bytes().to_vec();
        huge.extend_from_slice(&[0; 8]);
        assert!(matches!(read_frame::<Request>(&mut Cursor::new(huge)), Err(FrameError::TooLarge(_))));

        let mut junk = 3u32.to_le_bytes().to_vec();
        junk.extend_from_slice(&[0xFF, 0xFF, 0xFF]);
        assert!(matches!(read_frame::<Request>(&mut Cursor::new(junk)), Err(FrameError::Decode(_))));
        assert!(matches!(read_frame::<Request>(&mut Cursor::new(Vec::<u8>::new())), Err(FrameError::Closed)));
        let truncated = [10u8, 0, 0, 0, 1, 2];
        assert!(matches!(read_frame::<Request>(&mut Cursor::new(truncated)), Err(FrameError::Closed)));
    }
}
