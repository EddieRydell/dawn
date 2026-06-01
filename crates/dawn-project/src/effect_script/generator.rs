use std::collections::{BTreeMap, HashMap};

use crate::model::{Color, CurveValue};

use super::ast::{BinaryOp, EmitStmt, Expr, Stmt, UnaryOp};
use super::params::PreparedEffectParams;
use super::{
    binary_result_type, GeneratorTarget, GeneratorTargetItem, RuntimeError, RuntimeValue,
    ScriptType,
};

const MAX_GENERATED_CHILDREN: usize = 4096;
const MAX_GENERATOR_LOOP_ITERATIONS: usize = 16384;

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedChildEffect {
    pub alias: String,
    pub effect: String,
    pub target: GeneratorTarget,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub params: BTreeMap<String, RuntimeValue>,
}

pub fn run_generator(
    statements: &[Stmt],
    params: &PreparedEffectParams,
    param_names: &[String],
    target: GeneratorTarget,
    duration_seconds: f64,
) -> Result<Vec<GeneratedChildEffect>, RuntimeError> {
    let mut scopes = vec![HashMap::new()];
    scopes[0].insert("target".to_string(), RuntimeValue::Target(target));
    scopes[0].insert(
        "duration".to_string(),
        RuntimeValue::Float(duration_seconds),
    );
    for (index, name) in param_names.iter().enumerate() {
        scopes[0].insert(name.clone(), params.values[index].clone());
    }
    let mut runtime = GeneratorRuntime {
        scopes,
        emitted: Vec::new(),
        loop_iterations: 0,
    };
    runtime.run_statements(statements)?;
    Ok(runtime.emitted)
}

struct GeneratorRuntime {
    scopes: Vec<HashMap<String, RuntimeValue>>,
    emitted: Vec<GeneratedChildEffect>,
    loop_iterations: usize,
}

impl GeneratorRuntime {
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
                self.scopes
                    .last_mut()
                    .expect("generator always has scope")
                    .insert(name.clone(), value);
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
                self.scopes
                    .last_mut()
                    .expect("generator always has scope")
                    .insert(name.clone(), value);
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
        let mut params = BTreeMap::new();
        for param in &emit.params {
            params.insert(param.name.clone(), self.eval(&param.expr)?);
        }
        let start_seconds = self.float(&emit.start)?;
        let duration_seconds = self.float(&emit.duration)?;
        self.emitted.push(GeneratedChildEffect {
            alias: emit.alias.clone(),
            effect: emit.effect.clone(),
            target,
            start_seconds,
            duration_seconds,
            params,
        });
        Ok(())
    }

    fn eval(&mut self, expr: &Expr) -> Result<RuntimeValue, RuntimeError> {
        match expr {
            Expr::Float(value) => Ok(RuntimeValue::Float(*value)),
            Expr::Int(value) => Ok(RuntimeValue::Int(*value)),
            Expr::Bool(value) => Ok(RuntimeValue::Bool(*value)),
            Expr::Color(value) => Ok(RuntimeValue::Color(*value)),
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
                Ok(RuntimeValue::Int(marks.len() as i64))
            }
            "mark_at" => {
                let RuntimeValue::Marks(marks) = self.eval(&args[0])? else {
                    return Err(self.error("mark_at expects marks"));
                };
                let index = self.int(&args[1])?;
                let fallback = self.float(&args[2])?;
                Ok(RuntimeValue::Float(
                    usize::try_from(index)
                        .ok()
                        .and_then(|index| marks.get(index))
                        .copied()
                        .unwrap_or(fallback),
                ))
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
        let Some(result_type) = binary_result_type(left_type, op, right_type) else {
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
