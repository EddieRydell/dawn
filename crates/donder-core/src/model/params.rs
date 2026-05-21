use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use ts_rs::TS;

use super::color::Color;
use super::color_gradient::ColorGradient;
use super::curve::Curve;

/// Which direction a wipe effect sweeps across fixtures.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    TS,
    JsonSchema,
    strum::VariantArray,
)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum WipeDirection {
    #[default]
    Horizontal,
    Vertical,
    DiagonalUp,
    DiagonalDown,
    Burst,
    Circle,
    Diamond,
}

impl WipeDirection {
    /// Serde-serialized variant names, for use in schema `ParamType::WipeDirection { options }`.
    pub fn schema_options() -> Vec<String> {
        crate::util::serde_variant_names(<Self as strum::VariantArray>::VARIANTS)
    }
}

/// How gradient colors are applied across time/space.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema, strum::VariantArray,
)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ColorMode {
    Static,
    GradientPerPulse,
    GradientThroughEffect,
    GradientAcrossItems,
}

impl ColorMode {
    /// Serde-serialized variant names, for use in schema `ParamType::ColorMode { options }`.
    pub fn schema_options() -> Vec<String> {
        crate::util::serde_variant_names(<Self as strum::VariantArray>::VARIANTS)
    }
}

/// All known effect parameter keys. Compile-time checked.
/// Built-in keys serialize as their variant name; `Custom` keys serialize as their raw string.
/// Unknown strings deserialize as `Custom(s)` so script params round-trip through JSON.
#[derive(Debug, Clone, PartialEq, Eq, Hash, TS, JsonSchema, strum::Display, strum::EnumString)]
#[ts(export)]
pub enum ParamKey {
    Color,
    Colors,
    Gradient,
    MovementCurve,
    PulseCurve,
    IntensityCurve,
    ColorMode,
    Speed,
    PulseWidth,
    BackgroundLevel,
    Reverse,
    Spread,
    Saturation,
    Brightness,
    Rate,
    DutyCycle,
    Density,
    Offset,
    Direction,
    CenterX,
    CenterY,
    PassCount,
    WipeOn,
    TailLength,
    MaxLevel,
    /// Custom parameter key for DSL-defined effects.
    #[strum(default, to_string = "{0}")]
    Custom(String),
}

impl Serialize for ParamKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ParamKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse::<ParamKey>().map_err(serde::de::Error::custom)
    }
}

/// Type-safe parameter values for effects.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub enum ParamValue {
    Float(f64),
    Int(i32),
    Bool(bool),
    Color(Color),
    ColorList(Vec<Color>),
    Text(String),
    Curve(Curve),
    ColorGradient(ColorGradient),
    ColorMode(ColorMode),
    WipeDirection(WipeDirection),
    /// A variant of a DSL-defined enum type.
    EnumVariant(String),
    /// A set of selected flags from a DSL-defined flags type.
    FlagSet(Vec<String>),
    /// Reference to a gradient in the global library by name.
    GradientRef(String),
    /// Reference to a motion path in the sequence's motion_paths by name.
    PathRef(String),
}

/// Generate a `ParamValue::as_*` accessor that copies a `Copy` inner value.
macro_rules! param_copy {
    ($method:ident, $variant:ident, $ty:ty) => {
        pub fn $method(&self) -> Option<$ty> {
            match self {
                ParamValue::$variant(v) => Some(*v),
                _ => None,
            }
        }
    };
}

/// Generate a `ParamValue::as_*` accessor that borrows an inner value as a slice.
macro_rules! param_slice {
    ($method:ident, $variant:ident, $elem:ty) => {
        pub fn $method(&self) -> Option<&[$elem]> {
            match self {
                ParamValue::$variant(v) => Some(v),
                _ => None,
            }
        }
    };
}

/// Generate a `ParamValue::as_*` accessor that borrows an inner value as `&str`.
macro_rules! param_str {
    ($method:ident, $variant:ident) => {
        pub fn $method(&self) -> Option<&str> {
            match self {
                ParamValue::$variant(v) => Some(v),
                _ => None,
            }
        }
    };
}

/// Generate a `ParamValue::as_*` accessor that borrows a reference to the inner value.
macro_rules! param_ref {
    ($method:ident, $variant:ident, $ty:ty) => {
        pub fn $method(&self) -> Option<&$ty> {
            match self {
                ParamValue::$variant(v) => Some(v),
                _ => None,
            }
        }
    };
}

impl ParamValue {
    pub fn as_float(&self) -> Option<f64> {
        match self {
            ParamValue::Float(v) => Some(*v),
            ParamValue::Int(v) => Some(f64::from(*v)),
            _ => None,
        }
    }

    param_copy!(as_int, Int, i32);
    param_copy!(as_bool, Bool, bool);
    param_copy!(as_color, Color, Color);
    param_copy!(as_color_mode, ColorMode, ColorMode);
    param_copy!(as_wipe_direction, WipeDirection, WipeDirection);

    param_slice!(as_color_list, ColorList, Color);
    param_slice!(as_flag_set, FlagSet, String);

    param_str!(as_text, Text);
    param_str!(as_enum_variant, EnumVariant);
    param_str!(as_gradient_ref, GradientRef);
    param_str!(as_path_ref, PathRef);

    param_ref!(as_curve, Curve, Curve);
    param_ref!(as_color_gradient, ColorGradient, ColorGradient);
}

/// Describes the type and constraints for an effect parameter, used to drive UI generation.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum ParamType {
    Float {
        min: f64,
        max: f64,
        step: f64,
    },
    Int {
        min: i32,
        max: i32,
    },
    Bool,
    Color,
    ColorList {
        min_colors: usize,
        max_colors: usize,
    },
    Curve,
    ColorGradient {
        min_stops: usize,
        max_stops: usize,
    },
    ColorMode {
        options: Vec<String>,
    },
    WipeDirection {
        options: Vec<String>,
    },
    Text {
        options: Vec<String>,
    },
    /// DSL-defined enum: exclusive selection (dropdown in UI).
    Enum {
        options: Vec<String>,
    },
    /// DSL-defined flags: multi-select (checkboxes in UI).
    Flags {
        options: Vec<String>,
    },
    /// Motion path: dropdown of sequence motion paths.
    Path,
}

/// Schema entry for one effect parameter: key, label, type constraints, and default value.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ParamSchema {
    pub key: ParamKey,
    pub label: String,
    pub param_type: ParamType,
    pub default: ParamValue,
}

/// Named, typed parameters for an effect instance.
/// Serializes as a flat JSON object (transparent over the inner HashMap).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS, JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
#[ts(export)]
pub struct EffectParams(
    #[ts(as = "HashMap<String, ParamValue>")]
    #[schemars(with = "HashMap<String, ParamValue>")]
    HashMap<ParamKey, ParamValue>,
);

/// Generate an owned-value `*_or` accessor on `EffectParams`.
macro_rules! owned_or {
    ($method:ident, $accessor:ident, $ty:ty) => {
        pub fn $method(&self, key: ParamKey, default: $ty) -> $ty {
            self.get(&key)
                .and_then(ParamValue::$accessor)
                .unwrap_or(default)
        }
    };
}

/// Generate a borrowed-value `*_or` accessor on `EffectParams`.
macro_rules! ref_or {
    ($method:ident, $accessor:ident, $ty:ty) => {
        pub fn $method<'a>(&'a self, key: ParamKey, default: &'a $ty) -> &'a $ty {
            self.get(&key)
                .and_then(ParamValue::$accessor)
                .unwrap_or(default)
        }
    };
}

impl EffectParams {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn set(mut self, key: ParamKey, value: ParamValue) -> Self {
        self.0.insert(key, value);
        self
    }

    pub fn set_mut(&mut self, key: ParamKey, value: ParamValue) {
        self.0.insert(key, value);
    }

    pub fn get(&self, key: &ParamKey) -> Option<&ParamValue> {
        self.0.get(key)
    }

    pub fn inner(&self) -> &HashMap<ParamKey, ParamValue> {
        &self.0
    }

    owned_or!(float_or, as_float, f64);
    owned_or!(bool_or, as_bool, bool);
    owned_or!(color_or, as_color, Color);
    owned_or!(color_mode_or, as_color_mode, ColorMode);
    owned_or!(wipe_direction_or, as_wipe_direction, WipeDirection);

    ref_or!(color_list_or, as_color_list, [Color]);
    ref_or!(curve_or, as_curve, Curve);
    ref_or!(gradient_or, as_color_gradient, ColorGradient);
    ref_or!(flag_set_or, as_flag_set, [String]);

    pub fn text_or<'a>(&'a self, key: ParamKey, default: &'a str) -> &'a str {
        self.get(&key).and_then(|v| v.as_text()).unwrap_or(default)
    }

    pub fn enum_or<'a>(&'a self, key: ParamKey, default: &'a str) -> &'a str {
        self.get(&key)
            .and_then(ParamValue::as_enum_variant)
            .unwrap_or(default)
    }

    /// Returns true if any parameter value is a library reference.
    pub fn has_refs(&self) -> bool {
        self.0
            .values()
            .any(|v| matches!(v, ParamValue::GradientRef(_) | ParamValue::PathRef(_)))
    }

    /// Clone the params, resolving any `GradientRef` into inline values.
    /// Unknown refs are left as-is (the effect will fall back to its default).
    pub fn resolve_refs(&self, gradient_lib: &HashMap<String, ColorGradient>) -> Self {
        let resolved = self
            .0
            .iter()
            .map(|(k, v)| {
                let new_v = match v {
                    ParamValue::GradientRef(name) => gradient_lib
                        .get(name)
                        .map(|g| ParamValue::ColorGradient(g.clone())),
                    _ => None,
                };
                (k.clone(), new_v.unwrap_or_else(|| v.clone()))
            })
            .collect();
        Self(resolved)
    }

    /// Mutable iterator over all parameter values.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut ParamValue> {
        self.0.values_mut()
    }
}

// ── Display impls ──────────────────────────────────────────────────

impl fmt::Display for ParamValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Float(v) => write!(f, "{v:.2}"),
            Self::Int(v) => write!(f, "{v}"),
            Self::Bool(v) => write!(f, "{v}"),
            Self::Color(c) => write!(f, "rgba({},{},{},{})", c.r, c.g, c.b, c.a),
            Self::ColorList(colors) => write!(f, "[{} colors]", colors.len()),
            Self::Text(s) => write!(f, "\"{s}\""),
            Self::Curve(c) => write!(f, "Curve({} pts)", c.points().len()),
            Self::ColorGradient(g) => write!(f, "Gradient({} stops)", g.stops().len()),
            Self::ColorMode(m) => write!(f, "{m:?}"),
            Self::WipeDirection(d) => write!(f, "{d:?}"),
            Self::EnumVariant(v) => write!(f, "{v}"),
            Self::FlagSet(flags) => write!(f, "[{}]", flags.join(", ")),
            Self::GradientRef(name) => write!(f, "GradientRef(\"{name}\")"),
            Self::PathRef(name) => write!(f, "PathRef(\"{name}\")"),
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::assertions_on_constants,
    clippy::expect_used,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    #[test]
    fn effect_params_empty_returns_defaults() {
        let params = EffectParams::new();
        assert_eq!(params.float_or(ParamKey::Speed, 1.0), 1.0);
        assert!(!params.bool_or(ParamKey::Reverse, false));
        assert_eq!(params.color_or(ParamKey::Color, Color::WHITE), Color::WHITE);
    }

    #[test]
    fn effect_params_type_mismatch_returns_fallback() {
        let params = EffectParams::new().set(ParamKey::Speed, ParamValue::Bool(true));
        // Speed is stored as Bool, but requesting it as float should return fallback
        assert_eq!(params.float_or(ParamKey::Speed, 2.0), 2.0);
    }

    #[test]
    fn effect_params_correct_type_returns_value() {
        let params = EffectParams::new().set(ParamKey::Speed, ParamValue::Float(5.0));
        assert_eq!(params.float_or(ParamKey::Speed, 1.0), 5.0);
    }

    #[test]
    fn effect_params_int_coerces_to_float() {
        let params = EffectParams::new().set(ParamKey::Speed, ParamValue::Int(3));
        assert_eq!(params.float_or(ParamKey::Speed, 1.0), 3.0);
    }

    #[test]
    fn effect_params_has_refs() {
        let params = EffectParams::new().set(ParamKey::Speed, ParamValue::Float(1.0));
        assert!(!params.has_refs());

        let params_with_ref = EffectParams::new().set(
            ParamKey::Gradient,
            ParamValue::GradientRef("my_grad".to_string()),
        );
        assert!(params_with_ref.has_refs());
    }

    #[test]
    fn effect_params_resolve_refs_substitutes_known() {
        use crate::model::ColorStop;

        let mut gradient_lib = HashMap::new();
        let grad = ColorGradient::new(vec![
            ColorStop {
                position: 0.0,
                color: Color::rgb(255, 0, 0),
            },
            ColorStop {
                position: 1.0,
                color: Color::rgb(0, 0, 255),
            },
        ])
        .expect("valid gradient");
        gradient_lib.insert("my_grad".to_string(), grad);

        let params = EffectParams::new().set(
            ParamKey::Gradient,
            ParamValue::GradientRef("my_grad".to_string()),
        );

        let resolved = params.resolve_refs(&gradient_lib);
        assert!(resolved
            .get(&ParamKey::Gradient)
            .expect("should exist")
            .as_color_gradient()
            .is_some());
    }

    #[test]
    fn effect_params_resolve_refs_unknown_stays() {
        let gradient_lib = HashMap::new();

        let params = EffectParams::new().set(
            ParamKey::Gradient,
            ParamValue::GradientRef("missing".to_string()),
        );

        let resolved = params.resolve_refs(&gradient_lib);
        assert!(resolved
            .get(&ParamKey::Gradient)
            .expect("should exist")
            .as_gradient_ref()
            .is_some());
    }

    // ── ParamValue accessor boundary tests ───────────────────────

    #[test]
    fn param_value_as_float_wrong_type() {
        assert_eq!(ParamValue::Bool(true).as_float(), None);
        assert_eq!(ParamValue::Text("hi".to_string()).as_float(), None);
    }

    #[test]
    fn param_value_as_color_wrong_type() {
        assert_eq!(ParamValue::Float(1.0).as_color(), None);
    }

    #[test]
    fn param_value_as_bool_wrong_type() {
        assert_eq!(ParamValue::Float(1.0).as_bool(), None);
    }

    // ── ParamKey round-trip test ─────────────────────────────────

    #[test]
    fn param_key_builtin_roundtrip() {
        let key = ParamKey::Speed;
        let json = serde_json::to_string(&key).expect("serialize");
        assert_eq!(json, "\"Speed\"");
        let back: ParamKey = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, ParamKey::Speed);
    }

    #[test]
    fn param_key_custom_roundtrip() {
        let key = ParamKey::Custom("myParam".to_string());
        let json = serde_json::to_string(&key).expect("serialize");
        assert_eq!(json, "\"myParam\"");
        let back: ParamKey = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, ParamKey::Custom("myParam".to_string()));
    }

    #[test]
    fn param_key_unknown_deserializes_as_custom() {
        let back: ParamKey = serde_json::from_str("\"UnknownKey\"").expect("deserialize");
        assert_eq!(back, ParamKey::Custom("UnknownKey".to_string()));
    }
}
