//! Shared evaluation logic for the DSL.
//!
//! This module is the single source of truth for:
//! - **`Value` type**: shared between builtins' eval functions and the VM.
//! - **Binary operator semantics** (`eval_binary_op`): used by both the VM
//!   (runtime execution) and the optimizer (constant folding / peephole).
//! - **Builtin function semantics** (`eval_builtin_fn`): delegates to the
//!   canonical `eval` function pointer in the BUILTINS table.
//! - **Field-name resolution** (`FIELD_OPS`): used by both the type checker
//!   (field type resolution) and the compiler (opcode emission).
//! - **Noise algorithms** (`noise` module): Perlin, FBM, Worley — used by
//!   builtin eval functions.
//! - **Hash and easing functions**: canonical implementations used by builtins.

use super::ast::{BinOp, TypeName};
use super::compiler::Op;
use crate::model::color::Color;

// ── Shared Value type ───────────────────────────────────────────────

/// Runtime value on the VM stack. Shared between builtins (eval functions)
/// and the VM.
#[derive(Debug, Clone, Copy)]
pub enum Value {
    Float(f64),
    Color(Color),
    Vec2(f64, f64),
}

impl Value {
    /// Extract float. The type checker guarantees the correct variant at
    /// every call site; a mismatch here means a compiler bug.
    pub fn as_float(self) -> f64 {
        match self {
            Self::Float(f) => f,
            other => {
                debug_assert!(false, "Value::as_float called on {other:?}");
                0.0
            }
        }
    }
    /// Extract color. The type checker guarantees the correct variant.
    pub fn as_color(self) -> Color {
        match self {
            Self::Color(c) => c,
            other => {
                debug_assert!(false, "Value::as_color called on {other:?}");
                Color::BLACK
            }
        }
    }
    /// Extract vec2. The type checker guarantees the correct variant.
    pub fn as_vec2(self) -> (f64, f64) {
        match self {
            Self::Vec2(x, y) => (x, y),
            other => {
                debug_assert!(false, "Value::as_vec2 called on {other:?}");
                (0.0, 0.0)
            }
        }
    }
}

/// Convert a float in [0.0, 1.0] to a u8 in [0, 255], clamped.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn float_to_u8(f: f64) -> u8 {
    (f.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Relative epsilon for DSL float equality comparisons.
/// Scaled by the magnitude of the operands to handle values far from zero.
const RELATIVE_EPSILON: f64 = 1e-9;

// ── Binary operator evaluation ───────────────────────────────────────

/// Evaluate a binary operation on two f64 values.
///
/// This is the canonical semantics for every `BinOp` variant when both
/// operands are floats.  The VM and the constant-folder both call this.
///
/// Comparison and logical operators return 1.0 (true) or 0.0 (false).
/// Division and modulo by zero return 0.0 (safe fallback for light shows).
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
pub fn eval_binary_op(op: BinOp, a: f64, b: f64) -> f64 {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => {
            if b == 0.0 { 0.0 } else { a / b }
        }
        BinOp::Mod => {
            if b == 0.0 { 0.0 } else { a % b }
        }
        BinOp::Pow => a.powf(b),
        BinOp::Lt => if a < b { 1.0 } else { 0.0 },
        BinOp::Gt => if a > b { 1.0 } else { 0.0 },
        BinOp::Le => if a <= b { 1.0 } else { 0.0 },
        BinOp::Ge => if a >= b { 1.0 } else { 0.0 },
        BinOp::Eq => {
            let diff = (a - b).abs();
            let magnitude = a.abs().max(b.abs()).max(1.0);
            if diff < RELATIVE_EPSILON * magnitude { 1.0 } else { 0.0 }
        }
        BinOp::Ne => {
            let diff = (a - b).abs();
            let magnitude = a.abs().max(b.abs()).max(1.0);
            if diff >= RELATIVE_EPSILON * magnitude { 1.0 } else { 0.0 }
        }
        BinOp::And => if a != 0.0 && b != 0.0 { 1.0 } else { 0.0 },
        BinOp::Or => if a != 0.0 || b != 0.0 { 1.0 } else { 0.0 },
        BinOp::BitAnd => ((a as i64) & (b as i64)) as f64,
        BinOp::BitOr => ((a as i64) | (b as i64)) as f64,
        BinOp::BitXor => ((a as i64) ^ (b as i64)) as f64,
        BinOp::Shl => {
            let shift = (b as i64).clamp(0, 63) as u32;
            ((a as i64).wrapping_shl(shift)) as f64
        }
        BinOp::Shr => {
            let shift = (b as i64).clamp(0, 63) as u32;
            ((a as i64).wrapping_shr(shift)) as f64
        }
    }
}

// ── Builtin function evaluation ──────────────────────────────────────

/// Evaluate a builtin function at compile time (constant folding).
/// Returns `Some(result)` for foldable all-float builtins, `None` otherwise.
///
/// Delegates to the canonical eval function in the BUILTINS table,
/// ensuring constant folding always matches runtime semantics.
pub fn eval_builtin_fn(name: &str, args: &[f64]) -> Option<f64> {
    use super::builtins;
    let bi = builtins::lookup_builtin(name)?;
    if !bi.const_foldable {
        return None;
    }
    // Only fold if all params are Float and return is Float
    if bi.ret != TypeName::Float {
        return None;
    }
    if !bi.params.iter().all(|(_, ty)| *ty == TypeName::Float) {
        return None;
    }
    let mut values = [Value::Float(0.0); 8];
    for (i, &f) in args.iter().enumerate() {
        values[i] = Value::Float(f);
    }
    match (bi.eval)(&values[..args.len()]) {
        Value::Float(f) => Some(f),
        _ => None,
    }
}

// ── Field-name resolution ────────────────────────────────────────────

/// Mapping from field names to (owner type, VM opcode).
///
/// Used by the type checker to resolve field types and by the compiler
/// to emit the correct opcode for field access.
pub static FIELD_OPS: &[(&str, TypeName, Op)] = &[
    ("r",          TypeName::Color, Op::ColorR),
    ("g",          TypeName::Color, Op::ColorG),
    ("b",          TypeName::Color, Op::ColorB),
    ("a",          TypeName::Color, Op::ColorA),
    ("hue",        TypeName::Color, Op::ColorHue),
    ("saturation", TypeName::Color, Op::ColorSaturation),
    ("value",      TypeName::Color, Op::ColorValue),
    ("x",          TypeName::Vec2,  Op::Vec2X),
    ("y",          TypeName::Vec2,  Op::Vec2Y),
];

// ── Easing functions (canonical implementations) ────────────────────

#[inline]
pub fn ease_in(t: f64) -> f64 { t * t }
#[inline]
pub fn ease_out(t: f64) -> f64 { t * (2.0 - t) }
#[inline]
pub fn ease_in_out(t: f64) -> f64 {
    if t < 0.5 { 2.0 * t * t } else { -1.0 + (4.0 - 2.0 * t) * t }
}
#[inline]
pub fn ease_in_cubic(t: f64) -> f64 { t * t * t }
#[inline]
pub fn ease_out_cubic(t: f64) -> f64 {
    let t1 = t - 1.0;
    t1 * t1 * t1 + 1.0
}
#[inline]
pub fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let t1 = 2.0 * t - 2.0;
        0.5 * t1 * t1 * t1 + 1.0
    }
}

// ── Hash functions (canonical implementations) ──────────────────────

/// Deterministic hash: maps two floats to [0, 1].
/// Based on the classic sin-based hash used in GLSL shaders.
#[inline]
pub fn hash_f64(a: f64, b: f64) -> f64 {
    let dot = a * 12.9898 + b * 78.233;
    (dot.sin() * 43758.5453).fract().abs()
}

/// Deterministic 3-argument hash: maps three floats to [0, 1].
#[inline]
pub fn hash3_f64(a: f64, b: f64, c: f64) -> f64 {
    let dot = a * 12.9898 + b * 78.233 + c * 45.164;
    (dot.sin() * 43758.5453).fract().abs()
}

/// Look up a field by name on a given owner type.
/// Returns `Some((result_type, op))` if the field is valid.
pub fn lookup_field(owner_ty: &TypeName, field: &str) -> Option<(&'static TypeName, Op)> {
    FIELD_OPS.iter()
        .find(|(name, ty, _)| *name == field && *ty == *owner_ty)
        .map(|(_, _, op)| (&TypeName::Float, *op))
}

/// Returns true if `field` is a known field on `Color` (derived from `FIELD_OPS`).
pub fn is_color_field(field: &str) -> bool {
    FIELD_OPS.iter().any(|(name, ty, _)| *name == field && *ty == TypeName::Color)
}

// ── Op → BinOp mapping (for peephole optimizer) ─────────────────────

/// Map a VM opcode to its corresponding `BinOp`, if the opcode represents
/// a binary arithmetic/comparison/logic operation.  Used by the peephole
/// optimizer to delegate constant folding to `eval_binary_op`.
pub fn op_to_binop(op: Op) -> Option<BinOp> {
    Some(match op {
        Op::Add => BinOp::Add,
        Op::Sub => BinOp::Sub,
        Op::Mul => BinOp::Mul,
        Op::Div => BinOp::Div,
        Op::Mod => BinOp::Mod,
        Op::Pow => BinOp::Pow,
        Op::Lt => BinOp::Lt,
        Op::Gt => BinOp::Gt,
        Op::Le => BinOp::Le,
        Op::Ge => BinOp::Ge,
        Op::Eq => BinOp::Eq,
        Op::Ne => BinOp::Ne,
        Op::And => BinOp::And,
        Op::Or => BinOp::Or,
        Op::BitAnd => BinOp::BitAnd,
        Op::BitOr => BinOp::BitOr,
        Op::BitXor => BinOp::BitXor,
        Op::Shl => BinOp::Shl,
        Op::Shr => BinOp::Shr,
        _ => return None,
    })
}

// ── Noise algorithms ────────────────────────────────────────────────

/// Deterministic noise algorithms (Perlin, FBM, Worley).
/// All functions are pure — no RNG state, hardcoded permutation table.
pub mod noise {
    /// Hardcoded permutation table for Perlin noise (doubled for wrapping).
    const PERM: [u8; 512] = {
        const P: [u8; 256] = [
            151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225,
            140, 36, 103, 30, 69, 142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148,
            247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219, 203, 117, 35, 11, 32,
            57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175,
            74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122,
            60, 211, 133, 230, 220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54,
            65, 25, 63, 161, 1, 216, 80, 73, 209, 76, 132, 187, 208, 89, 18, 169,
            200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173, 186, 3, 64,
            52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212,
            207, 206, 59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213,
            119, 248, 152, 2, 44, 154, 163, 70, 221, 153, 101, 155, 167, 43, 172, 9,
            129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232, 178, 185, 112, 104,
            218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162, 241,
            81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157,
            184, 84, 204, 176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93,
            222, 114, 67, 29, 24, 72, 243, 141, 128, 195, 78, 66, 215, 61, 156, 180,
        ];
        let mut table = [0u8; 512];
        let mut i = 0;
        while i < 512 {
            table[i] = P[i & 255];
            i += 1;
        }
        table
    };

    #[inline]
    fn fade(t: f64) -> f64 {
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    #[inline]
    fn lerp(t: f64, a: f64, b: f64) -> f64 {
        a + t * (b - a)
    }

    /// Gradient function for 1D Perlin noise.
    #[inline]
    fn grad1(hash: u8, x: f64) -> f64 {
        if hash & 1 == 0 { x } else { -x }
    }

    /// Gradient function for 2D Perlin noise.
    #[inline]
    fn grad2(hash: u8, x: f64, y: f64) -> f64 {
        let h = hash & 3;
        match h {
            0 => x + y,
            1 => -x + y,
            2 => x - y,
            _ => -x - y,
        }
    }

    /// Gradient function for 3D Perlin noise.
    #[inline]
    fn grad3(hash: u8, x: f64, y: f64, z: f64) -> f64 {
        let h = hash & 15;
        let u = if h < 8 { x } else { y };
        let v = if h < 4 { y } else if h == 12 || h == 14 { x } else { z };
        (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn perm_idx(v: i32) -> usize {
        (v & 255) as usize
    }

    /// 1D Perlin noise, returns [-1, 1].
    #[allow(clippy::cast_possible_truncation)]
    pub fn perlin1(x: f64) -> f64 {
        let xi = x.floor() as i32;
        let xf = x - x.floor();
        let u = fade(xf);

        let a = PERM[perm_idx(xi)];
        let b = PERM[perm_idx(xi + 1)];

        lerp(u, grad1(a, xf), grad1(b, xf - 1.0))
    }

    /// 2D Perlin noise, returns [-1, 1].
    #[allow(clippy::cast_possible_truncation)]
    pub fn perlin2(x: f64, y: f64) -> f64 {
        let xi = x.floor() as i32;
        let yi = y.floor() as i32;
        let xf = x - x.floor();
        let yf = y - y.floor();
        let u = fade(xf);
        let v = fade(yf);

        let aa = PERM[perm_idx(i32::from(PERM[perm_idx(xi)]) + yi)];
        let ab = PERM[perm_idx(i32::from(PERM[perm_idx(xi)]) + yi + 1)];
        let ba = PERM[perm_idx(i32::from(PERM[perm_idx(xi + 1)]) + yi)];
        let bb = PERM[perm_idx(i32::from(PERM[perm_idx(xi + 1)]) + yi + 1)];

        lerp(v,
            lerp(u, grad2(aa, xf, yf), grad2(ba, xf - 1.0, yf)),
            lerp(u, grad2(ab, xf, yf - 1.0), grad2(bb, xf - 1.0, yf - 1.0)),
        )
    }

    /// 3D Perlin noise, returns [-1, 1].
    #[allow(clippy::cast_possible_truncation)]
    pub fn perlin3(x: f64, y: f64, z: f64) -> f64 {
        let xi = x.floor() as i32;
        let yi = y.floor() as i32;
        let zi = z.floor() as i32;
        let xf = x - x.floor();
        let yf = y - y.floor();
        let zf = z - z.floor();
        let u = fade(xf);
        let v = fade(yf);
        let w = fade(zf);

        let a  = i32::from(PERM[perm_idx(xi)]) + yi;
        let aa = i32::from(PERM[perm_idx(a)]) + zi;
        let ab = i32::from(PERM[perm_idx(a + 1)]) + zi;
        let b  = i32::from(PERM[perm_idx(xi + 1)]) + yi;
        let ba = i32::from(PERM[perm_idx(b)]) + zi;
        let bb = i32::from(PERM[perm_idx(b + 1)]) + zi;

        lerp(w,
            lerp(v,
                lerp(u,
                    grad3(PERM[perm_idx(aa)], xf, yf, zf),
                    grad3(PERM[perm_idx(ba)], xf - 1.0, yf, zf),
                ),
                lerp(u,
                    grad3(PERM[perm_idx(ab)], xf, yf - 1.0, zf),
                    grad3(PERM[perm_idx(bb)], xf - 1.0, yf - 1.0, zf),
                ),
            ),
            lerp(v,
                lerp(u,
                    grad3(PERM[perm_idx(aa + 1)], xf, yf, zf - 1.0),
                    grad3(PERM[perm_idx(ba + 1)], xf - 1.0, yf, zf - 1.0),
                ),
                lerp(u,
                    grad3(PERM[perm_idx(ab + 1)], xf, yf - 1.0, zf - 1.0),
                    grad3(PERM[perm_idx(bb + 1)], xf - 1.0, yf - 1.0, zf - 1.0),
                ),
            ),
        )
    }

    /// Fractal Brownian Motion using 2D Perlin noise.
    /// Lacunarity = 2.0, gain = 0.5.
    pub fn fbm(x: f64, y: f64, octaves: u32) -> f64 {
        let octaves = octaves.clamp(1, 10);
        let mut sum = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = 1.0;
        let mut max_amp = 0.0;

        for _ in 0..octaves {
            sum += amplitude * perlin2(x * frequency, y * frequency);
            max_amp += amplitude;
            amplitude *= 0.5;
            frequency *= 2.0;
        }

        sum / max_amp
    }

    /// 2D Worley (cellular) noise, returns [0, 1].
    /// Returns the distance to the nearest cell point.
    #[allow(clippy::cast_possible_truncation)]
    pub fn worley2(x: f64, y: f64) -> f64 {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let fx = x - x.floor();
        let fy = y - y.floor();

        let mut min_dist = f64::MAX;

        for dy in -1..=1 {
            for dx in -1..=1 {
                // Deterministic point position within neighbor cell
                let cell_x = ix + dx;
                let cell_y = iy + dy;
                let h = PERM[perm_idx(i32::from(PERM[perm_idx(cell_x)]) + cell_y)];
                let px = f64::from(dx) + (f64::from(h) / 255.0) - fx;
                let py = f64::from(dy) + (f64::from(PERM[perm_idx(i32::from(h) + 1)]) / 255.0) - fy;
                let dist = px * px + py * py;
                if dist < min_dist {
                    min_dist = dist;
                }
            }
        }

        min_dist.sqrt().min(1.0)
    }
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::dsl::builtins::BUILTINS;

    /// Verify that every const-foldable float-only builtin in the BUILTINS table
    /// is handled by eval_builtin_fn (which now delegates to the builtin's eval fn).
    #[test]
    fn eval_builtin_fn_covers_all_float_builtins() {
        for b in BUILTINS {
            // Only check builtins where all params are Float and return is Float
            let all_float_params = b.params.iter().all(|(_, ty)| *ty == TypeName::Float);
            if !all_float_params || b.ret != TypeName::Float {
                continue;
            }
            if !b.const_foldable {
                continue;
            }

            let dummy_args: Vec<f64> = (0..b.params.len()).map(|i| (i + 1) as f64).collect();
            assert!(
                eval_builtin_fn(b.name, &dummy_args).is_some(),
                "eval_builtin_fn does not handle float builtin '{}' with {} args — \
                 check the builtin's eval function and const_foldable flag",
                b.name,
                b.params.len(),
            );
        }
    }
}
