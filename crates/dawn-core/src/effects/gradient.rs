use crate::model::show::Position2D;
use crate::model::{BlendMode, Color, EffectParams, ParamKey, ParamSchema, ParamType, ParamValue};

use super::blend_pixel;

const DEFAULT_OFFSET: f64 = 0.0;
static DEFAULT_COLORS: [Color; 2] = [Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)];

/// Core gradient computation: maps a normalized position to an interpolated color.
///
/// The schema enforces `min_colors: 2`, so `colors` should never be empty or single-element
/// in normal operation. The guards below are defensive only.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn compute_pixel(colors: &[Color], pos: f64) -> Color {
    debug_assert!(
        !colors.is_empty(),
        "gradient colors should never be empty (schema enforces min_colors: 2)"
    );
    if colors.is_empty() {
        return Color::BLACK;
    }
    if let [single] = colors {
        return *single;
    }

    let segment_count = colors.len() - 1;
    let scaled = pos * segment_count as f64;
    let segment = (scaled as usize).min(segment_count - 1);
    let frac = scaled - segment as f64;

    match (colors.get(segment), colors.get(segment + 1)) {
        (Some(&a), Some(&b)) => a.lerp(b, frac),
        // Unreachable: `segment` is clamped to `segment_count - 1`, so both indices
        // are always in bounds for a slice with >= 2 elements.
        _ => Color::BLACK,
    }
}

struct Resolved<'a> {
    colors: &'a [Color],
    offset: f64,
}

impl<'a> Resolved<'a> {
    fn new(params: &'a EffectParams) -> Self {
        Self {
            colors: params.color_list_or(ParamKey::Colors, &DEFAULT_COLORS),
            offset: params.float_or(ParamKey::Offset, DEFAULT_OFFSET),
        }
    }
}

/// Batch evaluate: extract params once, loop over pixels.
#[allow(
    clippy::too_many_arguments,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
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

    debug_assert!(
        !r.colors.is_empty(),
        "gradient colors should never be empty (schema enforces min_colors: 2)"
    );
    if r.colors.is_empty() {
        // Defensive fallback — schema enforces min_colors: 2, so this shouldn't happen.
        let c = Color::BLACK;
        for pixel in dest.iter_mut() {
            blend_pixel(pixel, c, blend_mode, opacity);
        }
        return;
    }
    if r.colors.len() == 1 {
        let c = compute_pixel(r.colors, 0.0);
        for pixel in dest.iter_mut() {
            blend_pixel(pixel, c, blend_mode, opacity);
        }
        return;
    }

    let inv_total = super::linear_pixel_scale(total_pixels);
    let time_offset = t * r.offset;

    for (i, pixel) in dest.iter_mut().enumerate() {
        let pos = if total_pixels > 1 {
            ((global_offset + i) as f64) * inv_total
        } else {
            0.5
        };
        let pos = (pos + time_offset).fract().abs();
        let effect_color = compute_pixel(r.colors, pos);
        blend_pixel(pixel, effect_color, blend_mode, opacity);
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn evaluate_single(
    t: f64,
    pixel_index: usize,
    pixel_count: usize,
    params: &EffectParams,
) -> Color {
    let r = Resolved::new(params);

    let inv_total = super::linear_pixel_scale(pixel_count);
    let pos = if pixel_count > 1 {
        (pixel_index as f64) * inv_total
    } else {
        0.5
    };

    let pos = (pos + t * r.offset).fract().abs();
    compute_pixel(r.colors, pos)
}

pub fn schema() -> Vec<ParamSchema> {
    vec![
        ParamSchema {
            key: ParamKey::Colors,
            label: "Colors".into(),
            param_type: ParamType::ColorList {
                min_colors: 2,
                max_colors: 16,
            },
            default: ParamValue::ColorList(DEFAULT_COLORS.to_vec()),
        },
        ParamSchema {
            key: ParamKey::Offset,
            label: "Offset".into(),
            param_type: ParamType::Float {
                min: -5.0,
                max: 5.0,
                step: 0.1,
            },
            default: ParamValue::Float(DEFAULT_OFFSET),
        },
    ]
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::match_same_arms,
    clippy::cast_lossless
)]
mod tests {
    use super::*;

    #[test]
    fn first_pixel_is_first_color() {
        let red = Color::rgb(255, 0, 0);
        let blue = Color::rgb(0, 0, 255);
        let params =
            EffectParams::new().set(ParamKey::Colors, ParamValue::ColorList(vec![red, blue]));

        let first = evaluate_single(0.0, 0, 10, &params);
        assert_eq!(first, red);
    }

    #[test]
    fn gradient_progresses_spatially() {
        // Pixel near the end should have more blue than pixel near the start
        let params = EffectParams::new().set(
            ParamKey::Colors,
            ParamValue::ColorList(vec![Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]),
        );
        let near_start = evaluate_single(0.0, 1, 10, &params);
        let near_end = evaluate_single(0.0, 8, 10, &params);
        assert!(near_end.b > near_start.b);
        assert!(near_start.r > near_end.r);
    }

    #[test]
    fn middle_interpolates() {
        let params = EffectParams::new().set(
            ParamKey::Colors,
            ParamValue::ColorList(vec![Color::BLACK, Color::WHITE]),
        );
        let mid = evaluate_single(0.0, 5, 11, &params);
        assert!((mid.r as i16 - 127).abs() <= 1);
    }
}
