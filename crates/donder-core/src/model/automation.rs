use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::curve::Curve;
use super::motion_path::LoopMode;
use super::time_range::TimeRange;

// ── ClipId ──────────────────────────────────────────────────────────

/// Unique identifier for an automation clip.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(transparent)]
#[ts(export)]
pub struct ClipId(pub String);

impl ClipId {
    /// Borrow the inner string as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ── AutomationClip ──────────────────────────────────────────────────

/// A standalone automation clip: a normalized 0-1 curve on the timeline.
///
/// The curve maps normalized time (x: 0-1) to normalized value (y: 0-1).
/// At eval time, the engine maps the 0-1 output to the linked parameter's
/// type and range (e.g. Float min/max, Bool threshold, Int rounding).
///
/// Requires at least 2 control points (inherited from `Curve`).
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct AutomationClip {
    pub id: ClipId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    pub curve: Curve,
    pub time_range: TimeRange,
    #[serde(default)]
    pub loop_mode: LoopMode,
}

impl AutomationClip {
    /// Create an automation clip.
    pub fn new(
        id: ClipId,
        name: Option<String>,
        curve: Curve,
        time_range: TimeRange,
        loop_mode: LoopMode,
    ) -> Self {
        Self {
            id,
            name,
            curve,
            time_range,
            loop_mode,
        }
    }

    /// Replace the time range.
    pub fn set_time_range(&mut self, time_range: TimeRange) {
        self.time_range = time_range;
    }

    /// Map absolute time to a normalized 0-1 position on the curve,
    /// applying the clip's loop mode.
    fn map_time(&self, t: f64) -> f64 {
        let duration = self.time_range.duration();
        if duration <= 0.0 {
            return 0.0;
        }
        let local_t = (t - self.time_range.start()) / duration;
        self.loop_mode.map_normalized(local_t)
    }

    /// Evaluate the automation clip at absolute time `t` (seconds).
    /// Returns a normalized 0-1 value from the curve.
    pub fn evaluate(&self, t: f64) -> f64 {
        let x = self.map_time(t);
        self.curve.evaluate(x)
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::model::curve::CurvePoint;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn clip_range(start: f64, end: f64) -> TimeRange {
        TimeRange::new(start, end).unwrap()
    }

    /// Create a simple linear ramp clip (0→1) over the given time range.
    fn linear_clip(start: f64, end: f64) -> AutomationClip {
        AutomationClip::new(
            ClipId("test".into()),
            None,
            Curve::linear(),
            clip_range(start, end),
            LoopMode::Clamp,
        )
    }

    /// Create a clip with a custom curve over the given time range.
    fn custom_clip(
        points: Vec<CurvePoint>,
        start: f64,
        end: f64,
        loop_mode: LoopMode,
    ) -> AutomationClip {
        AutomationClip::new(
            ClipId("test".into()),
            None,
            Curve::new(points).unwrap(),
            clip_range(start, end),
            loop_mode,
        )
    }

    // ── Basic evaluation ─────────────────────────────────────────

    #[test]
    fn linear_ramp_evaluation() {
        let clip = linear_clip(0.0, 10.0);
        assert!(approx(clip.evaluate(0.0), 0.0));
        assert!(approx(clip.evaluate(5.0), 0.5));
        assert!(approx(clip.evaluate(10.0), 1.0));
    }

    #[test]
    fn constant_curve_evaluation() {
        let clip = AutomationClip::new(
            ClipId("const".into()),
            None,
            Curve::constant(0.75),
            clip_range(0.0, 5.0),
            LoopMode::Clamp,
        );
        assert!(approx(clip.evaluate(0.0), 0.75));
        assert!(approx(clip.evaluate(2.5), 0.75));
        assert!(approx(clip.evaluate(5.0), 0.75));
    }

    #[test]
    fn triangle_curve_evaluation() {
        let clip = AutomationClip::new(
            ClipId("tri".into()),
            None,
            Curve::triangle(),
            clip_range(0.0, 4.0),
            LoopMode::Clamp,
        );
        assert!(approx(clip.evaluate(0.0), 0.0));
        assert!(approx(clip.evaluate(2.0), 1.0)); // midpoint = peak
        assert!(approx(clip.evaluate(4.0), 0.0));
    }

    // ── Clamp mode ───────────────────────────────────────────────

    #[test]
    fn clamp_mode_holds_at_boundaries() {
        let clip = linear_clip(2.0, 4.0);
        // Before clip start: clamped to 0
        assert!(approx(clip.evaluate(0.0), 0.0));
        // After clip end: clamped to 1
        assert!(approx(clip.evaluate(6.0), 1.0));
        // At midpoint
        assert!(approx(clip.evaluate(3.0), 0.5));
    }

    // ── Loop mode ────────────────────────────────────────────────

    #[test]
    fn loop_mode_wraps() {
        let clip = AutomationClip::new(
            ClipId("loop".into()),
            None,
            Curve::linear(),
            clip_range(0.0, 2.0),
            LoopMode::Loop,
        );
        // At t=1.0, normalized = 0.5
        assert!(approx(clip.evaluate(1.0), 0.5));
        // At t=2.0, wraps to 0.0
        assert!(approx(clip.evaluate(2.0), 0.0));
        // At t=3.0, normalized = (3-0)/2 = 1.5, mod 1.0 = 0.5
        assert!(approx(clip.evaluate(3.0), 0.5));
    }

    // ── PingPong mode ────────────────────────────────────────────

    #[test]
    fn ping_pong_mode_reverses() {
        let clip = AutomationClip::new(
            ClipId("pp".into()),
            None,
            Curve::linear(),
            clip_range(0.0, 2.0),
            LoopMode::PingPong,
        );
        // Forward: t=1.0, normalized=0.5
        assert!(approx(clip.evaluate(1.0), 0.5));
        // At boundary: t=2.0, normalized=1.0, cycle=1, frac=0.0 → 1.0-0.0=1.0
        assert!(approx(clip.evaluate(2.0), 1.0));
        // Reverse: t=3.0, normalized=1.5, cycle=1, frac=0.5 → 1.0-0.5=0.5
        assert!(approx(clip.evaluate(3.0), 0.5));
        // Back to start: t=4.0, normalized=2.0, cycle=2, frac=0.0 → 0.0
        assert!(approx(clip.evaluate(4.0), 0.0));
    }

    // ── Multi-point curve ────────────────────────────────────────

    #[test]
    fn multi_point_curve() {
        let clip = custom_clip(
            vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.5, y: 1.0 },
                CurvePoint { x: 1.0, y: 0.5 },
            ],
            0.0,
            10.0,
            LoopMode::Clamp,
        );
        assert!(approx(clip.evaluate(0.0), 0.0));
        assert!(approx(clip.evaluate(5.0), 1.0)); // midpoint = peak
        assert!(approx(clip.evaluate(10.0), 0.5)); // end
        assert!(approx(clip.evaluate(2.5), 0.5)); // quarter = halfway up
    }

    // ── Serde round-trip ─────────────────────────────────────────

    #[test]
    fn serde_roundtrip() {
        let clip = linear_clip(0.0, 5.0);
        let json = serde_json::to_string(&clip).unwrap();
        let back: AutomationClip = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, clip.id);
        assert_eq!(back.loop_mode, clip.loop_mode);
        assert_eq!(back.curve.points().len(), 2);
    }

    #[test]
    fn set_time_range_works() {
        let mut clip = linear_clip(0.0, 5.0);
        clip.set_time_range(clip_range(1.0, 3.0));
        assert!(approx(clip.time_range.start(), 1.0));
        assert!(approx(clip.time_range.end(), 3.0));
    }
}
