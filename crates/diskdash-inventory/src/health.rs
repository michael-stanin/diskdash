//! Disk health: tiered sources, "worst of" aggregation with reasons, TRIM.

use crate::model::{DiskHealth, HealthField, HealthSource, HealthStatus, MediaClass, TrimState};
use crate::raw::RawReliability;
use crate::smartctl::SmartData;

pub const HDD_TEMP_WARN_C: f64 = 60.0;
pub const SSD_TEMP_WARN_C: f64 = 70.0;
pub const WEAR_WARN_PERCENT: f64 = 90.0;

pub const NEEDS_ADMIN: &str = "requires administrator";
pub const NOT_REPORTED: &str = "not reported by this drive";
pub const NOT_APPLICABLE_HDD: &str = "not applicable to hard disks";
pub const NOT_APPLICABLE_SSD: &str = "not applicable to SSDs";
pub const NEEDS_SMARTCTL: &str = "requires smartctl (optional) and administrator";

/// `MSFT_PhysicalDisk.HealthStatus`: 0 healthy, 1 warning, 2 unhealthy, 5 unknown.
pub fn cmdlet_status(code: i32) -> HealthStatus {
    match code {
        0 => HealthStatus::Healthy,
        1 => HealthStatus::Warning,
        2 => HealthStatus::Unhealthy,
        _ => HealthStatus::Unknown,
    }
}

/// Build the health record for one disk from whatever sources are available.
pub fn build(
    media: MediaClass,
    cmdlet_code: i32,
    elevated: bool,
    reliability: Option<&RawReliability>,
    smart: Option<&SmartData>,
) -> DiskHealth {
    let cmdlet = cmdlet_status(cmdlet_code);
    let rel_missing = if elevated { NOT_REPORTED } else { NEEDS_ADMIN };
    let rc = HealthSource::ReliabilityCounters;
    let sm = HealthSource::Smartctl;

    let pick = |smart_v: Option<f64>, rel_v: Option<f64>| -> HealthField {
        match (smart_v, rel_v) {
            (Some(v), _) => HealthField::known(v, sm),
            (None, Some(v)) => HealthField::known(v, rc),
            (None, None) => HealthField::unavailable(rel_missing),
        }
    };

    let temperature_c = pick(
        smart.and_then(|s| s.temperature_c.map(f64::from)),
        reliability.and_then(|r| r.temperature).filter(|t| *t > 0).map(f64::from),
    );
    let power_on_hours = pick(
        smart.and_then(|s| s.power_on_hours.map(|h| h as f64)),
        reliability.and_then(|r| r.power_on_hours).map(|h| h as f64),
    );
    let wear_percent = if media == MediaClass::HDD {
        HealthField::unavailable(NOT_APPLICABLE_HDD)
    } else {
        pick(smart.and_then(|s| s.percentage_used.map(f64::from)), reliability.and_then(|r| r.wear).map(f64::from))
    };
    let read_errors_total =
        HealthField::or_unavailable(reliability.and_then(|r| r.read_errors_total).map(|v| v as f64), rc, rel_missing);
    let write_errors_total =
        HealthField::or_unavailable(reliability.and_then(|r| r.write_errors_total).map(|v| v as f64), rc, rel_missing);
    let smart_missing = if smart.is_some() { NOT_REPORTED } else { NEEDS_SMARTCTL };
    let reallocated_sectors = if media.is_ssd() {
        HealthField::unavailable(NOT_APPLICABLE_SSD)
    } else {
        HealthField::or_unavailable(smart.and_then(|s| s.reallocated_sectors).map(|v| v as f64), sm, smart_missing)
    };
    let data_written_bytes = if media == MediaClass::HDD {
        HealthField::unavailable(NOT_APPLICABLE_HDD)
    } else {
        HealthField::or_unavailable(smart.and_then(|s| s.data_written_bytes).map(|v| v as f64), sm, smart_missing)
    };
    let smart_passed = smart.and_then(|s| s.passed);

    let (overall, reasons) =
        aggregate(media, cmdlet, smart_passed, &reallocated_sectors, &wear_percent, &temperature_c);
    DiskHealth {
        overall,
        reasons,
        cmdlet_status: cmdlet,
        temperature_c,
        power_on_hours,
        wear_percent,
        read_errors_total,
        write_errors_total,
        reallocated_sectors,
        data_written_bytes,
        smart_passed,
    }
}

/// Worst of all inputs, each contributing a human-readable reason.
pub fn aggregate(
    media: MediaClass,
    cmdlet: HealthStatus,
    smart_passed: Option<bool>,
    reallocated: &HealthField,
    wear: &HealthField,
    temperature: &HealthField,
) -> (HealthStatus, Vec<String>) {
    // `None` until some source says something; Unknown is never "raised".
    let mut status: Option<HealthStatus> = None;
    let mut reasons = Vec::new();
    let mut raise = |s: HealthStatus, reason: Option<String>| {
        status = Some(status.map_or(s, |cur| cur.max(s)));
        if let Some(r) = reason {
            reasons.push(r);
        }
    };

    match cmdlet {
        HealthStatus::Unknown => {}
        HealthStatus::Healthy => raise(HealthStatus::Healthy, None),
        HealthStatus::Warning => raise(HealthStatus::Warning, Some("Windows reports a health warning".into())),
        HealthStatus::Unhealthy => raise(HealthStatus::Unhealthy, Some("Windows reports the disk as unhealthy".into())),
    }
    match smart_passed {
        Some(true) => raise(HealthStatus::Healthy, None),
        Some(false) => raise(HealthStatus::Unhealthy, Some("SMART overall assessment: FAILED".into())),
        None => {}
    }
    if let Some(n) = reallocated.value.filter(|n| *n > 0.0) {
        raise(HealthStatus::Warning, Some(format!("{n:.0} reallocated sectors")));
    }
    if let Some(w) = wear.value.filter(|w| *w >= WEAR_WARN_PERCENT) {
        raise(HealthStatus::Warning, Some(format!("{w:.0}% of rated wear used")));
    }
    let limit = if media.is_ssd() { SSD_TEMP_WARN_C } else { HDD_TEMP_WARN_C };
    if let Some(t) = temperature.value.filter(|t| *t > limit) {
        raise(HealthStatus::Warning, Some(format!("temperature {t:.0} °C (above {limit:.0} °C)")));
    }
    (status.unwrap_or(HealthStatus::Unknown), reasons)
}

/// Parse `fsutil behavior query DisableDeleteNotify`.
/// `DisableDeleteNotify = 0` means TRIM is **enabled**.
pub fn parse_trim(output: &str) -> TrimState {
    let mut ntfs = None;
    let mut refs = None;
    for line in output.lines() {
        let line = line.trim();
        let Some((left, right)) = line.split_once('=') else { continue };
        let value = right.split_whitespace().next().and_then(|v| v.parse::<u32>().ok());
        let Some(value) = value else { continue };
        let enabled = Some(value == 0);
        let left = left.trim().to_ascii_lowercase();
        if left.starts_with("ntfs") {
            ntfs = enabled;
        } else if left.starts_with("refs") {
            refs = enabled;
        } else if left.contains("disabledeletenotify") && ntfs.is_none() {
            // Older Windows prints a single unlabelled line for NTFS.
            ntfs = enabled;
        }
    }
    let reason = (ntfs.is_none() && refs.is_none()).then(|| "could not read fsutil output".to_string());
    TrimState { ntfs, refs, reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smart(passed: Option<bool>, realloc: Option<u64>) -> SmartData {
        SmartData { serial: None, passed, reallocated_sectors: realloc, ..SmartData::default() }
    }

    #[test]
    fn reduced_mode_marks_admin_fields_unavailable() {
        let h = build(MediaClass::NVME_SSD, 0, false, None, None);
        assert_eq!(h.overall, HealthStatus::Healthy);
        assert_eq!(h.temperature_c.value, None);
        assert_eq!(h.temperature_c.reason.as_deref(), Some(NEEDS_ADMIN));
        assert_eq!(h.reallocated_sectors.reason.as_deref(), Some(NOT_APPLICABLE_SSD));
    }

    #[test]
    fn elevated_counters_fill_fields() {
        let rel = RawReliability {
            device_id: "2".into(),
            temperature: Some(41),
            wear: Some(3),
            power_on_hours: Some(5120),
            read_errors_total: Some(0),
            write_errors_total: Some(0),
        };
        let h = build(MediaClass::NVME_SSD, 0, true, Some(&rel), None);
        assert_eq!(h.temperature_c, HealthField::known(41.0, HealthSource::ReliabilityCounters));
        assert_eq!(h.power_on_hours.value, Some(5120.0));
        assert_eq!(h.wear_percent.value, Some(3.0));
        assert_eq!(h.overall, HealthStatus::Healthy);
    }

    #[test]
    fn reallocated_sectors_raise_warning_with_reason() {
        let s = smart(Some(true), Some(8));
        let h = build(MediaClass::HDD, 0, true, None, Some(&s));
        assert_eq!(h.overall, HealthStatus::Warning);
        assert!(h.reasons.iter().any(|r| r == "8 reallocated sectors"), "{:?}", h.reasons);
    }

    #[test]
    fn smart_failed_is_unhealthy() {
        let h = build(MediaClass::HDD, 0, true, None, Some(&smart(Some(false), Some(0))));
        assert_eq!(h.overall, HealthStatus::Unhealthy);
    }

    #[test]
    fn thresholds() {
        let hot_hdd = HealthField::known(61.0, HealthSource::ReliabilityCounters);
        let ok = HealthField::unavailable("x");
        let (s, _) = aggregate(MediaClass::HDD, HealthStatus::Healthy, None, &ok, &ok, &hot_hdd);
        assert_eq!(s, HealthStatus::Warning);
        let (s, _) = aggregate(MediaClass::NVME_SSD, HealthStatus::Healthy, None, &ok, &ok, &hot_hdd);
        assert_eq!(s, HealthStatus::Healthy, "61 °C is fine for an SSD");
        let worn = HealthField::known(90.0, HealthSource::Smartctl);
        let (s, r) = aggregate(MediaClass::SATA_SSD, HealthStatus::Healthy, None, &ok, &worn, &ok);
        assert_eq!(s, HealthStatus::Warning);
        assert_eq!(r, vec!["90% of rated wear used"]);
    }

    #[test]
    fn nothing_known_is_unknown() {
        let ok = HealthField::unavailable("x");
        let (s, _) = aggregate(MediaClass::UNKNOWN, HealthStatus::Unknown, None, &ok, &ok, &ok);
        assert_eq!(s, HealthStatus::Unknown);
    }

    #[test]
    fn cmdlet_warning_wins_over_healthy_smart() {
        let ok = HealthField::unavailable("x");
        let (s, r) = aggregate(MediaClass::HDD, HealthStatus::Warning, Some(true), &ok, &ok, &ok);
        assert_eq!(s, HealthStatus::Warning);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn trim_parsing() {
        let enabled = "NTFS DisableDeleteNotify = 0  (Disabled)\nReFS DisableDeleteNotify = 0  (Disabled)\n";
        assert_eq!(parse_trim(enabled), TrimState { ntfs: Some(true), refs: Some(true), reason: None });
        let disabled = "NTFS DisableDeleteNotify = 1  (Enabled)\nReFS DisableDeleteNotify is not currently set\n";
        let t = parse_trim(disabled);
        assert_eq!((t.ntfs, t.refs), (Some(false), None));
        let legacy = "DisableDeleteNotify = 0\n";
        assert_eq!(parse_trim(legacy).ntfs, Some(true));
        let junk = parse_trim("Access is denied.");
        assert_eq!((junk.ntfs, junk.refs), (None, None));
        assert!(junk.reason.is_some());
    }
}
