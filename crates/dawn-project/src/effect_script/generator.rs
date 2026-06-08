use std::collections::{BTreeMap, HashMap, HashSet};

use crate::model::{ArrayElementType, Color, CurveValue};

use super::ast::{BinaryOp, EmitEffectRef, EmitStmt, Expr, Stmt, UnaryOp};
use super::params::PreparedEffectParams;
use super::{
    binary_result_type, GeneratorTarget, GeneratorTargetItem, RuntimeError, RuntimeMarks,
    RuntimeValue, ScriptType,
};

const MAX_GENERATED_CHILDREN: usize = 4096;
const MAX_GENERATOR_LOOP_ITERATIONS: usize = 16384;

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedChildEffect {
    pub effect: GeneratedChildEffectRef,
    pub target: GeneratorTarget,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub params: BTreeMap<String, RuntimeValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedChildTopology {
    pub effect: GeneratedChildEffectRef,
    pub target: GeneratorTarget,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    params: Vec<GeneratedChildParamExpr>,
    captured_bindings: HashMap<String, RuntimeValue>,
}

#[derive(Debug, Clone, PartialEq)]
struct GeneratedChildParamExpr {
    name: String,
    expr: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratedChildEffectRef {
    Local { name: String },
    Imported { alias: String, name: String },
}

#[cfg(test)]
pub fn run_generator(
    statements: &[Stmt],
    params: &PreparedEffectParams,
    param_names: &[String],
    target: GeneratorTarget,
    duration_seconds: f64,
) -> Result<Vec<GeneratedChildEffect>, RuntimeError> {
    run_generator_topology(statements, params, param_names, target, duration_seconds)?
        .into_iter()
        .map(|child| {
            let emitted_params = evaluate_generated_child_params(&child, params, param_names)?;
            Ok(GeneratedChildEffect {
                effect: child.effect,
                target: child.target,
                start_seconds: child.start_seconds,
                duration_seconds: child.duration_seconds,
                params: emitted_params,
            })
        })
        .collect()
}

pub fn run_generator_topology(
    statements: &[Stmt],
    params: &PreparedEffectParams,
    param_names: &[String],
    target: GeneratorTarget,
    duration_seconds: f64,
) -> Result<Vec<GeneratedChildTopology>, RuntimeError> {
    let mut runtime = GeneratorRuntime::new(params, param_names, target, duration_seconds);
    runtime.run_statements(statements)?;
    Ok(runtime.emitted)
}

pub fn evaluate_generated_child_params(
    child: &GeneratedChildTopology,
    params: &PreparedEffectParams,
    param_names: &[String],
) -> Result<BTreeMap<String, RuntimeValue>, RuntimeError> {
    let mut current_params = HashMap::new();
    for (index, name) in param_names.iter().enumerate() {
        current_params.insert(name.clone(), params.values[index].clone());
    }
    let mut runtime = GeneratorRuntime {
        scopes: vec![current_params, child.captured_bindings.clone()],
        emitted: Vec::new(),
        loop_iterations: 0,
        parent_param_names: param_names.iter().cloned().collect(),
    };
    child
        .params
        .iter()
        .map(|param| Ok((param.name.clone(), runtime.eval(&param.expr)?)))
        .collect()
}

struct GeneratorRuntime {
    scopes: Vec<HashMap<String, RuntimeValue>>,
    emitted: Vec<GeneratedChildTopology>,
    loop_iterations: usize,
    parent_param_names: HashSet<String>,
}

impl GeneratorRuntime {
    fn new(
        params: &PreparedEffectParams,
        param_names: &[String],
        target: GeneratorTarget,
        duration_seconds: f64,
    ) -> Self {
        let mut scopes = vec![HashMap::new()];
        scopes[0].insert("target".to_string(), RuntimeValue::Target(target));
        scopes[0].insert(
            "duration".to_string(),
            RuntimeValue::Float(duration_seconds),
        );
        for (index, name) in param_names.iter().enumerate() {
            scopes[0].insert(name.clone(), params.values[index].clone());
        }
        Self {
            scopes,
            emitted: Vec::new(),
            loop_iterations: 0,
            parent_param_names: param_names.iter().cloned().collect(),
        }
    }

    fn run_statements(&mut self, statements: &[Stmt]) -> Result<(), RuntimeError> {
        for statement in statements {
            self.run_statement(statement)?;
        }
        Ok(())
    }

    fn run_statement(&mut self, statement: &Stmt) -> Result<(), RuntimeError> {
        match statement {
            Stmt::Let {
                name,
                value_type,
                expr,
            } => {
                let raw = self.eval(expr)?;
                let value = self.coerce(raw, *value_type)?;
                let Some(scope) = self.scopes.last_mut() else {
                    return Err(self.error("generator scope stack is empty"));
                };
                scope.insert(name.clone(), value);
            }
            Stmt::Assign { name, expr } => {
                let value = self.eval(expr)?;
                for scope in self.scopes.iter_mut().rev() {
                    if scope.contains_key(name) {
                        scope.insert(name.clone(), value);
                        return Ok(());
                    }
                }
                return Err(self.error(format!("unknown local `{name}`")));
            }
            Stmt::Expr(expr) => {
                self.eval(expr)?;
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
                let raw = self.eval(initializer)?;
                let value = self.coerce(raw, *value_type)?;
                let Some(scope) = self.scopes.last_mut() else {
                    return Err(self.error("generator scope stack is empty"));
                };
                scope.insert(name.clone(), value);
                while self.bool(condition)? {
                    self.loop_iterations += 1;
                    if self.loop_iterations > MAX_GENERATOR_LOOP_ITERATIONS {
                        return Err(self.error("generator exceeded maximum loop iteration count"));
                    }
                    self.push_scope();
                    self.run_statements(body)?;
                    self.pop_scope();
                    self.run_statement(update)?;
                }
                self.pop_scope();
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                self.push_scope();
                if self.bool(condition)? {
                    self.run_statements(then_body)?;
                } else {
                    self.run_statements(else_body)?;
                }
                self.pop_scope();
            }
            Stmt::Return(_) => return Err(self.error("generator cannot return")),
            Stmt::Emit(emit) => self.emit(emit)?,
        }
        Ok(())
    }

    fn emit(&mut self, emit: &EmitStmt) -> Result<(), RuntimeError> {
        if self.emitted.len() >= MAX_GENERATED_CHILDREN {
            return Err(self.error("generator exceeded maximum emitted child count"));
        }
        let RuntimeValue::Target(target) = self.eval(&emit.target)? else {
            return Err(self.error("emit target must be Target"));
        };
        let start_seconds = self.float(&emit.start)?;
        let duration_seconds = self.float(&emit.duration)?;
        self.emitted.push(GeneratedChildTopology {
            effect: match &emit.effect {
                EmitEffectRef::Local { name } => {
                    GeneratedChildEffectRef::Local { name: name.clone() }
                }
                EmitEffectRef::Imported { alias, name } => GeneratedChildEffectRef::Imported {
                    alias: alias.clone(),
                    name: name.clone(),
                },
            },
            target,
            start_seconds,
            duration_seconds,
            params: emit
                .params
                .iter()
                .map(|param| GeneratedChildParamExpr {
                    name: param.name.clone(),
                    expr: param.expr.clone(),
                })
                .collect(),
            captured_bindings: self.captured_bindings(),
        });
        Ok(())
    }

    fn eval(&mut self, expr: &Expr) -> Result<RuntimeValue, RuntimeError> {
        match expr {
            Expr::Float(value) => Ok(RuntimeValue::Float(*value)),
            Expr::Int(value) => Ok(RuntimeValue::Int(*value)),
            Expr::Bool(value) => Ok(RuntimeValue::Bool(*value)),
            Expr::Color(value) => Ok(RuntimeValue::Color(*value)),
            Expr::Array(items) => self.array_literal(items),
            Expr::Ident(name) => self
                .binding(name)
                .ok_or_else(|| self.error(format!("unknown identifier `{name}`"))),
            Expr::Unary { op, expr } => {
                let value = self.eval(expr)?;
                match (op, value) {
                    (UnaryOp::Negate, RuntimeValue::Float(value)) => {
                        Ok(RuntimeValue::Float(-value))
                    }
                    (UnaryOp::Negate, RuntimeValue::Int(value)) => Ok(RuntimeValue::Int(-value)),
                    (UnaryOp::Not, RuntimeValue::Bool(value)) => Ok(RuntimeValue::Bool(!value)),
                    _ => Err(self.error("invalid unary expression")),
                }
            }
            Expr::Binary { left, op, right } => {
                let left = self.eval(left)?;
                if *op == BinaryOp::LogicalAnd {
                    return Ok(RuntimeValue::Bool(
                        self.value_bool(&left)? && self.bool(right)?,
                    ));
                }
                if *op == BinaryOp::LogicalOr {
                    return Ok(RuntimeValue::Bool(
                        self.value_bool(&left)? || self.bool(right)?,
                    ));
                }
                let right = self.eval(right)?;
                self.binary(left, *op, right)
            }
            Expr::Call { name, args } => self.call(name, args),
            Expr::CallValue { callee, args } => self.call_value(callee, args),
            Expr::Index { array, index } => self.index(array, index),
            Expr::Member { object, member } => self.member(object, member),
            Expr::Qualified { .. } => Err(self.error("effect references are only valid in emit")),
        }
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Result<RuntimeValue, RuntimeError> {
        if let Some(RuntimeValue::Curve(curve)) = self.binding(name) {
            let amount = self.float(&args[0])?;
            return curve
                .evaluate(amount)
                .map(|value| match value {
                    CurveValue::Float(value) => RuntimeValue::Float(value),
                    CurveValue::Color(value) => RuntimeValue::Color(value),
                })
                .ok_or_else(|| self.error("curve has no points"));
        }
        match name {
            "min" => Ok(RuntimeValue::Float(
                self.float(&args[0])?.min(self.float(&args[1])?),
            )),
            "max" => Ok(RuntimeValue::Float(
                self.float(&args[0])?.max(self.float(&args[1])?),
            )),
            "clamp" => Ok(RuntimeValue::Float(
                self.float(&args[0])?
                    .clamp(self.float(&args[1])?, self.float(&args[2])?),
            )),
            "mix" => {
                let left = self.eval(&args[0])?;
                let right = self.eval(&args[1])?;
                let amount = self.float(&args[2])?;
                match (left, right) {
                    (RuntimeValue::Color(left), RuntimeValue::Color(right)) => {
                        Ok(RuntimeValue::Color(left.mix(right, amount)))
                    }
                    (left, right) => Ok(RuntimeValue::Float(
                        self.value_float(&left)?
                            + (self.value_float(&right)? - self.value_float(&left)?) * amount,
                    )),
                }
            }
            "rgb" => Ok(RuntimeValue::Color(Color::new(
                self.float(&args[0])?.round().clamp(0.0, 255.0) as u8,
                self.float(&args[1])?.round().clamp(0.0, 255.0) as u8,
                self.float(&args[2])?.round().clamp(0.0, 255.0) as u8,
            ))),
            "hsv" => Ok(RuntimeValue::Color(hsv_to_rgb(
                self.float(&args[0])?,
                self.float(&args[1])?,
                self.float(&args[2])?,
            ))),
            "floor" => Ok(RuntimeValue::Int(self.float(&args[0])?.floor() as i64)),
            "mark_count" => {
                let RuntimeValue::Marks(marks) = self.eval(&args[0])? else {
                    return Err(self.error("mark_count expects marks"));
                };
                Ok(RuntimeValue::Int(marks.windowed.len() as i64))
            }
            "mark_global_count" => {
                let RuntimeValue::Marks(marks) = self.eval(&args[0])? else {
                    return Err(self.error("mark_global_count expects marks"));
                };
                Ok(RuntimeValue::Int(marks.global.len() as i64))
            }
            "mark_at" => {
                let RuntimeValue::Marks(marks) = self.eval(&args[0])? else {
                    return Err(self.error("mark_at expects marks"));
                };
                let index = self.int(&args[1])?;
                let fallback = self.float(&args[2])?;
                Ok(RuntimeValue::Float(mark_at(
                    &marks.windowed,
                    index,
                    fallback,
                )))
            }
            "mark_global_at" => {
                let RuntimeValue::Marks(marks) = self.eval(&args[0])? else {
                    return Err(self.error("mark_global_at expects marks"));
                };
                let index = self.int(&args[1])?;
                let fallback = self.float(&args[2])?;
                Ok(RuntimeValue::Float(mark_at(&marks.global, index, fallback)))
            }
            "mark_prev" => self.mark_search(args, MarkSearchMode::Prev, MarkDomain::Windowed),
            "mark_next" => self.mark_search(args, MarkSearchMode::Next, MarkDomain::Windowed),
            "mark_nearest" => self.mark_search(args, MarkSearchMode::Nearest, MarkDomain::Windowed),
            "mark_phase" => self.mark_search(args, MarkSearchMode::Phase, MarkDomain::Windowed),
            "mark_elapsed" => self.mark_search(args, MarkSearchMode::Elapsed, MarkDomain::Windowed),
            "mark_global_prev" => self.mark_search(args, MarkSearchMode::Prev, MarkDomain::Global),
            "mark_global_next" => self.mark_search(args, MarkSearchMode::Next, MarkDomain::Global),
            "mark_global_nearest" => {
                self.mark_search(args, MarkSearchMode::Nearest, MarkDomain::Global)
            }
            "mark_global_phase" => {
                self.mark_search(args, MarkSearchMode::Phase, MarkDomain::Global)
            }
            "mark_global_elapsed" => {
                self.mark_search(args, MarkSearchMode::Elapsed, MarkDomain::Global)
            }
            "fixtures" => self.target_items(args, TargetItemsMode::Fixtures),
            "pixels" => self.target_items(args, TargetItemsMode::Pixels),
            "sections" => self.target_items(args, TargetItemsMode::Sections),
            "count" => {
                let RuntimeValue::TargetItems(items) = self.eval(&args[0])? else {
                    return Err(self.error("count expects TargetItems"));
                };
                Ok(RuntimeValue::Int(items.len() as i64))
            }
            "len" => {
                let RuntimeValue::Array(array) = self.eval(&args[0])? else {
                    return Err(self.error("len expects array"));
                };
                Ok(RuntimeValue::Int(array.values.len() as i64))
            }
            "pick" => {
                let RuntimeValue::TargetItems(items) = self.eval(&args[0])? else {
                    return Err(self.error("pick expects TargetItems"));
                };
                let index = usize::try_from(self.int(&args[1])?)
                    .map_err(|_| self.error("pick index is out of range"))?;
                items
                    .get(index)
                    .cloned()
                    .map(RuntimeValue::TargetItem)
                    .ok_or_else(|| self.error("pick index is out of range"))
            }
            "curve_crossing" => {
                let RuntimeValue::Curve(curve) = self.eval(&args[0])? else {
                    return Err(self.error("curve_crossing expects curve<float>"));
                };
                let value = self.float(&args[1])?;
                let fallback = self.float(&args[2])?;
                Ok(RuntimeValue::Float(
                    curve_crossing(&curve, value).unwrap_or(fallback),
                ))
            }
            "rand" => Ok(RuntimeValue::Float(deterministic_rand(
                self.float(&args[0])?,
                self.int(&args[1])?,
            ))),
            _ => Err(self.error(format!("unknown generator function `{name}`"))),
        }
    }

    fn array_literal(&mut self, items: &[Expr]) -> Result<RuntimeValue, RuntimeError> {
        let mut values = Vec::with_capacity(items.len());
        let mut element_type = None;
        for item in items {
            let value = self.eval(item)?;
            let item_element = runtime_array_element_type(&value).ok_or_else(|| {
                self.error(format!(
                    "array literal cannot contain {}",
                    value.value_type()
                ))
            })?;
            element_type = match element_type {
                None => Some(item_element),
                Some(ArrayElementType::Float) if item_element == ArrayElementType::Int => {
                    Some(ArrayElementType::Float)
                }
                Some(ArrayElementType::Int) if item_element == ArrayElementType::Float => {
                    Some(ArrayElementType::Float)
                }
                Some(expected) if expected == item_element => Some(expected),
                Some(expected) => {
                    return Err(self.error(format!(
                        "array literal elements must all be {}, but found {}",
                        array_element_label(expected),
                        value.value_type()
                    )));
                }
            };
            values.push(value);
        }
        let element_type = element_type.unwrap_or(ArrayElementType::Float);
        let values = values
            .into_iter()
            .map(|value| match (element_type, value) {
                (ArrayElementType::Float, RuntimeValue::Int(value)) => {
                    RuntimeValue::Float(value as f64)
                }
                (_, value) => value,
            })
            .collect();
        Ok(RuntimeValue::Array(super::RuntimeArrayValue {
            element_type,
            values,
        }))
    }

    fn index(&mut self, array: &Expr, index: &Expr) -> Result<RuntimeValue, RuntimeError> {
        let RuntimeValue::Array(array) = self.eval(array)? else {
            return Err(self.error("indexing expects array"));
        };
        let index = usize::try_from(self.int(index)?)
            .map_err(|_| self.error("array index must not be negative"))?;
        array
            .values
            .get(index)
            .cloned()
            .ok_or_else(|| self.error("array index is out of range"))
    }

    fn call_value(&mut self, callee: &Expr, args: &[Expr]) -> Result<RuntimeValue, RuntimeError> {
        let RuntimeValue::Curve(curve) = self.eval(callee)? else {
            return Err(self.error("call expects curve value"));
        };
        let amount = self.float(&args[0])?;
        curve
            .evaluate(amount)
            .map(|value| match value {
                CurveValue::Float(value) => RuntimeValue::Float(value),
                CurveValue::Color(value) => RuntimeValue::Color(value),
            })
            .ok_or_else(|| self.error("curve has no points"))
    }

    fn target_items(
        &mut self,
        args: &[Expr],
        mode: TargetItemsMode,
    ) -> Result<RuntimeValue, RuntimeError> {
        let RuntimeValue::Target(target) = self.eval(&args[0])? else {
            return Err(self.error("target helper expects Target"));
        };
        let width = if mode == TargetItemsMode::Sections {
            usize::try_from(self.int(&args[1])?)
                .ok()
                .filter(|width| *width > 0)
                .ok_or_else(|| self.error("section width must be greater than zero"))?
        } else {
            1
        };
        let items = match mode {
            TargetItemsMode::Fixtures => fixture_items(&target),
            TargetItemsMode::Pixels => section_items(&target, 1),
            TargetItemsMode::Sections => section_items(&target, width),
        };
        Ok(RuntimeValue::TargetItems(items))
    }

    fn mark_search(
        &mut self,
        args: &[Expr],
        mode: MarkSearchMode,
        domain: MarkDomain,
    ) -> Result<RuntimeValue, RuntimeError> {
        let RuntimeValue::Marks(marks) = self.eval(&args[0])? else {
            return Err(self.error("mark search expects marks"));
        };
        let time = self.float(&args[1])?;
        let fallback = self.float(&args[2])?;
        let marks = marks_for_domain(&marks, domain);
        let value = match mode {
            MarkSearchMode::Prev => mark_prev(marks, time),
            MarkSearchMode::Next => mark_next(marks, time),
            MarkSearchMode::Nearest => mark_nearest(marks, time),
            MarkSearchMode::Phase => mark_phase(marks, time),
            MarkSearchMode::Elapsed => mark_elapsed(marks, time),
        }
        .unwrap_or(fallback);
        Ok(RuntimeValue::Float(value))
    }

    fn member(&mut self, object: &Expr, member: &str) -> Result<RuntimeValue, RuntimeError> {
        let RuntimeValue::TargetItem(item) = self.eval(object)? else {
            return Err(self.error("member access expects TargetItem"));
        };
        match member {
            "target" => Ok(RuntimeValue::Target(item.target)),
            "index" => Ok(RuntimeValue::Int(item.index as i64)),
            "count" => Ok(RuntimeValue::Int(item.count as i64)),
            "position" => Ok(RuntimeValue::Int(item.position as i64)),
            "fixture_index" => Ok(RuntimeValue::Int(item.fixture_index as i64)),
            "pixel_start" => Ok(RuntimeValue::Int(item.pixel_start as i64)),
            "pixel_count" => Ok(RuntimeValue::Int(item.pixel_count as i64)),
            _ => Err(self.error(format!("unknown TargetItem member `{member}`"))),
        }
    }

    fn binary(
        &self,
        left: RuntimeValue,
        op: BinaryOp,
        right: RuntimeValue,
    ) -> Result<RuntimeValue, RuntimeError> {
        let left_type = left.value_type();
        let right_type = right.value_type();
        let Some(result_type) = binary_result_type(&left_type, op, &right_type) else {
            return Err(self.error("invalid binary expression"));
        };
        match result_type {
            ScriptType::Float => Ok(RuntimeValue::Float(match op {
                BinaryOp::Add => self.value_float(&left)? + self.value_float(&right)?,
                BinaryOp::Subtract => self.value_float(&left)? - self.value_float(&right)?,
                BinaryOp::Multiply => self.value_float(&left)? * self.value_float(&right)?,
                BinaryOp::Divide => self.value_float(&left)? / self.value_float(&right)?,
                _ => unreachable!(),
            })),
            ScriptType::Int => Ok(RuntimeValue::Int(match op {
                BinaryOp::Add => self.value_int(&left)? + self.value_int(&right)?,
                BinaryOp::Subtract => self.value_int(&left)? - self.value_int(&right)?,
                BinaryOp::Multiply => self.value_int(&left)? * self.value_int(&right)?,
                BinaryOp::Divide => self.value_int(&left)? / self.value_int(&right)?,
                BinaryOp::Modulo => {
                    let right = self.value_int(&right)?;
                    if right == 0 {
                        return Err(self.error("integer modulo by zero"));
                    }
                    self.value_int(&left)? % right
                }
                _ => unreachable!(),
            })),
            ScriptType::Bool => Ok(RuntimeValue::Bool(match op {
                BinaryOp::Less => self.value_float(&left)? < self.value_float(&right)?,
                BinaryOp::LessEqual => self.value_float(&left)? <= self.value_float(&right)?,
                BinaryOp::Greater => self.value_float(&left)? > self.value_float(&right)?,
                BinaryOp::GreaterEqual => self.value_float(&left)? >= self.value_float(&right)?,
                BinaryOp::Equal => values_equal(&left, &right),
                BinaryOp::NotEqual => !values_equal(&left, &right),
                _ => unreachable!(),
            })),
            ScriptType::Color => {
                let (color, factor) = match (&left, &right) {
                    (RuntimeValue::Color(color), right) => (*color, self.value_float(right)?),
                    (left, RuntimeValue::Color(color)) => (*color, self.value_float(left)?),
                    _ => unreachable!(),
                };
                Ok(RuntimeValue::Color(color.scale(factor)))
            }
            _ => Err(self.error("invalid binary result")),
        }
    }

    fn bool(&mut self, expr: &Expr) -> Result<bool, RuntimeError> {
        let value = self.eval(expr)?;
        self.value_bool(&value)
    }

    fn float(&mut self, expr: &Expr) -> Result<f64, RuntimeError> {
        let value = self.eval(expr)?;
        self.value_float(&value)
    }

    fn int(&mut self, expr: &Expr) -> Result<i64, RuntimeError> {
        let value = self.eval(expr)?;
        self.value_int(&value)
    }

    fn value_bool(&self, value: &RuntimeValue) -> Result<bool, RuntimeError> {
        match value {
            RuntimeValue::Bool(value) => Ok(*value),
            _ => Err(self.error("expected bool")),
        }
    }

    fn value_float(&self, value: &RuntimeValue) -> Result<f64, RuntimeError> {
        match value {
            RuntimeValue::Float(value) => Ok(*value),
            RuntimeValue::Int(value) => Ok(*value as f64),
            _ => Err(self.error("expected float")),
        }
    }

    fn value_int(&self, value: &RuntimeValue) -> Result<i64, RuntimeError> {
        match value {
            RuntimeValue::Int(value) => Ok(*value),
            _ => Err(self.error("expected int")),
        }
    }

    fn coerce(
        &self,
        value: RuntimeValue,
        expected: ScriptType,
    ) -> Result<RuntimeValue, RuntimeError> {
        match (expected, value) {
            (ScriptType::Float, RuntimeValue::Int(value)) => Ok(RuntimeValue::Float(value as f64)),
            (expected, value) if expected == value.value_type() => Ok(value),
            _ => Err(self.error("generator value type mismatch")),
        }
    }

    fn binding(&self, name: &str) -> Option<RuntimeValue> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn captured_bindings(&self) -> HashMap<String, RuntimeValue> {
        self.scopes
            .iter()
            .enumerate()
            .flat_map(|(scope_index, scope)| {
                scope
                    .iter()
                    .filter(move |(name, _)| {
                        scope_index != 0 || !self.parent_param_names.contains(name.as_str())
                    })
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn error(&self, message: impl Into<String>) -> RuntimeError {
        RuntimeError {
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetItemsMode {
    Fixtures,
    Pixels,
    Sections,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkDomain {
    Windowed,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkSearchMode {
    Prev,
    Next,
    Nearest,
    Phase,
    Elapsed,
}

fn marks_for_domain(marks: &RuntimeMarks, domain: MarkDomain) -> &[f64] {
    match domain {
        MarkDomain::Windowed => &marks.windowed,
        MarkDomain::Global => &marks.global,
    }
}

fn mark_at(marks: &[f64], index: i64, fallback: f64) -> f64 {
    usize::try_from(index)
        .ok()
        .and_then(|index| marks.get(index))
        .copied()
        .unwrap_or(fallback)
}

fn mark_prev(marks: &[f64], time: f64) -> Option<f64> {
    let index = marks.partition_point(|mark| *mark <= time);
    index.checked_sub(1).map(|index| marks[index])
}

fn mark_next(marks: &[f64], time: f64) -> Option<f64> {
    marks
        .get(marks.partition_point(|mark| *mark <= time))
        .copied()
}

fn mark_nearest(marks: &[f64], time: f64) -> Option<f64> {
    let previous = mark_prev(marks, time);
    let next = mark_next(marks, time);
    match (previous, next) {
        (Some(previous), Some(next)) if (time - previous) <= (next - time) => Some(previous),
        (Some(_), Some(next)) => Some(next),
        (Some(previous), None) => Some(previous),
        (None, Some(next)) => Some(next),
        (None, None) => None,
    }
}

fn mark_phase(marks: &[f64], time: f64) -> Option<f64> {
    let previous = mark_prev(marks, time)?;
    if (time - previous).abs() < f64::EPSILON {
        return Some(0.0);
    }
    let next = mark_next(marks, time)?;
    let span = next - previous;
    if span <= f64::EPSILON {
        return None;
    }
    Some(((time - previous) / span).clamp(0.0, 1.0))
}

fn mark_elapsed(marks: &[f64], time: f64) -> Option<f64> {
    mark_prev(marks, time).map(|previous| time - previous)
}

fn fixture_items(target: &GeneratorTarget) -> Vec<GeneratorTargetItem> {
    let mut ranges = Vec::<(usize, usize, usize)>::new();
    for (position, pixel) in target.pixels.iter().enumerate() {
        match ranges.last_mut() {
            Some((fixture_index, _, count)) if *fixture_index == pixel.fixture_index => *count += 1,
            _ => ranges.push((pixel.fixture_index, position, 1)),
        }
    }
    let count = ranges.len();
    ranges
        .into_iter()
        .enumerate()
        .map(|(index, (fixture_index, start, pixel_count))| {
            item(target, index, count, start, pixel_count, fixture_index)
        })
        .collect()
}

fn section_items(target: &GeneratorTarget, width: usize) -> Vec<GeneratorTargetItem> {
    let count = target.pixels.len().div_ceil(width);
    (0..count)
        .map(|index| {
            let start = index * width;
            let pixel_count = target.pixels.len().saturating_sub(start).min(width);
            let fixture_index = target
                .pixels
                .get(start)
                .map(|pixel| pixel.fixture_index)
                .unwrap_or(0);
            item(target, index, count, start, pixel_count, fixture_index)
        })
        .collect()
}

fn item(
    target: &GeneratorTarget,
    index: usize,
    count: usize,
    start: usize,
    pixel_count: usize,
    fixture_index: usize,
) -> GeneratorTargetItem {
    GeneratorTargetItem {
        target: GeneratorTarget {
            pixels: target.pixels[start..start + pixel_count].to_vec(),
        },
        index,
        count,
        position: start,
        fixture_index,
        pixel_start: start,
        pixel_count,
    }
}

fn curve_crossing(curve: &crate::model::Curve, value: f64) -> Option<f64> {
    let mut previous = curve.points.first()?;
    for point in &curve.points[1..] {
        let CurveValue::Float(left) = previous.value else {
            return None;
        };
        let CurveValue::Float(right) = point.value else {
            return None;
        };
        if (left <= value && right >= value) || (left >= value && right <= value) {
            let span = right - left;
            let amount = if span.abs() < f64::EPSILON {
                0.0
            } else {
                (value - left) / span
            };
            return Some(previous.time + (point.time - previous.time) * amount);
        }
        previous = point;
    }
    None
}

fn deterministic_rand(seed: f64, index: i64) -> f64 {
    let mut value = seed.to_bits() ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    ((value >> 11) as f64) / ((1u64 << 53) as f64)
}

fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> Color {
    let hue = hue.rem_euclid(360.0) / 60.0;
    let c = value.clamp(0.0, 1.0) * saturation.clamp(0.0, 1.0);
    let x = c * (1.0 - ((hue % 2.0) - 1.0).abs());
    let m = value.clamp(0.0, 1.0) - c;
    let (red, green, blue) = if hue < 1.0 {
        (c, x, 0.0)
    } else if hue < 2.0 {
        (x, c, 0.0)
    } else if hue < 3.0 {
        (0.0, c, x)
    } else if hue < 4.0 {
        (0.0, x, c)
    } else if hue < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Color::new(
        ((red + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((green + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((blue + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

fn values_equal(left: &RuntimeValue, right: &RuntimeValue) -> bool {
    match (left, right) {
        (RuntimeValue::Float(left), RuntimeValue::Float(right)) => left == right,
        (RuntimeValue::Float(left), RuntimeValue::Int(right)) => *left == *right as f64,
        (RuntimeValue::Int(left), RuntimeValue::Float(right)) => *left as f64 == *right,
        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left == right,
        (RuntimeValue::Bool(left), RuntimeValue::Bool(right)) => left == right,
        (RuntimeValue::Enum(left), RuntimeValue::Enum(right)) => left == right,
        _ => false,
    }
}

fn runtime_array_element_type(value: &RuntimeValue) -> Option<ArrayElementType> {
    match value {
        RuntimeValue::Int(_) => Some(ArrayElementType::Int),
        RuntimeValue::Float(_) => Some(ArrayElementType::Float),
        RuntimeValue::Bool(_) => Some(ArrayElementType::Bool),
        RuntimeValue::Color(_) => Some(ArrayElementType::Color),
        RuntimeValue::Curve(curve) => match curve.value_type {
            crate::model::CurveValueType::Float => Some(ArrayElementType::CurveFloat),
            crate::model::CurveValueType::Color => Some(ArrayElementType::CurveColor),
        },
        _ => None,
    }
}

fn array_element_label(element_type: ArrayElementType) -> &'static str {
    match element_type {
        ArrayElementType::Int => "int",
        ArrayElementType::Float => "float",
        ArrayElementType::Bool => "bool",
        ArrayElementType::Color => "color",
        ArrayElementType::CurveFloat => "curve<float>",
        ArrayElementType::CurveColor => "curve<color>",
    }
}
