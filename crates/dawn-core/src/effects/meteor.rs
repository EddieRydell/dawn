use crate::model::show::Position2D;
use crate::model::{
    BlendMode, Color, ColorGradient, ColorMode, EffectParams, ParamKey, ParamSchema, ParamType,
    ParamValue,
};

use super::defaults::{
    DEFAULT_BACKGROUND_LEVEL, DEFAULT_COLOR_MODE, DEFAULT_DENSITY, DEFAULT_REVERSE,
    DEFAULT_WHITE_GRADIENT,
};
use super::{blend_pixel, deterministic_hash};

/// Meteors default faster than other effects to ensure visible motion.
const DEFAULT_METEOR_SPEED: f64 = 3.0;
const DEFAULT_TAIL_LENGTH: f64 = 0.3;

/// Maximum concurrent meteor slots. Density maps [0,1] → [1, MAX_SLOTS].
const MAX_SLOTS: usize = 16;

struct Resolved<'a> {
    gradient: &'a ColorGradient,
    color_mode: ColorMode,
    speed: f64,
    tail_length: f64,
    num_meteors: usize,
    bg_level: f64,
    reverse: bool,
}

impl<'a> Resolved<'a> {
    #[allow(clippy::cast_precision_loss)]
    fn new(params: &'a EffectParams) -> Self {
        let density = params
            .float_or(ParamKey::Density, DEFAULT_DENSITY)
            .clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let num_meteors = ((density * MAX_SLOTS as f64).ceil() as usize).max(1);
        Self {
            gradient: params.gradient_or(ParamKey::Gradient, &DEFAULT_WHITE_GRADIENT),
            color_mode: params.color_mode_or(ParamKey::ColorMode, DEFAULT_COLOR_MODE),
            speed: params.float_or(ParamKey::Speed, DEFAULT_METEOR_SPEED),
            tail_length: params
                .float_or(ParamKey::TailLength, DEFAULT_TAIL_LENGTH)
                .clamp(0.01, 1.0),
            num_meteors,
            bg_level: params
                .float_or(ParamKey::BackgroundLevel, DEFAULT_BACKGROUND_LEVEL)
                .clamp(0.0, 1.0),
            reverse: params.bool_or(ParamKey::Reverse, DEFAULT_REVERSE),
        }
    }

    fn compute(&self, t: f64, pos: f64) -> Color {
        // Each meteor travels a distance of (1 + tail_length) so the tail
        // fully enters and exits the visible [0, 1] range.
        let travel = 1.0 + self.tail_length;
        let mut max_intensity = self.bg_level;

        for slot in 0..self.num_meteors {
            // Stagger each meteor's start position via hash
            let offset = deterministic_hash(slot, 42);
            // Phase: how far through the travel cycle this meteor is
            let phase = ((t * self.speed + offset * travel) % travel).abs();

            // Head position in pixel-space.
            // Forward: head enters at 0 and exits at 1 (tail trails behind toward 0).
            // The phase maps [0, travel] → head at [0, 1+tail], but we only care
            // about the head relative to [0, 1] pixels.
            let head_pos = if self.reverse {
                // Reverse: head enters at 1 and exits at 0, tail trails toward 1
                1.0 - phase + self.tail_length
            } else {
                phase
            };

            // Distance from head (positive = behind head, in the tail)
            let dist_from_head = if self.reverse {
                pos - head_pos
            } else {
                head_pos - pos
            };

            if dist_from_head >= 0.0 && dist_from_head <= self.tail_length {
                let frac = 1.0 - (dist_from_head / self.tail_length);
                // Quadratic falloff: bright at head, fading along tail
                let intensity = frac * frac;
                if intensity > max_intensity {
                    max_intensity = intensity;
                }
            }
        }

        if max_intensity <= 0.0 {
            return Color::BLACK;
        }

        let color = super::resolve_gradient_color(
            self.color_mode,
            self.gradient,
            t,
            pos,
            Some(max_intensity),
        );
        color.scale(max_intensity)
    }
}

/// Batch evaluate: extract params once, loop over pixels.
#[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
pub fn evaluate_pixels_batch(
    t: f64,
    dest: &mut [Color],
    global_offset: usize,
    total_pixels: usize,
    params: &EffectParams,
    blend_mode: BlendMode,
    opacity: f64,
    _positions: Option<&[Position2D]>,
) {
    let r = Resolved::new(params);
    let inv_total = super::linear_pixel_scale(total_pixels);

    for (i, pixel) in dest.iter_mut().enumerate() {
        let pos = ((global_offset + i) as f64) * inv_total;
        let effect_color = r.compute(t, pos);
        blend_pixel(pixel, effect_color, blend_mode, opacity);
    }
}

#[allow(clippy::cast_precision_loss)]
pub fn evaluate_single(
    t: f64,
    pixel_index: usize,
    pixel_count: usize,
    params: &EffectParams,
) -> Color {
    let pos = (pixel_index as f64) * super::linear_pixel_scale(pixel_count);
    Resolved::new(params).compute(t, pos)
}

pub fn schema() -> Vec<ParamSchema> {
    vec![
        super::defaults::gradient_schema(),
        super::defaults::color_mode_schema(),
        ParamSchema {
            key: ParamKey::Speed,
            label: "Speed".into(),
            param_type: ParamType::Float {
                min: 0.5,
                max: 30.0,
                step: 0.5,
            },
            default: ParamValue::Float(DEFAULT_METEOR_SPEED),
        },
        ParamSchema {
            key: ParamKey::Density,
            label: "Density".into(),
            param_type: ParamType::Float {
                min: 0.0,
                max: 1.0,
                step: 0.01,
            },
            default: ParamValue::Float(DEFAULT_DENSITY),
        },
        ParamSchema {
            key: ParamKey::TailLength,
            label: "Tail Length".into(),
            param_type: ParamType::Float {
                min: 0.01,
                max: 1.0,
                step: 0.01,
            },
            default: ParamValue::Float(DEFAULT_TAIL_LENGTH),
        },
        ParamSchema {
            key: ParamKey::BackgroundLevel,
            label: "Background Level".into(),
            param_type: ParamType::Float {
                min: 0.0,
                max: 1.0,
                step: 0.01,
            },
            default: ParamValue::Float(DEFAULT_BACKGROUND_LEVEL),
        },
        ParamSchema {
            key: ParamKey::Reverse,
            label: "Reverse".into(),
            param_type: ParamType::Bool,
            default: ParamValue::Bool(DEFAULT_REVERSE),
        },
    ]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_output() {
        let params = EffectParams::new();
        let a = evaluate_single(0.3, 5, 100, &params);
        let b = evaluate_single(0.3, 5, 100, &params);
        assert_eq!(a, b);
    }

    #[test]
    fn spatial_variation_exists() {
        let params = EffectParams::new()
            .set(ParamKey::Density, ParamValue::Float(0.5))
            .set(ParamKey::Speed, ParamValue::Float(2.0));
        let colors: Vec<_> = (0..50)
            .map(|i| evaluate_single(0.3, i, 50, &params))
            .collect();
        let all_same = colors.windows(2).all(|w| w[0] == w[1]);
        assert!(!all_same, "meteor should produce spatial variation");
    }

    #[test]
    fn tail_fades_behind_head() {
        // Single meteor (density = low), known time
        let params = EffectParams::new()
            .set(ParamKey::Density, ParamValue::Float(0.1)) // ~1 meteor
            .set(ParamKey::Speed, ParamValue::Float(1.0))
            .set(ParamKey::TailLength, ParamValue::Float(0.5))
            .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0));

        // Find the brightest pixel at t=0.3
        let strip: Vec<_> = (0..100)
            .map(|i| evaluate_single(0.3, i, 100, &params))
            .collect();

        let max_brightness = strip.iter().map(|c| c.r).max().unwrap_or(0);
        // There should be some lit pixels
        assert!(max_brightness > 0, "should have visible meteors");

        // Background pixels should be black
        let dark_count = strip.iter().filter(|c| c.r == 0).count();
        assert!(dark_count > 20, "most pixels should be dark outside tails");
    }

    #[test]
    fn reverse_changes_direction() {
        let base = EffectParams::new()
            .set(ParamKey::Density, ParamValue::Float(0.2))
            .set(ParamKey::Speed, ParamValue::Float(2.0))
            .set(ParamKey::TailLength, ParamValue::Float(0.3));

        let fwd: Vec<_> = (0..50)
            .map(|i| evaluate_single(0.3, i, 50, &base))
            .collect();
        let rev: Vec<_> = (0..50)
            .map(|i| {
                evaluate_single(
                    0.3,
                    i,
                    50,
                    &base.clone().set(ParamKey::Reverse, ParamValue::Bool(true)),
                )
            })
            .collect();

        // Forward and reverse should produce different patterns
        assert_ne!(fwd, rev, "reverse should change the pattern");
    }

    #[test]
    fn background_level_baseline() {
        let params = EffectParams::new()
            .set(ParamKey::Density, ParamValue::Float(0.1))
            .set(ParamKey::BackgroundLevel, ParamValue::Float(0.3));

        // All pixels should be at least 30% bright
        for i in 0..50 {
            let c = evaluate_single(0.0, i, 50, &params);
            // 0.3 * 255 ≈ 76
            assert!(
                c.r >= 74,
                "pixel {i} should be at least bg level, got r={}",
                c.r
            );
        }
    }
}
