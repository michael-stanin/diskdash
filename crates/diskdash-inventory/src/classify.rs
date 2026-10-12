//! Scope filter (fixed internal drives only) and media classification.

use crate::model::MediaClass;
use crate::raw::{bus, drive_type, media, SPINDLE_UNKNOWN};

/// Why a volume is not shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusion {
    Usb,
    Removable,
    Optical,
    Network,
    Virtual,
}

/// Bus types that are internal, directly attached storage.
pub fn bus_in_scope(bus_type: i32) -> bool {
    matches!(
        bus_type,
        bus::SCSI | bus::ATA | bus::RAID | bus::SAS | bus::SATA | bus::SPACES | bus::NVME | bus::SCM | bus::UNKNOWN
    )
}

/// Decide whether a volume is in scope, from its drive type and the bus types
/// of the physical disks it sits on.
pub fn exclusion(volume_drive_type: i32, disk_buses: &[i32]) -> Option<Exclusion> {
    match volume_drive_type {
        drive_type::CDROM => return Some(Exclusion::Optical),
        drive_type::REMOTE => return Some(Exclusion::Network),
        drive_type::REMOVABLE => {
            return Some(if disk_buses.contains(&bus::USB) { Exclusion::Usb } else { Exclusion::Removable });
        }
        drive_type::RAMDISK => return Some(Exclusion::Virtual),
        _ => {}
    }
    for &b in disk_buses {
        match b {
            bus::USB | bus::IEEE1394 => return Some(Exclusion::Usb),
            bus::SD | bus::MMC | bus::UFS => return Some(Exclusion::Removable),
            bus::VIRTUAL | bus::FILE_BACKED_VIRTUAL => return Some(Exclusion::Virtual),
            bus::ISCSI | bus::FIBRE => return Some(Exclusion::Network),
            _ => {}
        }
    }
    None
}

/// Media class, rule order from the spec:
/// NVMe bus → NVMe SSD; SSD media or zero spindle speed → SATA SSD;
/// HDD media or a positive spindle speed → HDD; else unknown.
pub fn classify(bus_type: i32, media_type: i32, spindle_speed: u32) -> MediaClass {
    if bus_type == bus::NVME {
        return MediaClass::NVME_SSD;
    }
    if media_type == media::SSD || media_type == media::SCM || spindle_speed == 0 {
        return MediaClass::SATA_SSD;
    }
    if media_type == media::HDD || (spindle_speed > 0 && spindle_speed != SPINDLE_UNKNOWN) {
        return MediaClass::HDD;
    }
    MediaClass::UNKNOWN
}

/// Spindle speed in RPM when it is a real value.
pub fn spindle_rpm(spindle_speed: u32) -> Option<u32> {
    (spindle_speed != SPINDLE_UNKNOWN).then_some(spindle_speed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_table() {
        let cases = [
            (bus::NVME, media::SSD, 0, MediaClass::NVME_SSD),
            (bus::NVME, media::UNSPECIFIED, SPINDLE_UNKNOWN, MediaClass::NVME_SSD),
            (bus::SATA, media::SSD, 0, MediaClass::SATA_SSD),
            (bus::SATA, media::UNSPECIFIED, 0, MediaClass::SATA_SSD),
            (bus::SATA, media::HDD, SPINDLE_UNKNOWN, MediaClass::HDD),
            (bus::SAS, media::UNSPECIFIED, 7200, MediaClass::HDD),
            (bus::SATA, media::UNSPECIFIED, SPINDLE_UNKNOWN, MediaClass::UNKNOWN),
            (bus::RAID, media::SCM, SPINDLE_UNKNOWN, MediaClass::SATA_SSD),
        ];
        for (b, m, s, expected) in cases {
            assert_eq!(classify(b, m, s), expected, "bus {b} media {m} spindle {s}");
        }
    }

    #[test]
    fn scope_filter_table() {
        use drive_type::*;
        assert_eq!(exclusion(FIXED, &[bus::NVME]), None);
        assert_eq!(exclusion(FIXED, &[bus::SATA]), None);
        assert_eq!(exclusion(FIXED, &[bus::RAID, bus::SATA]), None);
        assert_eq!(exclusion(FIXED, &[]), None, "unmapped fixed volume stays in scope");
        assert_eq!(exclusion(CDROM, &[]), Some(Exclusion::Optical));
        assert_eq!(exclusion(REMOTE, &[]), Some(Exclusion::Network));
        assert_eq!(exclusion(REMOVABLE, &[bus::USB]), Some(Exclusion::Usb));
        assert_eq!(exclusion(REMOVABLE, &[bus::SD]), Some(Exclusion::Removable));
        // A USB bridge that presents the drive as fixed is still USB.
        assert_eq!(exclusion(FIXED, &[bus::USB]), Some(Exclusion::Usb));
        assert_eq!(exclusion(FIXED, &[bus::FILE_BACKED_VIRTUAL]), Some(Exclusion::Virtual));
        assert_eq!(exclusion(FIXED, &[bus::ISCSI]), Some(Exclusion::Network));
        assert_eq!(exclusion(RAMDISK, &[]), Some(Exclusion::Virtual));
    }

    #[test]
    fn spindle() {
        assert_eq!(spindle_rpm(SPINDLE_UNKNOWN), None);
        assert_eq!(spindle_rpm(7200), Some(7200));
        assert_eq!(spindle_rpm(0), Some(0));
    }
}
