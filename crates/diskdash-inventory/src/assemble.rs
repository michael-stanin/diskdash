//! Turn a raw query into the drive list: map volumes to partitions and
//! physical disks, apply the scope filter, classify media, attach health.

use std::collections::BTreeSet;

use crate::classify::{self, Exclusion};
use crate::health;
use crate::model::{Drive, ExcludedCounts, HealthStatus, MediaClass, PhysicalDisk};
use crate::raw::{drive_type, RawInventory, RawReliability, RawVolume};
use crate::smartctl::{normalize_serial, SmartData};

/// Inputs beyond the raw query.
#[derive(Debug, Default)]
pub struct HealthInputs<'a> {
    pub elevated: bool,
    pub reliability: &'a [RawReliability],
    pub smart: &'a [SmartData],
    /// Drive letter holding `%WINDIR%`, e.g. "C".
    pub windows_letter: Option<String>,
}

/// `\\?\Volume{8F3BED5A-…}\` → `8f3bed5a-…`
pub fn volume_id(path: &str) -> String {
    let s = path.trim_end_matches('\\');
    let inner = s.rsplit_once('{').map(|(_, r)| r.trim_end_matches('}')).unwrap_or(s);
    inner.to_ascii_lowercase()
}

fn same_path(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\').eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

fn is_letter_root(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() == 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\'
}

pub fn assemble(raw: &RawInventory, inputs: &HealthInputs<'_>) -> (Vec<Drive>, ExcludedCounts) {
    let mut drives = Vec::new();
    let mut excluded = ExcludedCounts {
        network: raw.logical.iter().filter(|l| l.drive_type == drive_type::REMOTE).count() as u32,
        ..ExcludedCounts::default()
    };

    for v in &raw.volumes {
        let parts: Vec<_> = raw
            .partitions
            .iter()
            .filter(|p| p.access_paths.iter().flatten().any(|ap| same_path(ap, &v.path)))
            .collect();

        let mut letters = BTreeSet::new();
        if let Some(l) = v.drive_letter.as_deref().filter(|l| !l.is_empty()) {
            letters.insert(l.to_ascii_uppercase());
        }
        let mut mount_paths = BTreeSet::new();
        for p in &parts {
            if let Some(l) = p.drive_letter.as_deref().filter(|l| !l.is_empty()) {
                letters.insert(l.to_ascii_uppercase());
            }
            for ap in p.access_paths.iter().flatten() {
                if !ap.starts_with(r"\\?\") && !is_letter_root(ap) {
                    mount_paths.insert(ap.clone());
                }
            }
        }
        // Hidden system volumes (EFI, Recovery…) have neither: not shown, not counted.
        if letters.is_empty() && mount_paths.is_empty() {
            continue;
        }

        let disk_numbers: BTreeSet<i32> = parts.iter().map(|p| p.disk_number).collect();
        let buses: Vec<i32> = disk_numbers
            .iter()
            .filter_map(|n| {
                raw.disks
                    .iter()
                    .find(|d| d.number == *n)
                    .map(|d| d.bus_type)
                    .or_else(|| raw.physical.iter().find(|p| p.device_id == n.to_string()).map(|p| p.bus_type))
            })
            .collect();

        if let Some(ex) = classify::exclusion(v.drive_type, &buses) {
            match ex {
                Exclusion::Usb => excluded.usb += 1,
                Exclusion::Removable => excluded.removable += 1,
                Exclusion::Optical => excluded.optical += 1,
                Exclusion::Network => excluded.network += 1,
                Exclusion::Virtual => excluded.virtual_ += 1,
            }
            continue;
        }

        let physical_disks: Vec<PhysicalDisk> =
            disk_numbers.iter().filter_map(|n| physical_disk(raw, *n, inputs)).collect();
        if disk_numbers.is_empty() || physical_disks.is_empty() {
            tracing::warn!(volume = %v.path, "volume could not be mapped to a physical disk");
        }

        let is_boot = parts.iter().any(|p| p.is_boot);
        let is_system_volume = is_boot
            || inputs.windows_letter.as_ref().is_some_and(|w| letters.iter().any(|l| l.eq_ignore_ascii_case(w)));
        drives.push(drive(v, letters, mount_paths, physical_disks, is_system_volume));
    }

    drives.sort_by(|a, b| {
        let key = |d: &Drive| d.letters.first().cloned().unwrap_or_else(|| format!("~{}", d.label));
        key(a).cmp(&key(b))
    });
    (drives, excluded)
}

fn physical_disk(raw: &RawInventory, number: i32, inputs: &HealthInputs<'_>) -> Option<PhysicalDisk> {
    let pd = raw.physical.iter().find(|p| p.device_id == number.to_string());
    let disk = raw.disks.iter().find(|d| d.number == number);
    if pd.is_none() && disk.is_none() {
        return None;
    }
    let bus_type = pd.map(|p| p.bus_type).or(disk.map(|d| d.bus_type)).unwrap_or(0);
    let media_type = pd.map(|p| p.media_type).unwrap_or(0);
    let spindle = pd.map(|p| p.spindle_speed).unwrap_or(crate::raw::SPINDLE_UNKNOWN);
    let media = classify::classify(bus_type, media_type, spindle);
    let serial = pd
        .and_then(|p| p.serial_number.clone())
        .or_else(|| disk.and_then(|d| d.serial_number.clone()))
        .map(|s| s.trim().trim_end_matches('.').to_string())
        .filter(|s| !s.is_empty());
    let model = pd
        .and_then(|p| p.friendly_name.clone())
        .or_else(|| disk.and_then(|d| d.friendly_name.clone()))
        .unwrap_or_else(|| format!("Disk {number}"));

    let reliability = inputs.reliability.iter().find(|r| r.device_id == number.to_string());
    let smart = serial.as_ref().and_then(|s| {
        let want = normalize_serial(s);
        inputs.smart.iter().find(|d| d.serial.as_deref().map(normalize_serial).as_deref() == Some(want.as_str()))
    });
    let health = health::build(media, pd.map(|p| p.health_status).unwrap_or(5), inputs.elevated, reliability, smart);

    Some(PhysicalDisk {
        number,
        model: model.trim().to_string(),
        serial,
        bus_type: crate::raw::bus::name(bus_type).to_string(),
        media,
        spindle_rpm: classify::spindle_rpm(spindle).filter(|_| media == MediaClass::HDD),
        size: pd.map(|p| p.size).unwrap_or(0),
        is_system: disk.is_some_and(|d| d.is_system || d.is_boot),
        health,
    })
}

/// HDD wins when a volume spans disks of different kinds (scheduling worst case).
fn volume_media(disks: &[PhysicalDisk]) -> MediaClass {
    let has = |m: MediaClass| disks.iter().any(|d| d.media == m);
    if has(MediaClass::HDD) {
        MediaClass::HDD
    } else if disks.is_empty() || has(MediaClass::UNKNOWN) {
        MediaClass::UNKNOWN
    } else if has(MediaClass::SATA_SSD) {
        MediaClass::SATA_SSD
    } else {
        MediaClass::NVME_SSD
    }
}

fn drive(
    v: &RawVolume,
    letters: BTreeSet<String>,
    mount_paths: BTreeSet<String>,
    physical_disks: Vec<PhysicalDisk>,
    is_system_volume: bool,
) -> Drive {
    let capacity = v.size;
    let free = v.size_remaining.min(capacity);
    let used = capacity - free;
    let health = physical_disks.iter().map(|d| d.health.overall).max().unwrap_or(HealthStatus::Unknown);
    Drive {
        id: volume_id(&v.path),
        volume_path: v.path.clone(),
        letters: letters.into_iter().collect(),
        mount_paths: mount_paths.into_iter().collect(),
        label: v.file_system_label.clone().unwrap_or_default(),
        filesystem: v.file_system.clone().unwrap_or_default(),
        serial: None,
        cluster_size: v.allocation_unit_size,
        capacity,
        free,
        used,
        used_fraction: if capacity > 0 { used as f64 / capacity as f64 } else { 0.0 },
        media: volume_media(&physical_disks),
        physical_disks,
        health,
        is_system_volume,
        low_space: false,
        low_space_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::{RawPartition, RawPhysicalDisk};

    fn this_machine() -> RawInventory {
        RawInventory::from_json(include_str!("../tests/fixtures/this_machine.json")).unwrap()
    }

    fn inputs() -> HealthInputs<'static> {
        HealthInputs { elevated: false, reliability: &[], smart: &[], windows_letter: Some("C".into()) }
    }

    #[test]
    fn this_machine_shows_c_d_g_only() {
        let (drives, excluded) = assemble(&this_machine(), &inputs());
        let letters: Vec<_> = drives.iter().map(|d| d.letters.join(",")).collect();
        assert_eq!(letters, vec!["C", "D", "G"]);
        assert_eq!(excluded.optical, 1, "E: is optical");
        assert_eq!(excluded.total(), 1, "hidden EFI/Recovery/virtual volumes are not counted");

        let c = &drives[0];
        assert_eq!(c.media, MediaClass::NVME_SSD);
        assert!(c.is_system_volume);
        assert_eq!(c.physical_disks[0].model, "Lexar 512GB SSD");
        assert_eq!(c.physical_disks[0].bus_type, "NVMe");
        assert_eq!(c.free + c.used, c.capacity);
        assert_eq!(c.cluster_size, 4096);
        assert_eq!(c.id, "8f3bed5a-2851-45b5-aa21-90a47523de8e");

        let d = &drives[1];
        assert_eq!(d.media, MediaClass::HDD);
        assert_eq!(d.physical_disks[0].spindle_rpm, None, "unknown spindle speed stays unknown");
        assert!(!d.is_system_volume);

        let g = &drives[2];
        assert_eq!(g.media, MediaClass::SATA_SSD);
        assert_eq!(g.physical_disks[0].serial.as_deref(), Some("S000TESTSERIAL2"));
    }

    #[test]
    fn reduced_mode_health_on_this_machine() {
        let (drives, _) = assemble(&this_machine(), &inputs());
        for d in &drives {
            assert_eq!(d.health, HealthStatus::Healthy);
            let h = &d.physical_disks[0].health;
            assert_eq!(h.temperature_c.reason.as_deref(), Some(health::NEEDS_ADMIN));
        }
    }

    #[test]
    fn usb_stick_and_network_drive_are_excluded() {
        let mut raw = this_machine();
        raw.volumes.push(RawVolume {
            path: r"\\?\Volume{aaaa}\".into(),
            drive_letter: Some("F".into()),
            drive_type: drive_type::REMOVABLE,
            size: 32 << 30,
            size_remaining: 1 << 30,
            ..RawVolume::default()
        });
        raw.partitions.push(RawPartition {
            disk_number: 9,
            drive_letter: Some("F".into()),
            access_paths: vec![Some("F:\\".into()), Some(r"\\?\Volume{aaaa}\".into())],
            ..RawPartition::default()
        });
        raw.physical.push(RawPhysicalDisk {
            device_id: "9".into(),
            bus_type: crate::raw::bus::USB,
            ..Default::default()
        });
        raw.logical.push(crate::raw::RawLogical { device_id: "Z:".into(), drive_type: drive_type::REMOTE });

        let (drives, excluded) = assemble(&raw, &inputs());
        assert!(drives.iter().all(|d| !d.letters.contains(&"F".to_string())));
        assert_eq!((excluded.usb, excluded.network, excluded.optical), (1, 1, 1));
    }

    #[test]
    fn unmapped_fixed_volume_is_kept_as_unknown() {
        let raw = RawInventory {
            volumes: vec![RawVolume {
                path: r"\\?\Volume{bbbb}\".into(),
                drive_letter: Some("X".into()),
                drive_type: drive_type::FIXED,
                size: 100,
                size_remaining: 40,
                ..RawVolume::default()
            }],
            ..RawInventory::default()
        };
        let (drives, _) = assemble(&raw, &inputs());
        assert_eq!(drives.len(), 1);
        assert!(drives[0].physical_disks.is_empty());
        assert_eq!(drives[0].media, MediaClass::UNKNOWN);
        assert_eq!(drives[0].health, HealthStatus::Unknown);
    }

    #[test]
    fn folder_mounted_volume_is_listed() {
        let raw = RawInventory {
            volumes: vec![RawVolume {
                path: r"\\?\Volume{cccc}\".into(),
                drive_type: drive_type::FIXED,
                size: 100,
                size_remaining: 10,
                ..RawVolume::default()
            }],
            partitions: vec![RawPartition {
                disk_number: 0,
                access_paths: vec![Some(r"C:\Mount\Data\".into()), Some(r"\\?\Volume{cccc}\".into())],
                ..RawPartition::default()
            }],
            ..RawInventory::default()
        };
        let (drives, _) = assemble(&raw, &inputs());
        assert_eq!(drives[0].letters, Vec::<String>::new());
        assert_eq!(drives[0].mount_paths, vec![r"C:\Mount\Data\".to_string()]);
    }

    #[test]
    fn ids() {
        assert_eq!(
            volume_id(r"\\?\Volume{8F3BED5A-2851-45B5-AA21-90A47523DE8E}\"),
            "8f3bed5a-2851-45b5-aa21-90a47523de8e"
        );
    }
}
