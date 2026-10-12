//! Elevation detection and `GET /api/v1/system`.

use serde::Serialize;
use ts_rs::TS;

/// Features that need administrator rights. In reduced mode they are listed so
/// the UI can explain what is unavailable.
pub const ELEVATED_FEATURES: &[(&str, &str)] = &[
    ("health-counters", "Disk health counters (temperature, wear, power-on hours)"),
    ("smart", "Detailed SMART data via smartctl"),
    ("fast-scan", "Fast NTFS scan (reads the MFT directly)"),
    ("quick-rescan", "Quick rescan using the USN change journal"),
];

pub trait ElevationDetector: Send + Sync {
    fn is_elevated(&self) -> bool;
}

/// Reads the current process token.
pub struct WindowsElevation;

impl ElevationDetector for WindowsElevation {
    fn is_elevated(&self) -> bool {
        win::token_is_elevated().unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReducedFeature {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct OsInfo {
    pub product: Option<String>,
    pub display_version: Option<String>,
    pub build: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SystemInfo {
    pub version: String,
    pub elevated: bool,
    pub reduced_features: Vec<ReducedFeature>,
    pub os: OsInfo,
    pub data_dir: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Health {
    pub status: String,
    pub version: String,
}

pub fn system_info(detector: &dyn ElevationDetector, data_dir: &std::path::Path) -> SystemInfo {
    let elevated = detector.is_elevated();
    let reduced_features = if elevated {
        Vec::new()
    } else {
        ELEVATED_FEATURES
            .iter()
            .map(|(id, label)| ReducedFeature { id: (*id).into(), label: (*label).into() })
            .collect()
    };
    SystemInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        elevated,
        reduced_features,
        os: win::os_info(),
        data_dir: data_dir.display().to_string(),
    }
}

mod win {
    use super::OsInfo;
    use std::ffi::c_void;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    pub fn token_is_elevated() -> windows::core::Result<bool> {
        let mut token = HANDLE::default();
        // SAFETY: GetCurrentProcess returns a pseudo-handle; `token` is a valid out pointer.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)? };
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        // SAFETY: the buffer is a TOKEN_ELEVATION of the size we pass.
        let result = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut TOKEN_ELEVATION as *mut c_void),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            )
        };
        // SAFETY: `token` was opened above and is closed exactly once.
        let _ = unsafe { CloseHandle(token) };
        result?;
        Ok(elevation.TokenIsElevated != 0)
    }

    fn read_hklm_string(value: PCWSTR) -> Option<String> {
        let key = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
        let mut buf = [0u16; 256];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: buffer and size describe a valid, writable u16 array.
        let rc = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key,
                value,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut c_void),
                Some(&mut size),
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }
        let len = (size as usize / 2).saturating_sub(1).min(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    pub fn os_info() -> OsInfo {
        OsInfo {
            product: read_hklm_string(w!("ProductName")),
            display_version: read_hklm_string(w!("DisplayVersion")),
            build: read_hklm_string(w!("CurrentBuild")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    struct Fixed(bool);
    impl ElevationDetector for Fixed {
        fn is_elevated(&self) -> bool {
            self.0
        }
    }

    #[test]
    fn reduced_mode_lists_features() {
        let info = system_info(&Fixed(false), Path::new(r"C:\data"));
        assert!(!info.elevated);
        assert!(!info.reduced_features.is_empty());
    }

    #[test]
    fn elevated_mode_has_no_reduced_features() {
        let info = system_info(&Fixed(true), Path::new(r"C:\data"));
        assert!(info.elevated);
        assert!(info.reduced_features.is_empty());
    }

    #[test]
    fn real_detector_and_os_info_do_not_fail() {
        let _ = WindowsElevation.is_elevated();
        let os = super::win::os_info();
        assert!(os.build.is_some(), "CurrentBuild should be readable without admin");
    }
}
