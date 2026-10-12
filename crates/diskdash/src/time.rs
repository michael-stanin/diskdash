use std::time::SystemTime;

/// Current UTC time as RFC 3339 with second precision, e.g. `2026-10-12T09:30:00Z`.
pub fn now_rfc3339() -> String {
    humantime::format_rfc3339_seconds(SystemTime::now()).to_string()
}
