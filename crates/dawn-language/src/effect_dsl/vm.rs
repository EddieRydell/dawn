use super::ast::{BinaryOp, Block, Expr, ExprKind, Stmt, UnaryOp};
use super::bytecode::BytecodeFunction;
use super::types::{Identifier, Type, Value};
use super::{CompiledEffect, ParamDecl};
use crate::values::{Color, Curve, CurveValue, Marks};
use indexmap::IndexMap;

const LOOP_ITERATION_LIMIT: usize = 100_000;

#[derive(Clone, Debug)]
pub struct RunContext {
    pub progress: f64,
    pub seconds: f64,
    pub duration: f64,
    pub pixel_index: i64,
    pub pixel_count: i64,
    pub pixel_fraction: f64,
    pub global_marks: Marks,
    //TODO location based effects
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeError {
    pub message: String,
}

impl RuntimeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub(crate) fn run_effect(
    effect: &CompiledEffect,
    params: &IndexMap<Identifier, Value>,
    context: &RunContext,
) -> Result<Color, RuntimeError> {
    let mut vm = Vm::new(effect, params, context)?;
    match vm.run(&effect.sample)? {
        Value::Color(color) => Ok(color),
        other => Err(RuntimeError::new(format!(
            "`sample` returned non-color value {other:?}"
        ))),
    }
}

struct Vm<'a> {
    context: &'a RunContext,
    scopes: Vec<IndexMap<Identifier, Value>>,
    loop_iterations: usize,
}

enum Flow {
    Continue,
    Return(Value),
}

impl<'a> Vm<'a> {
    fn new(
        effect: &CompiledEffect,
        params: &IndexMap<Identifier, Value>,
        context: &'a RunContext,
    ) -> Result<Self, RuntimeError> {
        let mut scope = IndexMap::new();
        for param in &effect.params {
            let value = resolve_param(param, params);
            scope.insert(param.name.clone(), value);
        }
        Ok(Self {
            context,
            scopes: vec![scope],
            loop_iterations: 0,
        })
    }

    fn run(&mut self, function: &BytecodeFunction) -> Result<Value, RuntimeError> {
        match self.exec_block(&function.body)? {
            Flow::Return(value) => Ok(value),
            Flow::Continue => Err(RuntimeError::new("function completed without return")),
        }
    }

    fn exec_block(&mut self, block: &Block) -> Result<Flow, RuntimeError> {
        self.scopes.push(IndexMap::new());
        for statement in &block.statements {
            match self.exec_statement(statement)? {
                Flow::Continue => {}
                Flow::Return(value) => {
                    let _ = self.scopes.pop();
                    return Ok(Flow::Return(value));
                }
            }
        }
        let _ = self.scopes.pop();
        Ok(Flow::Continue)
    }

    fn exec_statement(&mut self, statement: &Stmt) -> Result<Flow, RuntimeError> {
        match statement {
            Stmt::Local {
                ty,
                name,
                initializer,
            } => {
                let value = if let Some(initializer) = initializer {
                    let evaluated = self.eval(initializer)?;
                    self.coerce(evaluated, ty)?
                } else {
                    default_value(ty)
                };
                self.define(name.clone(), value);
                Ok(Flow::Continue)
            }
            Stmt::Assign { name, value } => {
                let value = self.eval(value)?;
                self.assign(name, value)?;
                Ok(Flow::Continue)
            }
            Stmt::Expr(expr) => {
                let _ = self.eval(expr)?;
                Ok(Flow::Continue)
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
            } => {
                if to_bool(&self.eval(condition)?)? {
                    self.exec_block(then_block)
                } else if let Some(else_block) = else_block {
                    self.exec_block(else_block)
                } else {
                    Ok(Flow::Continue)
                }
            }
            Stmt::For {
                initializer,
                condition,
                update,
                body,
            } => {
                self.scopes.push(IndexMap::new());
                let _ = self.exec_statement(initializer)?;
                while to_bool(&self.eval(condition)?)? {
                    self.loop_iterations += 1;
                    if self.loop_iterations > LOOP_ITERATION_LIMIT {
                        let _ = self.scopes.pop();
                        return Err(RuntimeError::new("loop iteration limit exceeded"));
                    }
                    match self.exec_block(body)? {
                        Flow::Continue => {}
                        Flow::Return(value) => {
                            let _ = self.scopes.pop();
                            return Ok(Flow::Return(value));
                        }
                    }
                    let _ = self.exec_statement(update)?;
                }
                let _ = self.scopes.pop();
                Ok(Flow::Continue)
            }
            Stmt::Return(expr) => Ok(Flow::Return(self.eval(expr)?)),
        }
    }

    fn eval(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        match &expr.kind {
            ExprKind::Literal(value) => Ok(value.clone()),
            ExprKind::Variable(name) => self
                .lookup(name)
                .or_else(|| constant_value(name))
                .ok_or_else(|| RuntimeError::new(format!("unknown variable `{}`", name.as_str()))),
            ExprKind::Array(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.eval(item)?);
                }
                Ok(Value::Array(values))
            }
            ExprKind::Index { target, index } => {
                let target = self.eval(target)?;
                match target {
                    Value::Array(items) => {
                        let index = to_int(&self.eval(index)?)?;
                        let index = usize::try_from(index)
                            .map_err(|_| RuntimeError::new("array index cannot be negative"))?;
                        items
                            .get(index)
                            .cloned()
                            .ok_or_else(|| RuntimeError::new("array index out of bounds"))
                    }
                    Value::Curve(curve) => {
                        let position = to_float(&self.eval(index)?)?;
                        Ok(sample_curve(&curve, position))
                    }
                    _ => Err(RuntimeError::new("index target is not an array or curve")),
                }
            }
            ExprKind::Call { callee, args } => self.eval_call(callee, args),
            ExprKind::Unary { op, expr } => {
                let value = self.eval(expr)?;
                match op {
                    UnaryOp::Negate => match value {
                        Value::Int(value) => Ok(Value::Int(-value)),
                        Value::Float(value) => Ok(Value::Float(-value)),
                        _ => Err(RuntimeError::new("unary `-` requires a number")),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!to_bool(&value)?)),
                }
            }
            ExprKind::Binary { op, left, right } => {
                if *op == BinaryOp::And {
                    return Ok(Value::Bool(
                        to_bool(&self.eval(left)?)? && to_bool(&self.eval(right)?)?,
                    ));
                }
                if *op == BinaryOp::Or {
                    return Ok(Value::Bool(
                        to_bool(&self.eval(left)?)? || to_bool(&self.eval(right)?)?,
                    ));
                }
                let left = self.eval(left)?;
                let right = self.eval(right)?;
                self.eval_binary(*op, left, right)
            }
        }
    }

    fn eval_binary(&self, op: BinaryOp, left: Value, right: Value) -> Result<Value, RuntimeError> {
        match op {
            BinaryOp::Add => numeric_binary(left, right, |left, right| left + right),
            BinaryOp::Subtract => numeric_binary(left, right, |left, right| left - right),
            BinaryOp::Multiply => numeric_binary(left, right, |left, right| left * right),
            BinaryOp::Divide => numeric_binary(left, right, |left, right| left / right),
            BinaryOp::Remainder => numeric_binary(left, right, |left, right| left % right),
            BinaryOp::Less => compare_binary(left, right, |left, right| left < right),
            BinaryOp::LessEqual => compare_binary(left, right, |left, right| left <= right),
            BinaryOp::Greater => compare_binary(left, right, |left, right| left > right),
            BinaryOp::GreaterEqual => compare_binary(left, right, |left, right| left >= right),
            BinaryOp::Equal => Ok(Value::Bool(values_equal(&left, &right))),
            BinaryOp::NotEqual => Ok(Value::Bool(!values_equal(&left, &right))),
            BinaryOp::And | BinaryOp::Or => Err(RuntimeError::new("invalid boolean operator path")),
        }
    }

    fn eval_call(&mut self, callee: &Expr, args: &[Expr]) -> Result<Value, RuntimeError> {
        let ExprKind::Variable(name) = &callee.kind else {
            return Err(RuntimeError::new("call target must be a builtin name"));
        };

        if self.lookup(name).is_none() && !is_builtin(name.as_str()) {
            return Err(RuntimeError::new(format!(
                "unknown function `{}`",
                name.as_str()
            )));
        }

        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(arg)?);
        }
        self.call_builtin(name.as_str(), &values)
    }

    fn call_builtin(&self, name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
        match name {
            "progress" => Ok(Value::Float(self.context.progress)),
            "seconds" => Ok(Value::Float(self.context.seconds)),
            "duration" => Ok(Value::Float(self.context.duration)),
            "pixel_index" => Ok(Value::Int(self.context.pixel_index)),
            "pixel_count" => Ok(Value::Int(self.context.pixel_count)),
            "pixel_fraction" => Ok(Value::Float(self.context.pixel_fraction)),
            "section_position" => {
                let width = to_float(value_at(args, 0)?)?.max(1.0);
                let index = self.context.pixel_index as f64;
                Ok(Value::Float(
                    (index - (index / width).floor() * width) / width,
                ))
            }
            "sin" => Ok(Value::Float(to_float(value_at(args, 0)?)?.sin())),
            "cos" => Ok(Value::Float(to_float(value_at(args, 0)?)?.cos())),
            "abs" => Ok(Value::Float(to_float(value_at(args, 0)?)?.abs())),
            "floor" => Ok(Value::Float(to_float(value_at(args, 0)?)?.floor())),
            "min" => Ok(Value::Float(
                to_float(value_at(args, 0)?)?.min(to_float(value_at(args, 1)?)?),
            )),
            "max" => Ok(Value::Float(
                to_float(value_at(args, 0)?)?.max(to_float(value_at(args, 1)?)?),
            )),
            "clamp" => Ok(Value::Float(
                to_float(value_at(args, 0)?)?
                    .clamp(to_float(value_at(args, 1)?)?, to_float(value_at(args, 2)?)?),
            )),
            "smoothstep" => {
                let edge0 = to_float(value_at(args, 0)?)?;
                let edge1 = to_float(value_at(args, 1)?)?;
                let x = to_float(value_at(args, 2)?)?;
                let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                Ok(Value::Float(t * t * (3.0 - 2.0 * t)))
            }
            "mix" => mix_values(
                value_at(args, 0)?,
                value_at(args, 1)?,
                to_float(value_at(args, 2)?)?,
            ),
            "rgb" => Ok(Value::Color(Color {
                red: channel(to_float(value_at(args, 0)?)?),
                green: channel(to_float(value_at(args, 1)?)?),
                blue: channel(to_float(value_at(args, 2)?)?),
            })),
            "hsv" => Ok(Value::Color(hsv(
                to_float(value_at(args, 0)?)?,
                to_float(value_at(args, 1)?)?,
                to_float(value_at(args, 2)?)?,
            ))),
            "srand" | "rand" => Ok(Value::Float(random(args)?)),
            "curve_crossing" => Ok(Value::Float(to_float(value_at(args, 1)?)?)),
            "curve_float_clamped" => {
                let curve = to_curve(value_at(args, 0)?)?;
                let position = to_float(value_at(args, 1)?)?;
                let min = to_float(value_at(args, 2)?)?;
                let max = to_float(value_at(args, 3)?)?;
                Ok(Value::Float(
                    to_float(&sample_curve(curve, position))?.clamp(min, max),
                ))
            }
            "curve_color_scaled" => {
                let curve = to_curve(value_at(args, 0)?)?;
                let position = to_float(value_at(args, 1)?)?;
                let scale = to_float(value_at(args, 2)?)?.clamp(0.0, 1.0);
                let Value::Color(color) = sample_curve(curve, position) else {
                    return Err(RuntimeError::new("curve_color_scaled requires color curve"));
                };
                Ok(Value::Color(scale_color(color, scale)))
            }
            "len" => match value_at(args, 0)? {
                Value::Array(items) => i64::try_from(items.len())
                    .map(Value::Int)
                    .map_err(|_| RuntimeError::new("array length exceeds int range")),
                Value::Marks(marks) => i64::try_from(marks.marks.len())
                    .map(Value::Int)
                    .map_err(|_| RuntimeError::new("mark count exceeds int range")),
                _ => Err(RuntimeError::new("len requires array or marks")),
            },
            "mark_count" => Ok(Value::Int(mark_count(args, self.context)?)),
            "mark_at" => Ok(Value::Float(mark_at(args, self.context)?)),
            "mark_prev" => Ok(Value::Float(mark_prev(args, self.context)?)),
            "mark_prev_index" => Ok(Value::Int(mark_prev_index(args, self.context)?)),
            "mark_next_index" => Ok(Value::Int(mark_next_index(args, self.context)?)),
            "mark_elapsed" => Ok(Value::Float(mark_elapsed(args, self.context)?)),
            "mark_phase" => Ok(Value::Float(mark_phase(args, self.context)?)),
            _ => Err(RuntimeError::new(format!("unknown builtin `{name}`"))),
        }
    }

    fn coerce(&self, value: Value, ty: &Type) -> Result<Value, RuntimeError> {
        match (value, ty) {
            (Value::Int(value), Type::Float) => Ok(Value::Float(value as f64)),
            (value, _) => Ok(value),
        }
    }

    fn lookup(&self, name: &Identifier) -> Option<Value> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn define(&mut self, name: Identifier, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, value);
        }
    }

    fn assign(&mut self, name: &Identifier, value: Value) -> Result<(), RuntimeError> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.clone(), value);
                return Ok(());
            }
        }
        Err(RuntimeError::new(format!(
            "unknown assignment target `{}`",
            name.as_str()
        )))
    }
}

fn resolve_param(param: &ParamDecl, params: &IndexMap<Identifier, Value>) -> Value {
    if let Some(value) = params.get(&param.name) {
        return value.clone();
    }
    if let Some(default) = &param.default {
        return default.clone();
    }
    default_value(&param.ty)
}

fn default_value(ty: &Type) -> Value {
    match ty {
        Type::Void => Value::Void,
        Type::Int => Value::Int(0),
        Type::Float => Value::Float(0.0),
        Type::Bool => Value::Bool(false),
        Type::Color => Value::Color(Color {
            red: 0,
            green: 0,
            blue: 0,
        }),
        Type::Marks => Value::Marks(Marks { marks: Vec::new() }),
        Type::Curve(_) => Value::Curve(Curve { points: Vec::new() }),
        Type::Array(_) => Value::Array(Vec::new()),
        Type::Enum(options) => options
            .first()
            .cloned()
            .map(Value::Enum)
            .unwrap_or(Value::Void),
    }
}

fn value_at(args: &[Value], index: usize) -> Result<&Value, RuntimeError> {
    args.get(index)
        .ok_or_else(|| RuntimeError::new("missing argument"))
}

fn constant_value(name: &Identifier) -> Option<Value> {
    match name.as_str() {
        "PI" => Some(Value::Float(std::f64::consts::PI)),
        "TAU" => Some(Value::Float(std::f64::consts::TAU)),
        _ => None,
    }
}

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "progress"
            | "seconds"
            | "duration"
            | "pixel_index"
            | "pixel_count"
            | "pixel_fraction"
            | "section_position"
            | "sin"
            | "cos"
            | "abs"
            | "floor"
            | "min"
            | "max"
            | "clamp"
            | "smoothstep"
            | "mix"
            | "rgb"
            | "hsv"
            | "srand"
            | "rand"
            | "curve_crossing"
            | "curve_float_clamped"
            | "curve_color_scaled"
            | "len"
            | "mark_count"
            | "mark_at"
            | "mark_prev"
            | "mark_prev_index"
            | "mark_next_index"
            | "mark_elapsed"
            | "mark_phase"
    )
}

fn to_bool(value: &Value) -> Result<bool, RuntimeError> {
    match value {
        Value::Bool(value) => Ok(*value),
        _ => Err(RuntimeError::new("expected bool")),
    }
}

fn to_int(value: &Value) -> Result<i64, RuntimeError> {
    match value {
        Value::Int(value) => Ok(*value),
        Value::Float(value) => Ok(*value as i64),
        _ => Err(RuntimeError::new("expected int")),
    }
}

fn to_float(value: &Value) -> Result<f64, RuntimeError> {
    match value {
        Value::Int(value) => Ok(*value as f64),
        Value::Float(value) => Ok(*value),
        _ => Err(RuntimeError::new("expected float")),
    }
}

fn to_curve(value: &Value) -> Result<&Curve, RuntimeError> {
    match value {
        Value::Curve(curve) => Ok(curve),
        _ => Err(RuntimeError::new("expected curve")),
    }
}

fn numeric_binary(
    left: Value,
    right: Value,
    op: impl FnOnce(f64, f64) -> f64,
) -> Result<Value, RuntimeError> {
    let result = op(to_float(&left)?, to_float(&right)?);
    if matches!((&left, &right), (Value::Int(_), Value::Int(_))) && result.fract() == 0.0 {
        return Ok(Value::Int(result as i64));
    }
    Ok(Value::Float(result))
}

fn compare_binary(
    left: Value,
    right: Value,
    op: impl FnOnce(f64, f64) -> bool,
) -> Result<Value, RuntimeError> {
    Ok(Value::Bool(op(to_float(&left)?, to_float(&right)?)))
}

fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Void, Value::Void) => true,
        (Value::Int(left), Value::Int(right)) => left == right,
        (Value::Float(left), Value::Float(right)) => left == right,
        (Value::Int(left), Value::Float(right)) => (*left as f64) == *right,
        (Value::Float(left), Value::Int(right)) => *left == (*right as f64),
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Color(left), Value::Color(right)) => left == right,
        (Value::Enum(left), Value::Enum(right)) => left == right,
        _ => false,
    }
}

fn sample_curve(curve: &Curve, position: f64) -> Value {
    let Some(first) = curve.points.first() else {
        return Value::Float(0.0);
    };
    let mut previous = first;
    for point in &curve.points {
        if point.position >= position {
            let span = (point.position - previous.position).max(0.000000001);
            let t = ((position - previous.position) / span).clamp(0.0, 1.0);
            return mix_curve_values(&previous.value, &point.value, t);
        }
        previous = point;
    }
    curve_value_to_value(&previous.value)
}

fn mix_curve_values(left: &CurveValue, right: &CurveValue, t: f64) -> Value {
    match (left, right) {
        (CurveValue::Float(left), CurveValue::Float(right)) => {
            Value::Float(left + (right - left) * t)
        }
        (CurveValue::Color(left), CurveValue::Color(right)) => mix_colors(*left, *right, t)
            .map(Value::Color)
            .unwrap_or(Value::Color(*left)),
        _ => curve_value_to_value(left),
    }
}

fn curve_value_to_value(value: &CurveValue) -> Value {
    match value {
        CurveValue::Float(value) => Value::Float(*value),
        CurveValue::Color(value) => Value::Color(*value),
    }
}

fn mix_values(left: &Value, right: &Value, t: f64) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left + (right - left) * t)),
        (Value::Int(left), Value::Int(right)) => {
            Ok(Value::Float(*left as f64 + (*right - *left) as f64 * t))
        }
        (Value::Color(left), Value::Color(right)) => mix_colors(*left, *right, t).map(Value::Color),
        _ => Err(RuntimeError::new(
            "mix requires matching float or color values",
        )),
    }
}

fn mix_colors(left: Color, right: Color, t: f64) -> Result<Color, RuntimeError> {
    Ok(Color {
        red: channel(
            left.red as f64 / 255.0 + (right.red as f64 / 255.0 - left.red as f64 / 255.0) * t,
        ),
        green: channel(
            left.green as f64 / 255.0
                + (right.green as f64 / 255.0 - left.green as f64 / 255.0) * t,
        ),
        blue: channel(
            left.blue as f64 / 255.0 + (right.blue as f64 / 255.0 - left.blue as f64 / 255.0) * t,
        ),
    })
}

fn scale_color(color: Color, scale: f64) -> Color {
    Color {
        red: channel(color.red as f64 / 255.0 * scale),
        green: channel(color.green as f64 / 255.0 * scale),
        blue: channel(color.blue as f64 / 255.0 * scale),
    }
}

fn channel(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn hsv(h: f64, s: f64, v: f64) -> Color {
    let h = h - h.floor();
    let sector = h * 6.0;
    let c = v * s;
    let x = c * (1.0 - (sector - (sector / 2.0).floor() * 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if sector < 1.0 {
        (c, x, 0.0)
    } else if sector < 2.0 {
        (x, c, 0.0)
    } else if sector < 3.0 {
        (0.0, c, x)
    } else if sector < 4.0 {
        (0.0, x, c)
    } else if sector < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Color {
        red: channel(r + m),
        green: channel(g + m),
        blue: channel(b + m),
    }
}

fn random(args: &[Value]) -> Result<f64, RuntimeError> {
    let mut seed = 0.0;
    for arg in args {
        seed = seed * 31.0 + to_float(arg)?;
    }
    Ok((seed.sin() * 43_758.545_312_3).fract().abs())
}

fn mark_source(args: &[Value]) -> Result<&Marks, RuntimeError> {
    let Some(value) = args.first() else {
        return Err(RuntimeError::new(
            "mark builtin requires marks as the first argument",
        ));
    };
    match value {
        Value::Marks(marks) => Ok(marks),
        _ => Err(RuntimeError::new("mark builtin first arg must be marks")),
    }
}

fn mark_count(args: &[Value], context: &RunContext) -> Result<i64, RuntimeError> {
    let _ = context;
    i64::try_from(mark_source(args)?.marks.len())
        .map_err(|_| RuntimeError::new("mark count exceeds int range"))
}

fn mark_at(args: &[Value], _context: &RunContext) -> Result<f64, RuntimeError> {
    let marks = mark_source(args)?;
    let index_arg = if matches!(args.first(), Some(Value::Marks(_))) {
        1
    } else {
        0
    };
    let index = to_int(value_at(args, index_arg)?)?;
    let fallback = args
        .get(index_arg + 1)
        .map(to_float)
        .transpose()?
        .unwrap_or(0.0);
    mark_at_from(marks, index, fallback)
}

fn mark_prev(args: &[Value], context: &RunContext) -> Result<f64, RuntimeError> {
    let marks = mark_source(args)?;
    let seconds = mark_query_seconds(args, context, 1)?;
    let fallback = args.get(2).map(to_float).transpose()?.unwrap_or(0.0);
    let index = prev_index(marks, seconds)?;
    if index < 0 {
        Ok(fallback)
    } else {
        mark_at_from(marks, index, fallback)
    }
}

fn mark_at_from(marks: &Marks, index: i64, fallback: f64) -> Result<f64, RuntimeError> {
    let index =
        usize::try_from(index).map_err(|_| RuntimeError::new("mark index cannot be negative"))?;
    Ok(marks
        .marks
        .get(index)
        .map(|mark| mark.as_seconds_f64())
        .unwrap_or(fallback))
}

fn mark_prev_index(args: &[Value], context: &RunContext) -> Result<i64, RuntimeError> {
    let seconds = mark_query_seconds(args, context, 1)?;
    prev_index(mark_source(args)?, seconds)
}

fn mark_next_index(args: &[Value], context: &RunContext) -> Result<i64, RuntimeError> {
    let seconds = mark_query_seconds(args, context, 1)?;
    next_index(mark_source(args)?, seconds)
}

fn prev_index(marks: &Marks, seconds: f64) -> Result<i64, RuntimeError> {
    let mut previous = -1;
    for (index, mark) in marks.marks.iter().enumerate() {
        if mark.as_seconds_f64() <= seconds {
            previous = i64::try_from(index)
                .map_err(|_| RuntimeError::new("mark index exceeds int range"))?;
        }
    }
    Ok(previous)
}

fn next_index(marks: &Marks, seconds: f64) -> Result<i64, RuntimeError> {
    for (index, mark) in marks.marks.iter().enumerate() {
        if mark.as_seconds_f64() > seconds {
            return i64::try_from(index)
                .map_err(|_| RuntimeError::new("mark index exceeds int range"));
        }
    }
    Ok(-1)
}

fn mark_elapsed(args: &[Value], context: &RunContext) -> Result<f64, RuntimeError> {
    let seconds = mark_query_seconds(args, context, 1)?;
    elapsed(mark_source(args)?, seconds)
}

fn elapsed(marks: &Marks, seconds: f64) -> Result<f64, RuntimeError> {
    let previous = prev_index(marks, seconds)?;
    if previous < 0 {
        return Ok(seconds);
    }
    Ok(seconds - mark_at_from(marks, previous, 0.0)?)
}

fn mark_phase(args: &[Value], context: &RunContext) -> Result<f64, RuntimeError> {
    let seconds = mark_query_seconds(args, context, 1)?;
    phase(mark_source(args)?, seconds, context.duration)
}

fn phase(marks: &Marks, seconds: f64, duration: f64) -> Result<f64, RuntimeError> {
    let previous = prev_index(marks, seconds)?;
    let next = next_index(marks, seconds)?;
    let start = if previous >= 0 {
        mark_at_from(marks, previous, 0.0)?
    } else {
        0.0
    };
    let end = if next >= 0 {
        mark_at_from(marks, next, duration)?
    } else {
        duration
    };
    Ok(((seconds - start) / (end - start).max(0.000000001)).clamp(0.0, 1.0))
}

fn mark_query_seconds(
    args: &[Value],
    context: &RunContext,
    marks_arg_offset: usize,
) -> Result<f64, RuntimeError> {
    if matches!(args.first(), Some(Value::Marks(_))) {
        args.get(marks_arg_offset)
            .map(to_float)
            .transpose()
            .map(|value| value.unwrap_or(context.seconds))
    } else {
        args.first()
            .map(to_float)
            .transpose()
            .map(|value| value.unwrap_or(context.seconds))
    }
}
