use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Easing function for interpolation between keyframes.
///
/// Standard easing curves following CSS/web animation conventions.
/// `evaluate(t)` maps normalized input [0,1] to eased output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EasingFunction {
    /// Snap to destination (no interpolation).
    Hold,
    /// Constant-speed interpolation.
    #[default]
    Linear,
    // Quadratic
    EaseIn,
    EaseOut,
    EaseInOut,
    // Cubic
    EaseInCubic,
    EaseOutCubic,
    EaseInOutCubic,
    // Quartic
    EaseInQuart,
    EaseOutQuart,
    EaseInOutQuart,
    // Sinusoidal
    EaseInSine,
    EaseOutSine,
    EaseInOutSine,
    // Exponential
    EaseInExpo,
    EaseOutExpo,
    EaseInOutExpo,
    // Back (overshoot)
    EaseInBack,
    EaseOutBack,
    EaseInOutBack,
    // Elastic (spring)
    EaseInElastic,
    EaseOutElastic,
    EaseInOutElastic,
    // Circular
    EaseInCirc,
    EaseOutCirc,
    EaseInOutCirc,
    // Bounce
    EaseInBounce,
    EaseOutBounce,
    EaseInOutBounce,
}

impl EasingFunction {
    /// Evaluate the easing function at normalized time `t` (clamped to [0,1]).
    /// Returns the eased value, typically in [0,1] but may overshoot for
    /// Back and Elastic easings.
    #[allow(
        clippy::many_single_char_names,
        clippy::float_cmp,
        clippy::unreadable_literal
    )]
    pub fn evaluate(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Hold => 0.0, // stays at origin; snap happens at destination waypoint
            Self::Linear => t,

            // Quadratic
            Self::EaseIn => t * t,
            Self::EaseOut => t * (2.0 - t),
            Self::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }

            // Cubic
            Self::EaseInCubic => t * t * t,
            Self::EaseOutCubic => {
                let u = t - 1.0;
                u * u * u + 1.0
            }
            Self::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let u = 2.0 * t - 2.0;
                    0.5 * u * u * u + 1.0
                }
            }

            // Quartic
            Self::EaseInQuart => t * t * t * t,
            Self::EaseOutQuart => {
                let u = t - 1.0;
                1.0 - u * u * u * u
            }
            Self::EaseInOutQuart => {
                if t < 0.5 {
                    8.0 * t * t * t * t
                } else {
                    let u = t - 1.0;
                    1.0 - 8.0 * u * u * u * u
                }
            }

            // Sinusoidal
            Self::EaseInSine => 1.0 - (t * std::f64::consts::FRAC_PI_2).cos(),
            Self::EaseOutSine => (t * std::f64::consts::FRAC_PI_2).sin(),
            Self::EaseInOutSine => 0.5 * (1.0 - (std::f64::consts::PI * t).cos()),

            // Exponential
            Self::EaseInExpo => {
                if t == 0.0 {
                    0.0
                } else {
                    (2.0f64).powf(10.0 * (t - 1.0))
                }
            }
            Self::EaseOutExpo => {
                if t == 1.0 {
                    1.0
                } else {
                    1.0 - (2.0f64).powf(-10.0 * t)
                }
            }
            Self::EaseInOutExpo => {
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else if t < 0.5 {
                    0.5 * (2.0f64).powf(20.0 * t - 10.0)
                } else {
                    1.0 - 0.5 * (2.0f64).powf(-20.0 * t + 10.0)
                }
            }

            // Back (overshoot)
            Self::EaseInBack => {
                const C: f64 = 1.70158;
                (C + 1.0) * t * t * t - C * t * t
            }
            Self::EaseOutBack => {
                const C: f64 = 1.70158;
                let u = t - 1.0;
                1.0 + (C + 1.0) * u * u * u + C * u * u
            }
            Self::EaseInOutBack => {
                const C: f64 = 1.70158 * 1.525;
                if t < 0.5 {
                    let u = 2.0 * t;
                    0.5 * (u * u * ((C + 1.0) * u - C))
                } else {
                    let u = 2.0 * t - 2.0;
                    0.5 * (u * u * ((C + 1.0) * u + C) + 2.0)
                }
            }

            // Elastic (spring)
            Self::EaseInElastic => {
                if t == 0.0 || t == 1.0 {
                    t
                } else {
                    let p = 0.3;
                    -(2.0f64).powf(10.0 * (t - 1.0))
                        * ((t - 1.0 - p / 4.0) * std::f64::consts::TAU / p).sin()
                }
            }
            Self::EaseOutElastic => {
                if t == 0.0 || t == 1.0 {
                    t
                } else {
                    let p = 0.3;
                    (2.0f64).powf(-10.0 * t) * ((t - p / 4.0) * std::f64::consts::TAU / p).sin()
                        + 1.0
                }
            }
            Self::EaseInOutElastic => {
                if t == 0.0 || t == 1.0 {
                    t
                } else {
                    let p = 0.3 * 1.5;
                    let s = p / 4.0;
                    if t < 0.5 {
                        let u = 2.0 * t - 1.0;
                        -0.5 * (2.0f64).powf(10.0 * u) * ((u - s) * std::f64::consts::TAU / p).sin()
                    } else {
                        let u = 2.0 * t - 1.0;
                        0.5 * (2.0f64).powf(-10.0 * u) * ((u - s) * std::f64::consts::TAU / p).sin()
                            + 1.0
                    }
                }
            }

            // Circular
            Self::EaseInCirc => 1.0 - (1.0 - t * t).sqrt(),
            Self::EaseOutCirc => {
                let u = t - 1.0;
                (1.0 - u * u).sqrt()
            }
            Self::EaseInOutCirc => {
                if t < 0.5 {
                    0.5 * (1.0 - (1.0 - 4.0 * t * t).sqrt())
                } else {
                    let u = 2.0 * t - 2.0;
                    0.5 * ((1.0 - u * u).sqrt() + 1.0)
                }
            }

            // Bounce
            Self::EaseInBounce => 1.0 - Self::EaseOutBounce.evaluate(1.0 - t),
            Self::EaseOutBounce => bounce_out(t),
            Self::EaseInOutBounce => {
                if t < 0.5 {
                    0.5 * (1.0 - bounce_out(1.0 - 2.0 * t))
                } else {
                    0.5 * bounce_out(2.0 * t - 1.0) + 0.5
                }
            }
        }
    }
}

/// Bounce-out helper (standard implementation).
#[allow(clippy::unreadable_literal)]
fn bounce_out(t: f64) -> f64 {
    const N: f64 = 7.5625;
    const D: f64 = 2.75;

    if t < 1.0 / D {
        N * t * t
    } else if t < 2.0 / D {
        let t = t - 1.5 / D;
        N * t * t + 0.75
    } else if t < 2.5 / D {
        let t = t - 2.25 / D;
        N * t * t + 0.9375
    } else {
        let t = t - 2.625 / D;
        N * t * t + 0.984375
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn linear_identity() {
        assert!(approx(EasingFunction::Linear.evaluate(0.0), 0.0));
        assert!(approx(EasingFunction::Linear.evaluate(0.5), 0.5));
        assert!(approx(EasingFunction::Linear.evaluate(1.0), 1.0));
    }

    #[test]
    fn hold_always_zero() {
        assert!(approx(EasingFunction::Hold.evaluate(0.0), 0.0));
        assert!(approx(EasingFunction::Hold.evaluate(0.5), 0.0));
        assert!(approx(EasingFunction::Hold.evaluate(1.0), 0.0));
    }

    #[test]
    fn all_easings_start_end() {
        // Most easings satisfy f(0) ≈ 0 and f(1) ≈ 1 (except Hold).
        let easings = [
            EasingFunction::Linear,
            EasingFunction::EaseIn,
            EasingFunction::EaseOut,
            EasingFunction::EaseInOut,
            EasingFunction::EaseInCubic,
            EasingFunction::EaseOutCubic,
            EasingFunction::EaseInOutCubic,
            EasingFunction::EaseInQuart,
            EasingFunction::EaseOutQuart,
            EasingFunction::EaseInOutQuart,
            EasingFunction::EaseInSine,
            EasingFunction::EaseOutSine,
            EasingFunction::EaseInOutSine,
            EasingFunction::EaseInExpo,
            EasingFunction::EaseOutExpo,
            EasingFunction::EaseInOutExpo,
            EasingFunction::EaseInBack,
            EasingFunction::EaseOutBack,
            EasingFunction::EaseInOutBack,
            EasingFunction::EaseInElastic,
            EasingFunction::EaseOutElastic,
            EasingFunction::EaseInOutElastic,
            EasingFunction::EaseInCirc,
            EasingFunction::EaseOutCirc,
            EasingFunction::EaseInOutCirc,
            EasingFunction::EaseInBounce,
            EasingFunction::EaseOutBounce,
            EasingFunction::EaseInOutBounce,
        ];
        for e in easings {
            assert!(
                approx(e.evaluate(0.0), 0.0),
                "{e:?}: f(0) = {} (expected 0)",
                e.evaluate(0.0)
            );
            assert!(
                approx(e.evaluate(1.0), 1.0),
                "{e:?}: f(1) = {} (expected 1)",
                e.evaluate(1.0)
            );
        }
    }

    #[test]
    fn clamping() {
        // Input outside [0,1] should be clamped
        assert!(approx(EasingFunction::Linear.evaluate(-0.5), 0.0));
        assert!(approx(EasingFunction::Linear.evaluate(1.5), 1.0));
    }

    #[test]
    fn ease_in_slow_start() {
        // Ease-in at t=0.5 should be less than 0.5
        assert!(EasingFunction::EaseIn.evaluate(0.5) < 0.5);
        assert!(EasingFunction::EaseInCubic.evaluate(0.5) < 0.5);
        assert!(EasingFunction::EaseInQuart.evaluate(0.5) < 0.5);
    }

    #[test]
    fn ease_out_fast_start() {
        // Ease-out at t=0.5 should be greater than 0.5
        assert!(EasingFunction::EaseOut.evaluate(0.5) > 0.5);
        assert!(EasingFunction::EaseOutCubic.evaluate(0.5) > 0.5);
        assert!(EasingFunction::EaseOutQuart.evaluate(0.5) > 0.5);
    }

    #[test]
    fn bounce_out_monotonic_end() {
        // Bounce should reach 1.0 at t=1.0
        assert!(approx(EasingFunction::EaseOutBounce.evaluate(1.0), 1.0));
    }

    #[test]
    fn default_is_linear() {
        assert_eq!(EasingFunction::default(), EasingFunction::Linear);
    }

    #[test]
    fn serde_roundtrip() {
        let e = EasingFunction::EaseInOutCubic;
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, "\"ease_in_out_cubic\"");
        let back: EasingFunction = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
    }
}
