use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::easing::EasingFunction;

/// A single waypoint in a motion path.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct Waypoint {
    /// Absolute time in seconds within the sequence.
    pub time: f64,
    /// Normalized X position in layout space [0, 1].
    pub x: f64,
    /// Normalized Y position in layout space [0, 1].
    pub y: f64,
    /// Easing function for the transition FROM the previous waypoint TO this one.
    #[serde(default)]
    pub easing: EasingFunction,
}

/// How the motion path behaves outside its waypoint time range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LoopMode {
    /// Hold first/last position outside the time range.
    #[default]
    Clamp,
    /// Restart from the first waypoint after the last.
    Loop,
    /// Reverse direction at endpoints.
    PingPong,
}

impl LoopMode {
    /// Map a normalized time value (0.0 = start, 1.0+ = beyond end) according to loop mode.
    /// Input `t` is in [0, +inf) where 1.0 represents one full cycle.
    /// Output is clamped/wrapped to [0, 1].
    #[allow(clippy::cast_possible_truncation)]
    pub fn map_normalized(self, t: f64) -> f64 {
        match self {
            LoopMode::Clamp => t.clamp(0.0, 1.0),
            LoopMode::Loop => {
                if t < 0.0 {
                    0.0
                } else {
                    t % 1.0
                }
            }
            LoopMode::PingPong => {
                if t < 0.0 {
                    0.0
                } else {
                    let cycle = t.floor() as i64;
                    let frac = t - t.floor();
                    if cycle % 2 == 0 {
                        frac
                    } else {
                        1.0 - frac
                    }
                }
            }
        }
    }
}

/// A keyframed 2D path through layout space.
///
/// Waypoints are sorted by time. At least one waypoint is required.
/// Evaluate at any absolute time to get the interpolated (x, y) position.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(try_from = "MotionPathRaw")]
#[ts(export)]
pub struct MotionPath {
    waypoints: Vec<Waypoint>,
    #[serde(default)]
    pub loop_mode: LoopMode,
}

#[derive(Deserialize, JsonSchema)]
struct MotionPathRaw {
    waypoints: Vec<Waypoint>,
    #[serde(default)]
    loop_mode: LoopMode,
}

impl TryFrom<MotionPathRaw> for MotionPath {
    type Error = String;
    fn try_from(raw: MotionPathRaw) -> Result<Self, String> {
        MotionPath::new(raw.waypoints, raw.loop_mode)
            .ok_or_else(|| "MotionPath requires at least 1 waypoint".to_string())
    }
}

impl MotionPath {
    /// Create a motion path. Requires at least one waypoint.
    /// Waypoints are sorted by time.
    pub fn new(mut waypoints: Vec<Waypoint>, loop_mode: LoopMode) -> Option<Self> {
        if waypoints.is_empty() {
            return None;
        }
        waypoints.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Some(Self {
            waypoints,
            loop_mode,
        })
    }

    /// Create a static point (single waypoint).
    pub fn static_point(x: f64, y: f64) -> Self {
        Self {
            waypoints: vec![Waypoint {
                time: 0.0,
                x,
                y,
                easing: EasingFunction::Linear,
            }],
            loop_mode: LoopMode::Clamp,
        }
    }

    /// Access the underlying waypoints.
    pub fn waypoints(&self) -> &[Waypoint] {
        &self.waypoints
    }

    /// Evaluate the motion path at absolute time `t` (seconds).
    /// Returns (x, y) in normalized layout space.
    ///
    /// Uses binary search for O(log n) lookup with eased interpolation.
    #[allow(clippy::indexing_slicing, clippy::cast_possible_truncation)]
    pub fn evaluate(&self, t: f64) -> (f64, f64) {
        let n = self.waypoints.len();
        if n == 1 {
            return (self.waypoints[0].x, self.waypoints[0].y);
        }

        let first_t = self.waypoints[0].time;
        let last_t = self.waypoints[n - 1].time;
        let duration = last_t - first_t;

        // Map time according to loop mode
        let t = if duration <= 0.0 {
            first_t
        } else {
            let normalized = if t < first_t {
                0.0
            } else {
                (t - first_t) / duration
            };
            first_t + self.loop_mode.map_normalized(normalized) * duration
        };

        // Binary search: find first waypoint with time > t
        let idx = self.waypoints.partition_point(|w| w.time <= t);

        if idx == 0 {
            return (self.waypoints[0].x, self.waypoints[0].y);
        }
        if idx >= n {
            return (self.waypoints[n - 1].x, self.waypoints[n - 1].y);
        }

        let a = &self.waypoints[idx - 1];
        let b = &self.waypoints[idx];
        let dt = b.time - a.time;

        if dt <= 0.0 {
            return (b.x, b.y);
        }

        // Normalize time within this segment, then apply easing
        let seg_t = (t - a.time) / dt;
        let eased = b.easing.evaluate(seg_t);

        // Handle Hold easing: stay at previous position until snap
        if b.easing == EasingFunction::Hold {
            return (a.x, a.y);
        }

        // Lerp between waypoints
        let x = a.x + (b.x - a.x) * eased;
        let y = a.y + (b.y - a.y) * eased;
        (x, y)
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn approx_pair(a: (f64, f64), b: (f64, f64)) -> bool {
        approx(a.0, b.0) && approx(a.1, b.1)
    }

    #[test]
    fn single_waypoint_static() {
        let path = MotionPath::static_point(0.5, 0.3);
        assert!(approx_pair(path.evaluate(0.0), (0.5, 0.3)));
        assert!(approx_pair(path.evaluate(100.0), (0.5, 0.3)));
    }

    #[test]
    fn new_requires_at_least_one() {
        assert!(MotionPath::new(vec![], LoopMode::Clamp).is_none());
        assert!(MotionPath::new(
            vec![Waypoint {
                time: 0.0,
                x: 0.0,
                y: 0.0,
                easing: EasingFunction::Linear
            }],
            LoopMode::Clamp,
        )
        .is_some());
    }

    #[test]
    fn linear_interpolation() {
        let path = MotionPath::new(
            vec![
                Waypoint {
                    time: 0.0,
                    x: 0.0,
                    y: 0.0,
                    easing: EasingFunction::Linear,
                },
                Waypoint {
                    time: 1.0,
                    x: 1.0,
                    y: 1.0,
                    easing: EasingFunction::Linear,
                },
            ],
            LoopMode::Clamp,
        )
        .unwrap();

        assert!(approx_pair(path.evaluate(0.0), (0.0, 0.0)));
        assert!(approx_pair(path.evaluate(0.5), (0.5, 0.5)));
        assert!(approx_pair(path.evaluate(1.0), (1.0, 1.0)));
    }

    #[test]
    fn clamp_before_and_after() {
        let path = MotionPath::new(
            vec![
                Waypoint {
                    time: 1.0,
                    x: 0.2,
                    y: 0.3,
                    easing: EasingFunction::Linear,
                },
                Waypoint {
                    time: 3.0,
                    x: 0.8,
                    y: 0.7,
                    easing: EasingFunction::Linear,
                },
            ],
            LoopMode::Clamp,
        )
        .unwrap();

        // Before first waypoint
        assert!(approx_pair(path.evaluate(0.0), (0.2, 0.3)));
        // After last waypoint
        assert!(approx_pair(path.evaluate(5.0), (0.8, 0.7)));
    }

    #[test]
    fn loop_mode_wraps() {
        let path = MotionPath::new(
            vec![
                Waypoint {
                    time: 0.0,
                    x: 0.0,
                    y: 0.0,
                    easing: EasingFunction::Linear,
                },
                Waypoint {
                    time: 2.0,
                    x: 1.0,
                    y: 1.0,
                    easing: EasingFunction::Linear,
                },
            ],
            LoopMode::Loop,
        )
        .unwrap();

        // At t=1.0 (midpoint)
        assert!(approx_pair(path.evaluate(1.0), (0.5, 0.5)));
        // At t=2.0 (wraps to 0.0)
        assert!(approx_pair(path.evaluate(2.0), (0.0, 0.0)));
        // At t=3.0 (wraps to 1.0)
        assert!(approx_pair(path.evaluate(3.0), (0.5, 0.5)));
    }

    #[test]
    fn ping_pong_reverses() {
        let path = MotionPath::new(
            vec![
                Waypoint {
                    time: 0.0,
                    x: 0.0,
                    y: 0.0,
                    easing: EasingFunction::Linear,
                },
                Waypoint {
                    time: 1.0,
                    x: 1.0,
                    y: 1.0,
                    easing: EasingFunction::Linear,
                },
            ],
            LoopMode::PingPong,
        )
        .unwrap();

        // Forward pass
        assert!(approx_pair(path.evaluate(0.5), (0.5, 0.5)));
        // Reverse pass at t=1.5 (should be at 0.5 going back)
        let pos = path.evaluate(1.5);
        assert!(approx_pair(pos, (0.5, 0.5)));
        // At t=2.0 (back to start)
        let pos = path.evaluate(2.0);
        assert!(approx_pair(pos, (0.0, 0.0)));
    }

    #[test]
    fn hold_easing_snaps() {
        let path = MotionPath::new(
            vec![
                Waypoint {
                    time: 0.0,
                    x: 0.0,
                    y: 0.0,
                    easing: EasingFunction::Linear,
                },
                Waypoint {
                    time: 1.0,
                    x: 1.0,
                    y: 1.0,
                    easing: EasingFunction::Hold,
                },
            ],
            LoopMode::Clamp,
        )
        .unwrap();

        // During transition, should stay at previous position
        assert!(approx_pair(path.evaluate(0.5), (0.0, 0.0)));
        // At destination, should be at destination
        assert!(approx_pair(path.evaluate(1.0), (1.0, 1.0)));
    }

    #[test]
    fn three_waypoints() {
        let path = MotionPath::new(
            vec![
                Waypoint {
                    time: 0.0,
                    x: 0.0,
                    y: 0.0,
                    easing: EasingFunction::Linear,
                },
                Waypoint {
                    time: 1.0,
                    x: 0.5,
                    y: 1.0,
                    easing: EasingFunction::Linear,
                },
                Waypoint {
                    time: 2.0,
                    x: 1.0,
                    y: 0.0,
                    easing: EasingFunction::Linear,
                },
            ],
            LoopMode::Clamp,
        )
        .unwrap();

        assert!(approx_pair(path.evaluate(0.0), (0.0, 0.0)));
        assert!(approx_pair(path.evaluate(0.5), (0.25, 0.5)));
        assert!(approx_pair(path.evaluate(1.0), (0.5, 1.0)));
        assert!(approx_pair(path.evaluate(1.5), (0.75, 0.5)));
        assert!(approx_pair(path.evaluate(2.0), (1.0, 0.0)));
    }

    #[test]
    fn sorts_waypoints_by_time() {
        let path = MotionPath::new(
            vec![
                Waypoint {
                    time: 2.0,
                    x: 1.0,
                    y: 1.0,
                    easing: EasingFunction::Linear,
                },
                Waypoint {
                    time: 0.0,
                    x: 0.0,
                    y: 0.0,
                    easing: EasingFunction::Linear,
                },
            ],
            LoopMode::Clamp,
        )
        .unwrap();

        assert!(approx_pair(path.evaluate(1.0), (0.5, 0.5)));
    }

    #[test]
    fn serde_roundtrip() {
        let path = MotionPath::new(
            vec![
                Waypoint {
                    time: 0.0,
                    x: 0.0,
                    y: 0.0,
                    easing: EasingFunction::Linear,
                },
                Waypoint {
                    time: 1.0,
                    x: 1.0,
                    y: 1.0,
                    easing: EasingFunction::EaseInOut,
                },
            ],
            LoopMode::PingPong,
        )
        .unwrap();

        let json = serde_json::to_string(&path).unwrap();
        let back: MotionPath = serde_json::from_str(&json).unwrap();
        assert_eq!(back.waypoints.len(), 2);
        assert_eq!(back.loop_mode, LoopMode::PingPong);
    }
}
