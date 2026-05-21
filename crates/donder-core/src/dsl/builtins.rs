use super::ast::TypeName;
use super::ops::{self, Value};
use crate::model::color::Color;

/// Built-in function: single source of truth for name, type signature, semantics,
/// and constant-foldability. Adding a builtin means adding ONE entry here —
/// typeck, compiler, optimizer, and VM all read from this.
pub struct BuiltinFn {
    pub name: &'static str,
    pub params: &'static [(&'static str, TypeName)],
    pub ret: TypeName,
    pub category: &'static str,
    pub description: &'static str,
    /// Canonical evaluation function. Takes args in declaration order.
    /// The VM calls this via the generic `CallBuiltin(u8)` handler.
    pub eval: fn(&[Value]) -> Value,
    /// Whether this builtin can be evaluated at compile time (constant folding).
    /// False for noise functions (expensive, unlikely with all-constant args).
    pub const_foldable: bool,
}

// BuiltinFn contains a function pointer which is not Debug-printable,
// but we still want to be able to inspect the rest of the struct.
#[allow(clippy::missing_fields_in_debug)]
impl std::fmt::Debug for BuiltinFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltinFn")
            .field("name", &self.name)
            .field("params", &self.params)
            .field("ret", &self.ret)
            .field("category", &self.category)
            .field("description", &self.description)
            .field("const_foldable", &self.const_foldable)
            .finish()
    }
}

/// All built-in functions available in the DSL.
pub static BUILTINS: &[BuiltinFn] = &[
    // ── Math (1-arg) ────────────────────────────────────────────
    BuiltinFn {
        name: "sin", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Sine",
        eval: |a| Value::Float(a[0].as_float().sin()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "cos", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Cosine",
        eval: |a| Value::Float(a[0].as_float().cos()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "tan", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Tangent",
        eval: |a| Value::Float(a[0].as_float().tan()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "abs", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Absolute value",
        eval: |a| Value::Float(a[0].as_float().abs()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "floor", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Round down",
        eval: |a| Value::Float(a[0].as_float().floor()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "ceil", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Round up",
        eval: |a| Value::Float(a[0].as_float().ceil()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "round", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Round to nearest",
        eval: |a| Value::Float(a[0].as_float().round()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "fract", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Fractional part (x - floor(x))",
        eval: |a| Value::Float(a[0].as_float().fract()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "sqrt", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Square root",
        eval: |a| Value::Float(a[0].as_float().sqrt()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "sign", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Sign: -1.0, 0.0, or 1.0",
        eval: |a| Value::Float(a[0].as_float().signum()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "exp", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "e^x (exponential)",
        eval: |a| Value::Float(a[0].as_float().exp()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "log", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Natural logarithm (ln)",
        eval: |a| Value::Float(a[0].as_float().ln()),
        const_foldable: true,
    },
    // ── Math (2-arg) ────────────────────────────────────────────
    BuiltinFn {
        name: "pow", params: &[("base", TypeName::Float), ("exp", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Power",
        eval: |a| Value::Float(a[0].as_float().powf(a[1].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "min", params: &[("a", TypeName::Float), ("b", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Minimum",
        eval: |a| Value::Float(a[0].as_float().min(a[1].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "max", params: &[("a", TypeName::Float), ("b", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Maximum",
        eval: |a| Value::Float(a[0].as_float().max(a[1].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "step", params: &[("edge", TypeName::Float), ("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "0 if x < edge, else 1",
        eval: |a| {
            let edge = a[0].as_float();
            let x = a[1].as_float();
            Value::Float(if x < edge { 0.0 } else { 1.0 })
        },
        const_foldable: true,
    },
    BuiltinFn {
        name: "atan2", params: &[("y", TypeName::Float), ("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Arctangent of y/x",
        eval: |a| Value::Float(a[0].as_float().atan2(a[1].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "mod", params: &[("a", TypeName::Float), ("b", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Modulo (same as a % b). Returns 0 if b is 0",
        eval: |a| {
            let av = a[0].as_float();
            let bv = a[1].as_float();
            Value::Float(if bv == 0.0 { 0.0 } else { av % bv })
        },
        const_foldable: true,
    },
    // ── Math (3-arg) ────────────────────────────────────────────
    BuiltinFn {
        name: "clamp", params: &[("x", TypeName::Float), ("min", TypeName::Float), ("max", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Constrain x to [min, max]",
        eval: |a| Value::Float(a[0].as_float().clamp(a[1].as_float(), a[2].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "mix", params: &[("a", TypeName::Float), ("b", TypeName::Float), ("t", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Linear interpolation: a + (b - a) * t",
        eval: |a| {
            let av = a[0].as_float();
            let bv = a[1].as_float();
            let tv = a[2].as_float();
            Value::Float(av + (bv - av) * tv)
        },
        const_foldable: true,
    },
    BuiltinFn {
        name: "smoothstep", params: &[("e0", TypeName::Float), ("e1", TypeName::Float), ("x", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Smooth Hermite interpolation. Requires e0 < e1; returns 0 if e0 >= e1",
        eval: |a| {
            let e0 = a[0].as_float();
            let e1 = a[1].as_float();
            let x = a[2].as_float();
            if e0 >= e1 {
                Value::Float(0.0)
            } else {
                let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
                Value::Float(t * t * (3.0 - 2.0 * t))
            }
        },
        const_foldable: true,
    },
    BuiltinFn {
        name: "map", params: &[("x", TypeName::Float), ("in_min", TypeName::Float), ("in_max", TypeName::Float), ("out_min", TypeName::Float), ("out_max", TypeName::Float)], ret: TypeName::Float,
        category: "math", description: "Remap x from [in_min, in_max] to [out_min, out_max]. Returns out_min if in_min == in_max",
        eval: |a| {
            let x = a[0].as_float();
            let in_min = a[1].as_float();
            let in_max = a[2].as_float();
            let out_min = a[3].as_float();
            let out_max = a[4].as_float();
            let range = in_max - in_min;
            if range == 0.0 {
                Value::Float(out_min)
            } else {
                Value::Float(out_min + (x - in_min) / range * (out_max - out_min))
            }
        },
        const_foldable: true,
    },
    // ── Color constructors ──────────────────────────────────────
    BuiltinFn {
        name: "rgb", params: &[("r", TypeName::Float), ("g", TypeName::Float), ("b", TypeName::Float)], ret: TypeName::Color,
        category: "color", description: "RGB color (0.0-1.0 range)",
        eval: |a| Value::Color(Color::rgb(
            ops::float_to_u8(a[0].as_float()),
            ops::float_to_u8(a[1].as_float()),
            ops::float_to_u8(a[2].as_float()),
        )),
        const_foldable: true,
    },
    BuiltinFn {
        name: "hsv", params: &[("h", TypeName::Float), ("s", TypeName::Float), ("v", TypeName::Float)], ret: TypeName::Color,
        category: "color", description: "HSV color (h: 0-360, s: 0-1, v: 0-1)",
        eval: |a| Value::Color(Color::from_hsv(
            a[0].as_float(),
            a[1].as_float(),
            a[2].as_float(),
        )),
        const_foldable: true,
    },
    BuiltinFn {
        name: "rgba", params: &[("r", TypeName::Float), ("g", TypeName::Float), ("b", TypeName::Float), ("a", TypeName::Float)], ret: TypeName::Color,
        category: "color", description: "RGBA color (0.0-1.0 range)",
        eval: |a| Value::Color(Color::rgba(
            ops::float_to_u8(a[0].as_float()),
            ops::float_to_u8(a[1].as_float()),
            ops::float_to_u8(a[2].as_float()),
            ops::float_to_u8(a[3].as_float()),
        )),
        const_foldable: true,
    },
    // ── Vec2 ────────────────────────────────────────────────────
    BuiltinFn {
        name: "vec2", params: &[("x", TypeName::Float), ("y", TypeName::Float)], ret: TypeName::Vec2,
        category: "vec2", description: "Construct vec2",
        eval: |a| Value::Vec2(a[0].as_float(), a[1].as_float()),
        const_foldable: true,
    },
    BuiltinFn {
        name: "distance", params: &[("a", TypeName::Vec2), ("b", TypeName::Vec2)], ret: TypeName::Float,
        category: "vec2", description: "Euclidean distance between two vec2",
        eval: |a| {
            let (ax, ay) = a[0].as_vec2();
            let (bx, by) = a[1].as_vec2();
            let dx = bx - ax;
            let dy = by - ay;
            Value::Float((dx * dx + dy * dy).sqrt())
        },
        const_foldable: true,
    },
    BuiltinFn {
        name: "length", params: &[("v", TypeName::Vec2)], ret: TypeName::Float,
        category: "vec2", description: "Length of vec2",
        eval: |a| {
            let (x, y) = a[0].as_vec2();
            Value::Float((x * x + y * y).sqrt())
        },
        const_foldable: true,
    },
    BuiltinFn {
        name: "dot", params: &[("a", TypeName::Vec2), ("b", TypeName::Vec2)], ret: TypeName::Float,
        category: "vec2", description: "Dot product of two vec2",
        eval: |a| {
            let (ax, ay) = a[0].as_vec2();
            let (bx, by) = a[1].as_vec2();
            Value::Float(ax * bx + ay * by)
        },
        const_foldable: true,
    },
    BuiltinFn {
        name: "normalize", params: &[("v", TypeName::Vec2)], ret: TypeName::Vec2,
        category: "vec2", description: "Normalize vec2 to unit length",
        eval: |a| {
            let (x, y) = a[0].as_vec2();
            let len = (x * x + y * y).sqrt();
            if len > 0.0 {
                Value::Vec2(x / len, y / len)
            } else {
                Value::Vec2(0.0, 0.0)
            }
        },
        const_foldable: true,
    },
    BuiltinFn {
        name: "angle", params: &[("v", TypeName::Vec2)], ret: TypeName::Float,
        category: "vec2", description: "Angle of vec2 in radians (atan2(y, x))",
        eval: |a| {
            let (x, y) = a[0].as_vec2();
            Value::Float(y.atan2(x))
        },
        const_foldable: true,
    },
    BuiltinFn {
        name: "from_angle", params: &[("radians", TypeName::Float)], ret: TypeName::Vec2,
        category: "vec2", description: "Unit vec2 from angle: vec2(cos(r), sin(r))",
        eval: |a| {
            let r = a[0].as_float();
            Value::Vec2(r.cos(), r.sin())
        },
        const_foldable: true,
    },
    BuiltinFn {
        name: "rotate", params: &[("v", TypeName::Vec2), ("angle", TypeName::Float)], ret: TypeName::Vec2,
        category: "vec2", description: "Rotate vec2 by angle in radians",
        eval: |a| {
            let (x, y) = a[0].as_vec2();
            let angle = a[1].as_float();
            let cos_a = angle.cos();
            let sin_a = angle.sin();
            Value::Vec2(x * cos_a - y * sin_a, x * sin_a + y * cos_a)
        },
        const_foldable: true,
    },
    // ── Hash / Random ───────────────────────────────────────────
    BuiltinFn {
        name: "hash", params: &[("a", TypeName::Float), ("b", TypeName::Float)], ret: TypeName::Float,
        category: "hash", description: "Deterministic pseudo-random [0, 1]. Same inputs always produce same output",
        eval: |a| Value::Float(ops::hash_f64(a[0].as_float(), a[1].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "hash3", params: &[("a", TypeName::Float), ("b", TypeName::Float), ("c", TypeName::Float)], ret: TypeName::Float,
        category: "hash", description: "Deterministic pseudo-random [0, 1] with 3 inputs",
        eval: |a| Value::Float(ops::hash3_f64(a[0].as_float(), a[1].as_float(), a[2].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "random", params: &[("seed", TypeName::Float)], ret: TypeName::Float,
        category: "hash", description: "Pseudo-random [0, 1] from seed",
        eval: |a| Value::Float(ops::hash_f64(a[0].as_float(), 0.0)),
        const_foldable: true,
    },
    BuiltinFn {
        name: "random_range", params: &[("seed", TypeName::Float), ("min", TypeName::Float), ("max", TypeName::Float)], ret: TypeName::Float,
        category: "hash", description: "Pseudo-random in [min, max] from seed",
        eval: |a| {
            let h = ops::hash_f64(a[0].as_float(), 0.0);
            let min_val = a[1].as_float();
            let max_val = a[2].as_float();
            Value::Float(min_val + (max_val - min_val) * h)
        },
        const_foldable: true,
    },
    // ── Easing ──────────────────────────────────────────────────
    BuiltinFn {
        name: "ease_in", params: &[("t", TypeName::Float)], ret: TypeName::Float,
        category: "easing", description: "Quadratic ease-in (t^2)",
        eval: |a| Value::Float(ops::ease_in(a[0].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "ease_out", params: &[("t", TypeName::Float)], ret: TypeName::Float,
        category: "easing", description: "Quadratic ease-out",
        eval: |a| Value::Float(ops::ease_out(a[0].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "ease_in_out", params: &[("t", TypeName::Float)], ret: TypeName::Float,
        category: "easing", description: "Quadratic ease-in-out",
        eval: |a| Value::Float(ops::ease_in_out(a[0].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "ease_in_cubic", params: &[("t", TypeName::Float)], ret: TypeName::Float,
        category: "easing", description: "Cubic ease-in (t^3)",
        eval: |a| Value::Float(ops::ease_in_cubic(a[0].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "ease_out_cubic", params: &[("t", TypeName::Float)], ret: TypeName::Float,
        category: "easing", description: "Cubic ease-out",
        eval: |a| Value::Float(ops::ease_out_cubic(a[0].as_float())),
        const_foldable: true,
    },
    BuiltinFn {
        name: "ease_in_out_cubic", params: &[("t", TypeName::Float)], ret: TypeName::Float,
        category: "easing", description: "Cubic ease-in-out",
        eval: |a| Value::Float(ops::ease_in_out_cubic(a[0].as_float())),
        const_foldable: true,
    },
    // ── Noise ───────────────────────────────────────────────────
    BuiltinFn {
        name: "noise", params: &[("x", TypeName::Float)], ret: TypeName::Float,
        category: "noise", description: "1D Perlin noise. Returns [-1, 1]",
        eval: |a| Value::Float(ops::noise::perlin1(a[0].as_float())),
        const_foldable: false,
    },
    BuiltinFn {
        name: "noise2", params: &[("x", TypeName::Float), ("y", TypeName::Float)], ret: TypeName::Float,
        category: "noise", description: "2D Perlin noise. Returns [-1, 1]",
        eval: |a| Value::Float(ops::noise::perlin2(a[0].as_float(), a[1].as_float())),
        const_foldable: false,
    },
    BuiltinFn {
        name: "noise3", params: &[("x", TypeName::Float), ("y", TypeName::Float), ("z", TypeName::Float)], ret: TypeName::Float,
        category: "noise", description: "3D Perlin noise. Returns [-1, 1]",
        eval: |a| Value::Float(ops::noise::perlin3(a[0].as_float(), a[1].as_float(), a[2].as_float())),
        const_foldable: false,
    },
    BuiltinFn {
        name: "fbm", params: &[("x", TypeName::Float), ("y", TypeName::Float), ("octaves", TypeName::Float)], ret: TypeName::Float,
        category: "noise", description: "Fractal Brownian motion (layered 2D noise)",
        #[allow(clippy::cast_sign_loss)]
        eval: |a| Value::Float(ops::noise::fbm(a[0].as_float(), a[1].as_float(), a[2].as_float().max(0.0) as u32)),
        const_foldable: false,
    },
    BuiltinFn {
        name: "worley2", params: &[("x", TypeName::Float), ("y", TypeName::Float)], ret: TypeName::Float,
        category: "noise", description: "2D Worley/cellular noise. Returns [0, 1]",
        eval: |a| Value::Float(ops::noise::worley2(a[0].as_float(), a[1].as_float())),
        const_foldable: false,
    },
];

/// Look up the index of a builtin by name. Used by the compiler to emit
/// `CallBuiltin(idx)` opcodes.
#[allow(clippy::cast_possible_truncation)]
pub fn builtin_index(name: &str) -> Option<u8> {
    BUILTINS.iter().position(|b| b.name == name).map(|i| i as u8)
}

/// Implicit builtin variables: single source of truth for name, type, AND var enum.
/// Used by both the type checker (for type resolution) and the compiler (for opcode emission).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinVar {
    T,
    Pixel,
    Pixels,
    Pos,
    Pos2d,
    AbsT,
    Pi,
    Tau,
}

pub static IMPLICIT_VARS: &[(&str, TypeName, BuiltinVar, &str)] = &[
    ("t",     TypeName::Float, BuiltinVar::T,     "Normalized time [0.0, 1.0] within effect duration"),
    ("pixel", TypeName::Float, BuiltinVar::Pixel, "Current pixel index (0-based)"),
    ("pixels",TypeName::Float, BuiltinVar::Pixels,"Total pixel count in the effect's target"),
    ("pos",   TypeName::Float, BuiltinVar::Pos,   "Normalized position: pixel / (pixels - 1), range [0.0, 1.0]"),
    ("pos2d", TypeName::Vec2,  BuiltinVar::Pos2d, "2D position (requires @spatial true)"),
    ("abs_t", TypeName::Float, BuiltinVar::AbsT,  "Absolute time in seconds (for motion path evaluation)"),
    ("PI",    TypeName::Float, BuiltinVar::Pi,    "3.14159..."),
    ("TAU",   TypeName::Float, BuiltinVar::Tau,   "6.28318... (2\u{03C0})"),
];

pub fn lookup_builtin(name: &str) -> Option<&'static BuiltinFn> {
    BUILTINS.iter().find(|b| b.name == name)
}

pub fn lookup_implicit(name: &str) -> Option<(&TypeName, BuiltinVar)> {
    IMPLICIT_VARS.iter()
        .find(|&&(n, _, _, _)| n == name)
        .map(|(_, ty, var, _)| (ty, *var))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::fmt::Write;

    /// Generate `src-tauri/bindings/DslBuiltins.ts` from the actual BUILTINS and
    /// IMPLICIT_VARS arrays so the frontend editor always stays in sync.
    #[test]
    fn export_bindings_dslbuiltins() {
        let mut out = String::from(
            "// This file was generated by [builtins.rs]. Do not edit this file manually.\n\n",
        );

        // Keywords (must stay in sync with lexer.rs keyword match)
        out.push_str("export const DSL_KEYWORDS = [\n");
        for kw in &["let", "fn", "if", "else", "param", "enum", "flags", "return", "switch", "case", "default"] {
            let _ = writeln!(out, "  \"{kw}\",");
        }
        out.push_str("] as const;\n\n");

        // Type keywords (must stay in sync with lexer.rs type tokens)
        out.push_str("export const DSL_TYPES = [\n");
        for ty in &["float", "int", "bool", "color", "vec2", "gradient", "curve", "path"] {
            let _ = writeln!(out, "  \"{ty}\",");
        }
        out.push_str("] as const;\n\n");

        // Built-in functions — generated from BUILTINS
        out.push_str("export const DSL_BUILTINS = [\n");
        for b in BUILTINS {
            let params: Vec<String> = b.params.iter().map(|(name, ty)| {
                format!("{{ name: \"{name}\", type: \"{}\" }}", type_name_str(ty))
            }).collect();
            let _ = writeln!(
                out,
                "  {{ name: \"{}\", params: [{}], ret: \"{}\", category: \"{}\", description: \"{}\" }},",
                b.name,
                params.join(", "),
                type_name_str(&b.ret),
                b.category,
                b.description,
            );
        }
        out.push_str("] as const;\n\n");

        // Implicit variables — generated from IMPLICIT_VARS
        out.push_str("export const DSL_IMPLICIT_VARS = [\n");
        for &(name, ref ty, _, desc) in IMPLICIT_VARS {
            let _ = writeln!(
                out,
                "  {{ name: \"{name}\", type: \"{}\", description: \"{desc}\" }},",
                type_name_str(ty),
            );
        }
        out.push_str("] as const;\n");

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bindings")
            .join("DslBuiltins.ts");
        std::fs::write(&path, out).expect("Failed to write DslBuiltins.ts");
    }

    fn type_name_str(ty: &TypeName) -> &'static str {
        match ty {
            TypeName::Float => "float",
            TypeName::Int => "int",
            TypeName::Bool => "bool",
            TypeName::Color => "color",
            TypeName::Vec2 => "vec2",
            TypeName::Gradient => "gradient",
            TypeName::Curve => "curve",
        }
    }
}
