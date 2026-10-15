//! The few things measured outside the snapshot. All of them are metadata
//! queries about well-known system locations; none opens a user file.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// The item does not exist.
    Missing,
    /// Access denied (size unknown, the candidate is still shown).
    Denied(String),
    /// Cannot be measured in this mode (for example shadow storage without admin).
    Unavailable(String),
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeError::Missing => f.write_str("not found"),
            ProbeError::Denied(m) | ProbeError::Unavailable(m) => f.write_str(m),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecycleBin {
    pub bytes: u64,
    pub items: u64,
}

pub trait Probe: Send + Sync {
    /// Recycle Bin of the current user on one drive (`SHQueryRecycleBinW`).
    fn recycle_bin(&self, drive_root: &str) -> Result<RecycleBin, ProbeError>;
    /// Shadow-copy storage in use on a drive. Needs the elevated helper.
    fn shadow_storage(&self, drive_root: &str) -> Result<u64, ProbeError>;
    /// Allocated size of one well-known file from its attributes
    /// (hiberfil.sys, pagefile.sys, swapfile.sys). Never opens the file.
    fn file_size(&self, path: &str) -> Result<u64, ProbeError>;
}

/// Measures nothing. Used when no real probe is available and in tests that do not need one.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullProbe;

impl Probe for NullProbe {
    fn recycle_bin(&self, _: &str) -> Result<RecycleBin, ProbeError> {
        Err(ProbeError::Unavailable("not measured".into()))
    }

    fn shadow_storage(&self, _: &str) -> Result<u64, ProbeError> {
        Err(ProbeError::Unavailable("size unavailable without admin".into()))
    }

    fn file_size(&self, _: &str) -> Result<u64, ProbeError> {
        Err(ProbeError::Missing)
    }
}
