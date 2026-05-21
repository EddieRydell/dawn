use crate::model::show::Position2D;
use crate::model::{
    BlendMode, Color, ColorGradient, ColorMode, Curve, EffectParams, ParamKey, ParamSchema,
    ParamType, ParamValue,
};

use super::blend_pixel;
use super::defaults::{DEFAULT_PULSE, DEFAULT_WHITE_GRADIENT};

/// Fade defaults to GradientThroughEffect (unlike most effects which use Static)
/// because fading through a gradient over time is the natural use case.
const FADE_DEFAULT_COLOR_MODE: ColorMode = ColorMode::GradientThroughEffect;

/// Core fade computation: apply intensity to a color sampled by color mode.
fn compute_pixel(
    t: f64,
    pos: f64,
    intensity: f64,
    gradient: &ColorGradient,
    color_mode: ColorMode,
) -> Color {
    // For fade, GradientPerPulse behaves like GradientThroughEffect (pulse_pos = t)
    let color = super::resolve_gradient_color(color_mode, gradient, t, pos, Some(t));
    color.scale(intensity)
}

struct Resolved<'a> {
    intensity_curve: &'a Curve,
    gradient: &'a ColorGradient,
    color_mode: ColorMode,
}

impl<'a> Resolved<'a> {
    fn new(params: &'a EffectParams) -> Self {
        Self {
            intensity_curve: params.curve_or(ParamKey::IntensityCurve, &DEFAULT_PULSE),
            gradient: params.gradient_or(ParamKey::Gradient, &DEFAULT_WHITE_GRADIENT),
            color_mode: params.color_mode_or(ParamKey::ColorMode, FADE_DEFAULT_COLOR_MODE),
        }
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
    let intensity = r.intensity_curve.evaluate(t);
    let inv_total = super::linear_pixel_scale(total_pixels);

    for (i, pixel) in dest.iter_mut().enumerate() {
        let pos = ((global_offset + i) as f64) * inv_total;
        let effect_color = compute_pixel(t, pos, intensity, r.gradient, r.color_mode);
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
    let r = Resolved::new(params);
    let intensity = r.intensity_curve.evaluate(t);
    let pos = (pixel_index as f64) * super::linear_pixel_scale(pixel_count);

    compute_pixel(t, pos, intensity, r.gradient, r.color_mode)
}

pub fn schema() -> Vec<ParamSchema> {
    use super::defaults::DEFAULT_PULSE;

    vec![
        ParamSchema {
            key: ParamKey::IntensityCurve,
            label: "Intensity Curve".into(),
            param_type: ParamType::Curve,
            default: ParamValue::Curve(DEFAULT_PULSE.clone()),
        },
        super::defaults::gradient_schema(),
        super::defaults::color_mode_schema(),
    ]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn intensity_follows_default_triangle_curve() {
        let params = EffectParams::new();
        // Default curve is triangle: 0→1→0
        // At t=0.0, intensity is 0 → output should be black
        let at_start = evaluate_single(0.0, 0, 10, &params);
        assert_eq!(at_start, Color::BLACK);

        // At t=0.5, intensity is 1.0 → output should be white (default gradient)
        let at_peak = evaluate_single(0.5, 0, 10, &params);
        assert_eq!(at_peak, Color::WHITE);
    }

    #[test]
    fn zero_intensity_at_end() {
        let params = EffectParams::new();
        let at_end = evaluate_single(1.0, 0, 10, &params);
        assert_eq!(at_end, Color::BLACK);
    }
}
