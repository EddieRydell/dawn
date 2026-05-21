pub mod chase;
pub mod defaults;
pub mod fade;
pub mod gradient;
pub mod meteor;
pub mod rainbow;
pub mod script;
pub mod solid;
pub mod strobe;
pub mod twinkle;
pub mod wipe;

use crate::model::show::Position2D;
use crate::model::{
    BlendMode, BuiltInEffect, Color, ColorGradient, ColorMode, EffectKind, EffectParams,
    ParamSchema,
};

/// Reciprocal for endpoint-inclusive pixel mapping: first pixel → 0.0, last pixel → 1.0.
/// For 0 or 1 pixels, returns 0.0 (no spread). Used by linear (non-wrapping) effects.
#[inline]
#[allow(clippy::cast_precision_loss)]
pub fn linear_pixel_scale(total_pixels: usize) -> f64 {
    1.0 / total_pixels.saturating_sub(1).max(1) as f64
}

/// Reciprocal for wrapping pixel mapping: positions in [0, 1) with equal spacing.
/// First pixel → 0.0, last pixel → (n-1)/n. Used by wrapping effects (chase, rainbow)
/// where pixel 0 and pixel n would overlap.
#[inline]
#[allow(clippy::cast_precision_loss)]
pub fn wrapping_pixel_scale(total_pixels: usize) -> f64 {
    1.0 / total_pixels.max(1) as f64
}

/// Shared color-mode gradient sampling used by chase, fade, and wipe.
///
/// - `t`: normalized effect time [0, 1]
/// - `pos`: normalized pixel position [0, 1] across all targeted fixtures
/// - `pulse_pos`: per-pulse gradient position. `None` falls back to `0.0`
///   (first gradient color), which is correct for "outside the pulse" in chase/wipe
///   and never reached by fade (which always passes `Some(t)`).
pub fn resolve_gradient_color(
    color_mode: ColorMode,
    gradient: &ColorGradient,
    t: f64,
    pos: f64,
    pulse_pos: Option<f64>,
) -> Color {
    match color_mode {
        ColorMode::GradientPerPulse => gradient.evaluate(pulse_pos.unwrap_or(0.0)),
        ColorMode::GradientThroughEffect => gradient.evaluate(t),
        ColorMode::GradientAcrossItems => gradient.evaluate(pos),
        ColorMode::Static => gradient.evaluate(0.0),
    }
}

/// Deterministic hash mapping `(index, seed)` to a reproducible f64 in [0, 1].
/// Used by effects that need stateless pseudo-random variation (twinkle, meteor).
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(super) fn deterministic_hash(index: usize, seed: u64) -> f64 {
    let mut x = (index as u64).wrapping_mul(2_654_435_761) ^ seed.wrapping_mul(2_246_822_519);
    x = x.wrapping_mul(x).wrapping_add(x);
    x ^= x >> 16;
    (x & 0xFFFF) as f64 / 65535.0
}

/// Apply opacity scaling and blend an effect color into a destination pixel.
/// Shared by all built-in effects to avoid duplicating the opacity+blend boilerplate.
#[inline]
pub fn blend_pixel(pixel: &mut Color, effect_color: Color, blend_mode: BlendMode, opacity: f64) {
    let c = if opacity < 1.0 {
        effect_color.scale(opacity)
    } else {
        effect_color
    };
    *pixel = pixel.blend(c, blend_mode);
}

/// Single mapping table from `BuiltInEffect` variant → module. All dispatch
/// functions (schema, single-pixel eval, batch eval, needs_positions) are
/// generated from this one declaration, so adding a new effect is a single line.
///
/// Effects annotated with `[spatial]` declare that they use the `positions`
/// argument in `evaluate_pixels_batch`. The evaluator uses `needs_positions()`
/// to skip building the position vector for non-spatial effects (zero overhead).
macro_rules! dispatch_effects {
    ($( $variant:ident => $module:ident $( [$spatial:ident] )? ),+ $(,)?) => {
        pub fn effect_schema(kind: &BuiltInEffect) -> Vec<ParamSchema> {
            match kind {
                $( BuiltInEffect::$variant => $module::schema(), )+
            }
        }

        pub fn evaluate_single_pixel(
            kind: &BuiltInEffect,
            t: f64,
            pixel_index: usize,
            pixel_count: usize,
            params: &EffectParams,
        ) -> Color {
            match kind {
                $( BuiltInEffect::$variant => $module::evaluate_single(t, pixel_index, pixel_count, params), )+
            }
        }

        pub fn needs_positions(kind: &EffectKind) -> bool {
            match kind {
                $( EffectKind::BuiltIn(BuiltInEffect::$variant) => dispatch_effects!(@is_spatial $( $spatial )?), )+
                EffectKind::Script(_) => false,
            }
        }

        #[inline]
        #[allow(clippy::too_many_arguments)]
        pub fn evaluate_pixels(
            kind: &EffectKind,
            t: f64,
            dest: &mut [Color],
            global_offset: usize,
            total_pixels: usize,
            params: &EffectParams,
            blend_mode: BlendMode,
            opacity: f64,
            positions: Option<&[Position2D]>,
        ) -> bool {
            match kind {
                EffectKind::BuiltIn(b) => {
                    match b {
                        $( BuiltInEffect::$variant => $module::evaluate_pixels_batch(t, dest, global_offset, total_pixels, params, blend_mode, opacity, positions), )+
                    }
                    true
                }
                EffectKind::Script(_) => false,
            }
        }
    };

    // Internal helper: resolve [spatial] annotation to bool
    (@is_spatial spatial) => { true };
    (@is_spatial) => { false };
}

dispatch_effects! {
    Solid    => solid,
    Chase    => chase,
    Rainbow  => rainbow,
    Strobe   => strobe,
    Gradient => gradient,
    Twinkle  => twinkle,
    Fade     => fade,
    Wipe     => wipe [spatial],
    Meteor   => meteor,
}
