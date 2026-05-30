use std::collections::{BTreeMap, HashMap};

use crate::model::{Color, CurveValue};

use super::ast::{BinaryOp, Expr, Stmt, UnaryOp};
use super::{
    CompiledEffect, FixtureContext, ParamDefault, PixelContext, RuntimeError, RuntimeValue,
    ScriptDiagnostic, ScriptType,
};

const MAX_LOOP_ITERATIONS: usize = 4096;
pub(super) struct Vm<'a> {
    effect: &'a CompiledEffect,
    env: Vec<HashMap<String, RuntimeValue>>,
    rng: u64,
    loop_iterations: usize,
}

enum Flow {
    Continue,
    Return(Color),
}

impl<'a> Vm<'a> {
    pub(super) fn new(
        effect: &'a CompiledEffect,
        progress: f64,
        seconds: f64,
        fixture: FixtureContext,
        pixel: PixelContext,
        params: &BTreeMap<String, RuntimeValue>,
    ) -> Self {
        let mut env = HashMap::from([
            ("progress".to_string(), RuntimeValue::Float(progress)),
            ("seconds".to_string(), RuntimeValue::Float(seconds)),
            ("fixture".to_string(), RuntimeValue::Fixture(fixture)),
            ("pixel".to_string(), RuntimeValue::Pixel(pixel)),
            ("PI".to_string(), RuntimeValue::Float(std::f64::consts::PI)),
            (
                "TAU".to_string(),
                RuntimeValue::Float(std::f64::consts::TAU),
            ),
        ]);
        for param in &effect.params {
            if let Some(value) = params.get(&param.name) {
                env.insert(param.name.clone(), value.clone());
            } else if let Some(ParamDefault::Value(value)) = &param.default {
                env.insert(param.name.clone(), value.clone());
            }
        }
        Self {
            effect,
            env: vec![env],
            rng: 0x9e37_79b9_7f4a_7c15,
            loop_iterations: 0,
        }
    }

    pub(super) fn run(&mut self) -> Result<Color, RuntimeError> {
        match self.run_statements(&self.effect.sample)? {
            Flow::Return(color) => Ok(color),
            Flow::Continue => Err(self.error("sample did not return")),
        }
    }

    fn run_statements(&mut self, statements: &[Stmt]) -> Result<Flow, RuntimeError> {
        for statement in statements {
            match self.run_statement(statement)? {
                Flow::Continue => {}
                returned @ Flow::Return(_) => return Ok(returned),
            }
        }
        Ok(Flow::Continue)
    }

    fn run_statement(&mut self, statement: &Stmt) -> Result<Flow, RuntimeError> {
        match statement {
            Stmt::Let {
                name,
                value_type,
                expr,
            } => {
                let value = self.eval(expr)?;
                self.define(name.clone(), Self::coerce_value(value, *value_type)?);
                Ok(Flow::Continue)
            }
            Stmt::Assign { name, expr } => {
                let value = self.eval(expr)?;
                self.assign(name, value)?;
                Ok(Flow::Continue)
            }
            Stmt::Expr(expr) => {
                self.eval(expr)?;
                Ok(Flow::Continue)
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
                let initial = self.eval(initializer)?;
                self.define(name.clone(), Self::coerce_value(initial, *value_type)?);
                let flow = self.run_for_loop(condition, update, body);
                self.pop_scope();
                flow
            }
            Stmt::Return(expr) => {
                let RuntimeValue::Color(color) = self.eval(expr)? else {
                    return Err(self.error("sample returned a non-color value"));
                };
                Ok(Flow::Return(color))
            }
        }
    }

    fn run_for_loop(
        &mut self,
        condition: &Expr,
        update: &Stmt,
        body: &[Stmt],
    ) -> Result<Flow, RuntimeError> {
        loop {
            let RuntimeValue::Bool(condition) = self.eval(condition)? else {
                return Err(self.error("for loop condition returned a non-bool value"));
            };
            if !condition {
                return Ok(Flow::Continue);
            }
            self.loop_iterations += 1;
            if self.loop_iterations > MAX_LOOP_ITERATIONS {
                return Err(self.error("effect exceeded the maximum loop iteration count"));
            }
            self.push_scope();
            let flow = self.run_statements(body);
            self.pop_scope();
            match flow? {
                Flow::Continue => {}
                returned @ Flow::Return(_) => return Ok(returned),
            }
            self.run_statement(update)?;
        }
    }

    pub(super) fn eval_constant(expr: &Expr) -> Result<RuntimeValue, ScriptDiagnostic> {
        match expr {
            Expr::Float(value) => Ok(RuntimeValue::Float(*value)),
            Expr::Int(value) => Ok(RuntimeValue::Int(*value)),
            Expr::Bool(value) => Ok(RuntimeValue::Bool(*value)),
            Expr::Color(value) => Ok(RuntimeValue::Color(*value)),
            _ => Err(ScriptDiagnostic {
                range: None,
                message: "parameter defaults must be literals in Dawn v1".to_string(),
            }),
        }
    }

    fn eval(&mut self, expr: &Expr) -> Result<RuntimeValue, RuntimeError> {
        match expr {
            Expr::Float(value) => Ok(RuntimeValue::Float(*value)),
            Expr::Int(value) => Ok(RuntimeValue::Int(*value)),
            Expr::Bool(value) => Ok(RuntimeValue::Bool(*value)),
            Expr::Color(value) => Ok(RuntimeValue::Color(*value)),
            Expr::Ident(name) => self
                .get(name)
                .cloned()
                .ok_or_else(|| self.error(&format!("unknown identifier `{name}`"))),
            Expr::Unary { op, expr } => match (op, self.eval(expr)?) {
                (UnaryOp::Negate, RuntimeValue::Float(value)) => Ok(RuntimeValue::Float(-value)),
                (UnaryOp::Negate, RuntimeValue::Int(value)) => value
                    .checked_neg()
                    .map(RuntimeValue::Int)
                    .ok_or_else(|| self.error("integer overflow")),
                _ => Err(self.error("invalid unary expression")),
            },
            Expr::Binary { left, op, right } => self.eval_binary(left, *op, right),
            Expr::Call { name, args } => self.eval_call(name, args),
        }
    }

    fn eval_binary(
        &mut self,
        left: &Expr,
        op: BinaryOp,
        right: &Expr,
    ) -> Result<RuntimeValue, RuntimeError> {
        let left = self.eval(left)?;
        let right = self.eval(right)?;
        match (left, op, right) {
            (RuntimeValue::Float(left), BinaryOp::Add, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left + right))
            }
            (RuntimeValue::Float(left), BinaryOp::Add, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left + right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Add, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 + right))
            }
            (RuntimeValue::Int(left), BinaryOp::Add, RuntimeValue::Int(right)) => left
                .checked_add(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Float(left), BinaryOp::Subtract, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left - right))
            }
            (RuntimeValue::Float(left), BinaryOp::Subtract, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left - right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Subtract, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 - right))
            }
            (RuntimeValue::Int(left), BinaryOp::Subtract, RuntimeValue::Int(right)) => left
                .checked_sub(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Float(left), BinaryOp::Multiply, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left * right))
            }
            (RuntimeValue::Float(left), BinaryOp::Multiply, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left * right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Multiply, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 * right))
            }
            (RuntimeValue::Int(left), BinaryOp::Multiply, RuntimeValue::Int(right)) => left
                .checked_mul(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Float(left), BinaryOp::Divide, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left / right))
            }
            (RuntimeValue::Float(left), BinaryOp::Divide, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left / right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Divide, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 / right))
            }
            (RuntimeValue::Int(_), BinaryOp::Divide, RuntimeValue::Int(0)) => {
                Err(self.error("integer divide by zero"))
            }
            (RuntimeValue::Int(left), BinaryOp::Divide, RuntimeValue::Int(right)) => left
                .checked_div(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Float(left), BinaryOp::Less, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left < right))
            }
            (RuntimeValue::Float(left), BinaryOp::Less, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left < right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Less, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool((left as f64) < right))
            }
            (RuntimeValue::Int(left), BinaryOp::Less, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left < right))
            }
            (RuntimeValue::Float(left), BinaryOp::LessEqual, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left <= right))
            }
            (RuntimeValue::Float(left), BinaryOp::LessEqual, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left <= right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::LessEqual, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool((left as f64) <= right))
            }
            (RuntimeValue::Int(left), BinaryOp::LessEqual, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left <= right))
            }
            (RuntimeValue::Float(left), BinaryOp::Greater, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left > right))
            }
            (RuntimeValue::Float(left), BinaryOp::Greater, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left > right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Greater, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool((left as f64) > right))
            }
            (RuntimeValue::Int(left), BinaryOp::Greater, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left > right))
            }
            (RuntimeValue::Float(left), BinaryOp::GreaterEqual, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left >= right))
            }
            (RuntimeValue::Float(left), BinaryOp::GreaterEqual, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left >= right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::GreaterEqual, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool((left as f64) >= right))
            }
            (RuntimeValue::Int(left), BinaryOp::GreaterEqual, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left >= right))
            }
            (RuntimeValue::Color(color), BinaryOp::Multiply, RuntimeValue::Float(factor))
            | (RuntimeValue::Float(factor), BinaryOp::Multiply, RuntimeValue::Color(color)) => {
                Ok(RuntimeValue::Color(color.scale(factor)))
            }
            (RuntimeValue::Color(color), BinaryOp::Multiply, RuntimeValue::Int(factor))
            | (RuntimeValue::Int(factor), BinaryOp::Multiply, RuntimeValue::Color(color)) => {
                Ok(RuntimeValue::Color(color.scale(factor as f64)))
            }
            _ => Err(self.error("invalid binary expression")),
        }
    }

    fn eval_call(&mut self, name: &str, args: &[Expr]) -> Result<RuntimeValue, RuntimeError> {
        if let Some(RuntimeValue::Curve(curve)) = self.get(name).cloned() {
            let arg = self.eval(&args[0])?;
            let amount = self.expect_float(arg)?;
            return match curve.evaluate(amount) {
                Some(CurveValue::Float(value)) => Ok(RuntimeValue::Float(value)),
                Some(CurveValue::Color(value)) => Ok(RuntimeValue::Color(value)),
                None => Err(self.error("curve has no points")),
            };
        }

        let values = args
            .iter()
            .map(|arg| self.eval(arg))
            .collect::<Result<Vec<_>, _>>()?;
        match (name, values.as_slice()) {
            ("sin", [value]) => Ok(RuntimeValue::Float(self.expect_float(value.clone())?.sin())),
            ("cos", [value]) => Ok(RuntimeValue::Float(self.expect_float(value.clone())?.cos())),
            ("abs", [value]) => Ok(RuntimeValue::Float(self.expect_float(value.clone())?.abs())),
            ("floor", [value]) => Ok(RuntimeValue::Float(
                self.expect_float(value.clone())?.floor(),
            )),
            ("srand", [value]) => {
                self.rng = seed_from_float(self.expect_float(value.clone())?);
                Ok(RuntimeValue::Float(0.0))
            }
            ("rand", []) => Ok(RuntimeValue::Float(self.rand())),
            ("pixel_index", [RuntimeValue::Pixel(pixel)]) => {
                Ok(RuntimeValue::Int(pixel.index as i64))
            }
            ("pixel_count", [RuntimeValue::Pixel(pixel)]) => {
                Ok(RuntimeValue::Int(pixel.count as i64))
            }
            ("mark_count", [marks]) => Ok(RuntimeValue::Int(
                self.expect_marks(marks.clone())?.len() as i64,
            )),
            ("mark_at", [marks, index, fallback]) => {
                let marks = self.expect_marks(marks.clone())?;
                let index = self.expect_int(index.clone())?;
                let fallback = self.expect_float(fallback.clone())?;
                let value = usize::try_from(index)
                    .ok()
                    .and_then(|index| marks.get(index))
                    .copied()
                    .unwrap_or(fallback);
                Ok(RuntimeValue::Float(value))
            }
            ("mark_prev", [marks, time, fallback]) => {
                let marks = self.expect_marks(marks.clone())?;
                let time = self.expect_float(time.clone())?;
                let fallback = self.expect_float(fallback.clone())?;
                Ok(RuntimeValue::Float(
                    mark_prev(&marks, time).unwrap_or(fallback),
                ))
            }
            ("mark_next", [marks, time, fallback]) => {
                let marks = self.expect_marks(marks.clone())?;
                let time = self.expect_float(time.clone())?;
                let fallback = self.expect_float(fallback.clone())?;
                Ok(RuntimeValue::Float(
                    mark_next(&marks, time).unwrap_or(fallback),
                ))
            }
            ("mark_nearest", [marks, time, fallback]) => {
                let marks = self.expect_marks(marks.clone())?;
                let time = self.expect_float(time.clone())?;
                let fallback = self.expect_float(fallback.clone())?;
                Ok(RuntimeValue::Float(
                    mark_nearest(&marks, time).unwrap_or(fallback),
                ))
            }
            ("mark_phase", [marks, time, fallback]) => {
                let marks = self.expect_marks(marks.clone())?;
                let time = self.expect_float(time.clone())?;
                let fallback = self.expect_float(fallback.clone())?;
                Ok(RuntimeValue::Float(
                    mark_phase(&marks, time).unwrap_or(fallback),
                ))
            }
            ("mark_elapsed", [marks, time, fallback]) => {
                let marks = self.expect_marks(marks.clone())?;
                let time = self.expect_float(time.clone())?;
                let fallback = self.expect_float(fallback.clone())?;
                Ok(RuntimeValue::Float(
                    mark_elapsed(&marks, time).unwrap_or(fallback),
                ))
            }
            ("min", [left, right]) => Ok(RuntimeValue::Float(
                self.expect_float(left.clone())?
                    .min(self.expect_float(right.clone())?),
            )),
            ("max", [left, right]) => Ok(RuntimeValue::Float(
                self.expect_float(left.clone())?
                    .max(self.expect_float(right.clone())?),
            )),
            ("clamp", [value, min, max]) => Ok(RuntimeValue::Float(
                self.expect_float(value.clone())?.clamp(
                    self.expect_float(min.clone())?,
                    self.expect_float(max.clone())?,
                ),
            )),
            ("smoothstep", [edge0, edge1, value]) => {
                let edge0 = self.expect_float(edge0.clone())?;
                let edge1 = self.expect_float(edge1.clone())?;
                let value = self.expect_float(value.clone())?;
                let x = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                Ok(RuntimeValue::Float(x * x * (3.0 - 2.0 * x)))
            }
            ("mix", [RuntimeValue::Color(left), RuntimeValue::Color(right), amount]) => Ok(
                RuntimeValue::Color(left.mix(*right, self.expect_float(amount.clone())?)),
            ),
            ("mix", [left, right, amount]) => {
                let left = self.expect_float(left.clone())?;
                let right = self.expect_float(right.clone())?;
                let amount = self.expect_float(amount.clone())?;
                Ok(RuntimeValue::Float(left + (right - left) * amount))
            }
            ("rgb", [red, green, blue]) => Ok(RuntimeValue::Color(Color::new(
                self.expect_float(red.clone())?.round().clamp(0.0, 255.0) as u8,
                self.expect_float(green.clone())?.round().clamp(0.0, 255.0) as u8,
                self.expect_float(blue.clone())?.round().clamp(0.0, 255.0) as u8,
            ))),
            ("hsv", [hue, saturation, value]) => Ok(RuntimeValue::Color(hsv_to_rgb(
                self.expect_float(hue.clone())?,
                self.expect_float(saturation.clone())?,
                self.expect_float(value.clone())?,
            ))),
            _ => Err(self.error(&format!("invalid call `{name}`"))),
        }
    }

    fn expect_float(&self, value: RuntimeValue) -> Result<f64, RuntimeError> {
        match value {
            RuntimeValue::Float(value) => Ok(value),
            RuntimeValue::Int(value) => Ok(value as f64),
            _ => Err(self.error("expected float value")),
        }
    }

    fn expect_int(&self, value: RuntimeValue) -> Result<i64, RuntimeError> {
        match value {
            RuntimeValue::Int(value) => Ok(value),
            _ => Err(self.error("expected int value")),
        }
    }

    fn expect_marks(&self, value: RuntimeValue) -> Result<Vec<f64>, RuntimeError> {
        match value {
            RuntimeValue::Marks(value) => Ok(value),
            _ => Err(self.error("expected marks value")),
        }
    }

    pub(super) fn coerce_value(
        value: RuntimeValue,
        expected: ScriptType,
    ) -> Result<RuntimeValue, RuntimeError> {
        match (expected, value) {
            (ScriptType::Float, RuntimeValue::Int(value)) => Ok(RuntimeValue::Float(value as f64)),
            (expected, value) if value.value_type() == expected => Ok(value),
            (expected, value) => Err(RuntimeError {
                message: format!(
                    "expected {expected} value, but found {}",
                    value.value_type()
                ),
            }),
        }
    }

    fn define(&mut self, name: String, value: RuntimeValue) {
        if let Some(scope) = self.env.last_mut() {
            scope.insert(name, value);
        }
    }

    fn assign(&mut self, name: &str, value: RuntimeValue) -> Result<(), RuntimeError> {
        let Some(scope_index) = self.env.iter().rposition(|scope| scope.contains_key(name)) else {
            return Err(self.error(&format!("unknown local `{name}`")));
        };
        let Some(existing) = self.env[scope_index].get(name) else {
            return Err(self.error(&format!("unknown local `{name}`")));
        };
        let expected = existing.value_type();
        let value = Self::coerce_value(value, expected)?;
        self.env[scope_index].insert(name.to_string(), value);
        Ok(())
    }

    fn get(&self, name: &str) -> Option<&RuntimeValue> {
        self.env.iter().rev().find_map(|scope| scope.get(name))
    }

    fn push_scope(&mut self) {
        self.env.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.env.pop();
    }

    fn error(&self, message: &str) -> RuntimeError {
        RuntimeError {
            message: message.to_string(),
        }
    }

    fn rand(&mut self) -> f64 {
        self.rng = self
            .rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.rng >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

fn seed_from_float(value: f64) -> u64 {
    let mut seed = value.to_bits();
    seed ^= seed >> 30;
    seed = seed.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    seed ^= seed >> 27;
    seed = seed.wrapping_mul(0x94d0_49bb_1331_11eb);
    seed ^ (seed >> 31)
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

fn mark_prev(marks: &[f64], time: f64) -> Option<f64> {
    marks.iter().rev().copied().find(|mark| *mark <= time)
}

fn mark_next(marks: &[f64], time: f64) -> Option<f64> {
    marks.iter().copied().find(|mark| *mark > time)
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
