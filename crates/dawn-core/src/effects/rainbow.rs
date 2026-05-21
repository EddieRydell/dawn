use crate::model::show::Position2D;
use crate::model::{BlendMode, Color, EffectParams, ParamKey, ParamSchema, ParamType, ParamValue};

use super::blend_pixel;
use super::defaults::DEFAULT_SPEED;

const DEFAULT_SPREAD: f64 = 1.0;
const DEFAULT_SATURATION: f64 = 1.0;
const DEFAULT_BRIGHTNESS: f64 = 1.0;

struct Resolved {
    speed: f64,
    spread: f64,
    saturation: f64,
    brightness: f64,
}

impl Resolved {
    fn new(params: &EffectParams) -> Self {
        Self {
            speed: params.float_or(ParamKey::Speed, DEFAULT_SPEED),
            spread: params.float_or(ParamKey::Spread, DEFAULT_SPREAD),
            saturation: params
                .float_or(ParamKey::Saturation, DEFAULT_SATURATION)
                .clamp(0.0, 1.0),
            brightness: params
                .float_or(ParamKey::Brightness, DEFAULT_BRIGHTNESS)
                .clamp(0.0, 1.0),
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

    let time_offset = t * r.speed * 360.0;
    let spatial_scale = r.spread * super::wrapping_pixel_scale(total_pixels) * 360.0;

    for (i, pixel) in dest.iter_mut().enumerate() {
        let spatial = (global_offset + i) as f64 * spatial_scale;
        let hue = (time_offset + spatial) % 360.0;
        let effect_color = Color::from_hsv(hue, r.saturation, r.brightness);
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

    let spatial = (pixel_index as f64) * super::wrapping_pixel_scale(pixel_count) * r.spread;

    let hue = ((t * r.speed + spatial) * 360.0) % 360.0;
    Color::from_hsv(hue, r.saturation, r.brightness)
}

pub fn schema() -> Vec<ParamSchema> {
    vec![
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
            key: ParamKey::Spread,
            label: "Spread".into(),
            param_type: ParamType::Float {
                min: 0.1,
                max: 10.0,
                step: 0.1,
            },
            default: ParamValue::Float(DEFAULT_SPREAD),
        },
        ParamSchema {
            key: ParamKey::Saturation,
            label: "Saturation".into(),
            param_type: ParamType::Float {
                min: 0.0,
                max: 1.0,
                step: 0.01,
            },
            default: ParamValue::Float(DEFAULT_SATURATION),
        },
        ParamSchema {
            key: ParamKey::Brightness,
            label: "Brightness".into(),
            param_type: ParamType::Float {
                min: 0.0,
                max: 1.0,
                step: 0.01,
            },
            default: ParamValue::Float(DEFAULT_BRIGHTNESS),
        },
    ]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn different_pixels_get_different_hues() {
        let params = EffectParams::new();
        let c0 = evaluate_single(0.0, 0, 10, &params);
        let c5 = evaluate_single(0.0, 5, 10, &params);
        // Pixels at different positions should produce different colors
        assert_ne!(c0, c5);
    }
}
