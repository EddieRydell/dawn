use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A time range within a sequence. Start must be < end, both in seconds.
/// Constructed via `TimeRange::new` which enforces this invariant.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(try_from = "TimeRangeRaw")]
#[ts(export)]
pub struct TimeRange {
    start: f64,
    end: f64,
}

#[derive(Deserialize, JsonSchema)]
struct TimeRangeRaw {
    start: f64,
    end: f64,
}

impl TryFrom<TimeRangeRaw> for TimeRange {
    type Error = String;
    fn try_from(raw: TimeRangeRaw) -> Result<Self, String> {
        TimeRange::new(raw.start, raw.end)
            .ok_or_else(|| format!("Invalid TimeRange: start={}, end={}", raw.start, raw.end))
    }
}

/// Epsilon tolerance for floating-point time comparisons (seconds).
/// Used across the codebase for dedup, cache matching, and containment checks.
pub const TIME_EPSILON: f64 = 1e-9;

impl TimeRange {
    /// Create a time range. Returns None if start >= end, either is negative, or either is non-finite.
    pub fn new(start: f64, end: f64) -> Option<Self> {
        if start.is_finite() && end.is_finite() && start >= 0.0 && end > start {
            Some(Self { start, end })
        } else {
            None
        }
    }

    pub fn start(&self) -> f64 {
        self.start
    }

    pub fn end(&self) -> f64 {
        self.end
    }

    pub fn duration(&self) -> f64 {
        self.end - self.start
    }

    /// Returns true if the given time falls within this range (inclusive start, exclusive end).
    /// Uses a small epsilon tolerance to avoid single-frame gaps at effect boundaries
    /// caused by floating-point precision.
    pub fn contains(&self, t: f64) -> bool {
        t >= self.start - TIME_EPSILON && t < self.end + TIME_EPSILON
    }

    /// Raw normalization — may return values outside [0, 1].
    pub fn normalize_unclamped(&self, t: f64) -> f64 {
        (t - self.start) / self.duration()
    }

    /// Clamped normalization for effect evaluation.
    pub fn normalize(&self, t: f64) -> f64 {
        self.normalize_unclamped(t).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn time_range_valid() {
        assert!(TimeRange::new(0.0, 1.0).is_some());
        assert!(TimeRange::new(0.0, 0.001).is_some());
        assert!(TimeRange::new(5.0, 10.0).is_some());
    }

    #[test]
    fn time_range_equal_start_end_is_none() {
        assert!(TimeRange::new(0.0, 0.0).is_none());
        assert!(TimeRange::new(5.0, 5.0).is_none());
    }

    #[test]
    fn time_range_reversed_is_none() {
        assert!(TimeRange::new(5.0, 1.0).is_none());
    }

    #[test]
    fn time_range_negative_start_is_none() {
        assert!(TimeRange::new(-1.0, 5.0).is_none());
    }

    #[test]
    fn time_range_both_negative_is_none() {
        assert!(TimeRange::new(-5.0, -1.0).is_none());
    }

    #[test]
    fn time_range_nan_is_none() {
        assert!(TimeRange::new(f64::NAN, 5.0).is_none());
        assert!(TimeRange::new(0.0, f64::NAN).is_none());
    }

    #[test]
    fn time_range_infinity_is_none() {
        assert!(TimeRange::new(0.0, f64::INFINITY).is_none());
        assert!(TimeRange::new(f64::NEG_INFINITY, 5.0).is_none());
        assert!(TimeRange::new(f64::NEG_INFINITY, f64::INFINITY).is_none());
    }

    #[test]
    fn time_range_contains_boundaries() {
        let tr = TimeRange::new(1.0, 3.0).expect("valid range");
        assert!(tr.contains(1.0));
        assert!(tr.contains(2.0));
        // end is exclusive but with epsilon tolerance
        assert!(tr.contains(3.0));
        assert!(!tr.contains(0.0));
        assert!(!tr.contains(4.0));
    }

    #[test]
    fn time_range_contains_epsilon_tolerance() {
        let tr = TimeRange::new(1.0, 3.0).expect("valid range");
        // Just inside the epsilon window (1e-9) — should still be contained
        assert!(
            tr.contains(3.0 + 5e-10),
            "half-epsilon past end should be contained"
        );
        assert!(
            tr.contains(1.0 - 5e-10),
            "half-epsilon before start should be contained"
        );
        // Well outside the epsilon window — should NOT be contained
        assert!(
            !tr.contains(3.0 + 1e-6),
            "far past end should not be contained"
        );
        assert!(
            !tr.contains(1.0 - 1e-6),
            "far before start should not be contained"
        );
    }

    #[test]
    fn time_range_normalize_boundaries() {
        let tr = TimeRange::new(2.0, 4.0).expect("valid range");
        let tol = 1e-10;
        assert!((tr.normalize(2.0) - 0.0).abs() < tol);
        assert!((tr.normalize(3.0) - 0.5).abs() < tol);
        assert!((tr.normalize(4.0) - 1.0).abs() < tol);
        // Outside range: clamped to [0, 1]
        assert!((tr.normalize(0.0) - 0.0).abs() < tol);
        assert!((tr.normalize(10.0) - 1.0).abs() < tol);
    }

    #[test]
    fn time_range_normalize_unclamped_outside() {
        let tr = TimeRange::new(2.0, 4.0).expect("valid range");
        assert!(tr.normalize_unclamped(0.0) < 0.0);
        assert!(tr.normalize_unclamped(6.0) > 1.0);
    }

    #[test]
    fn time_range_duration() {
        let tr = TimeRange::new(1.0, 3.5).expect("valid range");
        assert!((tr.duration() - 2.5).abs() < 1e-10);
    }
}
