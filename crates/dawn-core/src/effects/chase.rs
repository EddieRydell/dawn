use crate::model::show::Position2D;
use crate::model::{
    BlendMode, Color, ColorGradient, ColorMode, Curve, EffectParams, ParamKey, ParamSchema,
    ParamType, ParamValue,
};

use super::blend_pixel;
use super::defaults::{
    DEFAULT_BACKGROUND_LEVEL, DEFAULT_CHASE_PULSE_WIDTH, DEFAULT_COLOR_MODE, DEFAULT_MOVEMENT,
    DEFAULT_PULSE, DEFAULT_REVERSE, DEFAULT_SPEED, DEFAULT_WHITE_GRADIENT,
};

/// Core chase computation: given a pixel's position, the head position, and
/// pulse parameters, compute the output color.
#[allow(clippy::too_many_arguments)]
fn compute_pixel(
    t: f64,
    pos: f64,
    head: f64,
    pulse_width: f64,
    inv_pulse: f64,
    background_level: f64,
    pulse_curve: &Curve,
    gradient: &ColorGradient,
    color_mode: ColorMode,
) -> Color {
    // Circular distance from head
    let mut dist = head - pos;
    if dist < 0.0 {
        dist += 1.0;
    }

    let intensity = if dist < pulse_width {
        let pulse_pos = dist * inv_pulse;
        pulse_curve.evaluate(pulse_pos).max(background_level)
    } else {
        background_level
    };

    // Sample color based on color mode
    let pulse_pos = if dist < pulse_width {
        Some(dist * inv_pulse)
    } else {
        None
    };
    let color = super::resolve_gradient_color(color_mode, gradient, t, pos, pulse_pos);

    color.scale(intensity)
}

struct Resolved<'a> {
    gradient: &'a ColorGradient,
    movement_curve: &'a Curve,
    pulse_curve: &'a Curve,
    color_mode: ColorMode,
    speed: f64,
    pulse_width: f64,
    background_level: f64,
    reverse: bool,
}

impl<'a> Resolved<'a> {
    fn new(params: &'a EffectParams) -> Self {
        Self {
            gradient: params.gradient_or(ParamKey::Gradient, &DEFAULT_WHITE_GRADIENT),
            movement_curve: params.curve_or(ParamKey::MovementCurve, &DEFAULT_MOVEMENT),
            pulse_curve: params.curve_or(ParamKey::PulseCurve, &DEFAULT_PULSE),
            color_mode: params.color_mode_or(ParamKey::ColorMode, DEFAULT_COLOR_MODE),
            speed: params.float_or(ParamKey::Speed, DEFAULT_SPEED),
            pulse_width: params
                .float_or(ParamKey::PulseWidth, DEFAULT_CHASE_PULSE_WIDTH)
                .clamp(0.01, 1.0),
            background_level: params
                .float_or(ParamKey::BackgroundLevel, DEFAULT_BACKGROUND_LEVEL)
                .clamp(0.0, 1.0),
            reverse: params.bool_or(ParamKey::Reverse, DEFAULT_REVERSE),
        }
    }

    fn head(&self, t: f64) -> f64 {
        let head = self.movement_curve.evaluate((t * self.speed).fract());
        if self.reverse {
            1.0 - head
        } else {
            head
        }
    }
}

/// Batch evaluate: extract params once, loop over pixels.
#[allow(
    clippy::too_many_arguments,
    clippy::cast_precision_loss,
    clippy::similar_names
)]
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

    if total_pixels == 0 {
        return;
    }

    let head = r.head(t);
    let inv_total = super::wrapping_pixel_scale(total_pixels);
    let inv_pulse = 1.0 / r.pulse_width;

    for (i, pixel) in dest.iter_mut().enumerate() {
        let pos = ((global_offset + i) as f64) * inv_total;
        let effect_color = compute_pixel(
            t,
            pos,
            head,
            r.pulse_width,
            inv_pulse,
            r.background_level,
            r.pulse_curve,
            r.gradient,
            r.color_mode,
        );
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

    if pixel_count == 0 {
        return Color::BLACK;
    }

    let pos = (pixel_index as f64) * super::wrapping_pixel_scale(pixel_count);
    let head = r.head(t);
    let inv_pulse = 1.0 / r.pulse_width;

    compute_pixel(
        t,
        pos,
        head,
        r.pulse_width,
        inv_pulse,
        r.background_level,
        r.pulse_curve,
        r.gradient,
        r.color_mode,
    )
}

pub fn schema() -> Vec<ParamSchema> {
    use super::defaults::{DEFAULT_MOVEMENT, DEFAULT_PULSE};

    vec![
        super::defaults::gradient_schema(),
        super::defaults::color_mode_schema(),
        ParamSchema {
            key: ParamKey::MovementCurve,
            label: "Movement Curve".into(),
            param_type: ParamType::Curve,
            default: ParamValue::Curve(DEFAULT_MOVEMENT.clone()),
        },
        ParamSchema {
            key: ParamKey::PulseCurve,
            label: "Pulse Curve".into(),
            param_type: ParamType::Curve,
            default: ParamValue::Curve(DEFAULT_PULSE.clone()),
        },
        ParamSchema {
            key: ParamKey::Speed,
            label: "Speed".into(),
            param_type: ParamType::Float {
                min: 0.1,
                max: 20.0,
                step: 0.1,
            },
            default: ParamValue::Float(DEFAULT_SPEED),
        },
        ParamSchema {
            key: ParamKey::PulseWidth,
            label: "Pulse Width".into(),
            param_type: ParamType::Float {
                min: 0.01,
                max: 1.0,
                step: 0.01,
            },
            default: ParamValue::Float(DEFAULT_CHASE_PULSE_WIDTH),
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
    fn pulse_bright_in_middle() {
        // At t=0, head at 0.0 (linear curve), pulse_width=0.3.
        // The default triangle pulse peaks at pulse_pos=0.5 → dist=0.15.
        // A pixel at pos=0.85 has dist = 0.0 - 0.85 + 1.0 = 0.15 → peak brightness.
        let params = EffectParams::new()
            .set(ParamKey::Speed, ParamValue::Float(1.0))
            .set(ParamKey::PulseWidth, ParamValue::Float(0.3))
            .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0));
        let mid_pulse = evaluate_single(0.0, 85, 100, &params);
        assert!(mid_pulse.r > 0 || mid_pulse.g > 0 || mid_pulse.b > 0);
    }

    #[test]
    fn background_level_outside_pulse() {
        // At t=0, head at 0.0, pulse_width=0.1. Pixel at pos=0.5 is outside pulse.
        let params = EffectParams::new()
            .set(ParamKey::Speed, ParamValue::Float(1.0))
            .set(ParamKey::PulseWidth, ParamValue::Float(0.1))
            .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0));
        let far_away = evaluate_single(0.0, 50, 100, &params);
        assert_eq!(far_away, Color::BLACK);
    }
}
