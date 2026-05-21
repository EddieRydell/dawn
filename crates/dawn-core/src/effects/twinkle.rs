use crate::model::show::Position2D;
use crate::model::{
    BlendMode, Color, ColorGradient, ColorMode, EffectParams, ParamKey, ParamSchema, ParamType,
    ParamValue,
};

use super::defaults::{
    DEFAULT_BACKGROUND_LEVEL, DEFAULT_COLOR_MODE, DEFAULT_DENSITY, DEFAULT_WHITE_GRADIENT,
};
use super::{blend_pixel, deterministic_hash};

const DEFAULT_TWINKLE_SPEED: f64 = 5.0;
const DEFAULT_MAX_LEVEL: f64 = 1.0;

struct Resolved<'a> {
    gradient: &'a ColorGradient,
    color_mode: ColorMode,
    speed: f64,
    min_level: f64,
    max_level: f64,
    threshold: f64,
    inv_density: f64,
}

impl<'a> Resolved<'a> {
    fn new(params: &'a EffectParams) -> Self {
        let density = params
            .float_or(ParamKey::Density, DEFAULT_DENSITY)
            .clamp(0.0, 1.0);
        let min_level = params
            .float_or(ParamKey::BackgroundLevel, DEFAULT_BACKGROUND_LEVEL)
            .clamp(0.0, 1.0);
        let max_level = params
            .float_or(ParamKey::MaxLevel, DEFAULT_MAX_LEVEL)
            .clamp(0.0, 1.0);
        Self {
            gradient: params.gradient_or(ParamKey::Gradient, &DEFAULT_WHITE_GRADIENT),
            color_mode: params.color_mode_or(ParamKey::ColorMode, DEFAULT_COLOR_MODE),
            speed: params.float_or(ParamKey::Speed, DEFAULT_TWINKLE_SPEED),
            min_level,
            max_level,
            threshold: 1.0 - density,
            inv_density: if density > 0.0 { 1.0 / density } else { 0.0 },
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn compute(&self, t: f64, pixel_index: usize, pos: f64) -> Color {
        let slot = (t * self.speed) as u64;
        let frac = (t * self.speed).fract();
        let brightness_current = deterministic_hash(pixel_index, slot);
        let brightness_next = deterministic_hash(pixel_index, slot + 1);
        let raw_brightness = brightness_current * (1.0 - frac) + brightness_next * frac;

        // Intensity: map hash value through min/max level range
        // Vixen: each twinkle has a triangle envelope between min_level and max_level.
        // Our hash-based approach maps the hash to a continuous brightness in that range.
        let intensity = if raw_brightness > self.threshold {
            // Active twinkle: scale between min_level and max_level
            let twinkle_intensity = (raw_brightness - self.threshold) * self.inv_density;
            self.min_level + twinkle_intensity * (self.max_level - self.min_level)
        } else {
            // Not twinkling: hold at min_level
            self.min_level
        };

        if intensity <= 0.0 {
            return Color::BLACK;
        }

        // Sample color: use hash as pulse_pos for GradientPerPulse
        let pulse_pos = if raw_brightness > self.threshold {
            Some((raw_brightness - self.threshold) * self.inv_density)
        } else {
            None
        };
        let color =
            super::resolve_gradient_color(self.color_mode, self.gradient, t, pos, pulse_pos);

        color.scale(intensity)
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
        let effect_color = r.compute(t, global_offset + i, pos);
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
    Resolved::new(params).compute(t, pixel_index, pos)
}

pub fn schema() -> Vec<ParamSchema> {
    vec![
        super::defaults::gradient_schema(),
        super::defaults::color_mode_schema(),
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
            key: ParamKey::Speed,
            label: "Speed".into(),
            param_type: ParamType::Float {
                min: 0.5,
                max: 30.0,
                step: 0.5,
            },
            default: ParamValue::Float(DEFAULT_TWINKLE_SPEED),
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
            key: ParamKey::MaxLevel,
            label: "Max Level".into(),
            param_type: ParamType::Float {
                min: 0.0,
                max: 1.0,
                step: 0.01,
            },
            default: ParamValue::Float(DEFAULT_MAX_LEVEL),
        },
    ]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_same_inputs_same_output() {
        let params = EffectParams::new();
        let a = evaluate_single(0.3, 5, 100, &params);
        let b = evaluate_single(0.3, 5, 100, &params);
        assert_eq!(a, b);
    }

    #[test]
    fn spatial_variation_exists() {
        let params = EffectParams::new().set(ParamKey::Density, ParamValue::Float(0.5));
        // Check several pixels — at least some should differ
        let colors: Vec<_> = (0..20)
            .map(|i| evaluate_single(0.0, i, 20, &params))
            .collect();
        let all_same = colors.windows(2).all(|w| w[0] == w[1]);
        assert!(!all_same, "twinkle should produce spatial variation");
    }

    #[test]
    fn min_level_provides_baseline() {
        let params = EffectParams::new()
            .set(ParamKey::Density, ParamValue::Float(0.3))
            .set(ParamKey::BackgroundLevel, ParamValue::Float(0.5)); // min_level = 0.5
                                                                     // All pixels should have at least 50% brightness
        for i in 0..50 {
            let c = evaluate_single(0.0, i, 50, &params);
            // min_level 0.5 * 255 = 127
            assert!(
                c.r >= 127 || c.g >= 127 || c.b >= 127,
                "pixel {i} should be at least 50% bright, got r={}",
                c.r
            );
        }
    }

    #[test]
    fn max_level_caps_brightness() {
        let params = EffectParams::new()
            .set(ParamKey::Density, ParamValue::Float(1.0)) // all pixels twinkle
            .set(ParamKey::MaxLevel, ParamValue::Float(0.5));
        // All pixels should be capped at 50% brightness
        for i in 0..50 {
            let c = evaluate_single(0.0, i, 50, &params);
            assert!(
                c.r <= 128,
                "pixel {i} should be at most 50% bright, got r={}",
                c.r
            );
        }
    }

    /// Vixen parity: AverageCoverage=80 → density=0.8, MinLevel=0, MaxLevel=1
    /// Most pixels should be twinkling at any given time.
    #[test]
    fn vixen_parity_high_coverage() {
        let params = EffectParams::new()
            .set(ParamKey::Density, ParamValue::Float(0.8))
            .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0))
            .set(ParamKey::MaxLevel, ParamValue::Float(1.0));

        let mut lit_count = 0;
        for i in 0..100 {
            let c = evaluate_single(0.0, i, 100, &params);
            if c.r > 0 || c.g > 0 || c.b > 0 {
                lit_count += 1;
            }
        }
        // With 80% density, roughly 80% of pixels should be lit
        assert!(lit_count > 60, "expected ~80% lit, got {lit_count}%");
    }

    #[test]
    fn gradient_colors_used() {
        use crate::model::ColorStop;
        let gradient = ColorGradient::new(vec![
            ColorStop {
                position: 0.0,
                color: Color::rgb(255, 0, 0),
            },
            ColorStop {
                position: 1.0,
                color: Color::rgb(0, 0, 255),
            },
        ])
        .unwrap();
        let params = EffectParams::new()
            .set(ParamKey::Gradient, ParamValue::ColorGradient(gradient))
            .set(
                ParamKey::ColorMode,
                ParamValue::ColorMode(ColorMode::GradientAcrossItems),
            )
            .set(ParamKey::Density, ParamValue::Float(1.0));

        // Pixel at start should be reddish, pixel at end should be bluish
        let start = evaluate_single(0.0, 0, 100, &params);
        let end = evaluate_single(0.0, 99, 100, &params);
        // They may not be pure red/blue due to intensity variation, but the ratios should differ
        assert!(
            start.r > start.b || start.r == 0,
            "start pixel should be more red"
        );
        assert!(end.b > end.r || end.b == 0, "end pixel should be more blue");
    }
}
