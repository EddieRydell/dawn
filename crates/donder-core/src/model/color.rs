use std::ops;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::BlendMode;

/// RGBA color with 8-bit channels. Alpha is used for blending during composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[repr(C)]
#[ts(export)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    #[must_use]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Create from HSV (hue 0-360, saturation 0-1, value 0-1)
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::many_single_char_names
    )]
    pub fn from_hsv(h: f64, s: f64, v: f64) -> Self {
        let h = h % 360.0;
        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;

        let (r1, g1, b1) = match h as u16 {
            0..60 => (c, x, 0.0),
            60..120 => (x, c, 0.0),
            120..180 => (0.0, c, x),
            180..240 => (0.0, x, c),
            240..300 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };

        Self::rgb(
            ((r1 + m) * 255.0) as u8,
            ((g1 + m) * 255.0) as u8,
            ((b1 + m) * 255.0) as u8,
        )
    }

    /// Linear interpolation between two colors. t is clamped to [0, 1].
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn lerp(self, other: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        let inv = 1.0 - t;
        Self {
            r: (f64::from(self.r) * inv + f64::from(other.r) * t) as u8,
            g: (f64::from(self.g) * inv + f64::from(other.g) * t) as u8,
            b: (f64::from(self.b) * inv + f64::from(other.b) * t) as u8,
            a: (f64::from(self.a) * inv + f64::from(other.a) * t) as u8,
        }
    }

    /// Multiplicative blend (0-255 scale).
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn multiply(self, other: Self) -> Self {
        Self {
            r: ((u16::from(self.r) * u16::from(other.r)) / 255) as u8,
            g: ((u16::from(self.g) * u16::from(other.g)) / 255) as u8,
            b: ((u16::from(self.b) * u16::from(other.b)) / 255) as u8,
            a: 255,
        }
    }

    /// Per-channel maximum.
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self {
            r: self.r.max(other.r),
            g: self.g.max(other.g),
            b: self.b.max(other.b),
            a: self.a.max(other.a),
        }
    }

    /// Scale brightness by a factor (0.0 - 1.0).
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn scale(self, factor: f64) -> Self {
        let f = factor.clamp(0.0, 1.0);
        Self {
            r: (f64::from(self.r) * f).round() as u8,
            g: (f64::from(self.g) * f).round() as u8,
            b: (f64::from(self.b) * f).round() as u8,
            a: self.a,
        }
    }

    /// Convert to HSV. Returns (hue: 0-360, saturation: 0-1, value: 0-1).
    #[must_use]
    #[allow(clippy::float_cmp)] // exact comparison is correct for max/min of same values
    pub fn to_hsv(self) -> (f64, f64, f64) {
        let r = f64::from(self.r) / 255.0;
        let g = f64::from(self.g) / 255.0;
        let b = f64::from(self.b) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let h = if delta == 0.0 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / delta) % 6.0)
        } else if max == g {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };
        let h = if h < 0.0 { h + 360.0 } else { h };

        let s = if max == 0.0 { 0.0 } else { delta / max };
        (h, s, max)
    }

    /// Saturating subtraction per channel.
    #[must_use]
    pub fn subtract(self, other: Self) -> Self {
        Self {
            r: self.r.saturating_sub(other.r),
            g: self.g.saturating_sub(other.g),
            b: self.b.saturating_sub(other.b),
            a: 255,
        }
    }

    /// Per-channel minimum.
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self {
            r: self.r.min(other.r),
            g: self.g.min(other.g),
            b: self.b.min(other.b),
            a: self.a.min(other.a),
        }
    }

    /// Per-channel average.
    #[must_use]
    pub fn average(self, other: Self) -> Self {
        Self {
            r: u8::midpoint(self.r, other.r),
            g: u8::midpoint(self.g, other.g),
            b: u8::midpoint(self.b, other.b),
            a: 255,
        }
    }

    /// Screen blend: complement of multiply.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn screen(self, other: Self) -> Self {
        Self {
            r: 255 - (((255 - u16::from(self.r)) * (255 - u16::from(other.r))) / 255) as u8,
            g: 255 - (((255 - u16::from(self.g)) * (255 - u16::from(other.g))) / 255) as u8,
            b: 255 - (((255 - u16::from(self.b)) * (255 - u16::from(other.b))) / 255) as u8,
            a: 255,
        }
    }

    /// Rec. 709 luma (perceived brightness), returns 0.0..1.0.
    #[must_use]
    pub fn brightness(self) -> f64 {
        (0.2126 * f64::from(self.r) + 0.7152 * f64::from(self.g) + 0.0722 * f64::from(self.b))
            / 255.0
    }

    /// Alpha-composite `self` over `other` (self is foreground).
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn over(self, other: Self) -> Self {
        let fa = f64::from(self.a) / 255.0;
        let ba = f64::from(other.a) / 255.0;
        let out_a = fa + ba * (1.0 - fa);
        if out_a == 0.0 {
            return Self::TRANSPARENT;
        }
        Self {
            r: ((f64::from(self.r) * fa + f64::from(other.r) * ba * (1.0 - fa)) / out_a) as u8,
            g: ((f64::from(self.g) * fa + f64::from(other.g) * ba * (1.0 - fa)) / out_a) as u8,
            b: ((f64::from(self.b) * fa + f64::from(other.b) * ba * (1.0 - fa)) / out_a) as u8,
            a: (out_a * 255.0) as u8,
        }
    }

    /// Blend `fg` onto `self` (background) using the given blend mode.
    #[inline]
    #[must_use]
    pub fn blend(self, fg: Self, mode: BlendMode) -> Self {
        match mode {
            BlendMode::Override => fg,
            BlendMode::Add => self + fg,
            BlendMode::Multiply => self.multiply(fg),
            BlendMode::Max => self.max(fg),
            BlendMode::Alpha => fg.over(self),
            BlendMode::Subtract => self.subtract(fg),
            BlendMode::Min => self.min(fg),
            BlendMode::Average => self.average(fg),
            BlendMode::Screen => self.screen(fg),
            BlendMode::Mask => {
                if fg.r > 0 || fg.g > 0 || fg.b > 0 {
                    Color::BLACK
                } else {
                    self
                }
            }
            BlendMode::IntensityOverlay => self.scale(fg.brightness()),
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

/// Additive blend, clamped at 255 per channel.
impl ops::Add for Color {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            r: self.r.saturating_add(other.r),
            g: self.g.saturating_add(other.g),
            b: self.b.saturating_add(other.b),
            a: 255,
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_lossless
)]
mod tests {
    use super::*;

    #[test]
    fn additive_saturates_at_255() {
        let a = Color::rgb(200, 200, 200);
        let b = Color::rgb(200, 200, 200);
        let result = a + b;
        assert_eq!(result, Color::rgb(255, 255, 255));
    }

    #[test]
    fn override_replaces_completely() {
        let bg = Color::rgb(100, 50, 200);
        let fg = Color::rgb(10, 20, 30);
        assert_eq!(bg.blend(fg, BlendMode::Override), fg);
    }

    #[test]
    fn multiply_with_white_is_identity() {
        let c = Color::rgb(123, 200, 50);
        let result = c.multiply(Color::WHITE);
        assert_eq!(result, Color::rgb(123, 200, 50));
    }

    #[test]
    fn multiply_with_black_is_black() {
        let c = Color::rgb(123, 200, 50);
        let result = c.multiply(Color::BLACK);
        assert_eq!(result, Color::rgb(0, 0, 0));
    }

    #[test]
    fn alpha_opaque_over_anything_replaces() {
        let bg = Color::rgb(100, 100, 100);
        let fg = Color::rgba(255, 0, 0, 255); // fully opaque
        let result = fg.over(bg);
        assert_eq!(result.r, 255);
        assert_eq!(result.g, 0);
        assert_eq!(result.b, 0);
        assert_eq!(result.a, 255);
    }

    #[test]
    fn alpha_transparent_over_anything_preserves() {
        let bg = Color::rgb(100, 150, 200);
        let fg = Color::TRANSPARENT;
        let result = fg.over(bg);
        assert_eq!(result.r, 100);
        assert_eq!(result.g, 150);
        assert_eq!(result.b, 200);
    }

    #[test]
    fn alpha_both_transparent_is_transparent() {
        let result = Color::TRANSPARENT.over(Color::TRANSPARENT);
        assert_eq!(result, Color::TRANSPARENT);
    }

    #[test]
    fn lerp_at_boundaries() {
        let a = Color::rgb(10, 20, 30);
        let b = Color::rgb(200, 100, 50);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
    }

    #[test]
    fn lerp_midpoint() {
        let result = Color::BLACK.lerp(Color::WHITE, 0.5);
        assert!((result.r as i16 - 127).abs() <= 1);
        assert!((result.g as i16 - 127).abs() <= 1);
        assert!((result.b as i16 - 127).abs() <= 1);
    }

    #[test]
    fn scale_zero_is_black_scale_one_is_identity() {
        let c = Color::rgb(100, 200, 50);
        let zeroed = c.scale(0.0);
        assert_eq!(zeroed.r, 0);
        assert_eq!(zeroed.g, 0);
        assert_eq!(zeroed.b, 0);

        let identity = c.scale(1.0);
        assert_eq!(identity, c);
    }

    #[test]
    fn subtract_saturates_at_zero() {
        let a = Color::rgb(100, 50, 200);
        let b = Color::rgb(150, 30, 255);
        let result = a.subtract(b);
        assert_eq!(result.r, 0);
        assert_eq!(result.g, 20);
        assert_eq!(result.b, 0);
    }

    #[test]
    fn min_per_channel() {
        let a = Color::rgb(100, 200, 50);
        let b = Color::rgb(150, 100, 75);
        let result = a.min(b);
        assert_eq!(result.r, 100);
        assert_eq!(result.g, 100);
        assert_eq!(result.b, 50);
    }

    #[test]
    fn average_of_black_and_white() {
        let result = Color::BLACK.average(Color::WHITE);
        assert_eq!(result.r, 127);
        assert_eq!(result.g, 127);
        assert_eq!(result.b, 127);
    }

    #[test]
    fn screen_with_black_is_identity() {
        let c = Color::rgb(123, 200, 50);
        let result = c.screen(Color::BLACK);
        assert_eq!(result, Color::rgb(123, 200, 50));
    }

    #[test]
    fn screen_with_white_is_white() {
        let c = Color::rgb(123, 200, 50);
        let result = c.screen(Color::WHITE);
        assert_eq!(result, Color::WHITE);
    }

    #[test]
    fn mask_non_black_fg_produces_black() {
        let bg = Color::rgb(100, 200, 50);
        let fg = Color::rgb(1, 0, 0);
        assert_eq!(bg.blend(fg, BlendMode::Mask), Color::BLACK);
    }

    #[test]
    fn mask_black_fg_preserves_bg() {
        let bg = Color::rgb(100, 200, 50);
        let fg = Color::BLACK;
        assert_eq!(bg.blend(fg, BlendMode::Mask), bg);
    }

    #[test]
    fn intensity_overlay_white_fg_preserves_bg() {
        let bg = Color::rgb(100, 200, 50);
        let fg = Color::WHITE;
        let result = bg.blend(fg, BlendMode::IntensityOverlay);
        assert_eq!(result, bg);
    }

    #[test]
    fn intensity_overlay_black_fg_darkens_to_black() {
        let bg = Color::rgb(100, 200, 50);
        let fg = Color::BLACK;
        let result = bg.blend(fg, BlendMode::IntensityOverlay);
        assert_eq!(result.r, 0);
        assert_eq!(result.g, 0);
        assert_eq!(result.b, 0);
    }

    #[test]
    fn brightness_white_is_one() {
        assert!((Color::WHITE.brightness() - 1.0).abs() < 0.01);
    }

    #[test]
    fn brightness_black_is_zero() {
        assert!((Color::BLACK.brightness() - 0.0).abs() < 0.01);
    }

    #[test]
    fn hsv_known_values() {
        let red = Color::from_hsv(0.0, 1.0, 1.0);
        assert_eq!(red.r, 255);
        assert_eq!(red.g, 0);
        assert_eq!(red.b, 0);

        let green = Color::from_hsv(120.0, 1.0, 1.0);
        assert_eq!(green.r, 0);
        assert_eq!(green.g, 255);
        assert_eq!(green.b, 0);

        let blue = Color::from_hsv(240.0, 1.0, 1.0);
        assert_eq!(blue.r, 0);
        assert_eq!(blue.g, 0);
        assert_eq!(blue.b, 255);
    }
}
