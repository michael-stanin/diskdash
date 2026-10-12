//! Native fallback (no PowerShell): drive letters, sizes, filesystem, bus type
//! and seek penalty via Win32. Also supplies volume serial numbers.
//! Read-only: devices are opened with zero access rights, which is enough for
//! the query IOCTLs used here.

use std::ffi::c_void;
use std::mem::size_of;

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
    GetVolumeNameForVolumeMountPointW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::{
    PropertyStandardQuery, StorageDeviceProperty, StorageDeviceSeekPenaltyProperty, DEVICE_SEEK_PENALTY_DESCRIPTOR,
    IOCTL_STORAGE_GET_DEVICE_NUMBER, IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR, STORAGE_DEVICE_NUMBER,
    STORAGE_PROPERTY_QUERY,
};
use windows::Win32::System::IO::DeviceIoControl;

use crate::raw::{
    drive_type, media, RawInventory, RawLogical, RawPartition, RawPhysicalDisk, RawVolume, SPINDLE_UNKNOWN,
};

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Volume serial number for a root like `C:\` or `\\?\Volume{…}\`.
pub fn volume_serial(root: &str) -> Option<u32> {
    let mut serial = 0u32;
    // SAFETY: valid NUL-terminated path; out pointer is a local.
    unsafe { GetVolumeInformationW(&HSTRING::from(root), None, Some(&mut serial), None, None, None) }.ok()?;
    Some(serial)
}

struct Device(HANDLE);

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: handle came from CreateFileW and is closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Open `\\.\C:` with no access rights (query-only IOCTLs).
fn open_device(letter: char) -> Option<Device> {
    let path = HSTRING::from(format!(r"\\.\{letter}:"));
    // SAFETY: zero access, OPEN_EXISTING; never creates or modifies anything.
    let h = unsafe {
        CreateFileW(
            &path,
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .ok()?;
    Some(Device(h))
}

fn ioctl<T: Default>(dev: &Device, code: u32, input: Option<&STORAGE_PROPERTY_QUERY>) -> Option<T> {
    let mut out = T::default();
    let mut returned = 0u32;
    let (in_ptr, in_len) = match input {
        Some(q) => (Some(q as *const _ as *const c_void), size_of::<STORAGE_PROPERTY_QUERY>() as u32),
        None => (None, 0),
    };
    // SAFETY: buffers are sized for the structures passed.
    unsafe {
        DeviceIoControl(
            dev.0,
            code,
            in_ptr,
            in_len,
            Some(&mut out as *mut T as *mut c_void),
            size_of::<T>() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    Some(out)
}

fn query(prop: windows::Win32::System::Ioctl::STORAGE_PROPERTY_ID) -> STORAGE_PROPERTY_QUERY {
    STORAGE_PROPERTY_QUERY { PropertyId: prop, QueryType: PropertyStandardQuery, AdditionalParameters: [0] }
}

/// Drive letters and their types from local mapping state only. Calling
/// `GetDriveTypeW` on a mapped network letter does not contact the share.
pub fn drive_types() -> Vec<RawLogical> {
    let mut buf = [0u16; 512];
    // SAFETY: buffer is writable and its length is passed.
    let n = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) } as usize;
    buf[..n.min(buf.len())]
        .split(|&c| c == 0)
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            let root = String::from_utf16_lossy(s);
            let letter = root.chars().next()?;
            // SAFETY: valid NUL-terminated root path.
            let dtype = unsafe { GetDriveTypeW(&HSTRING::from(root.as_str())) } as i32;
            Some(RawLogical { device_id: format!("{letter}:"), drive_type: dtype })
        })
        .collect()
}

/// Whether a letter of this type may be measured (size/free/label/IOCTLs).
/// Network shares are never contacted; optical and RAM disks are out of scope.
pub fn may_measure(dtype: i32) -> bool {
    matches!(dtype, drive_type::FIXED | drive_type::REMOVABLE)
}

/// Collect a [`RawInventory`] without PowerShell. Network drives are counted
/// from their drive type only; nothing is called that would reach the share.
pub fn collect() -> RawInventory {
    let mut buf = [0u16; 512];
    // SAFETY: buffer is writable and its length is passed.
    let n = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) } as usize;
    let roots: Vec<String> =
        buf[..n.min(buf.len())].split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect();

    let mut raw = RawInventory::default();
    for root in roots {
        let Some(letter) = root.chars().next() else { continue };
        let root_w = HSTRING::from(root.as_str());
        // SAFETY: valid NUL-terminated root path.
        let dtype = unsafe { GetDriveTypeW(&root_w) } as i32;
        raw.logical.push(RawLogical { device_id: format!("{letter}:"), drive_type: dtype });
        if !may_measure(dtype) {
            // Never touch network shares; optical/RAM disks need no measurement.
            let path = format!(r"\\?\Volume{{native-{letter}}}\");
            raw.volumes.push(RawVolume {
                path,
                drive_letter: Some(letter.to_string()),
                drive_type: dtype,
                ..Default::default()
            });
            continue;
        }

        let mut guid = [0u16; 64];
        // SAFETY: buffer is writable.
        let path = match unsafe { GetVolumeNameForVolumeMountPointW(&root_w, &mut guid) } {
            Ok(()) => wide_to_string(&guid),
            Err(_) => format!(r"\\?\Volume{{native-{letter}}}\"),
        };

        let (mut total, mut free) = (0u64, 0u64);
        // SAFETY: out pointers are locals.
        let _ = unsafe { GetDiskFreeSpaceExW(&root_w, None, Some(&mut total), Some(&mut free)) };
        let (mut spc, mut bps) = (0u32, 0u32);
        // SAFETY: out pointers are locals.
        let _ = unsafe { GetDiskFreeSpaceW(&root_w, Some(&mut spc), Some(&mut bps), None, None) };
        let mut label = [0u16; 261];
        let mut fs = [0u16; 64];
        // SAFETY: buffers are writable with their lengths implied by the slices.
        let _ = unsafe { GetVolumeInformationW(&root_w, Some(&mut label), None, None, None, Some(&mut fs)) };

        raw.volumes.push(RawVolume {
            path: path.clone(),
            drive_letter: Some(letter.to_string()),
            file_system_label: Some(wide_to_string(&label)),
            file_system: Some(wide_to_string(&fs)),
            drive_type: dtype,
            size: total,
            size_remaining: free,
            allocation_unit_size: u64::from(spc) * u64::from(bps),
            health_status: 0,
        });

        let Some(dev) = open_device(letter) else { continue };
        let Some(number) = ioctl::<STORAGE_DEVICE_NUMBER>(&dev, IOCTL_STORAGE_GET_DEVICE_NUMBER, None) else {
            continue;
        };
        let disk = number.DeviceNumber as i32;
        raw.partitions.push(RawPartition {
            disk_number: disk,
            drive_letter: Some(letter.to_string()),
            access_paths: vec![Some(root.clone()), Some(path)],
            ..Default::default()
        });
        if raw.physical.iter().any(|p| p.device_id == disk.to_string()) {
            continue;
        }
        let descriptor =
            ioctl::<STORAGE_DEVICE_DESCRIPTOR>(&dev, IOCTL_STORAGE_QUERY_PROPERTY, Some(&query(StorageDeviceProperty)));
        let seek = ioctl::<DEVICE_SEEK_PENALTY_DESCRIPTOR>(
            &dev,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query(StorageDeviceSeekPenaltyProperty)),
        );
        let media_type = match seek.map(|s| s.IncursSeekPenalty.as_bool()) {
            Some(true) => media::HDD,
            Some(false) => media::SSD,
            None => media::UNSPECIFIED,
        };
        raw.physical.push(RawPhysicalDisk {
            device_id: disk.to_string(),
            friendly_name: Some(format!("Disk {disk}")),
            serial_number: None,
            media_type,
            bus_type: descriptor.map(|d| d.BusType.0).unwrap_or(0),
            spindle_speed: SPINDLE_UNKNOWN,
            size: 0,
            health_status: 5,
            operational_status: Vec::new(),
        });
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_collect_finds_the_windows_drive() {
        let raw = collect();
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let letter = windir.chars().next().unwrap().to_string();
        let v = raw.volumes.iter().find(|v| v.drive_letter.as_deref() == Some(&letter)).expect("windows volume");
        assert!(v.size > 0 && v.size_remaining <= v.size);
        assert_eq!(v.file_system.as_deref(), Some("NTFS"));
        assert!(v.path.starts_with(r"\\?\Volume{"));
        assert!(raw.physical.iter().any(|p| p.bus_type != 0), "bus type via IOCTL");
    }

    #[test]
    fn network_and_optical_letters_are_never_measured() {
        assert!(!may_measure(drive_type::REMOTE));
        assert!(!may_measure(drive_type::CDROM));
        assert!(!may_measure(drive_type::RAMDISK));
        assert!(may_measure(drive_type::FIXED));
    }

    #[test]
    fn inventory_script_does_not_use_wmi_logical_disks() {
        // Win32_LogicalDisk may query mapped shares; letters are typed natively instead.
        assert!(!crate::INVENTORY_SCRIPT.contains("Win32_LogicalDisk"));
        assert!(!crate::INVENTORY_SCRIPT.contains("Get-PSDrive"));
    }

    #[test]
    fn drive_types_include_the_windows_drive_as_fixed() {
        let types = drive_types();
        assert!(types.iter().any(|t| t.device_id == "C:" && t.drive_type == drive_type::FIXED));
    }

    #[test]
    fn serial_of_windows_volume() {
        assert!(volume_serial(r"C:\").is_some());
    }

    #[test]
    fn strings() {
        assert_eq!(wide_to_string(&[0x43, 0x3a, 0, 0x58]), "C:");
    }
}
