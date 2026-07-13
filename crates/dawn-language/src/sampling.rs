use crate::values::{Color, Curve, Gradient};

#[inline]
pub fn sample_curve(curve: &Curve, position: f64) -> f64 {
    let Some(first) = curve.points.first() else {
        return 0.0;
    };
    let mut previous = first;
    for point in &curve.points {
        if point.position >= position {
            let span = (point.position - previous.position).max(1e-9);
            let t = ((position - previous.position) / span).clamp(0.0, 1.0);
            return previous.value + (point.value - previous.value) * t;
        }
        previous = point;
    }
    previous.value
}

#[inline]
pub fn curve_crossing(curve: &Curve, value: f64, fallback: f64) -> f64 {
    let Some(first) = curve.points.first() else {
        return fallback;
    };
    let mut previous = first;
    for point in &curve.points {
        let min = previous.value.min(point.value);
        let max = previous.value.max(point.value);
        if value >= min && value <= max {
            let span = point.value - previous.value;
            if span.abs() <= 1e-9 {
                return previous.position;
            }
            let t = ((value - previous.value) / span).clamp(0.0, 1.0);
            return previous.position + (point.position - previous.position) * t;
        }
        previous = point;
    }
    fallback
}

#[inline]
pub fn sample_gradient(gradient: &Gradient, position: f64) -> Option<Color> {
    let first = gradient.stops.first()?;
    let mut previous = first;
    for stop in &gradient.stops {
        if stop.position >= position {
            let span = (stop.position - previous.position).max(1e-9);
            return Some(mix_colors(
                previous.color,
                stop.color,
                ((position - previous.position) / span).clamp(0.0, 1.0),
            ));
        }
        previous = stop;
    }
    Some(previous.color)
}

#[inline(always)]
pub fn mix_colors(left: Color, right: Color, t: f64) -> Color {
    let channel = |left: u8, right: u8| {
        ((left as f64 + (right as f64 - left as f64) * t).clamp(0.0, 255.0) + 0.5) as u8
    };
    Color {
        red: channel(left.red, right.red),
        green: channel(left.green, right.green),
        blue: channel(left.blue, right.blue),
    }
}

#[inline(always)]
pub fn scale_color(color: Color, scale: f64) -> Color {
    let channel = |value: u8| ((value as f64 * scale).clamp(0.0, 255.0) + 0.5) as u8;
    Color {
        red: channel(color.red),
        green: channel(color.green),
        blue: channel(color.blue),
    }
}

#[inline]
pub fn hsv(h: f64, s: f64, v: f64) -> Color {
    let h = h - h.floor();
    let sector = h * 6.0;
    let c = v * s;
    let x = c * (1.0 - (sector - (sector / 2.0).floor() * 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if sector < 1.0 {
        (c, x, 0.0)
    } else if sector < 2.0 {
        (x, c, 0.0)
    } else if sector < 3.0 {
        (0.0, c, x)
    } else if sector < 4.0 {
        (0.0, x, c)
    } else if sector < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let channel = |value: f64| ((value * 255.0).clamp(0.0, 255.0) + 0.5) as u8;
    Color {
        red: channel(r + m),
        green: channel(g + m),
        blue: channel(b + m),
    }
}

#[inline(always)]
pub fn deterministic_random(values: impl Iterator<Item = f64>) -> f64 {
    let seed = values.fold(0.0, |seed, value| seed * 31.0 + value);
    deterministic_random_seed(seed)
}

#[inline(always)]
pub fn deterministic_random_seed(seed: f64) -> f64 {
    (seed.sin() * 43_758.545_312_3).fract().abs()
}
