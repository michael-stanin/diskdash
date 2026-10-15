//! Is NTFS last-access time trustworthy on a volume?
//!
//! `fsutil behavior query disablelastaccess` reports 0 (user enabled), 1 (user
//! disabled), 2 (system managed, disabled; `0x80000002`) or 3 (system managed,
//! enabled; `0x80000003`). Since Windows 10 1803 the system-managed default
//! only keeps last-access times on volumes of 128 GB or less. The output is
//! localised, so only the number is parsed.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const SYSTEM_MANAGED_LIMIT: u64 = 128 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    UserEnabled,
    UserDisabled,
    SystemDisabled,
    SystemEnabled,
}

/// Which timestamp the staleness analysis uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum TimeBasis {
    /// Last-access time (reliable on this volume).
    Atime,
    /// Last-modified time (the fallback).
    Mtime,
}

impl TimeBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            TimeBasis::Atime => "atime",
            TimeBasis::Mtime => "mtime",
        }
    }

    pub fn parse(s: &str) -> Self {
        if s == "atime" {
            TimeBasis::Atime
        } else {
            TimeBasis::Mtime
        }
    }
}

/// Why the basis was chosen (shown in the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum BasisReason {
    Enabled,
    Disabled,
    SystemDisabled,
    SystemEnabledSmallVolume,
    SystemEnabledLargeVolume,
    Unknown,
}

impl BasisReason {
    pub fn as_str(self) -> &'static str {
        match self {
            BasisReason::Enabled => "enabled",
            BasisReason::Disabled => "disabled",
            BasisReason::SystemDisabled => "system-disabled",
            BasisReason::SystemEnabledSmallVolume => "system-enabled-small-volume",
            BasisReason::SystemEnabledLargeVolume => "system-enabled-large-volume",
            BasisReason::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "enabled" => Self::Enabled,
            "disabled" => Self::Disabled,
            "system-disabled" => Self::SystemDisabled,
            "system-enabled-small-volume" => Self::SystemEnabledSmallVolume,
            "system-enabled-large-volume" => Self::SystemEnabledLargeVolume,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub basis: TimeBasis,
    pub reason: BasisReason,
}

fn parse_number(tok: &str) -> Option<u32> {
    let tok = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    match tok.strip_prefix("0x").or_else(|| tok.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => tok.parse().ok(),
    }
}

fn setting_of(v: u32) -> Option<Setting> {
    Some(match v & 0x3 {
        0 => Setting::UserEnabled,
        1 => Setting::UserDisabled,
        2 => Setting::SystemDisabled,
        _ => Setting::SystemEnabled,
    })
}

/// Parse the output of `fsutil behavior query disablelastaccess`.
pub fn parse_fsutil(output: &str) -> Option<Setting> {
    // "DisableLastAccess = 2  (System Managed, Last Access Disabled)"
    if let Some(rest) = output.split('=').nth(1) {
        if let Some(v) = rest.split_whitespace().next().and_then(parse_number) {
            return setting_of(v);
        }
    }
    // Text without an equals sign: look for the system-managed hex values.
    output
        .split_whitespace()
        .find(|t| t.to_ascii_lowercase().starts_with("0x8000000"))
        .and_then(parse_number)
        .and_then(setting_of)
}

/// Decide the time basis for a volume of `capacity` bytes.
pub fn decide(setting: Option<Setting>, capacity: u64) -> Decision {
    use BasisReason as R;
    use TimeBasis::{Atime, Mtime};
    let (basis, reason) = match setting {
        None => (Mtime, R::Unknown),
        Some(Setting::UserEnabled) => (Atime, R::Enabled),
        Some(Setting::UserDisabled) => (Mtime, R::Disabled),
        Some(Setting::SystemDisabled) => (Mtime, R::SystemDisabled),
        Some(Setting::SystemEnabled) if capacity <= SYSTEM_MANAGED_LIMIT => (Atime, R::SystemEnabledSmallVolume),
        Some(Setting::SystemEnabled) => (Mtime, R::SystemEnabledLargeVolume),
    };
    Decision { basis, reason }
}

/// Run the read-only query. Needs no administrator rights.
pub fn query_setting() -> Option<Setting> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let out = std::process::Command::new(format!(r"{windir}\System32\fsutil.exe"))
        .args(["behavior", "query", "disablelastaccess"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| tracing::warn!(error = %e, "cannot run fsutil"))
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let parsed = parse_fsutil(&text);
    if parsed.is_none() {
        tracing::warn!(output = %text.trim(), "cannot parse the fsutil disablelastaccess output");
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_hex_and_localised_output() {
        let cases = [
            ("DisableLastAccess = 0  (User Managed, Last Access Updates Enabled)", Some(Setting::UserEnabled)),
            ("DisableLastAccess = 1", Some(Setting::UserDisabled)),
            ("DisableLastAccess = 2  (System Managed, Last Access Updates Disabled)", Some(Setting::SystemDisabled)),
            ("DisableLastAccess = 3", Some(Setting::SystemEnabled)),
            ("DisableLastAccess = 0x80000002", Some(Setting::SystemDisabled)),
            ("DisableLastAccess = 0x80000003 (system managed)", Some(Setting::SystemEnabled)),
            ("Letzter Zugriff deaktivieren = 1 (Benutzerverwaltet)", Some(Setting::UserDisabled)),
            ("Zugriff 0x80000003", Some(Setting::SystemEnabled)),
            ("", None),
            ("Access is denied.", None),
            ("DisableLastAccess = banana", None),
        ];
        for (text, want) in cases {
            assert_eq!(parse_fsutil(text), want, "{text}");
        }
    }

    #[test]
    fn decision_follows_setting_and_volume_size() {
        let gib = 1024 * 1024 * 1024;
        let d = |s, cap| decide(s, cap);
        assert_eq!(d(Some(Setting::UserEnabled), 2000 * gib).basis, TimeBasis::Atime);
        assert_eq!(d(Some(Setting::UserDisabled), 10 * gib).basis, TimeBasis::Mtime);
        assert_eq!(d(Some(Setting::SystemDisabled), 10 * gib).basis, TimeBasis::Mtime);
        assert_eq!(d(Some(Setting::SystemEnabled), 128 * gib).basis, TimeBasis::Atime);
        let big = d(Some(Setting::SystemEnabled), 129 * gib);
        assert_eq!((big.basis, big.reason), (TimeBasis::Mtime, BasisReason::SystemEnabledLargeVolume));
        let unknown = d(None, gib);
        assert_eq!((unknown.basis, unknown.reason), (TimeBasis::Mtime, BasisReason::Unknown));
    }

    #[test]
    fn strings_round_trip() {
        for b in [TimeBasis::Atime, TimeBasis::Mtime] {
            assert_eq!(TimeBasis::parse(b.as_str()), b);
        }
        for r in [
            BasisReason::Enabled,
            BasisReason::Disabled,
            BasisReason::SystemDisabled,
            BasisReason::SystemEnabledSmallVolume,
            BasisReason::SystemEnabledLargeVolume,
            BasisReason::Unknown,
        ] {
            assert_eq!(BasisReason::parse(r.as_str()), r);
        }
    }
}
