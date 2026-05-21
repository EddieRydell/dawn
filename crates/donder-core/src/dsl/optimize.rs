use super::ast::{BinOp, Span, TypeName, UnaryOp};
use super::builtins::BuiltinVar;

use super::ops;
use super::typeck::{TypedExpr, TypedExprKind, TypedScript, TypedStmt, TypedStmtKind};

// ── Pass 1: Constant Folding on TypedExpr ────────────────────────────

/// Fold constant expressions in a typed script.
/// Recursively evaluates pure constant subtrees at compile time.
pub fn fold_constants(mut script: TypedScript) -> TypedScript {
    script.body = script.body.into_iter().map(fold_stmt).collect();
    script
}

fn fold_stmt(stmt: TypedStmt) -> TypedStmt {
    let kind = match stmt.kind {
        TypedStmtKind::Let { name, value, local_index } => TypedStmtKind::Let {
            name,
            value: fold_expr(value),
            local_index,
        },
        TypedStmtKind::Expr(expr) => TypedStmtKind::Expr(fold_expr(expr)),
    };
    TypedStmt { kind, span: stmt.span }
}

fn fold_expr(expr: TypedExpr) -> TypedExpr {
    let span = expr.span;
    let ty = expr.ty.clone();

    match expr.kind {
        // Resolve constant builtins to literals
        TypedExprKind::LoadBuiltin(BuiltinVar::Pi) => TypedExpr {
            kind: TypedExprKind::FloatLit(std::f64::consts::PI),
            ty,
            span,
        },
        TypedExprKind::LoadBuiltin(BuiltinVar::Tau) => TypedExpr {
            kind: TypedExprKind::FloatLit(std::f64::consts::TAU),
            ty,
            span,
        },

        // IntToFloat(IntLit) → FloatLit
        TypedExprKind::IntToFloat(inner) => {
            let folded = fold_expr(*inner);
            if let TypedExprKind::IntLit(v) = folded.kind {
                TypedExpr {
                    kind: TypedExprKind::FloatLit(f64::from(v)),
                    ty,
                    span,
                }
            } else {
                TypedExpr {
                    kind: TypedExprKind::IntToFloat(Box::new(folded)),
                    ty,
                    span,
                }
            }
        }

        // Unary ops
        TypedExprKind::UnaryOp { op, operand } => {
            let folded = fold_expr(*operand);
            match (&op, &folded.kind) {
                (UnaryOp::Neg, TypedExprKind::FloatLit(x)) => TypedExpr {
                    kind: TypedExprKind::FloatLit(-x),
                    ty,
                    span,
                },
                (UnaryOp::Neg, TypedExprKind::IntLit(x)) => TypedExpr {
                    kind: TypedExprKind::IntLit(-x),
                    ty,
                    span,
                },
                (UnaryOp::Not, TypedExprKind::BoolLit(x)) => TypedExpr {
                    kind: TypedExprKind::BoolLit(!x),
                    ty,
                    span,
                },
                _ => TypedExpr {
                    kind: TypedExprKind::UnaryOp {
                        op,
                        operand: Box::new(folded),
                    },
                    ty,
                    span,
                },
            }
        }

        // Binary ops
        TypedExprKind::BinOp { op, left, right } => {
            let l = fold_expr(*left);
            let r = fold_expr(*right);
            fold_binop(op, l, r, ty, span)
        }

        // Builtin calls — fold if all args are constant floats
        TypedExprKind::BuiltinCall { name, args } => {
            let folded_args: Vec<TypedExpr> = args.into_iter().map(fold_expr).collect();

            // Try to extract all-constant float args
            let const_floats: Option<Vec<f64>> = folded_args
                .iter()
                .map(|a| match &a.kind {
                    TypedExprKind::FloatLit(v) => Some(*v),
                    _ => None,
                })
                .collect();

            if let Some(vals) = const_floats {
                if let Some(result) = ops::eval_builtin_fn(&name, &vals) {
                    return TypedExpr {
                        kind: TypedExprKind::FloatLit(result),
                        ty,
                        span,
                    };
                }
            }

            TypedExpr {
                kind: TypedExprKind::BuiltinCall {
                    name,
                    args: folded_args,
                },
                ty,
                span,
            }
        }

        // Color field access on literal — only r/g/b since ColorLit has no alpha.
        // Other Color fields (a, hue, saturation, value) are in FIELD_OPS but
        // cannot be constant-folded from a ColorLit.
        TypedExprKind::Field { object, field } => {
            let folded_obj = fold_expr(*object);
            if let TypedExprKind::ColorLit { r, g, b } = &folded_obj.kind {
                match field.as_str() {
                    "r" => return TypedExpr {
                        kind: TypedExprKind::FloatLit(f64::from(*r) / 255.0),
                        ty,
                        span,
                    },
                    "g" => return TypedExpr {
                        kind: TypedExprKind::FloatLit(f64::from(*g) / 255.0),
                        ty,
                        span,
                    },
                    "b" => return TypedExpr {
                        kind: TypedExprKind::FloatLit(f64::from(*b) / 255.0),
                        ty,
                        span,
                    },
                    _ => {}
                }
            }
            TypedExpr {
                kind: TypedExprKind::Field {
                    object: Box::new(folded_obj),
                    field,
                },
                ty,
                span,
            }
        }

        // If with constant condition
        TypedExprKind::If { condition, then_body, else_body } => {
            let folded_cond = fold_expr(*condition);
            match &folded_cond.kind {
                TypedExprKind::BoolLit(true) => {
                    let folded_then: Vec<TypedStmt> =
                        then_body.into_iter().map(fold_stmt).collect();
                    // Emit a Block — the compiler knows how to handle it directly
                    TypedExpr {
                        kind: TypedExprKind::Block(folded_then),
                        ty,
                        span,
                    }
                }
                TypedExprKind::BoolLit(false) => {
                    if let Some(else_stmts) = else_body {
                        let folded_else: Vec<TypedStmt> =
                            else_stmts.into_iter().map(fold_stmt).collect();
                        // Replace with Block containing the else body
                        TypedExpr {
                            kind: TypedExprKind::Block(folded_else),
                            ty,
                            span,
                        }
                    } else {
                        // Unreachable after typeck validation: an `if false` with no else
                        // branch cannot pass the type checker (which now requires both
                        // branches to produce a value expression). Kept as a defensive
                        // fallback to avoid panics if this is somehow reached.
                        TypedExpr {
                            kind: TypedExprKind::ColorLit { r: 0, g: 0, b: 0 },
                            ty,
                            span,
                        }
                    }
                }
                _ => {
                    let folded_then: Vec<TypedStmt> =
                        then_body.into_iter().map(fold_stmt).collect();
                    let folded_else =
                        else_body.map(|stmts| stmts.into_iter().map(fold_stmt).collect());
                    TypedExpr {
                        kind: TypedExprKind::If {
                            condition: Box::new(folded_cond),
                            then_body: folded_then,
                            else_body: folded_else,
                        },
                        ty,
                        span,
                    }
                }
            }
        }

        // Recurse into color/vec2 binary operations — these cannot be
        // constant-folded (no literal representation) but their children can.
        TypedExprKind::ColorAdd { left, right } => fold_binary_pair(left, right, |l, r| TypedExprKind::ColorAdd { left: l, right: r }, ty, span),
        TypedExprKind::ColorSub { left, right } => fold_binary_pair(left, right, |l, r| TypedExprKind::ColorSub { left: l, right: r }, ty, span),
        TypedExprKind::Vec2Add { left, right } => fold_binary_pair(left, right, |l, r| TypedExprKind::Vec2Add { left: l, right: r }, ty, span),
        TypedExprKind::Vec2Sub { left, right } => fold_binary_pair(left, right, |l, r| TypedExprKind::Vec2Sub { left: l, right: r }, ty, span),
        TypedExprKind::Vec2Scale { vec, factor } => fold_binary_pair(vec, factor, |v, f| TypedExprKind::Vec2Scale { vec: v, factor: f }, ty, span),
        TypedExprKind::ColorScale { color, factor } => fold_binary_pair(color, factor, |c, f| TypedExprKind::ColorScale { color: c, factor: f }, ty, span),
        TypedExprKind::MakeVec2 { x, y } => fold_binary_pair(x, y, |fx, fy| TypedExprKind::MakeVec2 { x: fx, y: fy }, ty, span),
        TypedExprKind::ColorMix { a, b, t } => {
            let fa = fold_expr(*a);
            let fb = fold_expr(*b);
            let ft = fold_expr(*t);
            TypedExpr {
                kind: TypedExprKind::ColorMix {
                    a: Box::new(fa),
                    b: Box::new(fb),
                    t: Box::new(ft),
                },
                ty,
                span,
            }
        }
        TypedExprKind::EvalGradient { param_index, arg } => TypedExpr {
            kind: TypedExprKind::EvalGradient {
                param_index,
                arg: Box::new(fold_expr(*arg)),
            },
            ty,
            span,
        },
        TypedExprKind::EvalCurve { param_index, arg } => TypedExpr {
            kind: TypedExprKind::EvalCurve {
                param_index,
                arg: Box::new(fold_expr(*arg)),
            },
            ty,
            span,
        },
        TypedExprKind::EvalPath { param_index, arg } => TypedExpr {
            kind: TypedExprKind::EvalPath {
                param_index,
                arg: Box::new(fold_expr(*arg)),
            },
            ty,
            span,
        },

        // Block — recurse into statements
        TypedExprKind::Block(stmts) => TypedExpr {
            kind: TypedExprKind::Block(stmts.into_iter().map(fold_stmt).collect()),
            ty,
            span,
        },

        // Leaf nodes — no folding possible
        _ => expr,
    }
}

/// Fold a two-operand expression node: recurse into both children, then
/// rebuild with the provided constructor. Covers Color/Vec2 binary ops,
/// scale, and MakeVec2.
#[allow(clippy::boxed_local)] // Callers pass Box from enum destructuring; unboxing at call sites would add noise
fn fold_binary_pair(
    left: Box<TypedExpr>,
    right: Box<TypedExpr>,
    make: impl FnOnce(Box<TypedExpr>, Box<TypedExpr>) -> TypedExprKind,
    ty: TypeName,
    span: Span,
) -> TypedExpr {
    let l = fold_expr(*left);
    let r = fold_expr(*right);
    TypedExpr {
        kind: make(Box::new(l), Box::new(r)),
        ty,
        span,
    }
}

/// Try to fold a binary operation on two already-folded operands.
fn fold_binop(
    op: BinOp,
    left: TypedExpr,
    right: TypedExpr,
    ty: TypeName,
    span: Span,
) -> TypedExpr {
    // Float × Float
    if let (TypedExprKind::FloatLit(a), TypedExprKind::FloatLit(b)) =
        (&left.kind, &right.kind)
    {
        let result = ops::eval_binary_op(op, *a, *b);
        return TypedExpr {
            kind: if ty == TypeName::Bool {
                TypedExprKind::BoolLit(result != 0.0)
            } else {
                TypedExprKind::FloatLit(result)
            },
            ty,
            span,
        };
    }

    // Int × Int (bitwise/shift/arithmetic)
    if let (TypedExprKind::IntLit(a), TypedExprKind::IntLit(b)) =
        (&left.kind, &right.kind)
    {
        if let Some(result) = eval_int_binop(op, *a, *b) {
            return TypedExpr {
                kind: TypedExprKind::IntLit(result),
                ty,
                span,
            };
        }
    }

    // Bool × Bool (And/Or)
    if let (TypedExprKind::BoolLit(a), TypedExprKind::BoolLit(b)) =
        (&left.kind, &right.kind)
    {
        match op {
            BinOp::And => return TypedExpr {
                kind: TypedExprKind::BoolLit(*a && *b),
                ty,
                span,
            },
            BinOp::Or => return TypedExpr {
                kind: TypedExprKind::BoolLit(*a || *b),
                ty,
                span,
            },
            _ => {}
        }
    }

    TypedExpr {
        kind: TypedExprKind::BinOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        ty,
        span,
    }
}

/// Evaluate an integer binary operation at compile time.
#[allow(clippy::cast_sign_loss)]
fn eval_int_binop(op: BinOp, a: i32, b: i32) -> Option<i32> {
    Some(match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        BinOp::BitAnd => a & b,
        BinOp::BitOr => a | b,
        BinOp::BitXor => a ^ b,
        BinOp::Shl => a.wrapping_shl(b.clamp(0, 31) as u32),
        BinOp::Shr => a.wrapping_shr(b.clamp(0, 31) as u32),
        _ => return None,
    })
}

// ── Pass 2: Peephole Optimization on Vec<Op> ─────────────────────────
// Moved to `super::peephole` module.
pub use super::peephole::peephole;

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::unwrap_used)]
mod tests {
    use crate::dsl::compile_source;
    use crate::dsl::compiler::{compile, CompiledScript, Op};
    use crate::dsl::lexer::lex;
    use crate::dsl::parser::parse;
    use crate::dsl::typeck::type_check;
    use crate::dsl::vm::{self, VmContext};
    use crate::model::color::Color;

    /// Compile with optimization (the default pipeline).
    fn compile_opt(src: &str) -> CompiledScript {
        compile_source(src).unwrap()
    }

    /// Compile without optimization for comparison.
    fn compile_unopt(src: &str) -> CompiledScript {
        let tokens = lex(src).unwrap();
        let ast = parse(tokens).unwrap();
        let typed = type_check(&ast).unwrap();
        compile(&typed).unwrap()
    }

    fn run_compiled(compiled: &CompiledScript, t: f64, pixel: usize, pixels: usize) -> Color {
        let pos = if pixels > 1 { pixel as f64 / (pixels - 1) as f64 } else { 0.0 };
        let ctx = VmContext {
            t,
            pixel,
            pixels,
            pos,
            pos2d: (pos, 0.0),
            abs_t: 0.0,
            param_values: &[],
            gradients: &[],
            curves: &[],
            colors: &[],
            paths: &[],
        };
        vm::execute(compiled, &ctx)
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
    fn fold_arithmetic() {
        let compiled = compile_opt("rgb(1.0 + 2.0, 0.0, 0.0)");
        // Should fold 1.0+2.0 to 3.0 — no Add op
        assert!(
            !compiled.ops.contains(&Op::Add),
            "1.0 + 2.0 should be folded, ops: {:?}",
            compiled.ops
        );
        // Result should be correct: rgb(3.0, 0, 0) → clamped to 255
        let color = run_compiled(&compiled, 0.0, 0, 1);
        assert_eq!(color.r, 255);
    }

    #[test]
    fn fold_sin_pi() {
        let compiled = compile_opt("let x = sin(PI); rgb(abs(x), 0.0, 0.0)");
        // sin(PI) ≈ 0.0 — should be folded to a constant
        assert!(
            !has_builtin(&compiled, "sin"),
            "sin(PI) should be folded, ops: {:?}",
            compiled.ops
        );
        let color = run_compiled(&compiled, 0.0, 0, 1);
        assert!(color.r <= 1, "sin(PI) ≈ 0, got r={}", color.r);
    }

    #[test]
    fn fold_nested_sin_pi_div_2() {
        let compiled = compile_opt("let x = sin(PI / 2.0); rgb(x, x, x)");
        // sin(PI/2) = 1.0 — the entire chain should fold
        assert!(
            !has_builtin(&compiled, "sin"),
            "sin(PI/2) should be folded"
        );
        assert!(
            !compiled.ops.contains(&Op::Div),
            "PI/2 should be folded"
        );
        let color = run_compiled(&compiled, 0.0, 0, 1);
        assert_eq!(color.r, 255, "sin(PI/2) = 1.0 → 255");
    }

    #[test]
    fn fold_color_field() {
        let compiled = compile_opt("let x = #ff0000.r; rgb(x, 0.0, 0.0)");
        // #ff0000.r = 1.0 — should fold to constant
        assert!(
            !compiled.ops.contains(&Op::ColorR),
            "#ff0000.r should be folded"
        );
        let color = run_compiled(&compiled, 0.0, 0, 1);
        assert_eq!(color.r, 255);
    }

    #[test]
    fn fold_if_true() {
        let compiled = compile_opt("if true { rgb(1.0, 0.0, 0.0) } else { rgb(0.0, 1.0, 0.0) }");
        // Should eliminate the else branch — no Jump instruction needed
        // (JumpIfFalse over a BoolLit(true) condition still emits but the else is gone)
        let color = run_compiled(&compiled, 0.0, 0, 1);
        assert_eq!(color.r, 255);
        assert_eq!(color.g, 0);
    }

    #[test]
    fn fold_if_false() {
        let compiled = compile_opt("if false { rgb(1.0, 0.0, 0.0) } else { rgb(0.0, 1.0, 0.0) }");
        // Should inline else body
        let color = run_compiled(&compiled, 0.0, 0, 1);
        assert_eq!(color.r, 0);
        assert_eq!(color.g, 255);
    }

    #[test]
    fn peephole_identity_add_zero() {
        // x + 0.0 should eliminate the add
        let compiled = compile_opt("let x = t + 0.0; rgb(x, x, x)");
        // The peephole pass should remove PushConst(0.0) + Add
        assert!(
            !compiled.ops.contains(&Op::Add),
            "t + 0.0 should have Add eliminated, ops: {:?}",
            compiled.ops
        );
    }

    #[test]
    fn peephole_double_neg() {
        // -(-t) should eliminate both negations
        let src = "let x = -(-t); rgb(x, x, x)";
        let compiled = compile_opt(src);
        let neg_count = compiled.ops.iter().filter(|&&op| op == Op::Neg).count();
        assert_eq!(neg_count, 0, "double neg should be eliminated, ops: {:?}", compiled.ops);
    }

    #[test]
    fn no_fold_runtime() {
        // sin(t * PI) should NOT fold — t is runtime
        let compiled = compile_opt("let x = sin(t * PI); rgb(x, x, x)");
        assert!(
            has_builtin(&compiled, "sin"),
            "sin(t * PI) must NOT be folded (t is runtime), ops: {:?}",
            compiled.ops
        );
    }

    #[test]
    fn end_to_end_optimized_matches_unoptimized() {
        let src = "let x = sin(PI / 4.0) * 0.5 + 0.5; rgb(x, x, x)";
        let opt = compile_opt(src);
        let unopt = compile_unopt(src);

        for pixel in 0..10 {
            let c_opt = run_compiled(&opt, 0.5, pixel, 10);
            let c_unopt = run_compiled(&unopt, 0.5, pixel, 10);
            assert_eq!(
                c_opt, c_unopt,
                "pixel {pixel}: optimized ({},{},{}) != unoptimized ({},{},{})",
                c_opt.r, c_opt.g, c_opt.b, c_unopt.r, c_unopt.g, c_unopt.b
            );
        }
    }

    #[test]
    fn end_to_end_complex_expression() {
        // A more complex expression with multiple foldable subexpressions
        let src = "let base = cos(0.0); let x = base * 0.5; rgb(x, x, x)";
        let opt = compile_opt(src);
        let unopt = compile_unopt(src);

        let c_opt = run_compiled(&opt, 0.0, 0, 1);
        let c_unopt = run_compiled(&unopt, 0.0, 0, 1);
        assert_eq!(c_opt, c_unopt);
        // cos(0) = 1.0, * 0.5 = 0.5 → 128
        assert_eq!(c_opt.r, 128);
    }

    #[test]
    fn fold_preserves_runtime_if() {
        // Runtime condition should not be folded, but constant subexprs within should be
        let src = "if t > 0.5 { rgb(1.0 + 0.0, 0.0, 0.0) } else { rgb(0.0, 1.0 + 0.0, 0.0) }";
        let opt = compile_opt(src);
        let c_hi = run_compiled(&opt, 0.8, 0, 1);
        assert_eq!(c_hi.r, 255);
        let c_lo = run_compiled(&opt, 0.2, 0, 1);
        assert_eq!(c_lo.g, 255);
    }

    #[test]
    fn peephole_mul_by_one() {
        let compiled = compile_opt("let x = t * 1.0; rgb(x, x, x)");
        assert!(
            !compiled.ops.contains(&Op::Mul),
            "t * 1.0 should have Mul eliminated, ops: {:?}",
            compiled.ops
        );
    }

    #[test]
    fn fold_int_bitwise() {
        // 6 & 3 = 2, should fold at compile time
        let compiled = compile_opt("let x = 6 & 3; let n = x / 8.0; rgb(n, 0.0, 0.0)");
        assert!(
            !compiled.ops.contains(&Op::BitAnd),
            "6 & 3 should be folded, ops: {:?}",
            compiled.ops
        );
        let color = run_compiled(&compiled, 0.0, 0, 1);
        assert_eq!(color.r, 64); // 2/8 = 0.25 → 64
    }
}
