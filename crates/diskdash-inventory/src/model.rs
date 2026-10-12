//! Public inventory types (serialised to the API, exported to TypeScript).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[allow(non_camel_case_types)]
#[ts(export)]
pub enum MediaClass {
    HDD,
    SATA_SSD,
    NVME_SSD,
    UNKNOWN,
}

impl MediaClass {
    pub fn is_ssd(self) -> bool {
        matches!(self, MediaClass::SATA_SSD | MediaClass::NVME_SSD)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum HealthStatus {
    Healthy,
    Unknown,
    Warning,
    Unhealthy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HealthSource {
    StorageCmdlets,
    ReliabilityCounters,
    Smartctl,
    Unavailable,
}

/// One health value with where it came from, or why it is missing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HealthField {
    #[ts(type = "number | null")]
    pub value: Option<f64>,
    pub source: HealthSource,
    pub reason: Option<String>,
}

impl HealthField {
    pub fn known(value: f64, source: HealthSource) -> Self {
        Self { value: Some(value), source, reason: None }
    }

    pub fn unavailable(reason: &str) -> Self {
        Self { value: None, source: HealthSource::Unavailable, reason: Some(reason.to_string()) }
    }

    pub fn or_unavailable(value: Option<f64>, source: HealthSource, reason: &str) -> Self {
        match value {
            Some(v) => Self::known(v, source),
            None => Self::unavailable(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiskHealth {
    pub overall: HealthStatus,
    /// Why `overall` is what it is (e.g. "8 reallocated sectors").
    pub reasons: Vec<String>,
    pub cmdlet_status: HealthStatus,
    pub temperature_c: HealthField,
    pub power_on_hours: HealthField,
    pub wear_percent: HealthField,
    pub read_errors_total: HealthField,
    pub write_errors_total: HealthField,
    pub reallocated_sectors: HealthField,
    pub data_written_bytes: HealthField,
    pub smart_passed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PhysicalDisk {
    pub number: i32,
    pub model: String,
    pub serial: Option<String>,
    pub bus_type: String,
    pub media: MediaClass,
    /// RPM when known (HDD); 0 for non-rotational.
    pub spindle_rpm: Option<u32>,
    #[ts(type = "number")]
    pub size: u64,
    pub is_system: bool,
    pub health: DiskHealth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LowSpaceReason {
    Percent,
    Floor,
    Both,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Drive {
    /// Short URL-safe id: the volume GUID (lowercase, no braces).
    pub id: String,
    /// `\\?\Volume{…}\`
    pub volume_path: String,
    pub letters: Vec<String>,
    pub mount_paths: Vec<String>,
    pub label: String,
    pub filesystem: String,
    #[ts(type = "number | null")]
    pub serial: Option<u32>,
    #[ts(type = "number")]
    pub cluster_size: u64,
    #[ts(type = "number")]
    pub capacity: u64,
    #[ts(type = "number")]
    pub free: u64,
    #[ts(type = "number")]
    pub used: u64,
    pub used_fraction: f64,
    /// Media class for scheduling/badges (HDD wins if a volume spans both).
    pub media: MediaClass,
    pub physical_disks: Vec<PhysicalDisk>,
    pub health: HealthStatus,
    /// Holds `%WINDIR%` (hard links are detected here by default).
    pub is_system_volume: bool,
    pub low_space: bool,
    pub low_space_reason: Option<LowSpaceReason>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExcludedCounts {
    pub usb: u32,
    pub removable: u32,
    pub optical: u32,
    pub network: u32,
    #[serde(rename = "virtual")]
    #[ts(rename = "virtual")]
    pub virtual_: u32,
}

impl ExcludedCounts {
    pub fn total(&self) -> u32 {
        self.usb + self.removable + self.optical + self.network + self.virtual_
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TrimState {
    pub ntfs: Option<bool>,
    pub refs: Option<bool>,
    pub reason: Option<String>,
}

/// Where elevated health data came from in this refresh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HealthSources {
    pub elevated: bool,
    pub smartctl: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Inventory {
    pub drives: Vec<Drive>,
    pub excluded: ExcludedCounts,
    pub trim: TrimState,
    pub sources: HealthSources,
    /// Collected via the native fallback because PowerShell failed.
    pub fallback: bool,
}
