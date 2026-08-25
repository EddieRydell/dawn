use dawn_language::dsl::{Identifier, Value};
use dawn_language::values::Color;
use indexmap::IndexMap;

use super::RenderError;

pub(super) fn compose_max(target: &mut Color, source: Color) {
    target.red = target.red.max(source.red);
    target.green = target.green.max(source.green);
    target.blue = target.blue.max(source.blue);
}

pub(super) fn max_color(left: Color, right: Color) -> Color {
    Color {
        red: left.red.max(right.red),
        green: left.green.max(right.green),
        blue: left.blue.max(right.blue),
    }
}

pub(super) fn add_color(left: Color, right: Color) -> Color {
    Color {
        red: left.red.saturating_add(right.red),
        green: left.green.saturating_add(right.green),
        blue: left.blue.saturating_add(right.blue),
    }
}

pub(super) fn multiply_color(left: Color, right: Color) -> Color {
    Color {
        red: ((u16::from(left.red) * u16::from(right.red)) / 255) as u8,
        green: ((u16::from(left.green) * u16::from(right.green)) / 255) as u8,
        blue: ((u16::from(left.blue) * u16::from(right.blue)) / 255) as u8,
    }
}

pub(super) fn invert_color(color: Color) -> Color {
    Color {
        red: 255 - color.red,
        green: 255 - color.green,
        blue: 255 - color.blue,
    }
}

pub(super) fn scale_color(color: Color, amount: f64) -> Color {
    Color {
        red: scale_channel(color.red, amount),
        green: scale_channel(color.green, amount),
        blue: scale_channel(color.blue, amount),
    }
}

fn scale_channel(value: u8, amount: f64) -> u8 {
    (f64::from(value) * amount.clamp(0.0, 1.0)).round() as u8
}

pub(super) fn intensity(color: Color) -> f64 {
    f64::from(color.red.max(color.green).max(color.blue)) / 255.0
}

pub(super) fn color_param(
    params: &IndexMap<Identifier, Value>,
    name: &str,
) -> Result<Color, RenderError> {
    let name = Identifier::new(name.to_string()).map_err(|_| RenderError::BadGraph {
        message: format!("invalid operator parameter name `{name}`"),
    })?;
    params
        .get(&name)
        .and_then(|value| match value {
            Value::Color(value) => Some(*value),
            _ => None,
        })
        .ok_or_else(|| RenderError::BadGraph {
            message: format!("missing or invalid operator parameter `{}`", name.as_str()),
        })
}

pub(super) fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}
