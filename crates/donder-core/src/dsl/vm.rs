use super::compiler::{CompiledScript, Op};
use super::ops::{self, Value};
use crate::model::color::Color;
use crate::model::color_gradient::ColorGradient;
use crate::model::curve::Curve;
use crate::model::motion_path::MotionPath;

/// Maximum stack depth to prevent runaway scripts.
const MAX_STACK: usize = 256;

/// Maximum number of instructions executed per pixel to prevent infinite loops.
/// With no loops in the current DSL, real scripts complete in well under 10 000
/// instructions per pixel.  This limit is a safety net against compiler bugs
/// or future loop constructs that could produce backward jumps.
const MAX_INSTRUCTIONS: usize = 100_000;

/// Reusable VM working memory. Create once per batch, reuse across pixels
/// to avoid heap allocations in the per-pixel hot path.
#[derive(Default)]
pub struct VmBuffers {
    stack: Vec<Value>,
    locals: Vec<Value>,
}

impl VmBuffers {
    pub fn new() -> Self {
        Self {
            stack: Vec::with_capacity(64),
            locals: Vec::new(),
        }
    }

    /// Clear and resize for a new execution. Reuses existing heap allocations.
    fn reset(&mut self, local_count: usize) {
        self.stack.clear();
        self.locals.clear();
        self.locals.resize(local_count, Value::Float(0.0));
    }
}

/// Runtime context provided per-pixel.
pub struct VmContext<'a> {
    pub t: f64,
    pub pixel: usize,
    pub pixels: usize,
    pub pos: f64,
    pub pos2d: (f64, f64),
    /// Absolute time in seconds (for motion path evaluation).
    pub abs_t: f64,
    pub param_values: &'a [f64],
    pub gradients: &'a [Option<&'a ColorGradient>],
    pub curves: &'a [Option<&'a Curve>],
    pub colors: &'a [Option<Color>],
    pub paths: &'a [Option<&'a MotionPath>],
}

/// Execute a compiled script for one pixel, returning the output color.
///
/// For batch execution, prefer `execute_reuse` with a shared `VmBuffers`
/// to avoid per-pixel heap allocations.
pub fn execute(script: &CompiledScript, ctx: &VmContext<'_>) -> Color {
    let mut buffers = VmBuffers::new();
    execute_reuse(script, ctx, &mut buffers)
}

/// Execute a compiled script reusing pre-allocated buffers.
///
/// This avoids heap allocations on every pixel — call `execute_reuse` in a
/// loop with the same `VmBuffers` for zero-alloc per-pixel evaluation.
pub fn execute_reuse(script: &CompiledScript, ctx: &VmContext<'_>, buffers: &mut VmBuffers) -> Color {
    buffers.reset(script.local_count as usize);
    let stack = &mut buffers.stack;
    let locals = &mut buffers.locals;
    let mut ip: usize = 0;
    let mut instruction_count: usize = 0;
    let ops = &script.ops;
    let consts = &script.constants;

    while ip < ops.len() {
        instruction_count += 1;
        if instruction_count > MAX_INSTRUCTIONS || stack.len() >= MAX_STACK {
            return Color::BLACK;
        }

        match ops[ip] {
            Op::PushConst(idx) => {
                debug_assert!((idx as usize) < consts.len(), "PushConst index {idx} out of bounds (len {})", consts.len());
                let val = consts.get(idx as usize).copied().unwrap_or(0.0);
                stack.push(Value::Float(val));
            }
            Op::PushParam(idx) => {
                debug_assert!((idx as usize) < ctx.param_values.len(), "PushParam index {idx} out of bounds (len {})", ctx.param_values.len());
                let val = ctx.param_values.get(idx as usize).copied().unwrap_or(0.0);
                stack.push(Value::Float(val));
            }
            Op::LoadLocal(idx) => {
                debug_assert!((idx as usize) < locals.len(), "LoadLocal index {idx} out of bounds (len {})", locals.len());
                let val = locals.get(idx as usize).copied().unwrap_or(Value::Float(0.0));
                stack.push(val);
            }
            Op::StoreLocal(idx) => {
                debug_assert!(!stack.is_empty(), "StoreLocal: stack underflow");
                let val = pop(stack);
                if (idx as usize) < locals.len() {
                    locals[idx as usize] = val;
                }
            }
            Op::Pop => {
                debug_assert!(!stack.is_empty(), "Pop: stack underflow");
                pop(stack);
            }

            // Arithmetic, comparison, logic, and bitwise — all binary ops that
            // have a corresponding BinOp are dispatched through the shared
            // eval_binary_op in ops.rs to keep semantics in one place.
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod | Op::Pow
            | Op::Lt | Op::Gt | Op::Le | Op::Ge | Op::Eq | Op::Ne
            | Op::And | Op::Or
            | Op::BitAnd | Op::BitOr | Op::BitXor | Op::Shl | Op::Shr => {
                // SAFETY: all Op variants listed above have a matching BinOp
                if let Some(binop) = ops::op_to_binop(ops[ip]) {
                    float_binop(stack, |a, b| ops::eval_binary_op(binop, a, b));
                }
            }
            Op::Neg => {
                debug_assert!(!stack.is_empty(), "Neg: stack underflow");
                let val = pop(stack);
                stack.push(Value::Float(-val.as_float()));
            }
            Op::Not => {
                debug_assert!(!stack.is_empty(), "Not: stack underflow");
                let val = pop(stack);
                stack.push(Value::Float(if val.as_float() == 0.0 { 1.0 } else { 0.0 }));
            }

            // All builtin functions dispatch through a single handler.
            // The eval function pointer in the BUILTINS table is the single
            // source of truth for each builtin's semantics.
            Op::CallBuiltin(idx) => {
                let bi = &super::builtins::BUILTINS[idx as usize];
                let n = bi.params.len();
                debug_assert!(stack.len() >= n, "CallBuiltin({}): stack underflow ({} < {})", bi.name, stack.len(), n);
                let mut args = [Value::Float(0.0); 8];
                for i in (0..n).rev() {
                    args[i] = pop(stack);
                }
                stack.push((bi.eval)(&args[..n]));
            }
            Op::ColorScale => {
                debug_assert!(stack.len() >= 2, "ColorScale: stack underflow");
                let factor = pop(stack).as_float();
                let color = pop(stack).as_color();
                stack.push(Value::Color(color.scale(factor)));
            }
            Op::ColorAdd => {
                debug_assert!(stack.len() >= 2, "ColorAdd: stack underflow");
                let b = pop(stack).as_color();
                let a = pop(stack).as_color();
                stack.push(Value::Color(a + b));
            }
            Op::ColorSub => {
                debug_assert!(stack.len() >= 2, "ColorSub: stack underflow");
                let b = pop(stack).as_color();
                let a = pop(stack).as_color();
                stack.push(Value::Color(a.subtract(b)));
            }
            Op::Vec2Add => {
                debug_assert!(stack.len() >= 2, "Vec2Add: stack underflow");
                let (bx, by) = pop(stack).as_vec2();
                let (ax, ay) = pop(stack).as_vec2();
                stack.push(Value::Vec2(ax + bx, ay + by));
            }
            Op::Vec2Sub => {
                debug_assert!(stack.len() >= 2, "Vec2Sub: stack underflow");
                let (bx, by) = pop(stack).as_vec2();
                let (ax, ay) = pop(stack).as_vec2();
                stack.push(Value::Vec2(ax - bx, ay - by));
            }
            Op::Vec2Scale => {
                debug_assert!(stack.len() >= 2, "Vec2Scale: stack underflow");
                let factor = pop(stack).as_float();
                let (vx, vy) = pop(stack).as_vec2();
                stack.push(Value::Vec2(vx * factor, vy * factor));
            }
            Op::ColorMix => {
                debug_assert!(stack.len() >= 3, "ColorMix: stack underflow");
                let t = pop(stack).as_float();
                let b = pop(stack).as_color();
                let a = pop(stack).as_color();
                stack.push(Value::Color(a.lerp(b, t)));
            }
            Op::ColorHue => {
                debug_assert!(!stack.is_empty(), "ColorHue: stack underflow");
                let c = pop(stack).as_color();
                let (h, _, _) = c.to_hsv();
                stack.push(Value::Float(h));
            }
            Op::ColorSaturation => {
                debug_assert!(!stack.is_empty(), "ColorSaturation: stack underflow");
                let c = pop(stack).as_color();
                let (_, s, _) = c.to_hsv();
                stack.push(Value::Float(s));
            }
            Op::ColorValue => {
                debug_assert!(!stack.is_empty(), "ColorValue: stack underflow");
                let c = pop(stack).as_color();
                let (_, _, v) = c.to_hsv();
                stack.push(Value::Float(v));
            }
            Op::ColorR => {
                debug_assert!(!stack.is_empty(), "ColorR: stack underflow");
                let c = pop(stack).as_color();
                stack.push(Value::Float(f64::from(c.r) / 255.0));
            }
            Op::ColorG => {
                debug_assert!(!stack.is_empty(), "ColorG: stack underflow");
                let c = pop(stack).as_color();
                stack.push(Value::Float(f64::from(c.g) / 255.0));
            }
            Op::ColorB => {
                debug_assert!(!stack.is_empty(), "ColorB: stack underflow");
                let c = pop(stack).as_color();
                stack.push(Value::Float(f64::from(c.b) / 255.0));
            }
            Op::ColorA => {
                debug_assert!(!stack.is_empty(), "ColorA: stack underflow");
                let c = pop(stack).as_color();
                stack.push(Value::Float(f64::from(c.a) / 255.0));
            }

            // Vec2 field access
            Op::Vec2X => {
                debug_assert!(!stack.is_empty(), "Vec2X: stack underflow");
                let (x, _) = pop(stack).as_vec2();
                stack.push(Value::Float(x));
            }
            Op::Vec2Y => {
                debug_assert!(!stack.is_empty(), "Vec2Y: stack underflow");
                let (_, y) = pop(stack).as_vec2();
                stack.push(Value::Float(y));
            }
            // Gradient/Curve evaluation
            Op::EvalGradient(param_idx) => {
                debug_assert!(!stack.is_empty(), "EvalGradient: stack underflow");
                let t = pop(stack).as_float();
                let color = ctx.gradients.get(param_idx as usize)
                    .and_then(|g| g.as_ref())
                    .map_or(Color::BLACK, |g| g.evaluate(t));
                stack.push(Value::Color(color));
            }
            Op::EvalCurve(param_idx) => {
                debug_assert!(!stack.is_empty(), "EvalCurve: stack underflow");
                let x = pop(stack).as_float();
                let y = ctx.curves.get(param_idx as usize)
                    .and_then(|c| c.as_ref())
                    .map_or(0.0, |c| c.evaluate(x));
                stack.push(Value::Float(y));
            }
            Op::LoadColor(param_idx) => {
                let color = ctx.colors.get(param_idx as usize)
                    .and_then(|c| *c)
                    .unwrap_or(Color::BLACK);
                stack.push(Value::Color(color));
            }
            Op::EvalPath(param_idx) => {
                debug_assert!(!stack.is_empty(), "EvalPath: stack underflow");
                let t = pop(stack).as_float();
                let (x, y) = ctx.paths.get(param_idx as usize)
                    .and_then(|p| p.as_ref())
                    .map_or((0.0, 0.0), |p| p.evaluate(t));
                stack.push(Value::Vec2(x, y));
            }
            Op::EvalPathAtT(param_idx) => {
                let (x, y) = ctx.paths.get(param_idx as usize)
                    .and_then(|p| p.as_ref())
                    .map_or((0.0, 0.0), |p| p.evaluate(ctx.abs_t));
                stack.push(Value::Vec2(x, y));
            }

            // Enum/Flags
            #[allow(clippy::cast_sign_loss)]
            Op::EnumEq(variant_idx) => {
                debug_assert!(!stack.is_empty(), "EnumEq: stack underflow");
                let param_val = pop(stack).as_float() as u32;
                stack.push(Value::Float(
                    if param_val == u32::from(variant_idx) { 1.0 } else { 0.0 }
                ));
            }
            #[allow(clippy::cast_sign_loss)]
            Op::FlagTest(bit_mask) => {
                debug_assert!(!stack.is_empty(), "FlagTest: stack underflow");
                let flags = pop(stack).as_float() as u32;
                stack.push(Value::Float(
                    if flags & bit_mask != 0 { 1.0 } else { 0.0 }
                ));
            }

            // Control flow
            Op::JumpIfFalse(target) => {
                debug_assert!(!stack.is_empty(), "JumpIfFalse: stack underflow");
                let val = pop(stack);
                if val.as_float() == 0.0 {
                    ip = target as usize;
                    continue;
                }
            }
            Op::Jump(target) => {
                ip = target as usize;
                continue;
            }

            // Type conversion
            Op::IntToFloat => {
                // Int is already stored as f64, so this is a no-op in our VM
            }

            // Builtin variables
            Op::PushT => stack.push(Value::Float(ctx.t)),
            #[allow(clippy::cast_precision_loss)]
            Op::PushPixel => stack.push(Value::Float(ctx.pixel as f64)),
            #[allow(clippy::cast_precision_loss)]
            Op::PushPixels => stack.push(Value::Float(ctx.pixels as f64)),
            Op::PushPos => stack.push(Value::Float(ctx.pos)),
            Op::PushPos2d => stack.push(Value::Vec2(ctx.pos2d.0, ctx.pos2d.1)),
            Op::PushAbsT => stack.push(Value::Float(ctx.abs_t)),

            Op::Return => break,
        }

        ip += 1;
    }

    // Top of stack is the result color
    debug_assert!(!stack.is_empty(), "VM finished with empty stack");
    pop(stack).as_color()
}

/// Pop a value from the stack. The compiler guarantees the stack is non-empty
/// at every pop site; this is a convenience wrapper over `Vec::pop` + unwrap_or
/// (the fallback can only fire due to a compiler bug, caught by debug_assert
/// at call sites).
fn pop(stack: &mut Vec<Value>) -> Value {
    stack.pop().unwrap_or(Value::Float(0.0))
}

/// Binary operation on two floats from the stack.
/// Used by binary operator dispatch (Add, Sub, Mul, etc.).
fn float_binop(stack: &mut Vec<Value>, op: impl FnOnce(f64, f64) -> f64) {
    debug_assert!(stack.len() >= 2, "float_binop: stack underflow");
    let b = pop(stack).as_float();
    let a = pop(stack).as_float();
    stack.push(Value::Float(op(a, b)));
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "vm_tests.rs"]
mod vm_tests;
