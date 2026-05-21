use crate::model::show::Position2D;
use crate::model::{
    BlendMode, Color, ColorGradient, ColorMode, Curve, EffectParams, ParamKey, ParamSchema,
    ParamType, ParamValue,
};

use super::blend_pixel;
use super::defaults::{DEFAULT_COLOR_MODE, DEFAULT_WHITE_GRADIENT};

const DEFAULT_RATE: f64 = 10.0;
const DEFAULT_DUTY_CYCLE: f64 = 0.5;

struct Resolved<'a> {
    gradient: &'a ColorGradient,
    intensity_curve: Option<&'a Curve>,
    color_mode: ColorMode,
    rate: f64,
    duty_cycle: f64,
}

impl<'a> Resolved<'a> {
    fn new(params: &'a EffectParams) -> Self {
        Self {
            gradient: params.gradient_or(ParamKey::Gradient, &DEFAULT_WHITE_GRADIENT),
            intensity_curve: params
                .get(&ParamKey::IntensityCurve)
                .and_then(|v| v.as_curve()),
            color_mode: params.color_mode_or(ParamKey::ColorMode, DEFAULT_COLOR_MODE),
            rate: params.float_or(ParamKey::Rate, DEFAULT_RATE),
            duty_cycle: params
                .float_or(ParamKey::DutyCycle, DEFAULT_DUTY_CYCLE)
                .clamp(0.0, 1.0),
        }
    }

    fn compute(&self, t: f64, pos: f64) -> Color {
        let phase = (t * self.rate).fract();
        if phase >= self.duty_cycle {
            return Color::BLACK;
        }

        // Position within the on-phase [0, 1]
        let on_pos = if self.duty_cycle > 0.0 {
            phase / self.duty_cycle
        } else {
            0.0
        };

        // Sample color from gradient based on color mode
        let color =
            super::resolve_gradient_color(self.color_mode, self.gradient, t, pos, Some(on_pos));

        // Apply intensity curve if present (Vixen applies a brightness envelope per flash)
        if let Some(curve) = self.intensity_curve {
            color.scale(curve.evaluate(on_pos))
        } else {
            color
        }
    }
}

/// Batch evaluate: extract params once, compute per-pixel color, blend all pixels.
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

    // Fast path: if no gradient variation and no intensity curve, all pixels are identical
    let uniform = matches!(
        r.color_mode,
        ColorMode::Static | ColorMode::GradientThroughEffect
    ) || total_pixels <= 1;

    if uniform {
        let effect_color = r.compute(t, 0.0);
        for pixel in dest.iter_mut() {
            blend_pixel(pixel, effect_color, blend_mode, opacity);
        }
    } else {
        let inv_total = super::linear_pixel_scale(total_pixels);
        for (i, pixel) in dest.iter_mut().enumerate() {
            let pos = ((global_offset + i) as f64) * inv_total;
            let effect_color = r.compute(t, pos);
            blend_pixel(pixel, effect_color, blend_mode, opacity);
        }
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
            key: ParamKey::IntensityCurve,
            label: "Flash Curve".into(),
            param_type: ParamType::Curve,
            default: ParamValue::Curve(Curve::constant(1.0)),
        },
        ParamSchema {
            key: ParamKey::Rate,
            label: "Rate".into(),
            param_type: ParamType::Float {
                min: 1.0,
                max: 50.0,
                step: 0.5,
            },
            default: ParamValue::Float(DEFAULT_RATE),
        },
        ParamSchema {
            key: ParamKey::DutyCycle,
            label: "Duty Cycle".into(),
            param_type: ParamType::Float {
                min: 0.0,
                max: 1.0,
                step: 0.01,
            },
            default: ParamValue::Float(DEFAULT_DUTY_CYCLE),
        },
    ]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn on_during_first_half_of_cycle() {
        // rate=1.0, duty_cycle=0.5: on for t in [0, 0.5), off for [0.5, 1.0)
        let params = EffectParams::new()
            .set(ParamKey::Rate, ParamValue::Float(1.0))
            .set(ParamKey::DutyCycle, ParamValue::Float(0.5));

        let on = evaluate_single(0.0, 0, 1, &params);
        assert_eq!(on, Color::WHITE); // default gradient = white

        let off = evaluate_single(0.5, 0, 1, &params);
        assert_eq!(off, Color::BLACK);
    }

    #[test]
    fn custom_color_strobes() {
        let red = Color::rgb(255, 0, 0);
        let params = EffectParams::new()
            .set(
                ParamKey::Gradient,
                ParamValue::ColorGradient(ColorGradient::solid(red)),
            )
            .set(ParamKey::Rate, ParamValue::Float(1.0))
            .set(ParamKey::DutyCycle, ParamValue::Float(0.5));
        assert_eq!(evaluate_single(0.25, 0, 1, &params), red);
    }

    #[test]
    fn intensity_curve_shapes_flash() {
        // Triangle curve: 0→1→0 over the on-phase
        let params = EffectParams::new()
            .set(ParamKey::Rate, ParamValue::Float(1.0))
            .set(ParamKey::DutyCycle, ParamValue::Float(1.0))
            .set(
                ParamKey::IntensityCurve,
                ParamValue::Curve(Curve::triangle()),
            );

        // At t=0.0 (start of on-phase), triangle = 0 → black
        let at_start = evaluate_single(0.0, 0, 1, &params);
        assert_eq!(at_start, Color::BLACK);

        // At t=0.5 (middle of on-phase), triangle = 1.0 → full white
        let at_peak = evaluate_single(0.5, 0, 1, &params);
        assert_eq!(at_peak, Color::WHITE);
    }

    #[test]
    fn gradient_through_effect_changes_over_time() {
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
                ParamValue::ColorMode(ColorMode::GradientThroughEffect),
            )
            .set(ParamKey::Rate, ParamValue::Float(1.0))
            .set(ParamKey::DutyCycle, ParamValue::Float(1.0));

        let early = evaluate_single(0.0, 0, 1, &params);
        let late = evaluate_single(1.0, 0, 1, &params);
        // Early should be more red, late more blue
        assert!(early.r > late.r);
        assert!(late.b > early.b);
    }

    /// Vixen parity: CycleTime=150ms on a 1s effect → rate = 1000/150 ≈ 6.67 Hz.
    /// On-time is 50% of cycle. At t=0 (start of cycle), should be on.
    /// At t=0.075 (half cycle), should be off.
    #[test]
    fn vixen_parity_cycle_time_150ms() {
        let rate = 1000.0 / 150.0; // 6.667 Hz
        let params = EffectParams::new()
            .set(ParamKey::Rate, ParamValue::Float(rate))
            .set(ParamKey::DutyCycle, ParamValue::Float(0.5));

        // t=0.0 normalized → phase = 0.0 → on
        let on = evaluate_single(0.0, 0, 1, &params);
        assert!(on.r > 200, "should be on at start of cycle");

        // t at 75% through first cycle → phase = 0.75 → off (> 0.5 duty)
        let cycle_time = 1.0 / rate;
        let off_t = cycle_time * 0.75;
        let off = evaluate_single(off_t, 0, 1, &params);
        assert_eq!(off, Color::BLACK, "should be off past duty cycle");
    }
}
