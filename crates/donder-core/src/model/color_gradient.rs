use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::color::Color;

/// A color stop at a position along the gradient [0, 1].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct ColorStop {
    pub position: f64,
    pub color: Color,
}

/// A color gradient defined by stops with linear RGB interpolation.
/// Stops are always sorted by position.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(try_from = "ColorGradientRaw")]
#[ts(export)]
pub struct ColorGradient {
    stops: Vec<ColorStop>,
}

#[derive(Deserialize, JsonSchema)]
struct ColorGradientRaw {
    stops: Vec<ColorStop>,
}

impl TryFrom<ColorGradientRaw> for ColorGradient {
    type Error = String;
    fn try_from(raw: ColorGradientRaw) -> Result<Self, String> {
        ColorGradient::new(raw.stops)
            .ok_or_else(|| "ColorGradient requires at least 1 stop".to_string())
    }
}

impl ColorGradient {
    /// Create a gradient from stops. Requires at least 1 stop.
    /// Positions are clamped to [0, 1] and sorted.
    pub fn new(mut stops: Vec<ColorStop>) -> Option<Self> {
        if stops.is_empty() {
            return None;
        }
        for s in &mut stops {
            s.position = s.position.clamp(0.0, 1.0);
        }
        stops.sort_by(|a, b| {
            a.position
                .partial_cmp(&b.position)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Some(Self { stops })
    }

    /// Single solid color at positions 0 and 1.
    pub fn solid(color: Color) -> Self {
        Self {
            stops: vec![
                ColorStop {
                    position: 0.0,
                    color,
                },
                ColorStop {
                    position: 1.0,
                    color,
                },
            ],
        }
    }

    /// Gradient between two colors.
    pub fn two_color(start: Color, end: Color) -> Self {
        Self {
            stops: vec![
                ColorStop {
                    position: 0.0,
                    color: start,
                },
                ColorStop {
                    position: 1.0,
                    color: end,
                },
            ],
        }
    }

    /// Access the underlying stops.
    pub fn stops(&self) -> &[ColorStop] {
        &self.stops
    }

    /// Evaluate the gradient at a position (clamped to [0, 1]).
    /// Uses binary search for O(log n) lookup with linear RGB interpolation via `Color::lerp`.
    // Indexing is safe: stops is always non-empty (constructor returns None for
    // empty), and idx bounds are checked before each access.
    #[allow(clippy::indexing_slicing)]
    pub fn evaluate(&self, pos: f64) -> Color {
        let pos = pos.clamp(0.0, 1.0);

        if self.stops.len() == 1 {
            return self.stops[0].color;
        }

        // Find the first stop with position > pos
        let idx = self.stops.partition_point(|s| s.position <= pos);

        if idx == 0 {
            return self.stops[0].color;
        }
        if idx >= self.stops.len() {
            return self.stops.last().map_or(Color::BLACK, |s| s.color);
        }

        let a = &self.stops[idx - 1];
        let b = &self.stops[idx];
        let dp = b.position - a.position;
        if dp <= 0.0 {
            return a.color;
        }

        let t = (pos - a.position) / dp;
        a.color.lerp(b.color, t)
    }
}

impl Default for ColorGradient {
    fn default() -> Self {
        Self::solid(Color::WHITE)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_lossless,
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;

    #[test]
    fn test_solid_gradient() {
        let g = ColorGradient::solid(Color::rgb(100, 200, 50));
        let c = g.evaluate(0.5);
        assert_eq!(c.r, 100);
        assert_eq!(c.g, 200);
        assert_eq!(c.b, 50);
    }

    #[test]
    fn test_two_color_gradient() {
        let g = ColorGradient::two_color(Color::BLACK, Color::WHITE);
        let mid = g.evaluate(0.5);
        // Linear interpolation: should be ~127-128
        assert!((mid.r as i16 - 127).abs() <= 1);
        assert!((mid.g as i16 - 127).abs() <= 1);
        assert!((mid.b as i16 - 127).abs() <= 1);
    }

    #[test]
    fn test_gradient_endpoints() {
        let g = ColorGradient::two_color(Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        let start = g.evaluate(0.0);
        assert_eq!(start.r, 255);
        assert_eq!(start.b, 0);
        let end = g.evaluate(1.0);
        assert_eq!(end.r, 0);
        assert_eq!(end.b, 255);
    }

    #[test]
    fn test_gradient_clamping() {
        let g = ColorGradient::two_color(Color::BLACK, Color::WHITE);
        let below = g.evaluate(-1.0);
        assert_eq!(below.r, 0);
        let above = g.evaluate(2.0);
        assert_eq!(above.r, 255);
    }

    #[test]
    fn test_new_requires_min_stops() {
        assert!(ColorGradient::new(vec![]).is_none());
        assert!(ColorGradient::new(vec![ColorStop {
            position: 0.5,
            color: Color::WHITE,
        }])
        .is_some());
    }

    #[test]
    fn test_new_sorts_stops() {
        let g = ColorGradient::new(vec![
            ColorStop {
                position: 1.0,
                color: Color::rgb(0, 0, 255),
            },
            ColorStop {
                position: 0.0,
                color: Color::rgb(255, 0, 0),
            },
        ])
        .unwrap();
        let stops = g.stops();
        assert!((stops[0].position - 0.0).abs() < 1e-10);
        assert_eq!(stops[0].color.r, 255);
        assert!((stops[1].position - 1.0).abs() < 1e-10);
        assert_eq!(stops[1].color.b, 255);
    }
}
