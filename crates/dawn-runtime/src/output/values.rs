use dawn_language::values::{Color, Curve, Gradient};

use super::errors::SequenceOutputRenderError;

pub(crate) fn set_cell<T: Copy>(
    cells: &mut [T],
    cell: u32,
    value: T,
    clip: u32,
) -> Result<(), SequenceOutputRenderError> {
    let target =
        cells
            .get_mut(cell as usize)
            .ok_or_else(|| SequenceOutputRenderError::Control {
                clip,
                reason: "control cell is out of range".to_string(),
            })?;
    *target = value;
    Ok(())
}
pub(crate) fn check_source_width(
    expected: usize,
    actual: usize,
) -> Result<(), SequenceOutputRenderError> {
    if expected == actual {
        Ok(())
    } else {
        Err(SequenceOutputRenderError::Patch(format!(
            "source width {actual} does not match declared width {expected}"
        )))
    }
}
pub(crate) fn sample_curve(curve: &Curve, position: f64) -> f64 {
    let Some(first) = curve.points.first() else {
        return 0.0;
    };
    if position <= first.position {
        return first.value;
    }
    for pair in curve.points.windows(2) {
        if position <= pair[1].position {
            let span = pair[1].position - pair[0].position;
            let amount = if span <= 0.0 {
                0.0
            } else {
                (position - pair[0].position) / span
            };
            return pair[0].value + (pair[1].value - pair[0].value) * amount;
        }
    }
    curve.points.last().map_or(0.0, |point| point.value)
}
pub(crate) fn sample_gradient(gradient: &Gradient, position: f64) -> Option<Color> {
    let first = gradient.stops.first()?;
    if position <= first.position {
        return Some(first.color);
    }
    for pair in gradient.stops.windows(2) {
        if position <= pair[1].position {
            let span = pair[1].position - pair[0].position;
            let amount = if span <= 0.0 {
                0.0
            } else {
                (position - pair[0].position) / span
            };
            return Some(Color {
                red: lerp_u8(pair[0].color.red, pair[1].color.red, amount),
                green: lerp_u8(pair[0].color.green, pair[1].color.green, amount),
                blue: lerp_u8(pair[0].color.blue, pair[1].color.blue, amount),
            });
        }
    }
    gradient.stops.last().map(|stop| stop.color)
}
fn lerp_u8(left: u8, right: u8, amount: f64) -> u8 {
    (f64::from(left) + (f64::from(right) - f64::from(left)) * amount.clamp(0.0, 1.0)).round() as u8
}
pub(crate) fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}
pub(crate) fn grayscale(value: f64) -> Color {
    let channel = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color {
        red: channel,
        green: channel,
        blue: channel,
    }
}
