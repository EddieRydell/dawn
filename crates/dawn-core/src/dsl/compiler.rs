use super::ast::{BinOp, Expr, ParamType, Span, UnaryOp};
use super::builtins::{self, BuiltinVar};
use super::error::CompileError;
use super::ops;
use super::typeck::{TypedExpr, TypedExprKind, TypedScript, TypedStmt, TypedStmtKind};

/// A compiled DSL script ready for VM execution.
#[derive(Debug, Clone)]
pub struct CompiledScript {
    pub name: String,
    pub spatial: bool,
    pub ops: Vec<Op>,
    pub constants: Vec<f64>,
    /// Param metadata for the runtime to map ParamValue → f64/gradient/curve.
    pub params: Vec<CompiledParam>,
    /// Number of local variable slots needed.
    pub local_count: u16,
    /// Enum definitions: name → variant names (for runtime variant resolution).
    pub enums: Vec<EnumDef>,
    /// Flags definitions: name → flag names (for runtime bitmask resolution).
    pub flags: Vec<FlagsDef>,
}

/// Enum type definition carried into the compiled script for runtime resolution.
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<String>,
}

/// Flags type definition carried into the compiled script for runtime resolution.
#[derive(Debug, Clone)]
pub struct FlagsDef {
    pub name: String,
    pub flags: Vec<String>,
}

/// Compiled param info (index matches TypedParam order).
#[derive(Debug, Clone)]
pub struct CompiledParam {
    pub name: String,
    pub ty: ParamType,
    /// The default-value expression from the DSL source, preserved for UI display.
    /// `None` for param types that have no default (e.g. `Path`).
    pub default: Option<Expr>,
}

/// Bytecode operations for the stack-based VM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    /// Push a constant from the constant pool.
    PushConst(u16),
    /// Push a parameter value (resolved at runtime).
    PushParam(u16),
    /// Load a local variable onto the stack.
    LoadLocal(u16),
    /// Store top of stack into a local variable slot.
    StoreLocal(u16),
    /// Pop top of stack.
    Pop,

    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,

    // Comparison
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,

    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,

    // Logic
    And,
    Or,
    Not,

    // Binary power operator (also usable as builtin `pow()` via CallBuiltin)
    Pow,

    /// Call a builtin function by index into the BUILTINS table.
    /// The VM pops args by arity and calls the builtin's eval function.
    CallBuiltin(u8),

    // Type-level ops (not builtins — emitted by typed AST nodes)
    /// Pop color, pop float → push scaled color
    ColorScale,
    /// Pop two colors → push saturating add
    ColorAdd,
    /// Pop two colors → push saturating subtract (a - b)
    ColorSub,
    /// Pop two Vec2 → push component-wise add
    Vec2Add,
    /// Pop two Vec2 → push component-wise subtract (a - b)
    Vec2Sub,
    /// Pop Vec2, pop float → push scaled Vec2
    Vec2Scale,
    /// Pop color_a, color_b, float_t → push lerp(a, b, t)
    ColorMix,

    // Field access ops
    /// Pop color → push float (hue 0-360)
    ColorHue,
    /// Pop color → push float (saturation 0-1)
    ColorSaturation,
    /// Pop color → push float (value 0-1)
    ColorValue,
    /// Pop color → push float (r channel)
    ColorR,
    /// Pop color → push float (g channel)
    ColorG,
    /// Pop color → push float (b channel)
    ColorB,
    /// Pop color → push float (a channel)
    ColorA,
    /// Pop Vec2 → push float (x)
    Vec2X,
    /// Pop Vec2 → push float (y)
    Vec2Y,

    // Gradient/Curve/Color param evaluation
    /// Pop float t → push Color from gradient param
    EvalGradient(u16),
    /// Pop float t → push float from curve param
    EvalCurve(u16),
    /// Push Color from a color param
    LoadColor(u16),
    /// Pop float t → push Vec2 from motion path param
    EvalPath(u16),
    /// Push Vec2 from motion path param evaluated at abs_t
    EvalPathAtT(u16),

    // Enum/Flags
    /// Pop int → compare with variant index → push bool
    EnumEq(u16),
    /// Pop int → test bitmask → push bool
    FlagTest(u32),

    // Control flow
    /// Jump forward if top of stack is false (pop condition).
    JumpIfFalse(u16),
    /// Unconditional jump.
    Jump(u16),

    // Type conversion
    /// Pop int → push float
    IntToFloat,

    // Builtin variables
    PushT,
    PushPixel,
    PushPixels,
    PushPos,
    PushPos2d,
    PushAbsT,

    /// Halt execution, top of stack is the return color.
    Return,
}

pub fn compile(typed: &TypedScript) -> Result<CompiledScript, CompileError> {
    let mut compiler = Compiler::new();

    // Register params
    for p in &typed.params {
        compiler.params.push(CompiledParam {
            name: p.name.clone(),
            ty: p.ty.clone(),
            default: p.default.clone(),
        });
    }

    // Compile body
    compiler.compile_block(&typed.body)?;

    compiler.emit(Op::Return);

    Ok(CompiledScript {
        name: typed.name.clone(),
        spatial: typed.spatial,
        ops: compiler.ops,
        constants: compiler.constants,
        params: compiler.params,
        local_count: compiler.local_count,
        enums: typed.enums.iter().map(|td| EnumDef {
            name: td.name.clone(),
            variants: td.variants.clone(),
        }).collect(),
        flags: typed.flags.iter().map(|td| FlagsDef {
            name: td.name.clone(),
            flags: td.variants.clone(),
        }).collect(),
    })
}

struct Compiler {
    ops: Vec<Op>,
    constants: Vec<f64>,
    params: Vec<CompiledParam>,
    local_count: u16,
}

impl Compiler {
    fn new() -> Self {
        Self {
            ops: Vec::new(),
            constants: Vec::new(),
            params: Vec::new(),
            local_count: 0,
        }
    }

    fn emit(&mut self, op: Op) {
        self.ops.push(op);
    }

    fn emit_const(&mut self, value: f64) -> Result<(), CompileError> {
        let idx = self.add_constant(value)?;
        self.emit(Op::PushConst(idx));
        Ok(())
    }

    fn add_constant(&mut self, value: f64) -> Result<u16, CompileError> {
        super::intern_constant(&mut self.constants, value).ok_or_else(|| {
            CompileError::compiler("Too many constants (max 65535)", Span::new(0, 0))
        })
    }

    fn current_offset(&self) -> usize {
        self.ops.len()
    }

    fn patch_jump(&mut self, idx: usize) -> Result<(), CompileError> {
        let target = u16::try_from(self.ops.len()).map_err(|_| {
            CompileError::compiler("Bytecode too large (max 65535 ops)", Span::new(0, 0))
        })?;
        match &mut self.ops[idx] {
            Op::JumpIfFalse(ref mut dest) | Op::Jump(ref mut dest) => *dest = target,
            _ => {}
        }
        Ok(())
    }

    fn compile_stmt(&mut self, stmt: &TypedStmt) -> Result<(), CompileError> {
        match &stmt.kind {
            TypedStmtKind::Let { value, local_index, .. } => {
                self.compile_expr(value)?;
                let idx = *local_index;
                if idx >= self.local_count {
                    self.local_count = idx.checked_add(1).ok_or_else(|| {
                        CompileError::compiler("Too many local variables (max 65535)", Span::new(0, 0))
                    })?;
                }
                self.emit(Op::StoreLocal(idx));
                Ok(())
            }
            TypedStmtKind::Expr(expr) => {
                self.compile_expr(expr)?;
                Ok(())
            }
        }
    }

    /// Compile a block of statements, emitting `Pop` after intermediate expression
    /// statements to keep the stack clean. Only the last statement's value remains.
    fn compile_block(&mut self, stmts: &[TypedStmt]) -> Result<(), CompileError> {
        for (i, stmt) in stmts.iter().enumerate() {
            // Pop the value left by a previous expression statement (non-last).
            // `let` statements don't leave values on the stack (StoreLocal pops).
            if i > 0 {
                if let TypedStmtKind::Expr(_) = &stmts[i - 1].kind {
                    self.emit(Op::Pop);
                }
            }
            self.compile_stmt(stmt)?;
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &TypedExpr) -> Result<(), CompileError> {
        match &expr.kind {
            TypedExprKind::FloatLit(v) => {
                self.emit_const(*v)?;
            }
            TypedExprKind::IntLit(v) => {
                self.emit_const(f64::from(*v))?;
            }
            TypedExprKind::BoolLit(v) => {
                self.emit_const(if *v { 1.0 } else { 0.0 })?;
            }
            TypedExprKind::ColorLit { r, g, b } => {
                self.emit_const(f64::from(*r) / 255.0)?;
                self.emit_const(f64::from(*g) / 255.0)?;
                self.emit_const(f64::from(*b) / 255.0)?;
                self.emit(Self::builtin_op("rgb", expr.span)?);
            }
            TypedExprKind::LoadLocal(idx) => {
                self.emit(Op::LoadLocal(*idx));
            }
            TypedExprKind::LoadParam(idx) => {
                self.emit(Op::PushParam(*idx));
            }
            TypedExprKind::LoadColor(idx) => {
                self.emit(Op::LoadColor(*idx));
            }
            TypedExprKind::LoadBuiltin(var) => {
                self.emit(match var {
                    BuiltinVar::T => Op::PushT,
                    BuiltinVar::Pixel => Op::PushPixel,
                    BuiltinVar::Pixels => Op::PushPixels,
                    BuiltinVar::Pos => Op::PushPos,
                    BuiltinVar::Pos2d => Op::PushPos2d,
                    BuiltinVar::AbsT => Op::PushAbsT,
                    BuiltinVar::Pi => {
                        self.emit_const(std::f64::consts::PI)?;
                        return Ok(());
                    }
                    BuiltinVar::Tau => {
                        self.emit_const(std::f64::consts::TAU)?;
                        return Ok(());
                    }
                });
            }
            TypedExprKind::BinOp { op, left, right } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                self.emit(match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Mod => Op::Mod,
                    BinOp::Pow => Op::Pow,
                    BinOp::Lt => Op::Lt,
                    BinOp::Gt => Op::Gt,
                    BinOp::Le => Op::Le,
                    BinOp::Ge => Op::Ge,
                    BinOp::Eq => Op::Eq,
                    BinOp::Ne => Op::Ne,
                    BinOp::And => Op::And,
                    BinOp::Or => Op::Or,
                    BinOp::BitOr => Op::BitOr,
                    BinOp::BitAnd => Op::BitAnd,
                    BinOp::BitXor => Op::BitXor,
                    BinOp::Shl => Op::Shl,
                    BinOp::Shr => Op::Shr,
                });
            }
            TypedExprKind::UnaryOp { op, operand } => {
                self.compile_expr(operand)?;
                self.emit(match op {
                    UnaryOp::Neg => Op::Neg,
                    UnaryOp::Not => Op::Not,
                });
            }
            TypedExprKind::BuiltinCall { name, args } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.emit(Self::builtin_op(name, expr.span)?);
            }
            TypedExprKind::EvalGradient { param_index, arg } => {
                self.compile_expr(arg)?;
                self.emit(Op::EvalGradient(*param_index));
            }
            TypedExprKind::EvalCurve { param_index, arg } => {
                self.compile_expr(arg)?;
                self.emit(Op::EvalCurve(*param_index));
            }
            TypedExprKind::EvalPath { param_index, arg } => {
                self.compile_expr(arg)?;
                self.emit(Op::EvalPath(*param_index));
            }
            TypedExprKind::EvalPathAtT { param_index } => {
                self.emit(Op::EvalPathAtT(*param_index));
            }
            TypedExprKind::ColorScale { color, factor } => {
                self.compile_expr(color)?;
                self.compile_expr(factor)?;
                self.emit(Op::ColorScale);
            }
            TypedExprKind::ColorAdd { left, right } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                self.emit(Op::ColorAdd);
            }
            TypedExprKind::ColorSub { left, right } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                self.emit(Op::ColorSub);
            }
            TypedExprKind::Vec2Add { left, right } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                self.emit(Op::Vec2Add);
            }
            TypedExprKind::Vec2Sub { left, right } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                self.emit(Op::Vec2Sub);
            }
            TypedExprKind::Vec2Scale { vec, factor } => {
                self.compile_expr(vec)?;
                self.compile_expr(factor)?;
                self.emit(Op::Vec2Scale);
            }
            TypedExprKind::ColorMix { a, b, t } => {
                self.compile_expr(a)?;
                self.compile_expr(b)?;
                self.compile_expr(t)?;
                self.emit(Op::ColorMix);
            }
            TypedExprKind::Field { object, field } => {
                self.compile_expr(object)?;
                // Look up field op from the shared FIELD_OPS table.
                // The type checker already validated this field, so a miss
                // here indicates a compiler bug.
                let (_, op) = ops::lookup_field(&object.ty, field).ok_or_else(|| {
                    CompileError::compiler(
                        format!("Unknown field '{field}' in compiler"),
                        expr.span,
                    )
                })?;
                self.emit(op);
            }
            TypedExprKind::MakeVec2 { x, y } => {
                self.compile_expr(x)?;
                self.compile_expr(y)?;
                self.emit(Self::builtin_op("vec2", expr.span)?);
            }
            TypedExprKind::If { condition, then_body, else_body } => {
                self.compile_expr(condition)?;
                let jump_else = self.current_offset();
                self.emit(Op::JumpIfFalse(0)); // placeholder

                // Compile then branch
                self.compile_block(then_body)?;

                if let Some(else_stmts) = else_body {
                    let jump_end = self.current_offset();
                    self.emit(Op::Jump(0)); // placeholder
                    self.patch_jump(jump_else)?;

                    // Compile else branch
                    self.compile_block(else_stmts)?;
                    self.patch_jump(jump_end)?;
                } else {
                    self.patch_jump(jump_else)?;
                }
            }
            TypedExprKind::EnumEq { param_index, variant_index } => {
                self.emit(Op::PushParam(*param_index));
                self.emit(Op::EnumEq(*variant_index));
            }
            TypedExprKind::FlagTest { param_index, bit_mask } => {
                self.emit(Op::PushParam(*param_index));
                self.emit(Op::FlagTest(*bit_mask));
            }
            TypedExprKind::IntToFloat(inner) => {
                self.compile_expr(inner)?;
                self.emit(Op::IntToFloat);
            }
            TypedExprKind::Block(stmts) => {
                self.compile_block(stmts)?;
            }
        }
        Ok(())
    }

    fn builtin_op(name: &str, span: Span) -> Result<Op, CompileError> {
        builtins::builtin_index(name)
            .map(Op::CallBuiltin)
            .ok_or_else(|| CompileError::compiler(
                format!("Unknown builtin function '{name}' in compiler"),
                span,
            ))
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::dsl::lexer::lex;
    use crate::dsl::parser::parse;
    use crate::dsl::typeck::type_check;

    fn compile_src(src: &str) -> CompiledScript {
        let tokens = lex(src).unwrap();
        let script = parse(tokens).unwrap();
        let typed = type_check(&script).unwrap();
        compile(&typed).unwrap()
    }

    /// Check whether compiled bytecode contains a CallBuiltin for the given name.
    fn has_builtin(compiled: &CompiledScript, name: &str) -> bool {
        if let Some(idx) = crate::dsl::builtins::builtin_index(name) {
            compiled.ops.contains(&Op::CallBuiltin(idx))
        } else {
            false
        }
    }

    #[test]
    fn solid_red() {
        let compiled = compile_src("rgb(1.0, 0.0, 0.0)");
        assert!(has_builtin(&compiled, "rgb"));
        assert!(compiled.ops.contains(&Op::Return));
    }

    #[test]
    fn uses_time() {
        let compiled = compile_src("let x = sin(t * 3.14); rgb(x, x, x)");
        assert!(compiled.ops.contains(&Op::PushT));
        assert!(has_builtin(&compiled, "sin"));
        assert!(compiled.ops.contains(&Op::Mul));
        assert!(compiled.ops.contains(&Op::StoreLocal(0)));
        assert!(compiled.ops.contains(&Op::LoadLocal(0)));
    }

    #[test]
    fn if_else_branches() {
        let compiled = compile_src("if t > 0.5 { rgb(1.0, 0.0, 0.0) } else { rgb(0.0, 0.0, 1.0) }");
        // Should have JumpIfFalse and Jump instructions
        let has_jump_if = compiled.ops.iter().any(|op| matches!(op, Op::JumpIfFalse(_)));
        let has_jump = compiled.ops.iter().any(|op| matches!(op, Op::Jump(_)));
        assert!(has_jump_if);
        assert!(has_jump);
    }

    #[test]
    fn constant_dedup() {
        let compiled = compile_src("rgb(1.0, 1.0, 1.0)");
        // 1.0 should appear only once in the constant pool
        let ones = compiled.constants.iter().filter(|&&c| c == 1.0).count();
        assert_eq!(ones, 1, "Duplicate constants should be deduplicated");
    }

    #[test]
    fn param_push() {
        let compiled = compile_src("param speed: float(0.0, 10.0) = 1.0; let x = t * speed; rgb(x, x, x)");
        assert!(compiled.ops.contains(&Op::PushParam(0)));
        assert_eq!(compiled.params.len(), 1);
        assert_eq!(compiled.params[0].name, "speed");
    }

    #[test]
    fn color_literal() {
        let compiled = compile_src("#ff0000");
        // Should push r=1.0, g=0.0, b=0.0, then CallBuiltin(rgb)
        assert!(has_builtin(&compiled, "rgb"));
        assert!(compiled.constants.iter().any(|&c| (c - 1.0).abs() < f64::EPSILON));
    }

    #[test]
    fn local_count_tracks_lets() {
        let compiled = compile_src("let a = 1.0; let b = 2.0; let c = a + b; rgb(c, c, c)");
        assert!(compiled.local_count >= 3);
    }

    #[test]
    fn pi_and_tau_as_constants() {
        let compiled = compile_src("let x = sin(t * PI); rgb(x, x, x)");
        // PI should be in the constant pool
        assert!(compiled.constants.iter().any(|&c| (c - std::f64::consts::PI).abs() < f64::EPSILON));
    }

    #[test]
    fn hsv_function() {
        let compiled = compile_src("hsv(t * 360.0, 1.0, 1.0)");
        assert!(has_builtin(&compiled, "hsv"));
        assert!(compiled.ops.contains(&Op::PushT));
    }

    #[test]
    fn bitwise_ops_compile() {
        let compiled = compile_src("let x = 3 & 1; let y = x | 2; let z = y ^ 1; rgb(0.0, 0.0, 0.0)");
        assert!(compiled.ops.contains(&Op::BitAnd));
        assert!(compiled.ops.contains(&Op::BitOr));
        assert!(compiled.ops.contains(&Op::BitXor));
    }

    #[test]
    fn shift_ops_compile() {
        let compiled = compile_src("let x = 1 << 3; let y = x >> 1; rgb(0.0, 0.0, 0.0)");
        assert!(compiled.ops.contains(&Op::Shl));
        assert!(compiled.ops.contains(&Op::Shr));
    }

    #[test]
    fn enum_comparison_bytecode() {
        let compiled = compile_src("enum Mode { A, B }\nparam mode: Mode = A;\nif mode == Mode.A { rgb(1.0, 0.0, 0.0) } else { rgb(0.0, 1.0, 0.0) }");
        // Enum comparison: PushParam(mode) + PushConst(variant_index) + Eq
        assert!(compiled.ops.contains(&Op::PushParam(0)));
        assert!(compiled.ops.contains(&Op::Eq));
    }
}
