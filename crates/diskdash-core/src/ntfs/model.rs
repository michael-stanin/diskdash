//! The entry model shared by every MFT source: what the volume reader produces
//! inside the helper and what the importer consumes in the server.

use serde::{Deserialize, Serialize};

/// First record number that is an ordinary file or folder. Records below it are NTFS metafiles
/// (`$MFT`, `$Bitmap`, ...), except record 5, the root folder.
pub const FIRST_USER_FRN: u64 = 24;
pub const ROOT_FRN: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryName {
    /// Record number of the parent folder.
    pub parent: u64,
    pub parent_seq: u16,
    pub name: String,
}

/// One file or folder, with every name it has (hard links) and the sizes the
/// directory walk would report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MftEntry {
    pub frn: u64,
    pub seq: u16,
    pub is_dir: bool,
    /// Win32 attributes, directory bit included.
    pub attrs: u32,
    pub ctime: i64,
    pub mtime: i64,
    pub atime: i64,
    pub names: Vec<EntryName>,
    pub size: u64,
    /// On-disk allocation of the data stream; 0 when it is stored in the record.
    pub allocated: u64,
    pub resident: bool,
    pub compressed_or_sparse: bool,
    pub reparse_tag: u32,
}

impl MftEntry {
    pub fn is_metafile(&self) -> bool {
        self.frn < FIRST_USER_FRN && self.frn != ROOT_FRN
    }
}

/// Facts about the volume an MFT source reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MftInfo {
    pub bytes_per_cluster: u64,
    pub record_size: u32,
    /// Number of record slots in the MFT (in use or not).
    pub record_count: u64,
    pub serial: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScanTally {
    pub records: u64,
    pub bad_records: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum MftError {
    #[error("cannot open the volume: {0}")]
    Open(String),
    #[error("cannot read the MFT: {0}")]
    Read(String),
    #[error("{bad} of {total} MFT records were unreadable; use the directory walk instead")]
    TooManyBadRecords { bad: u64, total: u64 },
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

/// Where entries come from: the elevated helper in production, a vector in tests.
pub trait MftSource {
    fn info(&self) -> &MftInfo;

    /// Stream every in-use entry once; `f` returns false to stop early. May be called more than once
    /// (the importer reads twice).
    fn for_each(&mut self, f: &mut dyn FnMut(MftEntry) -> bool) -> Result<ScanTally, MftError>;
}

/// In-memory source for tests and fixtures.
#[derive(Debug, Clone)]
pub struct VecSource {
    pub info: MftInfo,
    pub entries: Vec<MftEntry>,
}

impl MftSource for VecSource {
    fn info(&self) -> &MftInfo {
        &self.info
    }

    fn for_each(&mut self, f: &mut dyn FnMut(MftEntry) -> bool) -> Result<ScanTally, MftError> {
        for e in &self.entries {
            if !f(e.clone()) {
                break;
            }
        }
        Ok(ScanTally { records: self.entries.len() as u64, bad_records: 0 })
    }
}
