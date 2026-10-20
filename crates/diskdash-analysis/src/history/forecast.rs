//! Days-until-full forecast: ordinary least squares of free bytes against time.
//! Honest about its limits: it needs enough points over enough time, and says
//! how well the line fits.

use serde::Serialize;
use ts_rs::TS;

pub const MIN_POINTS: usize = 3;
pub const MIN_SPAN_DAYS: f64 = 7.0;
pub const WINDOW_DAYS: f64 = 90.0;
/// A rise in free space bigger than this share of capacity (a cleanup) restarts the window.
pub const RESET_JUMP: f64 = 0.05;
pub const LOW_CONFIDENCE_R2: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Unix seconds.
    pub ts: i64,
    pub free: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct ForecastLimits {
    /// Low-space percentage (default 10).
    pub low_percent: f64,
    /// Low-space floor in bytes (default 20 GiB).
    pub low_floor: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "state", rename_all = "kebab-case")]
#[ts(export)]
pub enum Forecast {
    /// Fewer than 3 samples, or less than 7 days of them.
    NotEnoughHistory {
        #[ts(type = "number")]
        points: u32,
        span_days: f64,
    },
    /// Free space is flat or growing.
    NotFilling {
        #[ts(type = "number")]
        points: u32,
        span_days: f64,
    },
    Filling {
        /// Days until free space reaches zero.
        days_to_full: f64,
        /// Days until it reaches the low-space limit (0 when already below).
        days_to_low: f64,
        /// Bytes of free space lost per day.
        #[ts(type = "number")]
        bytes_per_day: f64,
        /// How well a straight line fits, 0..=1.
        r2: f64,
        low_confidence: bool,
        #[ts(type = "number")]
        points: u32,
        span_days: f64,
    },
}

/// Forecast from `samples` (any order) of a volume with `capacity` bytes.
pub fn forecast(samples: &[Sample], capacity: u64, limits: ForecastLimits) -> Forecast {
    let mut s: Vec<Sample> = samples.to_vec();
    s.sort_by_key(|x| x.ts);
    let Some(last) = s.last().copied() else { return Forecast::NotEnoughHistory { points: 0, span_days: 0.0 } };
    s.retain(|x| (last.ts - x.ts) as f64 <= WINDOW_DAYS * 86_400.0);
    // Start after the last cleanup (a big jump up), so it does not flatten the slope.
    let jump = capacity as f64 * RESET_JUMP;
    if let Some(i) = (1..s.len()).rev().find(|&i| s[i].free as f64 - s[i - 1].free as f64 > jump) {
        s.drain(..i);
    }
    let points = s.len() as u32;
    let span_days = s.last().zip(s.first()).map_or(0.0, |(l, f)| (l.ts - f.ts) as f64 / 86_400.0);
    if s.len() < MIN_POINTS || span_days < MIN_SPAN_DAYS {
        return Forecast::NotEnoughHistory { points, span_days };
    }
    // OLS: x in days since the first sample, y = free bytes.
    let t0 = s[0].ts;
    let xs: Vec<f64> = s.iter().map(|p| (p.ts - t0) as f64 / 86_400.0).collect();
    let ys: Vec<f64> = s.iter().map(|p| p.free as f64).collect();
    let n = xs.len() as f64;
    let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let syy: f64 = ys.iter().map(|y| (y - my).powi(2)).sum();
    if sxx <= 0.0 {
        return Forecast::NotEnoughHistory { points, span_days };
    }
    let slope = sxy / sxx;
    // Less than a kilobyte per day is flat.
    if slope >= -1024.0 {
        return Forecast::NotFilling { points, span_days };
    }
    let intercept = my - slope * mx;
    let r2 = if syy > 0.0 { (sxy * sxy / (sxx * syy)).clamp(0.0, 1.0) } else { 1.0 };
    let x_now = *xs.last().unwrap_or(&0.0);
    let fitted = (intercept + slope * x_now).max(0.0);
    let limit = ((capacity as f64 * limits.low_percent / 100.0).max(limits.low_floor as f64)).min(capacity as f64);
    let down = -slope;
    Forecast::Filling {
        days_to_full: fitted / down,
        days_to_low: ((fitted - limit) / down).max(0.0),
        bytes_per_day: down,
        r2,
        low_confidence: r2 < LOW_CONFIDENCE_R2,
        points,
        span_days,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;
    const DAY: i64 = 86_400;
    const LIM: ForecastLimits = ForecastLimits { low_percent: 10.0, low_floor: 20 * GIB };

    fn series(days: i64, start_gib: f64, per_day_gib: f64) -> Vec<Sample> {
        (0..=days)
            .map(|d| Sample {
                ts: 1_700_000_000 + d * DAY,
                free: ((start_gib - per_day_gib * d as f64) * GIB as f64) as u64,
            })
            .collect()
    }

    #[test]
    fn steady_growth_forecasts_about_fifty_days_with_high_confidence() {
        // 1 GiB/day over 30 days, 50 GiB left at the end.
        let f = forecast(&series(30, 80.0, 1.0), 300 * GIB, LIM);
        let Forecast::Filling { days_to_full, days_to_low, bytes_per_day, r2, low_confidence, .. } = f else {
            panic!("{f:?}")
        };
        assert!((days_to_full - 50.0).abs() < 0.5, "{days_to_full}");
        assert!((bytes_per_day / GIB as f64 - 1.0).abs() < 0.01);
        assert!(r2 > 0.99 && !low_confidence);
        assert!((days_to_low - 20.0).abs() < 0.5, "50 GiB left, low limit 30 GiB of 300: {days_to_low}");
    }

    #[test]
    fn needs_three_points_over_a_week() {
        let two = series(30, 80.0, 1.0).into_iter().step_by(30).collect::<Vec<_>>();
        assert_eq!(two.len(), 2);
        assert!(matches!(forecast(&two, 500 * GIB, LIM), Forecast::NotEnoughHistory { points: 2, .. }));
        let short: Vec<Sample> = series(3, 80.0, 1.0)
            .into_iter()
            .chain([Sample { ts: 1_700_000_000 + 3 * DAY + 1, free: 70 * GIB }])
            .collect();
        assert!(
            matches!(forecast(&short, 500 * GIB, LIM), Forecast::NotEnoughHistory { points: 5, span_days } if span_days < 7.0)
        );
        assert!(matches!(forecast(&[], 500 * GIB, LIM), Forecast::NotEnoughHistory { points: 0, .. }));
    }

    #[test]
    fn flat_or_growing_free_space_is_not_filling() {
        assert!(matches!(forecast(&series(20, 80.0, 0.0), 500 * GIB, LIM), Forecast::NotFilling { .. }));
        assert!(matches!(forecast(&series(20, 80.0, -2.0), 500 * GIB, LIM), Forecast::NotFilling { .. }));
    }

    #[test]
    fn noisy_data_is_flagged_low_confidence() {
        let noisy: Vec<Sample> = (0..=20)
            .map(|d| Sample {
                ts: 1_700_000_000 + d * DAY,
                free: ((100.0 - 0.5 * d as f64 + if d % 2 == 0 { 25.0 } else { -25.0 }) * GIB as f64) as u64,
            })
            .collect();
        match forecast(&noisy, 2000 * GIB, LIM) {
            Forecast::Filling { low_confidence, r2, .. } => assert!(low_confidence && r2 < 0.5, "{r2}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_cleanup_restarts_the_window() {
        // Falling 2 GiB/day, then a 100 GiB cleanup on day 20, then falling 1 GiB/day.
        let mut s = series(20, 200.0, 2.0);
        s.extend(
            (21..=40)
                .map(|d| Sample { ts: 1_700_000_000 + d * DAY, free: ((260.0 - (d - 21) as f64) * GIB as f64) as u64 }),
        );
        let Forecast::Filling { bytes_per_day, points, .. } = forecast(&s, 500 * GIB, LIM) else { panic!() };
        assert!((bytes_per_day / GIB as f64 - 1.0).abs() < 0.01, "slope of the part after the cleanup");
        assert_eq!(points, 20);
    }

    #[test]
    fn only_the_last_ninety_days_count() {
        let mut s = series(200, 400.0, 1.0);
        s.sort_by_key(|x| x.ts);
        let Forecast::Filling { span_days, .. } = forecast(&s, 1000 * GIB, LIM) else { panic!() };
        assert!(span_days <= 90.0);
    }

    #[test]
    fn already_below_the_low_limit_reports_zero_days_to_low() {
        let f = forecast(&series(14, 40.0, 0.5), 500 * GIB, LIM);
        let Forecast::Filling { days_to_low, days_to_full, .. } = f else { panic!() };
        assert_eq!(days_to_low, 0.0);
        assert!(days_to_full > 0.0);
    }
}
