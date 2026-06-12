use super::ast::{BinaryOp, UnaryOp};
use super::bytecode::{Builtin, BytecodeFunction, GeneratorContextId, Instruction};
use super::types::{Identifier, Type, Value};
use super::types::{TargetItemValue, TargetItemsValue, TargetPixelValue, TargetValue};
use super::{CompiledEffect, EffectKind, ParamDecl};
use crate::values::{Color, Curve, CurveValue, Marks};
use indexmap::IndexMap;
use std::sync::Arc;

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

#[derive(Clone, Debug)]
pub struct GeneratorContext {
    pub duration: f64,
    pub target: Arc<TargetValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedEffect {
    pub definition: Identifier,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub target: Arc<TargetItemValue>,
    pub params: IndexMap<Identifier, Value>,
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

#[derive(Clone, Debug)]
pub struct BoundEffectParams {
    values: Vec<Value>,
}

#[derive(Clone, Debug, Default)]
pub struct EffectVmScratch {
    stack: Vec<RuntimeValue>,
    locals: Vec<Value>,
    param_overrides: Vec<Option<Value>>,
    generated: Vec<GeneratedEffect>,
}

pub(crate) fn bind_effect_params(
    effect: &CompiledEffect,
    params: &IndexMap<Identifier, Value>,
) -> BoundEffectParams {
    BoundEffectParams {
        values: effect
            .params
            .iter()
            .map(|param| resolve_param(param, params))
            .collect(),
    }
}

pub(crate) fn run_sample_effect(
    effect: &CompiledEffect,
    params: &BoundEffectParams,
    context: &RunContext,
    scratch: &mut EffectVmScratch,
) -> Result<Color, RuntimeError> {
    if effect.kind != EffectKind::Sample {
        return Err(RuntimeError::new("cannot sample generator effect"));
    }
    let mut vm = Vm::new(
        &effect.function,
        params,
        VmContext::Sample(context),
        scratch,
    );
    match vm.run()? {
        RuntimeValue::Color(color) => Ok(color),
        other => Err(RuntimeError::new(format!(
            "`sample` returned non-color value {other:?}"
        ))),
    }
}

pub(crate) fn run_generator_effect(
    effect: &CompiledEffect,
    params: &BoundEffectParams,
    context: &GeneratorContext,
    scratch: &mut EffectVmScratch,
) -> Result<Vec<GeneratedEffect>, RuntimeError> {
    if effect.kind != EffectKind::Generator {
        return Err(RuntimeError::new("cannot generate sample effect"));
    }
    let mut vm = Vm::new(
        &effect.function,
        params,
        VmContext::Generator(context),
        scratch,
    );
    let _ = vm.run()?;
    Ok(std::mem::take(&mut vm.scratch.generated))
}

#[derive(Clone, Debug)]
enum RuntimeValue {
    Void,
    Int(i64),
    Float(f64),
    Bool(bool),
    Color(Color),
    Marks(Arc<Marks>),
    Timeline,
    Target(Arc<TargetValue>),
    TargetItems(Arc<TargetItemsValue>),
    TargetItem(Arc<TargetItemValue>),
    Curve(Arc<Curve>),
    Array(Arc<Vec<Value>>),
    Enum(Identifier),
    Param(usize),
}

impl RuntimeValue {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::Void => Self::Void,
            Value::Int(value) => Self::Int(*value),
            Value::Float(value) => Self::Float(*value),
            Value::Bool(value) => Self::Bool(*value),
            Value::Color(value) => Self::Color(*value),
            Value::Marks(value) => Self::Marks(Arc::clone(value)),
            Value::Target(value) => Self::Target(Arc::clone(value)),
            Value::TargetItems(value) => Self::TargetItems(Arc::clone(value)),
            Value::TargetItem(value) => Self::TargetItem(Arc::clone(value)),
            Value::Curve(value) => Self::Curve(Arc::clone(value)),
            Value::Array(value) => Self::Array(Arc::clone(value)),
            Value::Enum(value) => Self::Enum(value.clone()),
        }
    }
}

struct Vm<'a> {
    function: &'a BytecodeFunction,
    params: &'a BoundEffectParams,
    context: VmContext<'a>,
    scratch: &'a mut EffectVmScratch,
    ip: usize,
    loop_iterations: usize,
}

#[derive(Clone, Copy)]
enum VmContext<'a> {
    Sample(&'a RunContext),
    Generator(&'a GeneratorContext),
}

impl<'a> Vm<'a> {
    fn new(
        function: &'a BytecodeFunction,
        params: &'a BoundEffectParams,
        context: VmContext<'a>,
        scratch: &'a mut EffectVmScratch,
    ) -> Self {
        scratch.stack.clear();
        scratch.stack.reserve(function.max_stack);
        scratch.locals.clear();
        scratch.locals.resize(function.local_count, Value::Void);
        scratch.param_overrides.clear();
        scratch.param_overrides.resize(params.values.len(), None);
        scratch.generated.clear();
        Self {
            function,
            params,
            context,
            scratch,
            ip: 0,
            loop_iterations: 0,
        }
    }

    fn run(&mut self) -> Result<RuntimeValue, RuntimeError> {
        loop {
            let Some(instruction) = self.function.instructions.get(self.ip) else {
                return Err(RuntimeError::new("function completed without return"));
            };
            self.ip += 1;
            match instruction {
                Instruction::LoadConst(index) => self.push_constant(*index)?,
                Instruction::LoadDefault(ty) => {
                    self.push(RuntimeValue::from_value(&default_value(ty)))
                }
                Instruction::LoadParam(index) => self.push_param(*index)?,
                Instruction::LoadGeneratorContext(slot) => self.load_generator_context(*slot)?,
                Instruction::StoreParam(index) => {
                    let value = self.pop_to_value()?;
                    let Some(slot) = self.scratch.param_overrides.get_mut(*index) else {
                        return Err(RuntimeError::new("invalid param slot"));
                    };
                    *slot = Some(value);
                }
                Instruction::LoadLocal(index) => {
                    let value = self
                        .scratch
                        .locals
                        .get(*index)
                        .ok_or_else(|| RuntimeError::new("invalid local slot"))?;
                    self.push(RuntimeValue::from_value(value));
                }
                Instruction::StoreLocal(index) => {
                    let value = self.pop_to_value()?;
                    let Some(slot) = self.scratch.locals.get_mut(*index) else {
                        return Err(RuntimeError::new("invalid local slot"));
                    };
                    *slot = value;
                }
                Instruction::Pop => {
                    let _ = self.pop()?;
                }
                Instruction::MakeArray(count) => self.make_array(*count)?,
                Instruction::Index => self.index()?,
                Instruction::Member(member) => self.member(member)?,
                Instruction::CoerceFloat => self.coerce_float()?,
                Instruction::Unary(op) => self.unary(*op)?,
                Instruction::Binary(op) => self.binary(*op)?,
                Instruction::Jump(target) => self.ip = *target,
                Instruction::JumpIfFalse(target) => {
                    let value = self.pop()?;
                    if !to_bool_runtime(&value, self.params)? {
                        self.ip = *target;
                    }
                }
                Instruction::JumpIfFalseOrPop(target) => {
                    if !to_bool_runtime(self.peek()?, self.params)? {
                        self.ip = *target;
                    } else {
                        let _ = self.pop()?;
                    }
                }
                Instruction::JumpIfTrueOrPop(target) => {
                    if to_bool_runtime(self.peek()?, self.params)? {
                        self.ip = *target;
                    } else {
                        let _ = self.pop()?;
                    }
                }
                Instruction::CallBuiltin(builtin, arity) => self.call_builtin(*builtin, *arity)?,
                Instruction::CheckLoopLimit => {
                    self.loop_iterations += 1;
                    if self.loop_iterations > LOOP_ITERATION_LIMIT {
                        return Err(RuntimeError::new("loop iteration limit exceeded"));
                    }
                }
                Instruction::Emit { effect, fields } => self.emit_generated(effect, fields)?,
                Instruction::Return => return self.pop(),
            }
        }
    }

    fn load_generator_context(&mut self, slot: GeneratorContextId) -> Result<(), RuntimeError> {
        let VmContext::Generator(context) = self.context else {
            return Err(RuntimeError::new("generator context is unavailable"));
        };
        match slot {
            GeneratorContextId::Timeline => self.push(RuntimeValue::Timeline),
            GeneratorContextId::Target => {
                self.push(RuntimeValue::Target(Arc::clone(&context.target)))
            }
            GeneratorContextId::Duration => self.push(RuntimeValue::Float(context.duration)),
        }
        Ok(())
    }

    fn push_constant(&mut self, index: usize) -> Result<(), RuntimeError> {
        let value = self
            .function
            .constants
            .get(index)
            .ok_or_else(|| RuntimeError::new("invalid constant slot"))?;
        self.push(RuntimeValue::from_value(value));
        Ok(())
    }

    fn push_param(&mut self, index: usize) -> Result<(), RuntimeError> {
        if let Some(Some(value)) = self.scratch.param_overrides.get(index) {
            self.push(RuntimeValue::from_value(value));
            return Ok(());
        }
        let value = self
            .params
            .values
            .get(index)
            .ok_or_else(|| RuntimeError::new("invalid param slot"))?;
        self.push(match value {
            Value::Void => RuntimeValue::Void,
            Value::Int(value) => RuntimeValue::Int(*value),
            Value::Float(value) => RuntimeValue::Float(*value),
            Value::Bool(value) => RuntimeValue::Bool(*value),
            Value::Color(value) => RuntimeValue::Color(*value),
            Value::Marks(_)
            | Value::Target(_)
            | Value::TargetItems(_)
            | Value::TargetItem(_)
            | Value::Curve(_)
            | Value::Array(_)
            | Value::Enum(_) => RuntimeValue::Param(index),
        });
        Ok(())
    }

    fn make_array(&mut self, count: usize) -> Result<(), RuntimeError> {
        if self.scratch.stack.len() < count {
            return Err(RuntimeError::new("stack underflow"));
        }
        let start = self.scratch.stack.len() - count;
        let values = self
            .scratch
            .stack
            .drain(start..)
            .map(|value| runtime_to_value(value, self.params))
            .collect();
        self.push(RuntimeValue::Array(Arc::new(values)));
        Ok(())
    }

    fn index(&mut self) -> Result<(), RuntimeError> {
        let index = self.pop()?;
        let target = self.pop()?;
        match target {
            RuntimeValue::Array(items) => {
                let index = to_int_runtime(&index, self.params)?;
                let index = usize::try_from(index)
                    .map_err(|_| RuntimeError::new("array index cannot be negative"))?;
                let value = items
                    .get(index)
                    .cloned()
                    .ok_or_else(|| RuntimeError::new("array index out of bounds"))?;
                self.push(RuntimeValue::from_value(&value));
                Ok(())
            }
            RuntimeValue::TargetItems(items) => {
                let index = usize::try_from(to_int_runtime(&index, self.params)?)
                    .map_err(|_| RuntimeError::new("target item index cannot be negative"))?;
                let value = items
                    .groups
                    .get(index)
                    .cloned()
                    .ok_or_else(|| RuntimeError::new("target item index out of bounds"))?;
                self.push(RuntimeValue::TargetItem(value));
                Ok(())
            }
            RuntimeValue::Param(param_index) => {
                let value = self.param_value(param_index)?;
                match value {
                    Value::Array(items) => {
                        let index = to_int_runtime(&index, self.params)?;
                        let index = usize::try_from(index)
                            .map_err(|_| RuntimeError::new("array index cannot be negative"))?;
                        let value = items
                            .get(index)
                            .cloned()
                            .ok_or_else(|| RuntimeError::new("array index out of bounds"))?;
                        self.push(RuntimeValue::from_value(&value));
                        Ok(())
                    }
                    Value::Curve(curve) => {
                        let position = to_float_runtime(&index, self.params)?;
                        self.push(RuntimeValue::from_value(&sample_curve(curve, position)));
                        Ok(())
                    }
                    _ => Err(RuntimeError::new("index target is not an array or curve")),
                }
            }
            RuntimeValue::Curve(curve) => {
                let position = to_float_runtime(&index, self.params)?;
                self.push(RuntimeValue::from_value(&sample_curve(&curve, position)));
                Ok(())
            }
            _ => Err(RuntimeError::new("index target is not an array or curve")),
        }
    }

    fn member(&mut self, member: &Identifier) -> Result<(), RuntimeError> {
        let target = self.pop()?;
        let RuntimeValue::TargetItem(item) = target else {
            return Err(RuntimeError::new("member access requires TargetItem"));
        };
        let Some(pixel) = item.pixels.first() else {
            return Err(RuntimeError::new("empty TargetItem has no fields"));
        };
        let value = match member.as_str() {
            "fixture_index" => RuntimeValue::Int(pixel.fixture_index),
            "fixture_pixel_index" => RuntimeValue::Int(pixel.fixture_pixel_index),
            "pixel_index" => RuntimeValue::Int(pixel.pixel_index),
            "pixel_count" => RuntimeValue::Int(pixel.pixel_count),
            "pixel_fraction" => RuntimeValue::Float(pixel.pixel_fraction),
            _ => return Err(RuntimeError::new("unknown TargetItem member")),
        };
        self.push(value);
        Ok(())
    }

    fn coerce_float(&mut self) -> Result<(), RuntimeError> {
        let value = self.pop()?;
        self.push(match value {
            RuntimeValue::Int(value) => RuntimeValue::Float(value as f64),
            value => value,
        });
        Ok(())
    }

    fn unary(&mut self, op: UnaryOp) -> Result<(), RuntimeError> {
        let value = self.pop()?;
        let result = match op {
            UnaryOp::Negate => match value {
                RuntimeValue::Int(value) => RuntimeValue::Int(-value),
                RuntimeValue::Float(value) => RuntimeValue::Float(-value),
                _ => return Err(RuntimeError::new("unary `-` requires a number")),
            },
            UnaryOp::Not => RuntimeValue::Bool(!to_bool_runtime(&value, self.params)?),
        };
        self.push(result);
        Ok(())
    }

    fn binary(&mut self, op: BinaryOp) -> Result<(), RuntimeError> {
        let right = self.pop()?;
        let left = self.pop()?;
        let result = match op {
            BinaryOp::Add => {
                numeric_binary(&left, &right, self.params, |left, right| left + right)?
            }
            BinaryOp::Subtract => {
                numeric_binary(&left, &right, self.params, |left, right| left - right)?
            }
            BinaryOp::Multiply => {
                numeric_binary(&left, &right, self.params, |left, right| left * right)?
            }
            BinaryOp::Divide => {
                numeric_binary(&left, &right, self.params, |left, right| left / right)?
            }
            BinaryOp::Remainder => {
                numeric_binary(&left, &right, self.params, |left, right| left % right)?
            }
            BinaryOp::Less => {
                compare_binary(&left, &right, self.params, |left, right| left < right)?
            }
            BinaryOp::LessEqual => {
                compare_binary(&left, &right, self.params, |left, right| left <= right)?
            }
            BinaryOp::Greater => {
                compare_binary(&left, &right, self.params, |left, right| left > right)?
            }
            BinaryOp::GreaterEqual => {
                compare_binary(&left, &right, self.params, |left, right| left >= right)?
            }
            BinaryOp::Equal => RuntimeValue::Bool(values_equal(&left, &right, self.params)),
            BinaryOp::NotEqual => RuntimeValue::Bool(!values_equal(&left, &right, self.params)),
            BinaryOp::And | BinaryOp::Or => {
                return Err(RuntimeError::new("invalid boolean operator path"));
            }
        };
        self.push(result);
        Ok(())
    }

    fn call_builtin(&mut self, builtin: Builtin, arity: usize) -> Result<(), RuntimeError> {
        if self.scratch.stack.len() < arity {
            return Err(RuntimeError::new("stack underflow"));
        }
        let args_start = self.scratch.stack.len() - arity;
        let value = call_builtin(
            builtin,
            &self.scratch.stack[args_start..],
            self.context,
            self.params,
        )?;
        self.scratch.stack.truncate(args_start);
        self.push(value);
        Ok(())
    }

    fn emit_generated(
        &mut self,
        effect: &Identifier,
        fields: &[Identifier],
    ) -> Result<(), RuntimeError> {
        if self.scratch.stack.len() < fields.len() {
            return Err(RuntimeError::new("stack underflow"));
        }
        let start = self.scratch.stack.len() - fields.len();
        let values = self.scratch.stack.drain(start..).collect::<Vec<_>>();
        let mut start_seconds = None;
        let mut duration_seconds = None;
        let mut target = None;
        let mut params = IndexMap::new();
        for (field, value) in fields.iter().zip(values) {
            match field.as_str() {
                "start" => start_seconds = Some(to_float_runtime(&value, self.params)?),
                "duration" => duration_seconds = Some(to_float_runtime(&value, self.params)?),
                "target" => {
                    target = Some(match value {
                        RuntimeValue::TargetItem(item) => item,
                        RuntimeValue::TargetItems(items) => {
                            if items.groups.len() == 1 {
                                Arc::clone(&items.groups[0])
                            } else {
                                let pixels = items
                                    .groups
                                    .iter()
                                    .flat_map(|item| item.pixels.iter().copied())
                                    .collect();
                                Arc::new(TargetItemValue {
                                    pixels: Arc::new(pixels),
                                })
                            }
                        }
                        RuntimeValue::Target(target) => {
                            if target.groups.len() == 1 {
                                Arc::clone(&target.groups[0])
                            } else {
                                let pixels = target
                                    .groups
                                    .iter()
                                    .flat_map(|item| item.pixels.iter().copied())
                                    .collect();
                                Arc::new(TargetItemValue {
                                    pixels: Arc::new(pixels),
                                })
                            }
                        }
                        _ => return Err(RuntimeError::new("emit target must be target items")),
                    });
                }
                _ => {
                    params.insert(field.clone(), runtime_to_value(value, self.params));
                }
            }
        }
        self.scratch.generated.push(GeneratedEffect {
            definition: effect.clone(),
            start_seconds: start_seconds.ok_or_else(|| RuntimeError::new("emit missing start"))?,
            duration_seconds: duration_seconds
                .ok_or_else(|| RuntimeError::new("emit missing duration"))?,
            target: target.ok_or_else(|| RuntimeError::new("emit missing target"))?,
            params,
        });
        Ok(())
    }

    fn push(&mut self, value: RuntimeValue) {
        self.scratch.stack.push(value);
    }

    fn pop(&mut self) -> Result<RuntimeValue, RuntimeError> {
        self.scratch
            .stack
            .pop()
            .ok_or_else(|| RuntimeError::new("stack underflow"))
    }

    fn pop_to_value(&mut self) -> Result<Value, RuntimeError> {
        self.pop().map(|value| runtime_to_value(value, self.params))
    }

    fn param_value(&self, index: usize) -> Result<&Value, RuntimeError> {
        self.params
            .values
            .get(index)
            .ok_or_else(|| RuntimeError::new("invalid param slot"))
    }

    fn peek(&self) -> Result<&RuntimeValue, RuntimeError> {
        self.scratch
            .stack
            .last()
            .ok_or_else(|| RuntimeError::new("stack underflow"))
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
        Type::Marks => Value::Marks(Arc::new(Marks { marks: Vec::new() })),
        Type::Timeline => Value::Void,
        Type::Target => Value::Target(Arc::new(TargetValue { groups: Vec::new() })),
        Type::TargetItems => Value::TargetItems(Arc::new(TargetItemsValue { groups: Vec::new() })),
        Type::TargetItem => Value::TargetItem(Arc::new(TargetItemValue {
            pixels: Arc::new(Vec::new()),
        })),
        Type::Curve(_) => Value::Curve(Arc::new(Curve { points: Vec::new() })),
        Type::Array(_) => Value::Array(Arc::new(Vec::new())),
        Type::Enum(options) => options
            .first()
            .cloned()
            .map(Value::Enum)
            .unwrap_or(Value::Void),
    }
}

fn value_at(args: &[RuntimeValue], index: usize) -> Result<&RuntimeValue, RuntimeError> {
    args.get(index)
        .ok_or_else(|| RuntimeError::new("missing argument"))
}

fn runtime_to_value(value: RuntimeValue, params: &BoundEffectParams) -> Value {
    match value {
        RuntimeValue::Void => Value::Void,
        RuntimeValue::Int(value) => Value::Int(value),
        RuntimeValue::Float(value) => Value::Float(value),
        RuntimeValue::Bool(value) => Value::Bool(value),
        RuntimeValue::Color(value) => Value::Color(value),
        RuntimeValue::Marks(value) => Value::Marks(value),
        RuntimeValue::Timeline => Value::Void,
        RuntimeValue::Target(value) => Value::Target(value),
        RuntimeValue::TargetItems(value) => Value::TargetItems(value),
        RuntimeValue::TargetItem(value) => Value::TargetItem(value),
        RuntimeValue::Curve(value) => Value::Curve(value),
        RuntimeValue::Array(value) => Value::Array(value),
        RuntimeValue::Enum(value) => Value::Enum(value),
        RuntimeValue::Param(index) => params.values.get(index).cloned().unwrap_or(Value::Void),
    }
}

fn call_builtin(
    builtin: Builtin,
    args: &[RuntimeValue],
    context: VmContext<'_>,
    params: &BoundEffectParams,
) -> Result<RuntimeValue, RuntimeError> {
    match builtin {
        Builtin::Progress => Ok(RuntimeValue::Float(sample_context(context)?.progress)),
        Builtin::Seconds => Ok(RuntimeValue::Float(sample_context(context)?.seconds)),
        Builtin::Duration => Ok(RuntimeValue::Float(match context {
            VmContext::Sample(context) => context.duration,
            VmContext::Generator(context) => context.duration,
        })),
        Builtin::PixelIndex => Ok(RuntimeValue::Int(sample_context(context)?.pixel_index)),
        Builtin::PixelCount => Ok(RuntimeValue::Int(sample_context(context)?.pixel_count)),
        Builtin::PixelFraction => Ok(RuntimeValue::Float(sample_context(context)?.pixel_fraction)),
        Builtin::SectionPosition => {
            let width = to_float_runtime(value_at(args, 0)?, params)?.max(1.0);
            let index = sample_context(context)?.pixel_index as f64;
            Ok(RuntimeValue::Float(
                (index - (index / width).floor() * width) / width,
            ))
        }
        Builtin::Sin => Ok(RuntimeValue::Float(
            to_float_runtime(value_at(args, 0)?, params)?.sin(),
        )),
        Builtin::Cos => Ok(RuntimeValue::Float(
            to_float_runtime(value_at(args, 0)?, params)?.cos(),
        )),
        Builtin::Abs => Ok(RuntimeValue::Float(
            to_float_runtime(value_at(args, 0)?, params)?.abs(),
        )),
        Builtin::Floor => Ok(RuntimeValue::Float(
            to_float_runtime(value_at(args, 0)?, params)?.floor(),
        )),
        Builtin::Min => Ok(RuntimeValue::Float(
            to_float_runtime(value_at(args, 0)?, params)?
                .min(to_float_runtime(value_at(args, 1)?, params)?),
        )),
        Builtin::Max => Ok(RuntimeValue::Float(
            to_float_runtime(value_at(args, 0)?, params)?
                .max(to_float_runtime(value_at(args, 1)?, params)?),
        )),
        Builtin::Clamp => Ok(RuntimeValue::Float(
            to_float_runtime(value_at(args, 0)?, params)?.clamp(
                to_float_runtime(value_at(args, 1)?, params)?,
                to_float_runtime(value_at(args, 2)?, params)?,
            ),
        )),
        Builtin::Smoothstep => {
            let edge0 = to_float_runtime(value_at(args, 0)?, params)?;
            let edge1 = to_float_runtime(value_at(args, 1)?, params)?;
            let x = to_float_runtime(value_at(args, 2)?, params)?;
            let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
            Ok(RuntimeValue::Float(t * t * (3.0 - 2.0 * t)))
        }
        Builtin::Mix => mix_values(
            value_at(args, 0)?,
            value_at(args, 1)?,
            to_float_runtime(value_at(args, 2)?, params)?,
            params,
        ),
        Builtin::Rgb => Ok(RuntimeValue::Color(Color {
            red: channel(to_float_runtime(value_at(args, 0)?, params)?),
            green: channel(to_float_runtime(value_at(args, 1)?, params)?),
            blue: channel(to_float_runtime(value_at(args, 2)?, params)?),
        })),
        Builtin::Hsv => Ok(RuntimeValue::Color(hsv(
            to_float_runtime(value_at(args, 0)?, params)?,
            to_float_runtime(value_at(args, 1)?, params)?,
            to_float_runtime(value_at(args, 2)?, params)?,
        ))),
        Builtin::Srand | Builtin::Rand => Ok(RuntimeValue::Float(random(args, params)?)),
        Builtin::CurveCrossing => Ok(RuntimeValue::Float(to_float_runtime(
            value_at(args, 1)?,
            params,
        )?)),
        Builtin::CurveFloatClamped => {
            let curve = to_curve_runtime(value_at(args, 0)?, params)?;
            let position = to_float_runtime(value_at(args, 1)?, params)?;
            let min = to_float_runtime(value_at(args, 2)?, params)?;
            let max = to_float_runtime(value_at(args, 3)?, params)?;
            Ok(RuntimeValue::Float(
                to_float_runtime(
                    &RuntimeValue::from_value(&sample_curve(curve, position)),
                    params,
                )?
                .clamp(min, max),
            ))
        }
        Builtin::CurveColorScaled => {
            let curve = to_curve_runtime(value_at(args, 0)?, params)?;
            let position = to_float_runtime(value_at(args, 1)?, params)?;
            let scale = to_float_runtime(value_at(args, 2)?, params)?.clamp(0.0, 1.0);
            let Value::Color(color) = sample_curve(curve, position) else {
                return Err(RuntimeError::new("curve_color_scaled requires color curve"));
            };
            Ok(RuntimeValue::Color(scale_color(color, scale)))
        }
        Builtin::Len => match value_at(args, 0)? {
            RuntimeValue::Array(items) => i64::try_from(items.len())
                .map(RuntimeValue::Int)
                .map_err(|_| RuntimeError::new("array length exceeds int range")),
            RuntimeValue::Marks(marks) => i64::try_from(marks.marks.len())
                .map(RuntimeValue::Int)
                .map_err(|_| RuntimeError::new("mark count exceeds int range")),
            RuntimeValue::Param(index) => match params.values.get(*index) {
                Some(Value::Array(items)) => i64::try_from(items.len())
                    .map(RuntimeValue::Int)
                    .map_err(|_| RuntimeError::new("array length exceeds int range")),
                Some(Value::Marks(marks)) => i64::try_from(marks.marks.len())
                    .map(RuntimeValue::Int)
                    .map_err(|_| RuntimeError::new("mark count exceeds int range")),
                _ => Err(RuntimeError::new("len requires array or marks")),
            },
            _ => Err(RuntimeError::new("len requires array or marks")),
        },
        Builtin::MarkCount => Ok(RuntimeValue::Int(mark_count(args, context, params)?)),
        Builtin::MarkAt => Ok(RuntimeValue::Float(mark_at(args, context, params)?)),
        Builtin::MarkPrev => Ok(RuntimeValue::Float(mark_prev(args, context, params)?)),
        Builtin::MarkPrevIndex => Ok(RuntimeValue::Int(mark_prev_index(args, context, params)?)),
        Builtin::MarkNextIndex => Ok(RuntimeValue::Int(mark_next_index(args, context, params)?)),
        Builtin::MarkElapsed => Ok(RuntimeValue::Float(mark_elapsed(args, context, params)?)),
        Builtin::MarkPhase => Ok(RuntimeValue::Float(mark_phase(args, context, params)?)),
        Builtin::Fixtures => Ok(RuntimeValue::TargetItems(Arc::new(fixtures(value_at(
            args, 0,
        )?)?))),
        Builtin::Pixels => Ok(RuntimeValue::TargetItems(Arc::new(pixels(value_at(
            args, 0,
        )?)?))),
        Builtin::Sections => Ok(RuntimeValue::TargetItems(
            sections(
                value_at(args, 0)?,
                to_float_runtime(value_at(args, 1)?, params)?,
            )?
            .into(),
        )),
        Builtin::Count => Ok(RuntimeValue::Int(
            target_items(value_at(args, 0)?)?.groups.len() as i64,
        )),
        Builtin::Pick => {
            let items = target_items(value_at(args, 0)?)?;
            let index = usize::try_from(to_int_runtime(value_at(args, 1)?, params)?)
                .map_err(|_| RuntimeError::new("target item index cannot be negative"))?;
            Ok(RuntimeValue::TargetItem(
                items
                    .groups
                    .get(index)
                    .cloned()
                    .ok_or_else(|| RuntimeError::new("target item index out of bounds"))?,
            ))
        }
    }
}

fn sample_context(context: VmContext<'_>) -> Result<&RunContext, RuntimeError> {
    match context {
        VmContext::Sample(context) => Ok(context),
        VmContext::Generator(_) => Err(RuntimeError::new("sample context is unavailable")),
    }
}

fn to_bool_runtime(value: &RuntimeValue, params: &BoundEffectParams) -> Result<bool, RuntimeError> {
    match value {
        RuntimeValue::Bool(value) => Ok(*value),
        RuntimeValue::Param(index) => match params.values.get(*index) {
            Some(Value::Bool(value)) => Ok(*value),
            _ => Err(RuntimeError::new("expected bool")),
        },
        _ => Err(RuntimeError::new("expected bool")),
    }
}

fn to_int_runtime(value: &RuntimeValue, params: &BoundEffectParams) -> Result<i64, RuntimeError> {
    match value {
        RuntimeValue::Int(value) => Ok(*value),
        RuntimeValue::Float(value) => Ok(*value as i64),
        RuntimeValue::Param(index) => match params.values.get(*index) {
            Some(Value::Int(value)) => Ok(*value),
            Some(Value::Float(value)) => Ok(*value as i64),
            _ => Err(RuntimeError::new("expected int")),
        },
        _ => Err(RuntimeError::new("expected int")),
    }
}

fn to_float_runtime(value: &RuntimeValue, params: &BoundEffectParams) -> Result<f64, RuntimeError> {
    match value {
        RuntimeValue::Int(value) => Ok(*value as f64),
        RuntimeValue::Float(value) => Ok(*value),
        RuntimeValue::Param(index) => match params.values.get(*index) {
            Some(Value::Int(value)) => Ok(*value as f64),
            Some(Value::Float(value)) => Ok(*value),
            _ => Err(RuntimeError::new("expected float")),
        },
        _ => Err(RuntimeError::new("expected float")),
    }
}

fn to_curve_runtime<'a>(
    value: &'a RuntimeValue,
    params: &'a BoundEffectParams,
) -> Result<&'a Curve, RuntimeError> {
    match value {
        RuntimeValue::Curve(curve) => Ok(curve),
        RuntimeValue::Param(index) => match params.values.get(*index) {
            Some(Value::Curve(curve)) => Ok(curve),
            _ => Err(RuntimeError::new("expected curve")),
        },
        _ => Err(RuntimeError::new("expected curve")),
    }
}

fn target_items(value: &RuntimeValue) -> Result<&TargetItemsValue, RuntimeError> {
    match value {
        RuntimeValue::TargetItems(items) => Ok(items),
        _ => Err(RuntimeError::new("expected TargetItems")),
    }
}

fn target_groups(value: &RuntimeValue) -> Result<Vec<Arc<TargetItemValue>>, RuntimeError> {
    match value {
        RuntimeValue::Target(target) => Ok(target.groups.clone()),
        RuntimeValue::TargetItems(items) => Ok(items.groups.clone()),
        RuntimeValue::TargetItem(item) => Ok(vec![item.clone()]),
        _ => Err(RuntimeError::new("expected target")),
    }
}

fn fixtures(value: &RuntimeValue) -> Result<TargetItemsValue, RuntimeError> {
    let pixels = target_groups(value)?
        .into_iter()
        .flat_map(|item| item.pixels.iter().copied().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut raw_groups: Vec<Vec<TargetPixelValue>> = Vec::new();
    for pixel in pixels {
        if raw_groups
            .last()
            .and_then(|group| group.first())
            .is_some_and(|first| first.fixture_index == pixel.fixture_index)
        {
            if let Some(group) = raw_groups.last_mut() {
                group.push(pixel);
            }
        } else {
            raw_groups.push(vec![pixel]);
        }
    }
    let groups = raw_groups
        .into_iter()
        .map(|pixels| {
            Arc::new(TargetItemValue {
                pixels: Arc::new(pixels),
            })
        })
        .collect();
    Ok(TargetItemsValue { groups })
}

fn pixels(value: &RuntimeValue) -> Result<TargetItemsValue, RuntimeError> {
    Ok(TargetItemsValue {
        groups: target_groups(value)?
            .into_iter()
            .flat_map(|item| item.pixels.iter().copied().collect::<Vec<_>>())
            .map(|pixel| {
                Arc::new(TargetItemValue {
                    pixels: Arc::new(vec![pixel]),
                })
            })
            .collect(),
    })
}

fn sections(value: &RuntimeValue, width: f64) -> Result<TargetItemsValue, RuntimeError> {
    let width = width.max(1.0).floor() as i64;
    let mut raw_groups: Vec<Vec<TargetPixelValue>> = Vec::new();
    for pixel in target_groups(value)?
        .into_iter()
        .flat_map(|item| item.pixels.iter().copied().collect::<Vec<_>>())
    {
        if raw_groups
            .last()
            .and_then(|group| group.first())
            .is_some_and(|first| {
                first.fixture_index == pixel.fixture_index
                    && first.fixture_pixel_index / width == pixel.fixture_pixel_index / width
            })
        {
            if let Some(group) = raw_groups.last_mut() {
                group.push(pixel);
            }
        } else {
            raw_groups.push(vec![pixel]);
        }
    }
    let groups = raw_groups
        .into_iter()
        .map(|pixels| {
            Arc::new(TargetItemValue {
                pixels: Arc::new(pixels),
            })
        })
        .collect();
    Ok(TargetItemsValue { groups })
}

fn numeric_binary(
    left: &RuntimeValue,
    right: &RuntimeValue,
    params: &BoundEffectParams,
    op: impl FnOnce(f64, f64) -> f64,
) -> Result<RuntimeValue, RuntimeError> {
    let result = op(
        to_float_runtime(left, params)?,
        to_float_runtime(right, params)?,
    );
    if value_is_int(left, params) && value_is_int(right, params) && result.fract() == 0.0 {
        return Ok(RuntimeValue::Int(result as i64));
    }
    Ok(RuntimeValue::Float(result))
}

fn compare_binary(
    left: &RuntimeValue,
    right: &RuntimeValue,
    params: &BoundEffectParams,
    op: impl FnOnce(f64, f64) -> bool,
) -> Result<RuntimeValue, RuntimeError> {
    Ok(RuntimeValue::Bool(op(
        to_float_runtime(left, params)?,
        to_float_runtime(right, params)?,
    )))
}

fn value_is_int(value: &RuntimeValue, params: &BoundEffectParams) -> bool {
    match value {
        RuntimeValue::Int(_) => true,
        RuntimeValue::Param(index) => matches!(params.values.get(*index), Some(Value::Int(_))),
        _ => false,
    }
}

fn values_equal(left: &RuntimeValue, right: &RuntimeValue, params: &BoundEffectParams) -> bool {
    match (left, right) {
        (RuntimeValue::Void, RuntimeValue::Void) => true,
        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left == right,
        (RuntimeValue::Float(left), RuntimeValue::Float(right)) => left == right,
        (RuntimeValue::Int(left), RuntimeValue::Float(right)) => (*left as f64) == *right,
        (RuntimeValue::Float(left), RuntimeValue::Int(right)) => *left == (*right as f64),
        (RuntimeValue::Bool(left), RuntimeValue::Bool(right)) => left == right,
        (RuntimeValue::Color(left), RuntimeValue::Color(right)) => left == right,
        (RuntimeValue::Enum(left), RuntimeValue::Enum(right)) => left == right,
        (RuntimeValue::Param(left), RuntimeValue::Param(right)) => {
            params.values.get(*left) == params.values.get(*right)
        }
        (RuntimeValue::Param(index), value) | (value, RuntimeValue::Param(index)) => params
            .values
            .get(*index)
            .is_some_and(|param| values_equal(&RuntimeValue::from_value(param), value, params)),
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

fn mix_values(
    left: &RuntimeValue,
    right: &RuntimeValue,
    t: f64,
    params: &BoundEffectParams,
) -> Result<RuntimeValue, RuntimeError> {
    match (left, right) {
        (RuntimeValue::Float(left), RuntimeValue::Float(right)) => {
            Ok(RuntimeValue::Float(left + (right - left) * t))
        }
        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => Ok(RuntimeValue::Float(
            *left as f64 + (*right - *left) as f64 * t,
        )),
        (RuntimeValue::Color(left), RuntimeValue::Color(right)) => {
            mix_colors(*left, *right, t).map(RuntimeValue::Color)
        }
        (RuntimeValue::Param(_), _) | (_, RuntimeValue::Param(_)) => mix_values(
            &RuntimeValue::from_value(&runtime_to_value(left.clone(), params)),
            &RuntimeValue::from_value(&runtime_to_value(right.clone(), params)),
            t,
            params,
        ),
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

fn random(args: &[RuntimeValue], params: &BoundEffectParams) -> Result<f64, RuntimeError> {
    let mut seed = 0.0;
    for arg in args {
        seed = seed * 31.0 + to_float_runtime(arg, params)?;
    }
    Ok((seed.sin() * 43_758.545_312_3).fract().abs())
}

fn mark_source<'a>(
    args: &'a [RuntimeValue],
    params: &'a BoundEffectParams,
) -> Result<&'a Marks, RuntimeError> {
    let Some(value) = args.first() else {
        return Err(RuntimeError::new(
            "mark builtin requires marks as the first argument",
        ));
    };
    match value {
        RuntimeValue::Marks(marks) => Ok(marks),
        RuntimeValue::Param(index) => match params.values.get(*index) {
            Some(Value::Marks(marks)) => Ok(marks),
            _ => Err(RuntimeError::new("mark builtin first arg must be marks")),
        },
        _ => Err(RuntimeError::new("mark builtin first arg must be marks")),
    }
}

fn mark_count(
    args: &[RuntimeValue],
    context: VmContext<'_>,
    params: &BoundEffectParams,
) -> Result<i64, RuntimeError> {
    let _ = context;
    i64::try_from(mark_source(args, params)?.marks.len())
        .map_err(|_| RuntimeError::new("mark count exceeds int range"))
}

fn mark_at(
    args: &[RuntimeValue],
    _context: VmContext<'_>,
    params: &BoundEffectParams,
) -> Result<f64, RuntimeError> {
    let marks = mark_source(args, params)?;
    let index_arg = if first_arg_is_marks(args, params) {
        1
    } else {
        0
    };
    let index = to_int_runtime(value_at(args, index_arg)?, params)?;
    let fallback = args
        .get(index_arg + 1)
        .map(|value| to_float_runtime(value, params))
        .transpose()?
        .unwrap_or(0.0);
    mark_at_from(marks, index, fallback)
}

fn mark_prev(
    args: &[RuntimeValue],
    context: VmContext<'_>,
    params: &BoundEffectParams,
) -> Result<f64, RuntimeError> {
    let marks = mark_source(args, params)?;
    let seconds = mark_query_seconds(args, context, 1, params)?;
    let fallback = args
        .get(2)
        .map(|value| to_float_runtime(value, params))
        .transpose()?
        .unwrap_or(0.0);
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

fn mark_prev_index(
    args: &[RuntimeValue],
    context: VmContext<'_>,
    params: &BoundEffectParams,
) -> Result<i64, RuntimeError> {
    let seconds = mark_query_seconds(args, context, 1, params)?;
    prev_index(mark_source(args, params)?, seconds)
}

fn mark_next_index(
    args: &[RuntimeValue],
    context: VmContext<'_>,
    params: &BoundEffectParams,
) -> Result<i64, RuntimeError> {
    let seconds = mark_query_seconds(args, context, 1, params)?;
    next_index(mark_source(args, params)?, seconds)
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

fn mark_elapsed(
    args: &[RuntimeValue],
    context: VmContext<'_>,
    params: &BoundEffectParams,
) -> Result<f64, RuntimeError> {
    let seconds = mark_query_seconds(args, context, 1, params)?;
    elapsed(mark_source(args, params)?, seconds)
}

fn elapsed(marks: &Marks, seconds: f64) -> Result<f64, RuntimeError> {
    let previous = prev_index(marks, seconds)?;
    if previous < 0 {
        return Ok(seconds);
    }
    Ok(seconds - mark_at_from(marks, previous, 0.0)?)
}

fn mark_phase(
    args: &[RuntimeValue],
    context: VmContext<'_>,
    params: &BoundEffectParams,
) -> Result<f64, RuntimeError> {
    let seconds = mark_query_seconds(args, context, 1, params)?;
    let duration = match context {
        VmContext::Sample(context) => context.duration,
        VmContext::Generator(context) => context.duration,
    };
    phase(mark_source(args, params)?, seconds, duration)
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
    args: &[RuntimeValue],
    context: VmContext<'_>,
    marks_arg_offset: usize,
    params: &BoundEffectParams,
) -> Result<f64, RuntimeError> {
    let default_seconds = match context {
        VmContext::Sample(context) => context.seconds,
        VmContext::Generator(_) => 0.0,
    };
    if first_arg_is_marks(args, params) {
        args.get(marks_arg_offset)
            .map(|value| to_float_runtime(value, params))
            .transpose()
            .map(|value| value.unwrap_or(default_seconds))
    } else {
        args.first()
            .map(|value| to_float_runtime(value, params))
            .transpose()
            .map(|value| value.unwrap_or(default_seconds))
    }
}

fn first_arg_is_marks(args: &[RuntimeValue], params: &BoundEffectParams) -> bool {
    match args.first() {
        Some(RuntimeValue::Marks(_)) => true,
        Some(RuntimeValue::Param(index)) => {
            matches!(params.values.get(*index), Some(Value::Marks(_)))
        }
        _ => false,
    }
}
