use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A single point on a curve, both axes normalized to [0, 1].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct CurvePoint {
    pub x: f64,
    pub y: f64,
}

/// Piecewise-linear curve mapping time (x) to value (y), both normalized [0, 1].
/// Points are always sorted by x. Evaluate via binary search + linear interpolation.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(try_from = "CurveRaw")]
#[ts(export)]
pub struct Curve {
    points: Vec<CurvePoint>,
}

#[derive(Deserialize, JsonSchema)]
struct CurveRaw {
    points: Vec<CurvePoint>,
}

impl TryFrom<CurveRaw> for Curve {
    type Error = String;
    fn try_from(raw: CurveRaw) -> Result<Self, String> {
        Curve::new(raw.points).ok_or_else(|| "Curve requires at least 2 points".to_string())
    }
}

impl Curve {
    /// Create a curve from a list of points. Requires at least 2 points.
    /// Points are clamped to [0, 1] on both axes and sorted by x.
    pub fn new(mut points: Vec<CurvePoint>) -> Option<Self> {
        if points.len() < 2 {
            return None;
        }
        for p in &mut points {
            p.x = p.x.clamp(0.0, 1.0);
            p.y = p.y.clamp(0.0, 1.0);
        }
        points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
        Some(Self { points })
    }

    /// Linear ramp from (0,0) to (1,1).
    pub fn linear() -> Self {
        Self {
            points: vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }],
        }
    }

    /// Flat line at the given y value.
    pub fn constant(y: f64) -> Self {
        let y = y.clamp(0.0, 1.0);
        Self {
            points: vec![CurvePoint { x: 0.0, y }, CurvePoint { x: 1.0, y }],
        }
    }

    /// Triangle: rises to peak at midpoint, falls back to zero.
    pub fn triangle() -> Self {
        Self {
            points: vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.5, y: 1.0 },
                CurvePoint { x: 1.0, y: 0.0 },
            ],
        }
    }

    /// Access the underlying points.
    pub fn points(&self) -> &[CurvePoint] {
        &self.points
    }

    /// Evaluate the curve at position x (clamped to [0, 1]).
    /// Uses binary search for O(log n) lookup with linear interpolation.
    // Indexing is safe: points always has >= 2 entries (constructor returns None
    // otherwise), and idx bounds are checked before each access.
    #[allow(clippy::indexing_slicing)]
    pub fn evaluate(&self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);

        // Find the first point with x > input x
        let idx = self.points.partition_point(|p| p.x <= x);

        if idx == 0 {
            return self.points[0].y;
        }
        if idx >= self.points.len() {
            return self.points.last().map_or(0.0, |p| p.y);
        }

        let a = &self.points[idx - 1];
        let b = &self.points[idx];
        let dx = b.x - a.x;
        if dx <= 0.0 {
            return a.y;
        }

        let t = (x - a.x) / dx;
        a.y + (b.y - a.y) * t
    }
}

impl Default for Curve {
    fn default() -> Self {
        Self::linear()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_curve() {
        let c = Curve::linear();
        assert!((c.evaluate(0.0) - 0.0).abs() < 1e-10);
        assert!((c.evaluate(0.5) - 0.5).abs() < 1e-10);
        assert!((c.evaluate(1.0) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_constant_curve() {
        let c = Curve::constant(0.75);
        assert!((c.evaluate(0.0) - 0.75).abs() < 1e-10);
        assert!((c.evaluate(0.5) - 0.75).abs() < 1e-10);
        assert!((c.evaluate(1.0) - 0.75).abs() < 1e-10);
    }

    #[test]
    fn test_triangle_curve() {
        let c = Curve::triangle();
        assert!((c.evaluate(0.0) - 0.0).abs() < 1e-10);
        assert!((c.evaluate(0.25) - 0.5).abs() < 1e-10);
        assert!((c.evaluate(0.5) - 1.0).abs() < 1e-10);
        assert!((c.evaluate(0.75) - 0.5).abs() < 1e-10);
        assert!((c.evaluate(1.0) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_clamping() {
        let c = Curve::linear();
        assert!((c.evaluate(-0.5) - 0.0).abs() < 1e-10);
        assert!((c.evaluate(1.5) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_new_requires_min_points() {
        assert!(Curve::new(vec![CurvePoint { x: 0.0, y: 0.0 }]).is_none());
        assert!(Curve::new(vec![]).is_none());
        assert!(Curve::new(vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 1.0, y: 1.0 },
        ])
        .is_some());
    }

    #[test]
    fn test_new_sorts_and_clamps() {
        let c = Curve::new(vec![
            CurvePoint { x: 1.0, y: 0.0 },
            CurvePoint { x: -0.5, y: 1.5 },
        ])
        .unwrap();
        let pts = c.points();
        assert!((pts[0].x - 0.0).abs() < 1e-10);
        assert!((pts[0].y - 1.0).abs() < 1e-10);
        assert!((pts[1].x - 1.0).abs() < 1e-10);
        assert!((pts[1].y - 0.0).abs() < 1e-10);
    }
}
