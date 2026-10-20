//! Alert rules: when a drive is low on space, when a folder grows fast, and
//! when an alert may be raised again. Pure decisions over numbers; the server
//! keeps the state and does the delivery (inside the app only).

/// A low-space alert clears only when free space is above both limits plus these margins.
pub const HYSTERESIS_PERCENT: f64 = 2.0;
pub const HYSTERESIS_GIB: u64 = 2;
pub const DEFAULT_COOLDOWN_HOURS: u32 = 24;
const GIB: u64 = 1 << 30;
const DAY_SECS: i64 = 86_400;

/// Free space below the percentage threshold **or** below the absolute floor.
pub fn low_space(capacity: u64, free: u64, percent: f64, floor_gib: u32) -> bool {
    if capacity == 0 {
        return false;
    }
    (free as f64) < capacity as f64 * percent / 100.0 || free < u64::from(floor_gib) * GIB
}

/// Free space has recovered past both limits and the hysteresis margins.
pub fn low_space_cleared(capacity: u64, free: u64, percent: f64, floor_gib: u32) -> bool {
    if capacity == 0 {
        return true;
    }
    (free as f64) >= capacity as f64 * (percent + HYSTERESIS_PERCENT) / 100.0
        && free >= (u64::from(floor_gib) + HYSTERESIS_GIB) * GIB
}

/// Bytes per day a folder grew between two scans, or `None` when they are less than a day apart.
pub fn growth_rate_per_day(old_alloc: u64, new_alloc: u64, old_ts: i64, new_ts: i64) -> Option<f64> {
    let secs = new_ts - old_ts;
    (secs >= DAY_SECS).then(|| (new_alloc as f64 - old_alloc as f64) / (secs as f64 / DAY_SECS as f64))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlertState {
    pub active: bool,
    pub last_raised_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Record a new alert. `renewed` when the old one was still active and its cooldown ran out.
    Raise {
        renewed: bool,
    },
    /// The condition is over: mark the active alert resolved.
    Clear,
    Nothing,
}

/// What to do with an alert key given what is true now.
///
/// - `condition`: the alert condition holds right now.
/// - `cleared`: the condition is clearly over (past the hysteresis margin).
pub fn decide(condition: bool, cleared: bool, state: Option<AlertState>, now: i64, cooldown_secs: i64) -> Action {
    match (condition, state) {
        (true, None) => Action::Raise { renewed: false },
        (true, Some(s)) if !s.active => Action::Raise { renewed: false },
        (true, Some(s)) if now - s.last_raised_at >= cooldown_secs => Action::Raise { renewed: true },
        (true, Some(_)) => Action::Nothing,
        (false, Some(s)) if s.active && cleared => Action::Clear,
        (false, _) => Action::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP_500: u64 = 500 * GIB;

    #[test]
    fn percentage_or_floor() {
        // 8% free of 500 GiB with a 10% limit.
        assert!(low_space(CAP_500, 40 * GIB, 10.0, 20));
        // A 128 GiB drive with 18 GiB free is 14%: only the floor catches it.
        assert!(low_space(128 * GIB, 18 * GIB, 10.0, 20));
        assert!(!low_space(128 * GIB, 30 * GIB, 10.0, 20));
        assert!(!low_space(CAP_500, 100 * GIB, 10.0, 20));
        assert!(!low_space(0, 0, 10.0, 20), "an unknown size is not an alert");
    }

    #[test]
    fn low_space_clears_only_past_both_limits_plus_hysteresis() {
        // 500 GiB: 10% = 50 GiB, plus 2% = 60 GiB; floor 20 + 2 = 22 GiB.
        assert!(!low_space_cleared(CAP_500, 55 * GIB, 10.0, 20), "above the limit but inside the margin");
        assert!(low_space_cleared(CAP_500, 60 * GIB, 10.0, 20));
        // A small drive: the floor is the binding limit.
        assert!(!low_space_cleared(100 * GIB, 21 * GIB, 10.0, 20));
        assert!(low_space_cleared(100 * GIB, 22 * GIB, 5.0, 20));
    }

    #[test]
    fn growth_needs_a_day_between_scans() {
        let d = 86_400;
        assert_eq!(growth_rate_per_day(10 * GIB, 40 * GIB, 0, 2 * d), Some(15.0 * GIB as f64));
        assert_eq!(growth_rate_per_day(10 * GIB, 40 * GIB, 0, d - 1), None);
        assert_eq!(growth_rate_per_day(10 * GIB, 40 * GIB, 0, d), Some(30.0 * GIB as f64));
        assert!(growth_rate_per_day(40 * GIB, 10 * GIB, 0, d).unwrap() < 0.0, "shrinking is a negative rate");
    }

    const COOL: i64 = 24 * 3600;

    #[test]
    fn a_new_condition_raises_once_and_repeats_are_quiet() {
        assert_eq!(decide(true, false, None, 1000, COOL), Action::Raise { renewed: false });
        let st = AlertState { active: true, last_raised_at: 1000 };
        // Three scans within 6 hours: nothing new.
        for t in [1000 + 600, 1000 + 3 * 3600, 1000 + 6 * 3600] {
            assert_eq!(decide(true, false, Some(st), t, COOL), Action::Nothing, "{t}");
        }
    }

    #[test]
    fn the_cooldown_lets_a_still_active_alert_be_raised_again() {
        let st = AlertState { active: true, last_raised_at: 1000 };
        assert_eq!(decide(true, false, Some(st), 1000 + COOL - 1, COOL), Action::Nothing);
        assert_eq!(decide(true, false, Some(st), 1000 + COOL, COOL), Action::Raise { renewed: true });
    }

    #[test]
    fn clearing_needs_the_condition_to_be_clearly_over_and_a_recurrence_raises_again() {
        let st = AlertState { active: true, last_raised_at: 1000 };
        assert_eq!(
            decide(false, false, Some(st), 2000, COOL),
            Action::Nothing,
            "inside the hysteresis band the alert stays"
        );
        assert_eq!(decide(false, true, Some(st), 2000, COOL), Action::Clear);
        let resolved = AlertState { active: false, last_raised_at: 1000 };
        assert_eq!(
            decide(true, false, Some(resolved), 1100, COOL),
            Action::Raise { renewed: false },
            "recurred right after clearing: not a repeat"
        );
        assert_eq!(decide(false, true, Some(resolved), 2000, COOL), Action::Nothing);
        assert_eq!(decide(false, true, None, 2000, COOL), Action::Nothing);
    }
}
