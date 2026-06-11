use super::ast::{BinaryOp, Block, EffectDecl, Expr, ExprKind, Module, ParamDecl, Stmt, UnaryOp};
use super::diagnostic::Diagnostic;
use super::lexer::TextSpan;
use super::types::{Identifier, Type, Value};
use indexmap::IndexMap;

pub(crate) fn check_module(module: &Module) -> Result<(), Vec<Diagnostic>> {
    let mut checker = Checker {
        diagnostics: Vec::new(),
    };
    for effect in &module.effects {
        checker.check_effect(effect);
    }
    if checker.diagnostics.is_empty() {
        Ok(())
    } else {
        Err(checker.diagnostics)
    }
}

struct Checker {
    diagnostics: Vec<Diagnostic>,
}

impl Checker {
    fn check_effect(&mut self, effect: &EffectDecl) {
        if effect.sample.name.as_str() != "sample" {
            self.error(
                TextSpan { start: 0, end: 0 },
                "effect entrypoint must be named `sample`",
            );
        }
        if effect.sample.return_type != Type::Color {
            self.error(
                TextSpan { start: 0, end: 0 },
                "effect entrypoint must return `color`",
            );
        }
        if !effect.sample.params.is_empty() {
            for param in &effect.sample.params {
                self.error(
                    TextSpan { start: 0, end: 0 },
                    format!(
                        "`sample` does not accept arguments; remove `{:?} {}`",
                        param.ty,
                        param.name.as_str()
                    ),
                );
            }
        }

        let mut env = IndexMap::new();
        for param in &effect.params {
            self.check_param(param);
            env.insert(param.name.clone(), param.ty.clone());
        }

        let returns = self.check_block(&effect.sample.body, &mut env, &Type::Color);
        if !returns {
            self.error(
                TextSpan { start: 0, end: 0 },
                "`sample` must return a color on all paths",
            );
        }
    }

    fn check_param(&mut self, param: &ParamDecl) {
        match (&param.ty, &param.default) {
            (Type::Marks, None) => {}
            (_, None) => {}
            (ty, Some(value)) if value_matches_type(value, ty) => {}
            (Type::Float, Some(Value::Int(_))) => {}
            (Type::Enum(options), Some(Value::Enum(value))) => {
                if !options.iter().any(|option| option == value) {
                    self.error(
                        TextSpan { start: 0, end: 0 },
                        format!("enum default `{}` is not an option", value.as_str()),
                    );
                }
            }
            (_, Some(_)) => {
                self.error(
                    TextSpan { start: 0, end: 0 },
                    format!(
                        "default for `{}` does not match declared type",
                        param.name.as_str()
                    ),
                );
            }
        }
    }

    fn check_block(
        &mut self,
        block: &Block,
        env: &mut IndexMap<Identifier, Type>,
        return_type: &Type,
    ) -> bool {
        let mut returned = false;
        for statement in &block.statements {
            if self.check_statement(statement, env, return_type) {
                returned = true;
                break;
            }
        }
        returned
    }

    fn check_statement(
        &mut self,
        statement: &Stmt,
        env: &mut IndexMap<Identifier, Type>,
        return_type: &Type,
    ) -> bool {
        match statement {
            Stmt::Local {
                ty,
                name,
                initializer,
            } => {
                if let Some(initializer) = initializer {
                    let initializer_type = self.check_expr(initializer, env, Some(ty));
                    self.require_assignable(ty, &initializer_type, initializer.span);
                }
                env.insert(name.clone(), ty.clone());
                false
            }
            Stmt::Assign { name, value } => {
                let Some(target_type) = env.get(name).cloned() else {
                    self.error(
                        value.span,
                        format!("unknown assignment target `{}`", name.as_str()),
                    );
                    return false;
                };
                let value_type = self.check_expr(value, env, Some(&target_type));
                self.require_assignable(&target_type, &value_type, value.span);
                false
            }
            Stmt::Expr(expr) => {
                let _ = self.check_expr(expr, env, None);
                false
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
            } => {
                let condition_type = self.check_expr(condition, env, Some(&Type::Bool));
                self.require_assignable(&Type::Bool, &condition_type, condition.span);
                let mut then_env = env.clone();
                let then_returns = self.check_block(then_block, &mut then_env, return_type);
                let else_returns = if let Some(else_block) = else_block {
                    let mut else_env = env.clone();
                    self.check_block(else_block, &mut else_env, return_type)
                } else {
                    false
                };
                then_returns && else_returns
            }
            Stmt::For {
                initializer,
                condition,
                update,
                body,
            } => {
                let mut loop_env = env.clone();
                let _ = self.check_statement(initializer, &mut loop_env, return_type);
                let condition_type = self.check_expr(condition, &mut loop_env, Some(&Type::Bool));
                self.require_assignable(&Type::Bool, &condition_type, condition.span);
                let _ = self.check_statement(update, &mut loop_env, return_type);
                let _ = self.check_block(body, &mut loop_env, return_type);
                false
            }
            Stmt::Return(expr) => {
                let actual = self.check_expr(expr, env, Some(return_type));
                self.require_assignable(return_type, &actual, expr.span);
                true
            }
        }
    }

    fn check_expr(
        &mut self,
        expr: &Expr,
        env: &mut IndexMap<Identifier, Type>,
        expected: Option<&Type>,
    ) -> Type {
        match &expr.kind {
            ExprKind::Literal(value) => type_of_value(value),
            ExprKind::Variable(name) => env.get(name).cloned().unwrap_or_else(|| {
                if matches!(name.as_str(), "PI" | "TAU") {
                    Type::Float
                } else {
                    Type::Enum(vec![name.clone()])
                }
            }),
            ExprKind::Array(items) => {
                if items.is_empty() {
                    if let Some(Type::Array(item_type)) = expected {
                        return Type::Array(item_type.clone());
                    }
                    self.error(expr.span, "empty array literal needs an array type context");
                    return Type::Array(Box::new(Type::Void));
                }

                let expected_item = expected.and_then(|ty| match ty {
                    Type::Array(item) => Some(item.as_ref()),
                    _ => None,
                });
                let first_type = self.check_expr(&items[0], env, expected_item);
                for item in items.iter().skip(1) {
                    let item_type = self.check_expr(item, env, Some(&first_type));
                    self.require_assignable(&first_type, &item_type, item.span);
                }
                Type::Array(Box::new(first_type))
            }
            ExprKind::Index { target, index } => {
                let target_type = self.check_expr(target, env, None);
                match target_type {
                    Type::Array(item) => {
                        let index_type = self.check_expr(index, env, Some(&Type::Int));
                        self.require_assignable(&Type::Int, &index_type, index.span);
                        *item
                    }
                    Type::Curve(value_type) => {
                        let index_type = self.check_expr(index, env, Some(&Type::Float));
                        self.require_assignable(&Type::Float, &index_type, index.span);
                        *value_type
                    }
                    _ => {
                        self.error(target.span, "indexing requires an array or curve");
                        Type::Void
                    }
                }
            }
            ExprKind::Call { callee, args } => self.check_call(callee, args, env, expr.span),
            ExprKind::Unary { op, expr: inner } => {
                let inner_type = self.check_expr(inner, env, None);
                match op {
                    UnaryOp::Negate if numeric(&inner_type) => inner_type,
                    UnaryOp::Not if inner_type == Type::Bool => Type::Bool,
                    UnaryOp::Negate => {
                        self.error(expr.span, "unary `-` requires int or float");
                        Type::Void
                    }
                    UnaryOp::Not => {
                        self.error(expr.span, "unary `!` requires bool");
                        Type::Void
                    }
                }
            }
            ExprKind::Binary { op, left, right } => {
                let left_type = self.check_expr(left, env, None);
                let right_type = self.check_expr(right, env, Some(&left_type));
                self.check_binary(*op, &left_type, &right_type, expr.span)
            }
        }
    }

    fn check_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        env: &mut IndexMap<Identifier, Type>,
        span: TextSpan,
    ) -> Type {
        if let ExprKind::Variable(name) = &callee.kind {
            return self.check_builtin_call(name.as_str(), args, env, span);
        }

        self.error(callee.span, "call target must be a builtin");
        Type::Void
    }

    fn check_builtin_call(
        &mut self,
        name: &str,
        args: &[Expr],
        env: &mut IndexMap<Identifier, Type>,
        span: TextSpan,
    ) -> Type {
        match name {
            "progress" | "seconds" | "duration" | "pixel_fraction" => {
                self.require_arg_count(name, args.len(), 0, span);
                Type::Float
            }
            "pixel_index" | "pixel_count" => {
                self.require_arg_count(name, args.len(), 0, span);
                Type::Int
            }
            "section_position" => {
                self.require_arg_count(name, args.len(), 1, span);
                self.require_arg(args, 0, &Type::Float, env);
                Type::Float
            }
            "sin" | "cos" | "abs" | "floor" => {
                self.require_arg_count(name, args.len(), 1, span);
                self.require_arg(args, 0, &Type::Float, env);
                Type::Float
            }
            "min" | "max" => {
                self.require_arg_count(name, args.len(), 2, span);
                self.require_arg(args, 0, &Type::Float, env);
                self.require_arg(args, 1, &Type::Float, env);
                Type::Float
            }
            "clamp" | "smoothstep" => {
                self.require_arg_count(name, args.len(), 3, span);
                for index in 0..3 {
                    self.require_arg(args, index, &Type::Float, env);
                }
                Type::Float
            }
            "mix" => {
                self.require_arg_count(name, args.len(), 3, span);
                let first = self.require_arg_any(args, 0, env);
                let second = self.require_arg_any(args, 1, env);
                self.require_assignable(&first, &second, span);
                self.require_arg(args, 2, &Type::Float, env);
                first
            }
            "rgb" | "hsv" => {
                self.require_arg_count(name, args.len(), 3, span);
                for index in 0..3 {
                    self.require_arg(args, index, &Type::Float, env);
                }
                Type::Color
            }
            "srand" => {
                self.require_arg_count(name, args.len(), 1, span);
                self.require_arg(args, 0, &Type::Float, env);
                Type::Float
            }
            "rand" => {
                for index in 0..args.len() {
                    self.require_arg(args, index, &Type::Float, env);
                }
                Type::Float
            }
            "curve_crossing" => {
                if args.len() != 2 && args.len() != 3 {
                    self.error(span, "`curve_crossing` expects 2 or 3 arguments");
                }
                self.require_arg(args, 0, &Type::curve(Type::Float), env);
                self.require_arg(args, 1, &Type::Float, env);
                if args.len() == 3 {
                    self.require_arg(args, 2, &Type::Float, env);
                }
                Type::Float
            }
            "curve_float_clamped" => {
                self.require_arg_count(name, args.len(), 4, span);
                self.require_arg(args, 0, &Type::curve(Type::Float), env);
                for index in 1..4 {
                    self.require_arg(args, index, &Type::Float, env);
                }
                Type::Float
            }
            "curve_color_scaled" => {
                self.require_arg_count(name, args.len(), 3, span);
                self.require_arg(args, 0, &Type::curve(Type::Color), env);
                self.require_arg(args, 1, &Type::Float, env);
                self.require_arg(args, 2, &Type::Float, env);
                Type::Color
            }
            "len" => {
                self.require_arg_count(name, args.len(), 1, span);
                let _ = self.require_arg_any(args, 0, env);
                Type::Int
            }
            "mark_count" | "mark_prev_index" | "mark_next_index" => {
                self.require_mark_args(name, args, env, span);
                Type::Int
            }
            "mark_at" | "mark_prev" | "mark_elapsed" | "mark_phase" => {
                self.require_mark_args(name, args, env, span);
                Type::Float
            }
            _ => {
                self.error(span, format!("unknown function `{name}`"));
                Type::Void
            }
        }
    }

    fn require_mark_args(
        &mut self,
        name: &str,
        args: &[Expr],
        env: &mut IndexMap<Identifier, Type>,
        span: TextSpan,
    ) {
        if args.is_empty() {
            self.error(
                span,
                format!("`{name}` requires marks as the first argument"),
            );
            return;
        }
        if matches!(name, "mark_count") {
            self.require_arg_count(name, args.len(), 1, span);
            self.require_arg(args, 0, &Type::Marks, env);
            return;
        }
        self.require_arg(args, 0, &Type::Marks, env);
    }

    fn check_binary(&mut self, op: BinaryOp, left: &Type, right: &Type, span: TextSpan) -> Type {
        match op {
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Remainder => {
                if numeric(left) && numeric(right) {
                    if left == &Type::Float || right == &Type::Float {
                        Type::Float
                    } else {
                        Type::Int
                    }
                } else {
                    self.error(span, "arithmetic operators require numeric operands");
                    Type::Void
                }
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                if numeric(left) && numeric(right) {
                    Type::Bool
                } else {
                    self.error(span, "comparison operators require numeric operands");
                    Type::Bool
                }
            }
            BinaryOp::Equal | BinaryOp::NotEqual => {
                self.require_assignable(left, right, span);
                Type::Bool
            }
            BinaryOp::And | BinaryOp::Or => {
                self.require_assignable(&Type::Bool, left, span);
                self.require_assignable(&Type::Bool, right, span);
                Type::Bool
            }
        }
    }

    fn require_arg_count(&mut self, name: &str, actual: usize, expected: usize, span: TextSpan) {
        if actual != expected {
            self.error(
                span,
                format!("`{name}` expects {expected} arguments, got {actual}"),
            );
        }
    }

    fn require_arg(
        &mut self,
        args: &[Expr],
        index: usize,
        expected: &Type,
        env: &mut IndexMap<Identifier, Type>,
    ) {
        if let Some(arg) = args.get(index) {
            let actual = self.check_expr(arg, env, Some(expected));
            self.require_assignable(expected, &actual, arg.span);
        }
    }

    fn require_arg_any(
        &mut self,
        args: &[Expr],
        index: usize,
        env: &mut IndexMap<Identifier, Type>,
    ) -> Type {
        args.get(index)
            .map(|arg| self.check_expr(arg, env, None))
            .unwrap_or(Type::Void)
    }

    fn require_assignable(&mut self, expected: &Type, actual: &Type, span: TextSpan) {
        if expected == actual || (expected == &Type::Float && actual == &Type::Int) {
            return;
        }
        if let (Type::Enum(options), Type::Enum(actual_options)) = (expected, actual) {
            if actual_options
                .iter()
                .all(|value| options.iter().any(|option| option == value))
            {
                return;
            }
        }
        self.error(span, format!("expected {expected:?}, got {actual:?}"));
    }

    fn error(&mut self, span: TextSpan, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::new(span, message));
    }
}

fn numeric(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Float)
}

fn type_of_value(value: &Value) -> Type {
    match value {
        Value::Void => Type::Void,
        Value::Int(_) => Type::Int,
        Value::Float(_) => Type::Float,
        Value::Bool(_) => Type::Bool,
        Value::Color(_) => Type::Color,
        Value::Marks(_) => Type::Marks,
        Value::Curve(curve) => curve
            .points
            .first()
            .map(|point| match point.value {
                crate::values::CurveValue::Float(_) => Type::curve(Type::Float),
                crate::values::CurveValue::Color(_) => Type::curve(Type::Color),
            })
            .unwrap_or_else(|| Type::curve(Type::Void)),
        Value::Array(items) => items
            .first()
            .map(|item| Type::array(type_of_value(item)))
            .unwrap_or_else(|| Type::array(Type::Void)),
        Value::Enum(identifier) => Type::Enum(vec![identifier.clone()]),
    }
}

fn value_matches_type(value: &Value, ty: &Type) -> bool {
    match (value, ty) {
        (Value::Void, Type::Void)
        | (Value::Int(_), Type::Int)
        | (Value::Float(_), Type::Float)
        | (Value::Bool(_), Type::Bool)
        | (Value::Color(_), Type::Color)
        | (Value::Marks(_), Type::Marks) => true,
        (Value::Array(items), Type::Array(item_type)) => {
            items.iter().all(|item| value_matches_type(item, item_type))
        }
        (Value::Enum(identifier), Type::Enum(options)) => {
            options.iter().any(|option| option == identifier)
        }
        (Value::Curve(_), Type::Curve(_)) => true,
        _ => false,
    }
}
