use crate::model::show::Position2D;
use crate::model::{BlendMode, Color, EffectParams, ParamKey, ParamSchema, ParamType, ParamValue};

use super::blend_pixel;
use super::defaults::DEFAULT_COLOR;

/// Batch evaluate: extract color once, blend all pixels.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_pixels_batch(
    _t: f64,
    dest: &mut [Color],
    _global_offset: usize,
    _total_pixels: usize,
    params: &EffectParams,
    blend_mode: BlendMode,
    opacity: f64,
    _positions: Option<&[Position2D]>,
) {
    let color = params.color_or(ParamKey::Color, DEFAULT_COLOR);
    for pixel in dest.iter_mut() {
        blend_pixel(pixel, color, blend_mode, opacity);
    }
}

pub fn evaluate_single(
    _t: f64,
    _pixel_index: usize,
    _pixel_count: usize,
    params: &EffectParams,
) -> Color {
    params.color_or(ParamKey::Color, DEFAULT_COLOR)
}

pub fn schema() -> Vec<ParamSchema> {
    vec![ParamSchema {
        key: ParamKey::Color,
        label: "Color".into(),
        param_type: ParamType::Color,
        default: ParamValue::Color(DEFAULT_COLOR),
    }]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn default_is_white() {
        let params = EffectParams::new();
        assert_eq!(evaluate_single(0.0, 0, 1, &params), Color::WHITE);
    }
}
