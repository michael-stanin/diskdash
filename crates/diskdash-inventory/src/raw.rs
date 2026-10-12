//! Raw shapes returned by the inventory query (PowerShell over CIM, or the
//! native fallback). Numeric enum values follow the Windows storage API.

use serde::Deserialize;

/// `STORAGE_BUS_TYPE` values.
pub mod bus {
    pub const UNKNOWN: i32 = 0;
    pub const SCSI: i32 = 1;
    pub const ATAPI: i32 = 2;
    pub const ATA: i32 = 3;
    pub const IEEE1394: i32 = 4;
    pub const SSA: i32 = 5;
    pub const FIBRE: i32 = 6;
    pub const USB: i32 = 7;
    pub const RAID: i32 = 8;
    pub const ISCSI: i32 = 9;
    pub const SAS: i32 = 10;
    pub const SATA: i32 = 11;
    pub const SD: i32 = 12;
    pub const MMC: i32 = 13;
    pub const VIRTUAL: i32 = 14;
    pub const FILE_BACKED_VIRTUAL: i32 = 15;
    pub const SPACES: i32 = 16;
    pub const NVME: i32 = 17;
    pub const SCM: i32 = 18;
    pub const UFS: i32 = 19;

    pub fn name(b: i32) -> &'static str {
        match b {
            SCSI => "SCSI",
            ATAPI => "ATAPI",
            ATA => "ATA",
            IEEE1394 => "1394",
            SSA => "SSA",
            FIBRE => "Fibre Channel",
            USB => "USB",
            RAID => "RAID",
            ISCSI => "iSCSI",
            SAS => "SAS",
            SATA => "SATA",
            SD => "SD",
            MMC => "MMC",
            VIRTUAL => "Virtual",
            FILE_BACKED_VIRTUAL => "File Backed Virtual",
            SPACES => "Storage Spaces",
            NVME => "NVMe",
            SCM => "SCM",
            UFS => "UFS",
            _ => "Unknown",
        }
    }
}

/// `MSFT_PhysicalDisk.MediaType`.
pub mod media {
    pub const UNSPECIFIED: i32 = 0;
    pub const HDD: i32 = 3;
    pub const SSD: i32 = 4;
    pub const SCM: i32 = 5;
}

/// `MSFT_Volume.DriveType` / `GetDriveTypeW`.
pub mod drive_type {
    pub const REMOVABLE: i32 = 2;
    pub const FIXED: i32 = 3;
    pub const REMOTE: i32 = 4;
    pub const CDROM: i32 = 5;
    pub const RAMDISK: i32 = 6;
}

/// `SpindleSpeed` value meaning "unknown".
pub const SPINDLE_UNKNOWN: u32 = u32::MAX;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RawVolume {
    pub path: String,
    pub drive_letter: Option<String>,
    #[serde(default)]
    pub file_system_label: Option<String>,
    #[serde(default)]
    pub file_system: Option<String>,
    pub drive_type: i32,
    pub size: u64,
    pub size_remaining: u64,
    #[serde(default)]
    pub allocation_unit_size: u64,
    #[serde(default)]
    pub health_status: i32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RawPartition {
    pub disk_number: i32,
    pub drive_letter: Option<String>,
    #[serde(default)]
    pub access_paths: Vec<Option<String>>,
    #[serde(default)]
    pub is_system: bool,
    #[serde(default)]
    pub is_boot: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RawPhysicalDisk {
    pub device_id: String,
    pub friendly_name: Option<String>,
    pub serial_number: Option<String>,
    pub media_type: i32,
    pub bus_type: i32,
    pub spindle_speed: u32,
    pub size: u64,
    #[serde(default)]
    pub health_status: i32,
    #[serde(default)]
    pub operational_status: Vec<i32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RawDisk {
    pub number: i32,
    pub bus_type: i32,
    #[serde(default)]
    pub is_boot: bool,
    #[serde(default)]
    pub is_system: bool,
    pub serial_number: Option<String>,
    pub friendly_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawLogical {
    #[serde(rename = "DeviceID")]
    pub device_id: String,
    #[serde(rename = "DriveType")]
    pub drive_type: i32,
}

/// Everything one inventory query returns.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawInventory {
    #[serde(default)]
    pub volumes: Vec<RawVolume>,
    #[serde(default)]
    pub partitions: Vec<RawPartition>,
    #[serde(default)]
    pub physical: Vec<RawPhysicalDisk>,
    #[serde(default)]
    pub disks: Vec<RawDisk>,
    #[serde(default)]
    pub logical: Vec<RawLogical>,
}

impl RawInventory {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Values from `Get-StorageReliabilityCounter` (elevated only).
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct RawReliability {
    pub device_id: String,
    pub temperature: Option<u32>,
    pub wear: Option<u32>,
    pub power_on_hours: Option<u64>,
    pub read_errors_total: Option<u64>,
    pub write_errors_total: Option<u64>,
}
