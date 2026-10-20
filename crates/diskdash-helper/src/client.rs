//! The server's end of the protocol, generic over any `Read + Write` so the
//! same code runs against the real pipe and against test doubles.

use std::io::{Read, Write};

use diskdash_core::ntfs::model::{MftEntry, MftInfo, ScanTally};
use diskdash_core::ntfs::usn::{JournalState, StoredUsn};

use crate::proto::{
    read_frame, write_frame, ErrorCode, FrameError, HelperVolume, PrefetchRun, Request, Response, SmartCounters,
    UsnOutcome, PROTOCOL_VERSION,
};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("helper connection: {0}")]
    Frame(#[from] FrameError),
    #[error("{message}")]
    Remote { code: ErrorCode, message: String },
    #[error("unexpected reply from the helper: {0}")]
    Protocol(String),
}

pub struct Client<S: Read + Write> {
    stream: S,
}

impl<S: Read + Write> Client<S> {
    /// Authenticate with the per-launch secret.
    pub fn connect(mut stream: S, secret: &[u8]) -> Result<Self, ClientError> {
        write_frame(&mut stream, &Request::Hello { secret: secret.to_vec(), version: PROTOCOL_VERSION })?;
        match read_frame::<Response>(&mut stream)? {
            Response::Welcome { .. } => Ok(Self { stream }),
            Response::Error { code, message } => Err(ClientError::Remote { code, message }),
            other => Err(ClientError::Protocol(format!("{other:?}"))),
        }
    }

    fn send(&mut self, req: &Request) -> Result<(), ClientError> {
        Ok(write_frame(&mut self.stream, req)?)
    }

    fn recv(&mut self) -> Result<Response, ClientError> {
        match read_frame::<Response>(&mut self.stream)? {
            Response::Error { code, message } => Err(ClientError::Remote { code, message }),
            r => Ok(r),
        }
    }

    fn call(&mut self, req: &Request) -> Result<Response, ClientError> {
        self.send(req)?;
        self.recv()
    }

    pub fn volumes(&mut self) -> Result<Vec<HelperVolume>, ClientError> {
        match self.call(&Request::Volumes)? {
            Response::Volumes(v) => Ok(v),
            o => Err(ClientError::Protocol(format!("{o:?}"))),
        }
    }

    pub fn mft_info(&mut self, volume_id: &str) -> Result<MftInfo, ClientError> {
        match self.call(&Request::MftInfo { volume_id: volume_id.into() })? {
            Response::MftInfo(i) => Ok(i),
            o => Err(ClientError::Protocol(format!("{o:?}"))),
        }
    }

    /// Stream a scan into `f` (return false to stop). When stopped early the rest of the stream
    /// is read and dropped so the connection stays usable.
    pub fn mft_scan(
        &mut self,
        volume_id: &str,
        f: &mut dyn FnMut(MftEntry) -> bool,
    ) -> Result<(ScanTally, Option<JournalState>), ClientError> {
        self.send(&Request::MftScan { volume_id: volume_id.into() })?;
        let mut open = true;
        loop {
            match self.recv()? {
                Response::MftBatch(batch) => {
                    if open {
                        for e in batch {
                            if !f(e) {
                                open = false;
                                break;
                            }
                        }
                    }
                }
                Response::MftDone { tally, journal } => return Ok((tally, journal)),
                o => return Err(ClientError::Protocol(format!("{o:?}"))),
            }
        }
    }

    pub fn usn_read(
        &mut self,
        volume_id: &str,
        stored: Option<StoredUsn>,
        total_records: u64,
    ) -> Result<UsnOutcome, ClientError> {
        match self.call(&Request::UsnRead { volume_id: volume_id.into(), stored, total_records })? {
            Response::Usn(u) => Ok(u),
            o => Err(ClientError::Protocol(format!("{o:?}"))),
        }
    }

    pub fn smart(&mut self, disk_number: u32) -> Result<SmartCounters, ClientError> {
        match self.call(&Request::SmartCounters { disk_number })? {
            Response::Smart(s) => Ok(s),
            o => Err(ClientError::Protocol(format!("{o:?}"))),
        }
    }

    pub fn prefetch(&mut self) -> Result<Vec<PrefetchRun>, ClientError> {
        match self.call(&Request::PrefetchLastRun)? {
            Response::Prefetch(p) => Ok(p),
            o => Err(ClientError::Protocol(format!("{o:?}"))),
        }
    }
}
