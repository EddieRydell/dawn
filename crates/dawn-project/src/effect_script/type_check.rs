use std::collections::HashMap;

use super::ast::{EffectAst, EffectEntrypoint, EmitStmt, Expr, Stmt, UnaryOp};
use super::builtins::{BuiltinConstant, BuiltinContext, BuiltinFunction};
use super::{
    binary_result_type, is_assignable, is_float_compatible, EffectParamSchema, EffectScriptKind,
    ScriptDiagnostic, ScriptType,
};
pub fn type_check(effect: &EffectAst) -> Result<(), Vec<ScriptDiagnostic>> {
    type_check_with_imports(effect, &[])
}

#[derive(Debug, Clone, Copy)]
pub struct ImportedEffect<'a> {
    pub alias: &'a str,
    pub name: &'a str,
    pub kind: EffectScriptKind,
    pub params: &'a [EffectParamSchema],
}

pub fn type_check_with_imports(
    effect: &EffectAst,
    imports: &[ImportedEffect<'_>],
) -> Result<(), Vec<ScriptDiagnostic>> {
    let mut checker = TypeChecker::new(effect, imports);
    checker.check();
    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

struct TypeChecker<'a> {
    effect: &'a EffectAst,
    imports: &'a [ImportedEffect<'a>],
    kind: EffectScriptKind,
    scopes: Vec<HashMap<String, Binding>>,
    errors: Vec<ScriptDiagnostic>,
}

#[derive(Debug, Clone, Copy)]
struct Binding {
    value_type: ScriptType,
    mutable: bool,
}

impl<'a> TypeChecker<'a> {
    fn new(effect: &'a EffectAst, imports: &'a [ImportedEffect<'a>]) -> Self {
        let mut scopes = HashMap::new();
        let kind = match effect.entrypoint {
            EffectEntrypoint::Sample(_) => EffectScriptKind::Sample,
            EffectEntrypoint::Generator(_) => EffectScriptKind::Generator,
        };
        for context in BuiltinContext::ALL {
            let context =
                BuiltinContext::from_name(context.name()).expect("builtin context names are valid");
            scopes.insert(context.name().to_string(), readonly(context.value_type()));
        }
        for constant in BuiltinConstant::ALL {
            scopes.insert(constant.name().to_string(), readonly(ScriptType::Float));
        }
        for param in &effect.params {
            scopes.insert(param.name.clone(), readonly(param.value_type));
            if param.value_type == ScriptType::Enum {
                for option in &param.options {
                    scopes.insert(option.clone(), readonly(ScriptType::Enum));
                }
            }
        }
        if kind == EffectScriptKind::Generator {
            scopes.insert("timeline".to_string(), readonly(ScriptType::Timeline));
            scopes.insert("target".to_string(), readonly(ScriptType::Target));
            scopes.insert("duration".to_string(), readonly(ScriptType::Float));
        }
        Self {
            effect,
            imports,
            kind,
            scopes: vec![scopes],
            errors: Vec::new(),
        }
    }

    fn check(&mut self) {
        let mut saw_return = false;
        match &self.effect.entrypoint {
            EffectEntrypoint::Sample(statements) => {
                self.check_statements(statements, &mut saw_return)
            }
            EffectEntrypoint::Generator(statements) => {
                self.check_statements(statements, &mut saw_return)
            }
        }
        if self.kind == EffectScriptKind::Sample && !saw_return {
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
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                let condition_type = self.expr_type(condition);
                if condition_type != ScriptType::Bool {
                    self.errors.push(ScriptDiagnostic {
                        range: None,
                        message: format!("if condition must be bool, but found {condition_type}"),
                    });
                }
                self.push_scope();
                self.check_statements(then_body, saw_return);
                self.pop_scope();
                self.push_scope();
                self.check_statements(else_body, saw_return);
                self.pop_scope();
            }
            Stmt::Return(expr) => {
                if self.kind == EffectScriptKind::Generator {
                    self.errors.push(ScriptDiagnostic {
                        range: None,
                        message: "generator entrypoints cannot return values".to_string(),
                    });
                    return;
                }
                *saw_return = true;
                let actual = self.expr_type(expr);
                if actual != ScriptType::Color {
                    self.errors.push(ScriptDiagnostic {
                        range: None,
                        message: format!("sample must return color, but returned {actual}"),
                    });
                }
            }
            Stmt::Emit(emit) => self.check_emit(emit),
        }
    }

    fn check_emit(&mut self, emit: &EmitStmt) {
        if self.kind != EffectScriptKind::Generator {
            self.errors.push(ScriptDiagnostic {
                range: None,
                message: "only generator effects can emit child effects".to_string(),
            });
            return;
        }
        if self.expr_type(&emit.target) != ScriptType::Target {
            self.errors.push(ScriptDiagnostic {
                range: None,
                message: "emit target must be Target".to_string(),
            });
        }
        for (label, expr) in [("start", &emit.start), ("duration", &emit.duration)] {
            let value_type = self.expr_type(expr);
            if !is_float_compatible(value_type) {
                self.errors.push(ScriptDiagnostic {
                    range: None,
                    message: format!("emit {label} must be float"),
                });
            }
        }
        let Some(child) = self
            .imports
            .iter()
            .find(|import| import.alias == emit.alias && import.name == emit.effect)
        else {
            self.errors.push(ScriptDiagnostic {
                range: None,
                message: format!("unresolved emitted effect `{}.{}`", emit.alias, emit.effect),
            });
            return;
        };
        if child.kind != EffectScriptKind::Sample {
            self.errors.push(ScriptDiagnostic {
                range: None,
                message: format!(
                    "generator cannot emit generator effect `{}.{}`",
                    emit.alias, emit.effect
                ),
            });
            return;
        }
        for param in &emit.params {
            let Some(schema) = child.params.iter().find(|schema| schema.name == param.name) else {
                self.errors.push(ScriptDiagnostic {
                    range: None,
                    message: format!(
                        "emitted effect `{}.{}` has no parameter `{}`",
                        emit.alias, emit.effect, param.name
                    ),
                });
                continue;
            };
            let actual = self.expr_type(&param.expr);
            if !is_assignable(schema.value_type, actual) {
                self.errors.push(ScriptDiagnostic {
                    range: None,
                    message: format!(
                        "emitted parameter `{}` must be {}, but found {actual}",
                        param.name, schema.value_type
                    ),
                });
            }
        }
        for schema in child.params {
            if schema.default.is_none()
                && !emit.params.iter().any(|param| param.name == schema.name)
            {
                self.errors.push(ScriptDiagnostic {
                    range: None,
                    message: format!(
                        "emitted effect is missing required parameter `{}`",
                        schema.name
                    ),
                });
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
            Expr::Unary { op, expr } => {
                let inner = self.expr_type(expr);
                match op {
                    UnaryOp::Negate if is_float_compatible(inner) => inner,
                    UnaryOp::Negate => {
                        self.errors.push(ScriptDiagnostic {
                            range: None,
                            message: format!("cannot negate {inner}"),
                        });
                        ScriptType::Void
                    }
                    UnaryOp::Not if inner == ScriptType::Bool => ScriptType::Bool,
                    UnaryOp::Not => {
                        self.errors.push(ScriptDiagnostic {
                            range: None,
                            message: format!("cannot apply ! to {inner}"),
                        });
                        ScriptType::Void
                    }
                }
            }
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
            Expr::Member { object, member } => self.member_type(object, member),
            Expr::Qualified { alias, name } => {
                if self
                    .imports
                    .iter()
                    .any(|import| import.alias == alias && import.name == name)
                {
                    ScriptType::Void
                } else {
                    self.errors.push(ScriptDiagnostic {
                        range: None,
                        message: format!("unresolved effect `{alias}.{name}`"),
                    });
                    ScriptType::Void
                }
            }
        }
    }

    fn member_type(&mut self, object: &Expr, member: &str) -> ScriptType {
        match (self.expr_type(object), member) {
            (ScriptType::TargetItem, "target") => ScriptType::Target,
            (
                ScriptType::TargetItem,
                "index" | "count" | "position" | "fixture_index" | "pixel_start" | "pixel_count",
            ) => ScriptType::Int,
            (value_type, _) => {
                self.errors.push(ScriptDiagnostic {
                    range: None,
                    message: format!("type {value_type} has no member `{member}`"),
                });
                ScriptType::Void
            }
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
        let value_type = BuiltinFunction::from_name(name)
            .and_then(|function| function.return_type_for_kind(&arg_types, self.kind));
        match value_type {
            Some(value_type) => value_type,
            None => {
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
