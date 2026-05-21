//! Shared default constants for built-in effects.
//!
//! Effect-specific defaults (e.g. chase pulse width vs wipe edge width) stay in their own modules.

use std::sync::LazyLock;

use crate::model::{Color, ColorGradient, ColorMode, Curve};

/// Default color for simple effects (solid, strobe, twinkle).
pub const DEFAULT_COLOR: Color = Color::WHITE;

/// Default color mode for most effects (chase, strobe, meteor, twinkle, wipe).
/// Fade intentionally overrides this with `GradientThroughEffect`.
pub const DEFAULT_COLOR_MODE: ColorMode = ColorMode::Static;

/// Default reverse flag. Used by chase, wipe.
pub const DEFAULT_REVERSE: bool = false;

/// Default pulse width for chase effects.
pub const DEFAULT_CHASE_PULSE_WIDTH: f64 = 0.3;

/// Default color gradient: solid white. Used by chase, fade, wipe.
pub static DEFAULT_WHITE_GRADIENT: LazyLock<ColorGradient> =
    LazyLock::new(|| ColorGradient::solid(Color::WHITE));

/// Default movement curve: linear ramp 0→1. Used by chase, wipe.
pub static DEFAULT_MOVEMENT: LazyLock<Curve> = LazyLock::new(Curve::linear);

/// Default pulse curve: triangle 0→1→0. Used by chase.
pub static DEFAULT_PULSE: LazyLock<Curve> = LazyLock::new(Curve::triangle);

/// Default speed multiplier. Used by chase, rainbow, wipe.
pub const DEFAULT_SPEED: f64 = 1.0;

/// Default density for particle effects (twinkle, meteor).
pub const DEFAULT_DENSITY: f64 = 0.3;

/// Default background level for effects with a "floor" brightness
/// (chase, twinkle, meteor). Maps to `ParamKey::BackgroundLevel`.
pub const DEFAULT_BACKGROUND_LEVEL: f64 = 0.0;

// ── Shared schema entries ──────────────────────────────────────

use crate::model::{ParamKey, ParamSchema, ParamType, ParamValue};

/// Gradient parameter schema shared by chase, fade, strobe, twinkle, meteor, wipe.
/// Single source of truth for `min_stops` / `max_stops` constraints.
pub fn gradient_schema() -> ParamSchema {
    ParamSchema {
        key: ParamKey::Gradient,
        label: "Color Gradient".into(),
        param_type: ParamType::ColorGradient {
            min_stops: 1,
            max_stops: 16,
        },
        default: ParamValue::ColorGradient(DEFAULT_WHITE_GRADIENT.clone()),
    }
}

/// ColorMode parameter schema shared by chase, fade, strobe, twinkle, meteor, wipe.
/// Single source of truth for the available mode options.
pub fn color_mode_schema() -> ParamSchema {
    ParamSchema {
        key: ParamKey::ColorMode,
        label: "Color Mode".into(),
        param_type: ParamType::ColorMode {
            options: ColorMode::schema_options(),
        },
        default: ParamValue::ColorMode(DEFAULT_COLOR_MODE),
    }
}
