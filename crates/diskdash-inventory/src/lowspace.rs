//! Low free-space rule: below the percentage threshold OR below the floor.

use crate::model::{Drive, LowSpaceReason};

pub const GIB: u64 = 1024 * 1024 * 1024;

pub fn evaluate(capacity: u64, free: u64, percent: f64, floor_gib: u32) -> Option<LowSpaceReason> {
    if capacity == 0 {
        return None;
    }
    let below_percent = (free as f64) < capacity as f64 * percent / 100.0;
    let below_floor = free < u64::from(floor_gib) * GIB;
    match (below_percent, below_floor) {
        (true, true) => Some(LowSpaceReason::Both),
        (true, false) => Some(LowSpaceReason::Percent),
        (false, true) => Some(LowSpaceReason::Floor),
        (false, false) => None,
    }
}

/// Apply the current settings to every drive.
pub fn apply(drives: &mut [Drive], percent: f64, floor_gib: u32) {
    for d in drives {
        d.low_space_reason = evaluate(d.capacity, d.free, percent, floor_gib);
        d.low_space = d.low_space_reason.is_some();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_scenarios() {
        // D: 931 GiB with 80 GiB free (8.6%), defaults → percent.
        assert_eq!(evaluate(931 * GIB, 80 * GIB, 10.0, 20), Some(LowSpaceReason::Percent));
        // G: 232 GiB with 18 GiB free (7.8%), threshold 5% → floor only.
        assert_eq!(evaluate(232 * GIB, 18 * GIB, 5.0, 20), Some(LowSpaceReason::Floor));
        // C: 476 GiB with 30 GiB free (6.3%) after lowering to 5% / 10 GiB → fine.
        assert_eq!(evaluate(476 * GIB, 30 * GIB, 5.0, 10), None);
        // Both conditions.
        assert_eq!(evaluate(100 * GIB, 5 * GIB, 10.0, 20), Some(LowSpaceReason::Both));
        // No media.
        assert_eq!(evaluate(0, 0, 10.0, 20), None);
    }
}
