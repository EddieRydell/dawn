use std::collections::HashMap;

use super::ast::{EffectAst, Expr, Stmt};
use super::{binary_result_type, is_assignable, is_float_compatible, ScriptDiagnostic, ScriptType};
pub fn type_check(effect: &EffectAst) -> Result<(), Vec<ScriptDiagnostic>> {
    let mut checker = TypeChecker::new(effect);
    checker.check();
    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

struct TypeChecker<'a> {
    effect: &'a EffectAst,
    scopes: Vec<HashMap<String, Binding>>,
    errors: Vec<ScriptDiagnostic>,
}

#[derive(Debug, Clone, Copy)]
struct Binding {
    value_type: ScriptType,
    mutable: bool,
}

impl<'a> TypeChecker<'a> {
    fn new(effect: &'a EffectAst) -> Self {
        let mut scopes = HashMap::from([
            ("progress".to_string(), readonly(ScriptType::Float)),
            ("seconds".to_string(), readonly(ScriptType::Float)),
            ("fixture".to_string(), readonly(ScriptType::Fixture)),
            ("pixel".to_string(), readonly(ScriptType::Pixel)),
            ("PI".to_string(), readonly(ScriptType::Float)),
            ("TAU".to_string(), readonly(ScriptType::Float)),
        ]);
        for param in &effect.params {
            scopes.insert(param.name.clone(), readonly(param.value_type));
        }
        Self {
            effect,
            scopes: vec![scopes],
            errors: Vec::new(),
        }
    }

    fn check(&mut self) {
        let mut saw_return = false;
        self.check_statements(&self.effect.sample, &mut saw_return);
        if !saw_return {
            self.errors.push(ScriptDiagnostic {
                range: None,
                message: "sample must contain an explicit return".to_string(),
            });
        }
    }

    fn check_statements(&mut self, statements: &[Stmt], saw_return: &mut bool) {
        for statement in statements {
            self.check_statement(statement, saw_return);
        }
    }

    fn check_statement(&mut self, statement: &Stmt, saw_return: &mut bool) {
        match statement {
            Stmt::Let {
                name,
                value_type,
                expr,
            } => self.check_let(name, *value_type, expr),
            Stmt::Assign { name, expr } => self.check_assign(name, expr),
            Stmt::Expr(expr) => {
                self.expr_type(expr);
            }
            Stmt::For {
                name,
                value_type,
                initializer,
                condition,
                update,
                body,
            } => {
                self.push_scope();
                self.check_let(name, *value_type, initializer);
                let condition_type = self.expr_type(condition);
                if condition_type != ScriptType::Bool {
                    self.errors.push(ScriptDiagnostic {
                        range: None,
                        message: format!(
                            "for loop condition must be bool, but found {condition_type}"
                        ),
                    });
                }
                self.check_statement(update, saw_return);
                self.push_scope();
                self.check_statements(body, saw_return);
                self.pop_scope();
                self.pop_scope();
            }
            Stmt::Return(expr) => {
                *saw_return = true;
                let actual = self.expr_type(expr);
                if actual != ScriptType::Color {
                    self.errors.push(ScriptDiagnostic {
                        range: None,
                        message: format!("sample must return color, but returned {actual}"),
                    });
                }
            }
        }
    }

    fn check_let(&mut self, name: &str, value_type: ScriptType, expr: &Expr) {
        let actual = self.expr_type(expr);
        if !is_assignable(value_type, actual) {
            self.errors.push(ScriptDiagnostic {
                range: None,
                message: format!(
                    "local `{name}` is declared as {value_type}, but expression is {actual}"
                ),
            });
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name.to_string(),
                Binding {
                    value_type,
                    mutable: true,
                },
            );
        }
    }

    fn check_assign(&mut self, name: &str, expr: &Expr) {
        let actual = self.expr_type(expr);
        let Some(binding) = self.binding(name) else {
            self.errors.push(ScriptDiagnostic {
                range: None,
                message: format!("unknown local `{name}`"),
            });
            return;
        };
        if !binding.mutable {
            self.errors.push(ScriptDiagnostic {
                range: None,
                message: format!("`{name}` cannot be assigned"),
            });
            return;
        }
        if !is_assignable(binding.value_type, actual) {
            self.errors.push(ScriptDiagnostic {
                range: None,
                message: format!(
                    "local `{name}` is declared as {}, but assignment is {actual}",
                    binding.value_type
                ),
            });
        }
    }

    fn expr_type(&mut self, expr: &Expr) -> ScriptType {
        match expr {
            Expr::Float(_) => ScriptType::Float,
            Expr::Int(_) => ScriptType::Int,
            Expr::Bool(_) => ScriptType::Bool,
            Expr::Color(_) => ScriptType::Color,
            Expr::Ident(name) => self
                .binding(name)
                .map(|binding| binding.value_type)
                .unwrap_or_else(|| {
                    self.errors.push(ScriptDiagnostic {
                        range: None,
                        message: format!("unknown identifier `{name}`"),
                    });
                    ScriptType::Void
                }),
            Expr::Unary { expr, .. } => self.expr_type(expr),
            Expr::Binary { left, op, right } => {
                let left = self.expr_type(left);
                let right = self.expr_type(right);
                match binary_result_type(left, *op, right) {
                    Some(value_type) => value_type,
                    None => {
                        self.errors.push(ScriptDiagnostic {
                            range: None,
                            message: format!("cannot apply binary operator to {left} and {right}"),
                        });
                        ScriptType::Void
                    }
                }
            }
            Expr::Call { name, args } => self.call_type(name, args),
        }
    }

    fn call_type(&mut self, name: &str, args: &[Expr]) -> ScriptType {
        if let Some(param_type) = self.binding(name).map(|binding| binding.value_type) {
            let [arg] = args else {
                self.errors.push(ScriptDiagnostic {
                    range: None,
                    message: format!("curve parameter `{name}` expects one argument"),
                });
                return ScriptType::Void;
            };
            return match param_type {
                ScriptType::CurveFloat | ScriptType::CurveColor => {
                    let arg_type = self.expr_type(arg);
                    if !is_float_compatible(arg_type) {
                        self.errors.push(ScriptDiagnostic {
                            range: None,
                            message: format!("curve parameter `{name}` expects a float argument"),
                        });
                    }
                    match param_type {
                        ScriptType::CurveFloat => ScriptType::Float,
                        ScriptType::CurveColor => ScriptType::Color,
                        _ => unreachable!(),
                    }
                }
                _ => {
                    self.errors.push(ScriptDiagnostic {
                        range: None,
                        message: format!("`{name}` is not callable"),
                    });
                    ScriptType::Void
                }
            };
        }

        let arg_types = args
            .iter()
            .map(|arg| self.expr_type(arg))
            .collect::<Vec<_>>();
        match (name, arg_types.as_slice()) {
            ("sin" | "cos" | "abs", [value]) if is_float_compatible(*value) => ScriptType::Float,
            ("floor", [value]) if is_float_compatible(*value) => ScriptType::Float,
            ("srand", [value]) if is_float_compatible(*value) => ScriptType::Float,
            ("rand", []) => ScriptType::Float,
            ("pixel_index" | "pixel_count", [ScriptType::Pixel]) => ScriptType::Int,
            ("mark_count", [ScriptType::Marks]) => ScriptType::Int,
            ("mark_at", [ScriptType::Marks, ScriptType::Int, fallback])
                if is_float_compatible(*fallback) =>
            {
                ScriptType::Float
            }
            (
                "mark_prev" | "mark_next" | "mark_nearest" | "mark_phase" | "mark_elapsed",
                [ScriptType::Marks, time, fallback],
            ) if is_float_compatible(*time) && is_float_compatible(*fallback) => ScriptType::Float,
            ("min" | "max", [left, right])
                if is_float_compatible(*left) && is_float_compatible(*right) =>
            {
                ScriptType::Float
            }
            ("clamp" | "smoothstep", [first, second, third]) | ("mix", [first, second, third])
                if is_float_compatible(*first)
                    && is_float_compatible(*second)
                    && is_float_compatible(*third) =>
            {
                ScriptType::Float
            }
            ("rgb" | "hsv", [first, second, third])
                if is_float_compatible(*first)
                    && is_float_compatible(*second)
                    && is_float_compatible(*third) =>
            {
                ScriptType::Color
            }
            ("mix", [ScriptType::Color, ScriptType::Color, amount])
                if is_float_compatible(*amount) =>
            {
                ScriptType::Color
            }
            _ => {
                self.errors.push(ScriptDiagnostic {
                    range: None,
                    message: format!("unknown function or invalid call `{name}`"),
                });
                ScriptType::Void
            }
        }
    }

    fn binding(&self, name: &str) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
}

fn readonly(value_type: ScriptType) -> Binding {
    Binding {
        value_type,
        mutable: false,
    }
}
