mod diagonal;
mod linear;
mod radial;

use std::sync::LazyLock;

use crate::model::show::Position2D;
use crate::model::{
    BlendMode, Color, ColorGradient, ColorMode, Curve, EffectParams, ParamKey, ParamSchema,
    ParamType, ParamValue, WipeDirection,
};

use super::blend_pixel;
use super::defaults::{
    DEFAULT_COLOR_MODE, DEFAULT_MOVEMENT, DEFAULT_REVERSE, DEFAULT_SPEED, DEFAULT_WHITE_GRADIENT,
};

static DEFAULT_WIPE_EDGE: LazyLock<Curve> = LazyLock::new(Curve::linear);
const DEFAULT_WIPE_PULSE_WIDTH: f64 = 1.0;
const DEFAULT_DIRECTION: WipeDirection = WipeDirection::Horizontal;
const DEFAULT_CENTER_X: f64 = 0.5;
const DEFAULT_CENTER_Y: f64 = 0.5;
const DEFAULT_PASS_COUNT: f64 = 1.0;
const DEFAULT_WIPE_ON: bool = true;

/// Project a 2D position onto a 1D scalar [0, 1] based on direction.
fn project_position(pos: Position2D, direction: WipeDirection, cx: f32, cy: f32) -> f64 {
    let cx = f64::from(cx);
    let cy = f64::from(cy);

    match direction {
        WipeDirection::Horizontal => linear::project_horizontal(pos),
        WipeDirection::Vertical => linear::project_vertical(pos),
        WipeDirection::DiagonalUp => diagonal::project_diagonal_up(pos),
        WipeDirection::DiagonalDown => diagonal::project_diagonal_down(pos),
        WipeDirection::Burst => radial::project_burst(pos, cx, cy),
        WipeDirection::Circle => radial::project_circle(pos, cx, cy),
        WipeDirection::Diamond => radial::project_diamond(pos, cx, cy),
    }
}

/// Core wipe computation: given a pixel's spatial position (0-1), the head
/// position, and wipe parameters, compute the output color.
#[allow(clippy::too_many_arguments)]
fn compute_pixel(
    t: f64,
    spatial_pos: f64,
    head_pos: f64,
    pulse_width: f64,
    inv_pulse: f64,
    wipe_on: bool,
    pulse_curve: &Curve,
    gradient: &ColorGradient,
    color_mode: ColorMode,
) -> Color {
    // Distance from sweep head to this pixel
    let dist = spatial_pos - head_pos;

    // Wipe: pixels behind the head are lit, edge uses pulse_curve
    let intensity = if wipe_on {
        // Revealing: pixels at or behind the head are lit
        if dist <= 0.0 {
            // Fully revealed
            1.0
        } else if dist < pulse_width {
            // In the transition zone — pulse_curve controls falloff
            let edge_t = dist * inv_pulse;
            1.0 - pulse_curve.evaluate(edge_t)
        } else {
            // Not yet reached
            0.0
        }
    } else {
        // Concealing: pixels at or behind the head are dark
        if dist <= 0.0 {
            0.0
        } else if dist < pulse_width {
            let edge_t = dist * inv_pulse;
            pulse_curve.evaluate(edge_t)
        } else {
            1.0
        }
    };

    if intensity <= 0.0 {
        return Color::BLACK;
    }

    // Sample color based on color mode
    let pulse_pos = if dist > 0.0 && dist < pulse_width {
        Some(dist * inv_pulse)
    } else {
        None
    };
    let color = super::resolve_gradient_color(color_mode, gradient, t, spatial_pos, pulse_pos);

    color.scale(intensity)
}

#[allow(clippy::cast_possible_truncation)]
struct Resolved<'a> {
    gradient: &'a ColorGradient,
    movement_curve: &'a Curve,
    pulse_curve: &'a Curve,
    color_mode: ColorMode,
    speed: f64,
    pulse_width: f64,
    reverse: bool,
    direction: WipeDirection,
    center_x: f32,
    center_y: f32,
    pass_count: f64,
    wipe_on: bool,
}

#[allow(clippy::cast_possible_truncation)]
impl<'a> Resolved<'a> {
    fn new(params: &'a EffectParams) -> Self {
        Self {
            gradient: params.gradient_or(ParamKey::Gradient, &DEFAULT_WHITE_GRADIENT),
            movement_curve: params.curve_or(ParamKey::MovementCurve, &DEFAULT_MOVEMENT),
            pulse_curve: params.curve_or(ParamKey::PulseCurve, &DEFAULT_WIPE_EDGE),
            color_mode: params.color_mode_or(ParamKey::ColorMode, DEFAULT_COLOR_MODE),
            speed: params.float_or(ParamKey::Speed, DEFAULT_SPEED),
            pulse_width: params
                .float_or(ParamKey::PulseWidth, DEFAULT_WIPE_PULSE_WIDTH)
                .clamp(0.01, 1.0),
            reverse: params.bool_or(ParamKey::Reverse, DEFAULT_REVERSE),
            direction: params.wipe_direction_or(ParamKey::Direction, DEFAULT_DIRECTION),
            center_x: params.float_or(ParamKey::CenterX, DEFAULT_CENTER_X) as f32,
            center_y: params.float_or(ParamKey::CenterY, DEFAULT_CENTER_Y) as f32,
            pass_count: params
                .float_or(ParamKey::PassCount, DEFAULT_PASS_COUNT)
                .max(0.1),
            wipe_on: params.bool_or(ParamKey::WipeOn, DEFAULT_WIPE_ON),
        }
    }

    fn head_pos(&self, t: f64) -> f64 {
        let head = self
            .movement_curve
            .evaluate((t * self.pass_count * self.speed).fract());
        let head = if self.reverse { 1.0 - head } else { head };
        head * (1.0 + self.pulse_width) - self.pulse_width
    }
}

/// Batch evaluate: extract params once, loop over pixels with spatial positions.
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
    positions: Option<&[Position2D]>,
) {
    let r = Resolved::new(params);

    if total_pixels == 0 {
        return;
    }

    let head_pos = r.head_pos(t);
    let inv_total = super::linear_pixel_scale(total_pixels);
    let inv_pulse = 1.0 / r.pulse_width;

    for (i, pixel) in dest.iter_mut().enumerate() {
        let spatial_pos = if let Some(positions) = positions {
            debug_assert!(
                positions.len() > i,
                "positions.len() ({}) <= pixel index ({i})",
                positions.len()
            );
            let pos = positions
                .get(i)
                .copied()
                .unwrap_or(Position2D { x: 0.0, y: 0.0 });
            project_position(pos, r.direction, r.center_x, r.center_y)
        } else {
            ((global_offset + i) as f64) * inv_total
        };

        let effect_color = compute_pixel(
            t,
            spatial_pos,
            head_pos,
            r.pulse_width,
            inv_pulse,
            r.wipe_on,
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

    let spatial_pos = if pixel_count > 0 {
        pixel_index as f64 / pixel_count as f64
    } else {
        0.0
    };

    let head_pos = r.head_pos(t);
    let inv_pulse = 1.0 / r.pulse_width;

    compute_pixel(
        t,
        spatial_pos,
        head_pos,
        r.pulse_width,
        inv_pulse,
        r.wipe_on,
        r.pulse_curve,
        r.gradient,
        r.color_mode,
    )
}

pub fn schema() -> Vec<ParamSchema> {
    use super::defaults::DEFAULT_MOVEMENT;

    vec![
        ParamSchema {
            key: ParamKey::Direction,
            label: "Direction".into(),
            param_type: ParamType::WipeDirection {
                options: WipeDirection::schema_options(),
            },
            default: ParamValue::WipeDirection(DEFAULT_DIRECTION),
        },
        crate::effects::defaults::gradient_schema(),
        crate::effects::defaults::color_mode_schema(),
        ParamSchema {
            key: ParamKey::MovementCurve,
            label: "Movement Curve".into(),
            param_type: ParamType::Curve,
            default: ParamValue::Curve(DEFAULT_MOVEMENT.clone()),
        },
        ParamSchema {
            key: ParamKey::PulseCurve,
            label: "Edge Curve".into(),
            param_type: ParamType::Curve,
            default: ParamValue::Curve(DEFAULT_WIPE_EDGE.clone()),
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
            label: "Edge Width".into(),
            param_type: ParamType::Float {
                min: 0.01,
                max: 1.0,
                step: 0.01,
            },
            default: ParamValue::Float(DEFAULT_WIPE_PULSE_WIDTH),
        },
        ParamSchema {
            key: ParamKey::Reverse,
            label: "Reverse".into(),
            param_type: ParamType::Bool,
            default: ParamValue::Bool(DEFAULT_REVERSE),
        },
        ParamSchema {
            key: ParamKey::CenterX,
            label: "Center X".into(),
            param_type: ParamType::Float {
                min: 0.0,
                max: 1.0,
                step: 0.01,
            },
            default: ParamValue::Float(DEFAULT_CENTER_X),
        },
        ParamSchema {
            key: ParamKey::CenterY,
            label: "Center Y".into(),
            param_type: ParamType::Float {
                min: 0.0,
                max: 1.0,
                step: 0.01,
            },
            default: ParamValue::Float(DEFAULT_CENTER_Y),
        },
        ParamSchema {
            key: ParamKey::PassCount,
            label: "Pass Count".into(),
            param_type: ParamType::Float {
                min: 0.1,
                max: 10.0,
                step: 0.1,
            },
            default: ParamValue::Float(DEFAULT_PASS_COUNT),
        },
        ParamSchema {
            key: ParamKey::WipeOn,
            label: "Wipe On (Reveal)".into(),
            param_type: ParamType::Bool,
            default: ParamValue::Bool(DEFAULT_WIPE_ON),
        },
    ]
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;

    fn pos(x: f32, y: f32) -> Position2D {
        Position2D { x, y }
    }

    #[test]
    fn project_horizontal_corners() {
        assert!(
            (project_position(pos(0.0, 0.5), WipeDirection::Horizontal, 0.5, 0.5) - 0.0).abs()
                < 1e-10
        );
        assert!(
            (project_position(pos(1.0, 0.5), WipeDirection::Horizontal, 0.5, 0.5) - 1.0).abs()
                < 1e-10
        );
        assert!(
            (project_position(pos(0.5, 0.0), WipeDirection::Horizontal, 0.5, 0.5) - 0.5).abs()
                < 1e-10
        );
    }

    #[test]
    fn project_vertical_corners() {
        assert!(
            (project_position(pos(0.5, 0.0), WipeDirection::Vertical, 0.5, 0.5) - 0.0).abs()
                < 1e-10
        );
        assert!(
            (project_position(pos(0.5, 1.0), WipeDirection::Vertical, 0.5, 0.5) - 1.0).abs()
                < 1e-10
        );
    }

    #[test]
    fn project_diagonal_up() {
        assert!(
            (project_position(pos(0.0, 1.0), WipeDirection::DiagonalUp, 0.5, 0.5) - 0.0).abs()
                < 1e-10
        );
        assert!(
            (project_position(pos(1.0, 0.0), WipeDirection::DiagonalUp, 0.5, 0.5) - 1.0).abs()
                < 1e-10
        );
        assert!(
            (project_position(pos(0.5, 0.5), WipeDirection::DiagonalUp, 0.5, 0.5) - 0.5).abs()
                < 1e-10
        );
    }

    #[test]
    fn project_diagonal_down() {
        assert!(
            (project_position(pos(0.0, 0.0), WipeDirection::DiagonalDown, 0.5, 0.5) - 0.0).abs()
                < 1e-10
        );
        assert!(
            (project_position(pos(1.0, 1.0), WipeDirection::DiagonalDown, 0.5, 0.5) - 1.0).abs()
                < 1e-10
        );
    }

    #[test]
    fn project_circle_center_is_zero() {
        assert!(
            (project_position(pos(0.5, 0.5), WipeDirection::Circle, 0.5, 0.5) - 0.0).abs() < 1e-10
        );
    }

    #[test]
    fn project_circle_corner_is_one() {
        let val = project_position(pos(1.0, 1.0), WipeDirection::Circle, 0.5, 0.5);
        assert!((val - 1.0).abs() < 1e-6);
    }

    #[test]
    fn project_diamond_center_is_zero() {
        assert!(
            (project_position(pos(0.5, 0.5), WipeDirection::Diamond, 0.5, 0.5) - 0.0).abs() < 1e-10
        );
    }

    #[test]
    fn project_burst_center_is_zero() {
        assert!(
            (project_position(pos(0.5, 0.5), WipeDirection::Burst, 0.5, 0.5) - 0.0).abs() < 1e-10
        );
    }

    #[test]
    fn horizontal_wipe_half_lit() {
        let positions: Vec<Position2D> = (0..10).map(|i| pos(i as f32 / 9.0, 0.5)).collect();
        let mut dest = vec![Color::BLACK; 10];
        let params = EffectParams::new()
            .set(
                ParamKey::Direction,
                ParamValue::WipeDirection(WipeDirection::Horizontal),
            )
            .set(ParamKey::PulseWidth, ParamValue::Float(0.05))
            .set(ParamKey::WipeOn, ParamValue::Bool(true));

        evaluate_pixels_batch(
            0.5,
            &mut dest,
            0,
            10,
            &params,
            BlendMode::Override,
            1.0,
            Some(&positions),
        );

        assert!(
            dest[0].r > 200,
            "first pixel should be bright, got r={}",
            dest[0].r
        );
        assert!(
            dest[9].r < 50,
            "last pixel should be dark, got r={}",
            dest[9].r
        );
    }

    #[test]
    fn circle_wipe_center_lit_first() {
        let positions = vec![
            pos(0.5, 0.5), // center
            pos(0.0, 0.0), // corner
            pos(1.0, 1.0), // corner
        ];
        let mut dest = vec![Color::BLACK; 3];
        let params = EffectParams::new()
            .set(
                ParamKey::Direction,
                ParamValue::WipeDirection(WipeDirection::Circle),
            )
            .set(ParamKey::PulseWidth, ParamValue::Float(0.1))
            .set(ParamKey::WipeOn, ParamValue::Bool(true));

        evaluate_pixels_batch(
            0.3,
            &mut dest,
            0,
            3,
            &params,
            BlendMode::Override,
            1.0,
            Some(&positions),
        );

        assert!(
            dest[0].r > dest[1].r,
            "center should be brighter than corner"
        );
        assert!(
            dest[0].r > dest[2].r,
            "center should be brighter than corner"
        );
    }

    #[test]
    fn wipe_off_inverts() {
        let positions: Vec<Position2D> = (0..10).map(|i| pos(i as f32 / 9.0, 0.5)).collect();

        let mut dest_on = vec![Color::BLACK; 10];
        let params_on = EffectParams::new()
            .set(
                ParamKey::Direction,
                ParamValue::WipeDirection(WipeDirection::Horizontal),
            )
            .set(ParamKey::PulseWidth, ParamValue::Float(0.05))
            .set(ParamKey::WipeOn, ParamValue::Bool(true));
        evaluate_pixels_batch(
            0.5,
            &mut dest_on,
            0,
            10,
            &params_on,
            BlendMode::Override,
            1.0,
            Some(&positions),
        );

        let mut dest_off = vec![Color::BLACK; 10];
        let params_off = EffectParams::new()
            .set(
                ParamKey::Direction,
                ParamValue::WipeDirection(WipeDirection::Horizontal),
            )
            .set(ParamKey::PulseWidth, ParamValue::Float(0.05))
            .set(ParamKey::WipeOn, ParamValue::Bool(false));
        evaluate_pixels_batch(
            0.5,
            &mut dest_off,
            0,
            10,
            &params_off,
            BlendMode::Override,
            1.0,
            Some(&positions),
        );

        assert!(dest_on[0].r > dest_off[0].r);
        assert!(dest_off[9].r > dest_on[9].r);
    }

    #[test]
    fn fallback_without_positions() {
        let mut dest = vec![Color::BLACK; 10];
        let params = EffectParams::new().set(ParamKey::PulseWidth, ParamValue::Float(0.05));
        evaluate_pixels_batch(
            0.5,
            &mut dest,
            0,
            10,
            &params,
            BlendMode::Override,
            1.0,
            None,
        );
        assert!(dest.iter().any(|c| c.r > 0));
    }
}
