//! Optional smartctl adapter. Used only if smartctl is already installed, only
//! when elevated, and only with read-only query flags.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::runner::{CommandRunner, RunError};

/// The only arguments ever passed to smartctl. Anything else is refused before
/// a process starts (no self-tests, no settings changes, no writes).
pub const ALLOWED_ARGS: &[&str] = &["--scan-open", "-i", "-H", "-A", "-j"];

const DEFAULT_LOCATION: &str = r"C:\Program Files\smartmontools\bin\smartctl.exe";

#[derive(Debug, thiserror::Error)]
pub enum SmartError {
    #[error("argument `{0}` is not on the smartctl allow-list")]
    Forbidden(String),
    #[error("device path `{0}` is not a smartctl device name")]
    BadDevice(String),
    #[error(transparent)]
    Run(#[from] RunError),
    #[error("cannot parse smartctl output: {0}")]
    Parse(String),
}

/// Find smartctl on PATH or in the default install folder.
pub fn discover() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).map(|p| p.join("smartctl.exe")).find(|p| p.is_file()));
    on_path.or_else(|| Some(PathBuf::from(DEFAULT_LOCATION)).filter(|p| p.is_file()))
}

/// Build argv from the allow-list. Device names (e.g. `/dev/sda`, `/dev/nvme0`)
/// are the only non-flag argument accepted.
pub fn build_args(flags: &[&str], device: Option<&str>) -> Result<Vec<String>, SmartError> {
    let mut args = Vec::new();
    for f in flags {
        if !ALLOWED_ARGS.contains(f) {
            return Err(SmartError::Forbidden((*f).to_string()));
        }
        args.push((*f).to_string());
    }
    if let Some(dev) = device {
        let ok = dev.starts_with("/dev/") && dev[5..].chars().all(|c| c.is_ascii_alphanumeric());
        if !ok {
            return Err(SmartError::BadDevice(dev.to_string()));
        }
        args.push(dev.to_string());
    }
    Ok(args)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SmartData {
    pub serial: Option<String>,
    pub passed: Option<bool>,
    pub temperature_c: Option<u32>,
    pub power_on_hours: Option<u64>,
    pub reallocated_sectors: Option<u64>,
    /// Bytes written (NVMe data units × 512 000, or ATA LBAs written × 512).
    pub data_written_bytes: Option<u64>,
    /// NVMe "percentage used" / SSD wear.
    pub percentage_used: Option<u32>,
}

/// Parse `smartctl -i -H -A -j <dev>` JSON.
pub fn parse_device(json: &str) -> Result<SmartData, SmartError> {
    let v: Value = serde_json::from_str(json).map_err(|e| SmartError::Parse(e.to_string()))?;
    let u64_at = |p: &str| v.pointer(p).and_then(Value::as_u64);
    let mut d = SmartData {
        serial: v.pointer("/serial_number").and_then(Value::as_str).map(str::to_string),
        passed: v.pointer("/smart_status/passed").and_then(Value::as_bool),
        temperature_c: u64_at("/temperature/current").map(|t| t as u32),
        power_on_hours: u64_at("/power_on_time/hours"),
        ..SmartData::default()
    };
    if let Some(nvme) = v.pointer("/nvme_smart_health_information_log") {
        d.data_written_bytes =
            nvme.get("data_units_written").and_then(Value::as_u64).map(|u| u.saturating_mul(512_000));
        d.percentage_used = nvme.get("percentage_used").and_then(Value::as_u64).map(|p| p as u32);
    }
    if let Some(table) = v.pointer("/ata_smart_attributes/table").and_then(Value::as_array) {
        let raw = |id: u64| {
            table
                .iter()
                .find(|a| a.get("id").and_then(Value::as_u64) == Some(id))
                .and_then(|a| a.pointer("/raw/value"))
                .and_then(Value::as_u64)
        };
        d.reallocated_sectors = raw(5);
        // 241 Total_LBAs_Written (512-byte units on most drives).
        if d.data_written_bytes.is_none() {
            d.data_written_bytes = raw(241).map(|l| l.saturating_mul(512));
        }
        // 177 Wear_Leveling_Count (Samsung) / 233 Media_Wearout_Indicator: normalised value counts down from 100.
        if d.percentage_used.is_none() {
            let normalized = |id: u64| {
                table
                    .iter()
                    .find(|a| a.get("id").and_then(Value::as_u64) == Some(id))
                    .and_then(|a| a.get("value"))
                    .and_then(Value::as_u64)
            };
            d.percentage_used = normalized(177).or_else(|| normalized(233)).map(|n| 100u32.saturating_sub(n as u32));
        }
    }
    Ok(d)
}

/// Parse `smartctl --scan-open -j` into device names.
pub fn parse_scan(json: &str) -> Result<Vec<String>, SmartError> {
    let v: Value = serde_json::from_str(json).map_err(|e| SmartError::Parse(e.to_string()))?;
    Ok(v.get("devices")
        .and_then(Value::as_array)
        .map(|devs| devs.iter().filter_map(|d| d.get("name").and_then(Value::as_str).map(str::to_string)).collect())
        .unwrap_or_default())
}

/// Query every device smartctl can open. Errors per device are skipped.
pub fn collect(runner: &dyn CommandRunner, exe: &Path) -> Result<Vec<SmartData>, SmartError> {
    let scan = runner.run(exe, &build_args(&["--scan-open", "-j"], None)?)?;
    let mut out = Vec::new();
    for dev in parse_scan(&scan)? {
        let args = build_args(&["-i", "-H", "-A", "-j"], Some(&dev))?;
        // smartctl uses non-zero exit bits for informational states; the JSON is still valid.
        match runner.run(exe, &args).or_else(|e| match e {
            RunError::Failed { stdout, .. } if !stdout.is_empty() => Ok(stdout),
            other => Err(other),
        }) {
            Ok(json) => match parse_device(&json) {
                Ok(d) => out.push(d),
                Err(e) => tracing::warn!(device = %dev, error = %e, "smartctl output not understood"),
            },
            Err(e) => tracing::warn!(device = %dev, error = %e, "smartctl query failed"),
        }
    }
    Ok(out)
}

/// Normalise serial numbers for matching (whitespace, case, trailing dots).
pub fn normalize_serial(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect::<String>().trim_end_matches('.').to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_list_is_enforced_before_running() {
        assert!(build_args(&["-i", "-j"], Some("/dev/sda")).is_ok());
        for bad in ["-t", "--test=long", "-s", "on", "--set=standby,now", "-X", "--smart=off"] {
            assert!(matches!(build_args(&[bad], None), Err(SmartError::Forbidden(_))), "{bad} must be refused");
        }
        assert!(matches!(build_args(&["-i"], Some("/dev/sda; rm")), Err(SmartError::BadDevice(_))));
        assert!(matches!(build_args(&["-i"], Some("C:\\x")), Err(SmartError::BadDevice(_))));
    }

    #[test]
    fn parses_ata_hdd() {
        let json = r#"{
          "serial_number": "WD-TESTSERIAL01",
          "smart_status": {"passed": true},
          "temperature": {"current": 38},
          "power_on_time": {"hours": 41000},
          "ata_smart_attributes": {"table": [
            {"id": 5, "name": "Reallocated_Sector_Ct", "value": 200, "raw": {"value": 8}},
            {"id": 9, "name": "Power_On_Hours", "value": 44, "raw": {"value": 41000}}
          ]}
        }"#;
        let d = parse_device(json).unwrap();
        assert_eq!(d.passed, Some(true));
        assert_eq!(d.reallocated_sectors, Some(8));
        assert_eq!(d.temperature_c, Some(38));
        assert_eq!(d.power_on_hours, Some(41000));
    }

    #[test]
    fn parses_nvme_and_samsung_sata_ssd() {
        let nvme = r#"{"serial_number":"X","smart_status":{"passed":true},
          "nvme_smart_health_information_log":{"data_units_written":2000000,"percentage_used":4}}"#;
        let d = parse_device(nvme).unwrap();
        assert_eq!(d.data_written_bytes, Some(2_000_000 * 512_000));
        assert_eq!(d.percentage_used, Some(4));

        let sata = r#"{"ata_smart_attributes":{"table":[
          {"id":177,"value":93,"raw":{"value":120}},{"id":241,"value":99,"raw":{"value":1000}}]}}"#;
        let d = parse_device(sata).unwrap();
        assert_eq!(d.percentage_used, Some(7));
        assert_eq!(d.data_written_bytes, Some(512_000));
    }

    #[test]
    fn parses_scan() {
        let json = r#"{"devices":[{"name":"/dev/sda","type":"ata"},{"name":"/dev/nvme0","type":"nvme"}]}"#;
        assert_eq!(parse_scan(json).unwrap(), vec!["/dev/sda", "/dev/nvme0"]);
    }

    #[test]
    fn serial_normalisation() {
        assert_eq!(normalize_serial("     WD-WCC3F5NREL11"), "WD-WCC3F5NREL11");
        assert_eq!(normalize_serial("0000_0002_2109_1724_caf2."), "0000_0002_2109_1724_CAF2");
    }
}
