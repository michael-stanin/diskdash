//! Target selection and projected free space. Pure arithmetic over drive state.

use std::collections::HashMap;

pub const NO_TARGET_SPACE: &str = "no_target_with_space";
pub const NO_TARGET_TIER: &str = "no_target_tier";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Ssd,
    Hdd,
}

#[derive(Debug, Clone)]
pub struct DriveState {
    pub id: String,
    pub letter: String,
    pub tier: Tier,
    /// Physical disks the volume lives on (volumes sharing one are never source and target).
    pub disks: Vec<String>,
    pub capacity: u64,
    pub free: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Low-space percentage threshold (default 10).
    pub low_percent: f64,
    /// Low-space absolute floor in bytes (default 20 GiB).
    pub low_floor: u64,
    /// Extra room kept on a target, as a share of its capacity (default 10)...
    pub headroom_percent: f64,
    /// ...or this many bytes, whichever is larger (default 20 GiB).
    pub headroom_floor: u64,
}

impl Limits {
    /// Free space below which a drive counts as low (the low-space rule).
    pub fn low_limit(&self, capacity: u64) -> u64 {
        ((capacity as f64 * self.low_percent / 100.0) as u64).max(self.low_floor)
    }

    /// Free space a target must keep after the move: the low-space limit plus headroom.
    pub fn required_free(&self, capacity: u64) -> u64 {
        self.low_limit(capacity) + ((capacity as f64 * self.headroom_percent / 100.0) as u64).max(self.headroom_floor)
    }
}

#[derive(Debug, Clone)]
pub struct Need {
    pub key: String,
    pub bytes: u64,
    pub source_drive: String,
    pub target_tier: Tier,
    pub planned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub key: String,
    /// Chosen target drive id.
    pub target: Option<String>,
    pub reason: Option<&'static str>,
}

fn eligible<'a>(drives: &'a [DriveState], need: &Need) -> Vec<&'a DriveState> {
    let Some(src) = drives.iter().find(|d| d.id == need.source_drive) else { return Vec::new() };
    drives
        .iter()
        .filter(|d| d.id != src.id && d.tier == need.target_tier && !d.disks.iter().any(|x| src.disks.contains(x)))
        .collect()
}

/// Greedy assignment. Planned moves go first (largest first) and use up target
/// space; every other suggestion is then judged against what is left, without
/// using it up (they are alternatives until the user plans them).
pub fn assign(drives: &[DriveState], needs: &[Need], limits: &Limits) -> Vec<Assignment> {
    let mut budget: HashMap<&str, i64> = drives.iter().map(|d| (d.id.as_str(), d.free as i64)).collect();
    let mut order: Vec<&Need> = needs.iter().collect();
    order.sort_by(|a, b| b.planned.cmp(&a.planned).then(b.bytes.cmp(&a.bytes)).then(a.key.cmp(&b.key)));
    let mut out: HashMap<String, Assignment> = HashMap::new();
    for need in order {
        let candidates = eligible(drives, need);
        let best = candidates
            .iter()
            .map(|d| (d, budget[d.id.as_str()] - need.bytes as i64))
            .filter(|(d, after)| *after >= limits.required_free(d.capacity) as i64)
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.id.cmp(&a.0.id)));
        let a = match best {
            Some((d, _)) => {
                if need.planned {
                    *budget.entry(d.id.as_str()).or_insert(0) -= need.bytes as i64;
                }
                Assignment { key: need.key.clone(), target: Some(d.id.clone()), reason: None }
            }
            None => Assignment {
                key: need.key.clone(),
                target: None,
                reason: Some(if drives.iter().any(|d| d.tier == need.target_tier) {
                    NO_TARGET_SPACE
                } else {
                    NO_TARGET_TIER
                }),
            },
        };
        out.insert(need.key.clone(), a);
    }
    needs.iter().filter_map(|n| out.remove(&n.key)).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    pub drive_id: String,
    pub free: u64,
    pub projected_free: u64,
    /// Below the low-space rule after the planned moves.
    pub low_after: bool,
}

/// Free space per drive after `moves` (source drive, target drive, bytes).
pub fn project(drives: &[DriveState], moves: &[(String, String, u64)], limits: &Limits) -> Vec<Projection> {
    drives
        .iter()
        .map(|d| {
            let gained: u64 = moves.iter().filter(|m| m.0 == d.id).map(|m| m.2).sum();
            let lost: u64 = moves.iter().filter(|m| m.1 == d.id).map(|m| m.2).sum();
            let projected = (d.free + gained).saturating_sub(lost).min(d.capacity);
            Projection {
                drive_id: d.id.clone(),
                free: d.free,
                projected_free: projected,
                low_after: projected < limits.low_limit(d.capacity),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    fn drive(id: &str, tier: Tier, disk: &str, capacity: u64, free: u64) -> DriveState {
        DriveState { id: id.into(), letter: id.to_uppercase(), tier, disks: vec![disk.into()], capacity, free }
    }

    fn limits() -> Limits {
        Limits { low_percent: 10.0, low_floor: 20 * GIB, headroom_percent: 10.0, headroom_floor: 20 * GIB }
    }

    fn need(key: &str, gib: u64, src: &str, tier: Tier, planned: bool) -> Need {
        Need { key: key.into(), bytes: gib * GIB, source_drive: src.into(), target_tier: tier, planned }
    }

    #[test]
    fn required_free_is_the_low_limit_plus_headroom() {
        let l = limits();
        assert_eq!(l.low_limit(1000 * GIB), 100 * GIB);
        assert_eq!(l.required_free(1000 * GIB), 200 * GIB);
        assert_eq!(l.required_free(100 * GIB), 40 * GIB, "absolute floors win on small drives");
    }

    #[test]
    fn picks_the_target_with_the_most_room_left() {
        let drives = vec![
            drive("c", Tier::Ssd, "0", 500 * GIB, 20 * GIB),
            drive("d", Tier::Hdd, "1", 1000 * GIB, 600 * GIB),
            drive("e", Tier::Hdd, "2", 2000 * GIB, 1500 * GIB),
        ];
        let a = assign(&drives, &[need("x", 80, "c", Tier::Hdd, false)], &limits());
        assert_eq!(a[0].target.as_deref(), Some("e"));
        assert_eq!(a[0].reason, None);
    }

    #[test]
    fn a_target_below_the_floor_after_the_move_is_not_valid() {
        let drives =
            vec![drive("c", Tier::Ssd, "0", 500 * GIB, 20 * GIB), drive("d", Tier::Hdd, "1", 1000 * GIB, 215 * GIB)];
        // 215 GiB free, needs 200 GiB kept: 20 GiB fits, 30 GiB does not.
        assert!(assign(&drives, &[need("fits", 15, "c", Tier::Hdd, false)], &limits())[0].target.is_some());
        let a = assign(&drives, &[need("big", 30, "c", Tier::Hdd, false)], &limits());
        assert_eq!((a[0].target.clone(), a[0].reason), (None, Some(NO_TARGET_SPACE)));
    }

    #[test]
    fn same_physical_disk_and_missing_tier_are_never_targets() {
        let drives =
            vec![drive("c", Tier::Ssd, "0", 500 * GIB, 20 * GIB), drive("d", Tier::Hdd, "0", 1000 * GIB, 900 * GIB)];
        let a = assign(&drives, &[need("x", 10, "c", Tier::Hdd, false)], &limits());
        assert_eq!(a[0].reason, Some(NO_TARGET_SPACE), "d shares c's disk");
        let only_ssd = vec![drive("c", Tier::Ssd, "0", 500 * GIB, 20 * GIB)];
        assert_eq!(
            assign(&only_ssd, &[need("x", 10, "c", Tier::Hdd, false)], &limits())[0].reason,
            Some(NO_TARGET_TIER)
        );
    }

    #[test]
    fn planned_moves_use_up_the_budget_for_the_rest() {
        let drives =
            vec![drive("c", Tier::Ssd, "0", 500 * GIB, 20 * GIB), drive("d", Tier::Hdd, "1", 1000 * GIB, 300 * GIB)];
        let needs = vec![need("planned", 90, "c", Tier::Hdd, true), need("other", 30, "c", Tier::Hdd, false)];
        let a = assign(&drives, &needs, &limits());
        assert!(a[0].target.is_some());
        assert_eq!(a[1].reason, Some(NO_TARGET_SPACE), "300 - 90 - 30 would leave under 200 GiB");
        // Unplanned alternatives do not use each other up.
        let alts = vec![need("a", 50, "c", Tier::Hdd, false), need("b", 50, "c", Tier::Hdd, false)];
        assert!(assign(&drives, &alts, &limits()).iter().all(|x| x.target.is_some()));
    }

    #[test]
    fn projection_moves_free_space_between_drives() {
        let drives =
            vec![drive("c", Tier::Ssd, "0", 500 * GIB, 30 * GIB), drive("d", Tier::Hdd, "1", 1000 * GIB, 600 * GIB)];
        let p = project(&drives, &[("c".into(), "d".into(), 70 * GIB), ("c".into(), "d".into(), 50 * GIB)], &limits());
        assert_eq!(p[0].projected_free, 150 * GIB, "C: gains 120 GiB");
        assert_eq!(p[1].projected_free, 480 * GIB, "D: loses 120 GiB");
        assert!(!p[0].low_after && !p[1].low_after);
        assert!(p[0].free == 30 * GIB);
        let none = project(&drives, &[], &limits());
        assert!(none[0].low_after, "30 GiB free of 500 GiB is below 50 GiB");
    }
}
