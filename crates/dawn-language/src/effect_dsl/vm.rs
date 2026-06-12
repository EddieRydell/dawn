use super::ast::{BinaryOp, UnaryOp};
use super::bytecode::{
    ContextRead, FloatBinary, FloatUnary, GeneratorContextId, Instruction, MarkOp,
    RegisterFunction, RegisterId, TargetItemsOp,
};
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
    values: Vec<BoundParamValue>,
}

#[derive(Clone, Debug)]
enum BoundParamValue {
    Void,
    Int(i64),
    Float(f64),
    Bool(bool),
    Color(Color),
    Marks(Arc<Marks>),
    Target(Arc<TargetValue>),
    TargetItems(Arc<TargetItemsValue>),
    TargetItem(Arc<TargetItemValue>),
    Curve(Arc<PreparedCurve>),
    Array(Arc<Vec<Value>>),
    Enum(Identifier),
}

impl BoundParamValue {
    fn from_value(ty: &Type, value: Value) -> Self {
        match value {
            Value::Void => Self::Void,
            Value::Int(value) => Self::Int(value),
            Value::Float(value) => Self::Float(value),
            Value::Bool(value) => Self::Bool(value),
            Value::Color(value) => Self::Color(value),
            Value::Marks(value) => Self::Marks(value),
            Value::Target(value) => Self::Target(value),
            Value::TargetItems(value) => Self::TargetItems(value),
            Value::TargetItem(value) => Self::TargetItem(value),
            Value::Curve(value) => Self::Curve(Arc::new(PreparedCurve::new(ty, value))),
            Value::Array(value) => Self::Array(value),
            Value::Enum(value) => Self::Enum(value),
        }
    }

    fn to_runtime(&self) -> RuntimeValue {
        match self {
            Self::Void => RuntimeValue::Void,
            Self::Int(value) => RuntimeValue::Int(*value),
            Self::Float(value) => RuntimeValue::Float(*value),
            Self::Bool(value) => RuntimeValue::Bool(*value),
            Self::Color(value) => RuntimeValue::Color(*value),
            Self::Marks(value) => RuntimeValue::Marks(Arc::clone(value)),
            Self::Target(value) => RuntimeValue::Target(Arc::clone(value)),
            Self::TargetItems(value) => RuntimeValue::TargetItems(Arc::clone(value)),
            Self::TargetItem(value) => RuntimeValue::TargetItem(Arc::clone(value)),
            Self::Curve(value) => RuntimeValue::PreparedCurve(Arc::clone(value)),
            Self::Array(value) => RuntimeValue::Array(Arc::clone(value)),
            Self::Enum(value) => RuntimeValue::Enum(value.clone()),
        }
    }
}

#[derive(Clone, Debug)]
enum PreparedCurve {
    Float {
        raw: Arc<Curve>,
        segments: Vec<FloatCurveSegment>,
        crossings: PreparedCurveCrossings,
    },
    Color {
        raw: Arc<Curve>,
        segments: Vec<ColorCurveSegment>,
    },
}

#[derive(Clone, Copy, Debug)]
struct FloatCurveSegment {
    start_position: f64,
    end_position: f64,
    start_value: f64,
    end_value: f64,
}

#[derive(Clone, Copy, Debug)]
struct ColorCurveSegment {
    start_position: f64,
    end_position: f64,
    start_value: Color,
    end_value: Color,
}

#[derive(Clone, Debug)]
enum PreparedCurveCrossings {
    Increasing(Vec<CrossingSegment>),
    Decreasing(Vec<CrossingSegment>),
    Mixed(Vec<CrossingSegment>),
}

#[derive(Clone, Copy, Debug)]
struct CrossingSegment {
    start_position: f64,
    end_position: f64,
    start_value: f64,
    end_value: f64,
}

impl PreparedCurve {
    fn new(ty: &Type, raw: Arc<Curve>) -> Self {
        match ty {
            Type::Curve(inner) if matches!(inner.as_ref(), Type::Color) => Self::Color {
                segments: prepare_color_segments(&raw),
                raw,
            },
            _ => {
                let segments = prepare_float_segments(&raw);
                let crossings = prepare_curve_crossings(&segments);
                Self::Float {
                    raw,
                    segments,
                    crossings,
                }
            }
        }
    }

    fn raw(&self) -> Arc<Curve> {
        match self {
            Self::Float { raw, .. } | Self::Color { raw, .. } => Arc::clone(raw),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct EffectVmScratch {
    registers: Vec<RuntimeValue>,
    param_overrides: Vec<Option<RuntimeValue>>,
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
            .map(|param| bind_param_value(&param.ty, resolve_param(param, params)))
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
    PreparedCurve(Arc<PreparedCurve>),
    Array(Arc<Vec<Value>>),
    Enum(Identifier),
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
    function: &'a RegisterFunction,
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
        function: &'a RegisterFunction,
        params: &'a BoundEffectParams,
        context: VmContext<'a>,
        scratch: &'a mut EffectVmScratch,
    ) -> Self {
        let register_count = function.register_types.len();
        debug_assert_eq!(register_count, function.register_count);
        scratch.registers.clear();
        scratch.registers.resize(register_count, RuntimeValue::Void);
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
                Instruction::LoadConst { dst, constant } => {
                    let value = self
                        .function
                        .constants
                        .get(*constant)
                        .ok_or_else(|| RuntimeError::new("invalid constant slot"))?;
                    self.set(*dst, RuntimeValue::from_value(value))?;
                }
                Instruction::LoadDefault { dst, ty } => {
                    self.set(*dst, RuntimeValue::from_value(&default_value(ty)))?;
                }
                Instruction::LoadParam { dst, param } => {
                    let value = self.param_value(*param)?;
                    self.set(*dst, value)?;
                }
                Instruction::LoadGeneratorContext { dst, slot } => {
                    let value = self.generator_context_value(*slot)?;
                    self.set(*dst, value)?;
                }
                Instruction::StoreParam { param, src } => {
                    let value = self.get(*src)?.clone();
                    let Some(slot) = self.scratch.param_overrides.get_mut(*param) else {
                        return Err(RuntimeError::new("invalid param slot"));
                    };
                    *slot = Some(value);
                }
                Instruction::Move { dst, src } => {
                    let value = self.get(*src)?.clone();
                    self.set(*dst, value)?;
                }
                Instruction::MakeArray { dst, items } => {
                    let values = items
                        .iter()
                        .map(|item| self.get(*item).cloned().map(runtime_to_value))
                        .collect::<Result<Vec<_>, _>>()?;
                    self.set(*dst, RuntimeValue::Array(Arc::new(values)))?;
                }
                Instruction::Index { dst, target, index } => {
                    let value = self.index_value(self.get(*target)?, self.get(*index)?)?;
                    self.set(*dst, value)?;
                }
                Instruction::CurveParamSample {
                    dst,
                    param,
                    position,
                } => {
                    let position = to_float_runtime(self.get(*position)?, self.params)?;
                    let curve = self.prepared_curve_param(*param)?;
                    self.set(*dst, sample_prepared_curve(curve, position)?)?;
                }
                Instruction::Member {
                    dst,
                    target,
                    member,
                } => {
                    let value = member_value(self.get(*target)?, member)?;
                    self.set(*dst, value)?;
                }
                Instruction::CoerceFloat { dst, src } => {
                    let value = match self.get(*src)?.clone() {
                        RuntimeValue::Int(value) => RuntimeValue::Float(value as f64),
                        value => value,
                    };
                    self.set(*dst, value)?;
                }
                Instruction::Unary { dst, op, src } => {
                    let value = self.unary_value(*op, self.get(*src)?)?;
                    self.set(*dst, value)?;
                }
                Instruction::Binary {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let value = self.binary_value(*op, self.get(*left)?, self.get(*right)?)?;
                    self.set(*dst, value)?;
                }
                Instruction::Jump(target) => self.ip = *target,
                Instruction::JumpIfFalse { condition, target } => {
                    if !to_bool_runtime(self.get(*condition)?, self.params)? {
                        self.ip = *target;
                    }
                }
                Instruction::JumpIfTrue { condition, target } => {
                    if to_bool_runtime(self.get(*condition)?, self.params)? {
                        self.ip = *target;
                    }
                }
                Instruction::ContextRead { dst, read } => {
                    let value = self.context_read(*read)?;
                    self.set(*dst, value)?;
                }
                Instruction::SectionPosition { dst, width } => {
                    let width = to_float_runtime(self.get(*width)?, self.params)?.max(1.0);
                    let index = sample_context(self.context)?.pixel_index as f64;
                    self.set(
                        *dst,
                        RuntimeValue::Float((index - (index / width).floor() * width) / width),
                    )?;
                }
                Instruction::FloatUnary { dst, op, value } => {
                    let value = to_float_runtime(self.get(*value)?, self.params)?;
                    let result = match op {
                        FloatUnary::Sin => value.sin(),
                        FloatUnary::Cos => value.cos(),
                        FloatUnary::Abs => value.abs(),
                        FloatUnary::Floor => value.floor(),
                    };
                    self.set(*dst, RuntimeValue::Float(result))?;
                }
                Instruction::FloatBinary {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = to_float_runtime(self.get(*left)?, self.params)?;
                    let right = to_float_runtime(self.get(*right)?, self.params)?;
                    let result = match op {
                        FloatBinary::Min => left.min(right),
                        FloatBinary::Max => left.max(right),
                    };
                    self.set(*dst, RuntimeValue::Float(result))?;
                }
                Instruction::Clamp {
                    dst,
                    value,
                    min,
                    max,
                } => {
                    let value = to_float_runtime(self.get(*value)?, self.params)?;
                    let min = to_float_runtime(self.get(*min)?, self.params)?;
                    let max = to_float_runtime(self.get(*max)?, self.params)?;
                    self.set(*dst, RuntimeValue::Float(value.clamp(min, max)))?;
                }
                Instruction::Smoothstep {
                    dst,
                    edge0,
                    edge1,
                    value,
                } => {
                    let edge0 = to_float_runtime(self.get(*edge0)?, self.params)?;
                    let edge1 = to_float_runtime(self.get(*edge1)?, self.params)?;
                    let value = to_float_runtime(self.get(*value)?, self.params)?;
                    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                    self.set(*dst, RuntimeValue::Float(t * t * (3.0 - 2.0 * t)))?;
                }
                Instruction::Mix {
                    dst,
                    left,
                    right,
                    amount,
                } => {
                    let amount = to_float_runtime(self.get(*amount)?, self.params)?;
                    let value =
                        mix_values(self.get(*left)?, self.get(*right)?, amount, self.params)?;
                    self.set(*dst, value)?;
                }
                Instruction::Rgb {
                    dst,
                    red,
                    green,
                    blue,
                } => {
                    self.set(
                        *dst,
                        RuntimeValue::Color(Color {
                            red: channel(to_float_runtime(self.get(*red)?, self.params)?),
                            green: channel(to_float_runtime(self.get(*green)?, self.params)?),
                            blue: channel(to_float_runtime(self.get(*blue)?, self.params)?),
                        }),
                    )?;
                }
                Instruction::Hsv {
                    dst,
                    hue,
                    saturation,
                    value,
                } => {
                    self.set(
                        *dst,
                        RuntimeValue::Color(hsv(
                            to_float_runtime(self.get(*hue)?, self.params)?,
                            to_float_runtime(self.get(*saturation)?, self.params)?,
                            to_float_runtime(self.get(*value)?, self.params)?,
                        )),
                    )?;
                }
                Instruction::Rand { dst, args } => {
                    let args = self.collect_args(args)?;
                    self.set(*dst, RuntimeValue::Float(random(&args, self.params)?))?;
                }
                Instruction::CurveFloatClamped {
                    dst,
                    curve,
                    position,
                    min,
                    max,
                } => {
                    let curve = to_curve_runtime(self.get(*curve)?, self.params)?;
                    let position = to_float_runtime(self.get(*position)?, self.params)?;
                    let min = to_float_runtime(self.get(*min)?, self.params)?;
                    let max = to_float_runtime(self.get(*max)?, self.params)?;
                    let sampled = RuntimeValue::from_value(&sample_curve(curve, position));
                    self.set(
                        *dst,
                        RuntimeValue::Float(
                            to_float_runtime(&sampled, self.params)?.clamp(min, max),
                        ),
                    )?;
                }
                Instruction::CurveParamFloatClamped {
                    dst,
                    param,
                    position,
                    min,
                    max,
                } => {
                    let position = to_float_runtime(self.get(*position)?, self.params)?;
                    let min = to_float_runtime(self.get(*min)?, self.params)?;
                    let max = to_float_runtime(self.get(*max)?, self.params)?;
                    let curve = self.prepared_curve_param(*param)?;
                    let value = sample_prepared_float_curve(curve, position)?.clamp(min, max);
                    self.set(*dst, RuntimeValue::Float(value))?;
                }
                Instruction::CurveColorScaled {
                    dst,
                    curve,
                    position,
                    scale,
                } => {
                    let scale = to_float_runtime(self.get(*scale)?, self.params)?.clamp(0.0, 1.0);
                    if scale <= 0.0 {
                        self.set(*dst, RuntimeValue::Color(black()))?;
                    } else {
                        let curve = to_curve_runtime(self.get(*curve)?, self.params)?;
                        let position = to_float_runtime(self.get(*position)?, self.params)?;
                        let Value::Color(color) = sample_curve(curve, position) else {
                            return Err(RuntimeError::new(
                                "curve_color_scaled requires color curve",
                            ));
                        };
                        self.set(*dst, RuntimeValue::Color(scale_color(color, scale)))?;
                    }
                }
                Instruction::CurveParamColorScaled {
                    dst,
                    param,
                    position,
                    scale,
                } => {
                    let scale = to_float_runtime(self.get(*scale)?, self.params)?.clamp(0.0, 1.0);
                    if scale <= 0.0 {
                        self.set(*dst, RuntimeValue::Color(black()))?;
                    } else {
                        let position = to_float_runtime(self.get(*position)?, self.params)?;
                        let curve = self.prepared_curve_param(*param)?;
                        let color = sample_prepared_color_curve(curve, position)?;
                        self.set(*dst, RuntimeValue::Color(scale_color(color, scale)))?;
                    }
                }
                Instruction::CurveCrossing {
                    dst,
                    curve,
                    value,
                    fallback,
                } => {
                    let curve = to_curve_runtime(self.get(*curve)?, self.params)?;
                    let value = to_float_runtime(self.get(*value)?, self.params)?;
                    let fallback = fallback
                        .map(|fallback| to_float_runtime(self.get(fallback)?, self.params))
                        .transpose()?
                        .unwrap_or(value);
                    self.set(
                        *dst,
                        RuntimeValue::Float(curve_crossing_raw(curve, value, fallback)),
                    )?;
                }
                Instruction::CurveParamCrossing {
                    dst,
                    param,
                    value,
                    fallback,
                } => {
                    let value = to_float_runtime(self.get(*value)?, self.params)?;
                    let fallback = fallback
                        .map(|fallback| to_float_runtime(self.get(fallback)?, self.params))
                        .transpose()?
                        .unwrap_or(value);
                    let curve = self.prepared_curve_param(*param)?;
                    self.set(
                        *dst,
                        RuntimeValue::Float(curve_crossing(curve, value, fallback)?),
                    )?;
                }
                Instruction::Len { dst, value } => {
                    let value = match self.get(*value)? {
                        RuntimeValue::Array(items) => i64::try_from(items.len())
                            .map(RuntimeValue::Int)
                            .map_err(|_| RuntimeError::new("array length exceeds int range"))?,
                        RuntimeValue::Marks(marks) => i64::try_from(marks.marks.len())
                            .map(RuntimeValue::Int)
                            .map_err(|_| RuntimeError::new("mark count exceeds int range"))?,
                        _ => return Err(RuntimeError::new("len requires array or marks")),
                    };
                    self.set(*dst, value)?;
                }
                Instruction::Mark { dst, op, args } => {
                    let args = self.collect_args(args)?;
                    let value = match op {
                        MarkOp::Count => {
                            RuntimeValue::Int(mark_count(&args, self.context, self.params)?)
                        }
                        MarkOp::At => {
                            RuntimeValue::Float(mark_at(&args, self.context, self.params)?)
                        }
                        MarkOp::Prev => {
                            RuntimeValue::Float(mark_prev(&args, self.context, self.params)?)
                        }
                        MarkOp::PrevIndex => {
                            RuntimeValue::Int(mark_prev_index(&args, self.context, self.params)?)
                        }
                        MarkOp::NextIndex => {
                            RuntimeValue::Int(mark_next_index(&args, self.context, self.params)?)
                        }
                        MarkOp::Elapsed => {
                            RuntimeValue::Float(mark_elapsed(&args, self.context, self.params)?)
                        }
                        MarkOp::Phase => {
                            RuntimeValue::Float(mark_phase(&args, self.context, self.params)?)
                        }
                    };
                    self.set(*dst, value)?;
                }
                Instruction::TargetItems { dst, op, args } => {
                    let args = self.collect_args(args)?;
                    let value = match op {
                        TargetItemsOp::Fixtures => {
                            RuntimeValue::TargetItems(Arc::new(fixtures(value_at(&args, 0)?)?))
                        }
                        TargetItemsOp::Pixels => {
                            RuntimeValue::TargetItems(Arc::new(pixels(value_at(&args, 0)?)?))
                        }
                        TargetItemsOp::Sections => RuntimeValue::TargetItems(Arc::new(sections(
                            value_at(&args, 0)?,
                            to_float_runtime(value_at(&args, 1)?, self.params)?,
                        )?)),
                        TargetItemsOp::Count => {
                            RuntimeValue::Int(target_items(value_at(&args, 0)?)?.groups.len() as i64)
                        }
                        TargetItemsOp::Pick => {
                            let items = target_items(value_at(&args, 0)?)?;
                            let index =
                                usize::try_from(to_int_runtime(value_at(&args, 1)?, self.params)?)
                                    .map_err(|_| {
                                        RuntimeError::new("target item index cannot be negative")
                                    })?;
                            RuntimeValue::TargetItem(items.groups.get(index).cloned().ok_or_else(
                                || RuntimeError::new("target item index out of bounds"),
                            )?)
                        }
                    };
                    self.set(*dst, value)?;
                }
                Instruction::CheckLoopLimit => {
                    self.loop_iterations += 1;
                    if self.loop_iterations > LOOP_ITERATION_LIMIT {
                        return Err(RuntimeError::new("loop iteration limit exceeded"));
                    }
                }
                Instruction::Emit { effect, fields } => self.emit_generated(effect, fields)?,
                Instruction::Return(src) => return Ok(self.get(*src)?.clone()),
            }
        }
    }

    fn get(&self, register: RegisterId) -> Result<&RuntimeValue, RuntimeError> {
        self.scratch
            .registers
            .get(register)
            .ok_or_else(|| RuntimeError::new("invalid register slot"))
    }

    fn set(&mut self, register: RegisterId, value: RuntimeValue) -> Result<(), RuntimeError> {
        let Some(slot) = self.scratch.registers.get_mut(register) else {
            return Err(RuntimeError::new("invalid register slot"));
        };
        *slot = value;
        Ok(())
    }

    fn collect_args(&self, args: &[RegisterId]) -> Result<Vec<RuntimeValue>, RuntimeError> {
        args.iter().map(|arg| self.get(*arg).cloned()).collect()
    }

    fn param_value(&self, index: usize) -> Result<RuntimeValue, RuntimeError> {
        if let Some(Some(value)) = self.scratch.param_overrides.get(index) {
            return Ok(value.clone());
        }
        self.params
            .values
            .get(index)
            .ok_or_else(|| RuntimeError::new("invalid param slot"))
            .map(BoundParamValue::to_runtime)
    }

    fn generator_context_value(
        &self,
        slot: GeneratorContextId,
    ) -> Result<RuntimeValue, RuntimeError> {
        let VmContext::Generator(context) = self.context else {
            return Err(RuntimeError::new("generator context is unavailable"));
        };
        Ok(match slot {
            GeneratorContextId::Timeline => RuntimeValue::Timeline,
            GeneratorContextId::Target => RuntimeValue::Target(Arc::clone(&context.target)),
            GeneratorContextId::Duration => RuntimeValue::Float(context.duration),
        })
    }

    fn context_read(&self, read: ContextRead) -> Result<RuntimeValue, RuntimeError> {
        Ok(match read {
            ContextRead::Progress => RuntimeValue::Float(sample_context(self.context)?.progress),
            ContextRead::Seconds => RuntimeValue::Float(sample_context(self.context)?.seconds),
            ContextRead::Duration => RuntimeValue::Float(match self.context {
                VmContext::Sample(context) => context.duration,
                VmContext::Generator(context) => context.duration,
            }),
            ContextRead::PixelIndex => RuntimeValue::Int(sample_context(self.context)?.pixel_index),
            ContextRead::PixelCount => RuntimeValue::Int(sample_context(self.context)?.pixel_count),
            ContextRead::PixelFraction => {
                RuntimeValue::Float(sample_context(self.context)?.pixel_fraction)
            }
        })
    }

    fn index_value(
        &self,
        target: &RuntimeValue,
        index: &RuntimeValue,
    ) -> Result<RuntimeValue, RuntimeError> {
        match target {
            RuntimeValue::Array(items) => {
                let index = usize::try_from(to_int_runtime(index, self.params)?)
                    .map_err(|_| RuntimeError::new("array index cannot be negative"))?;
                let value = items
                    .get(index)
                    .cloned()
                    .ok_or_else(|| RuntimeError::new("array index out of bounds"))?;
                Ok(RuntimeValue::from_value(&value))
            }
            RuntimeValue::TargetItems(items) => {
                let index = usize::try_from(to_int_runtime(index, self.params)?)
                    .map_err(|_| RuntimeError::new("target item index cannot be negative"))?;
                let value = items
                    .groups
                    .get(index)
                    .cloned()
                    .ok_or_else(|| RuntimeError::new("target item index out of bounds"))?;
                Ok(RuntimeValue::TargetItem(value))
            }
            RuntimeValue::Curve(curve) => {
                let position = to_float_runtime(index, self.params)?;
                Ok(RuntimeValue::from_value(&sample_curve(curve, position)))
            }
            RuntimeValue::PreparedCurve(curve) => {
                let position = to_float_runtime(index, self.params)?;
                sample_prepared_curve(curve, position)
            }
            _ => Err(RuntimeError::new("index target is not an array or curve")),
        }
    }

    fn prepared_curve_param(&self, param: usize) -> Result<&PreparedCurve, RuntimeError> {
        match self.scratch.param_overrides.get(param) {
            Some(Some(RuntimeValue::PreparedCurve(curve))) => return Ok(curve),
            Some(Some(RuntimeValue::Curve(_))) => {
                return Err(RuntimeError::new("unprepared curve param override"));
            }
            Some(Some(_)) => return Err(RuntimeError::new("expected curve")),
            _ => {}
        }
        match self.params.values.get(param) {
            Some(BoundParamValue::Curve(curve)) => Ok(curve),
            Some(_) => Err(RuntimeError::new("expected curve")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn unary_value(&self, op: UnaryOp, value: &RuntimeValue) -> Result<RuntimeValue, RuntimeError> {
        Ok(match op {
            UnaryOp::Negate => match value {
                RuntimeValue::Int(value) => RuntimeValue::Int(-value),
                RuntimeValue::Float(value) => RuntimeValue::Float(-value),
                _ => return Err(RuntimeError::new("unary `-` requires a number")),
            },
            UnaryOp::Not => RuntimeValue::Bool(!to_bool_runtime(value, self.params)?),
        })
    }

    fn binary_value(
        &self,
        op: BinaryOp,
        left: &RuntimeValue,
        right: &RuntimeValue,
    ) -> Result<RuntimeValue, RuntimeError> {
        match op {
            BinaryOp::Add => numeric_binary(left, right, self.params, |left, right| left + right),
            BinaryOp::Subtract => {
                numeric_binary(left, right, self.params, |left, right| left - right)
            }
            BinaryOp::Multiply => {
                numeric_binary(left, right, self.params, |left, right| left * right)
            }
            BinaryOp::Divide => {
                numeric_binary(left, right, self.params, |left, right| left / right)
            }
            BinaryOp::Remainder => {
                numeric_binary(left, right, self.params, |left, right| left % right)
            }
            BinaryOp::Less => compare_binary(left, right, self.params, |left, right| left < right),
            BinaryOp::LessEqual => {
                compare_binary(left, right, self.params, |left, right| left <= right)
            }
            BinaryOp::Greater => {
                compare_binary(left, right, self.params, |left, right| left > right)
            }
            BinaryOp::GreaterEqual => {
                compare_binary(left, right, self.params, |left, right| left >= right)
            }
            BinaryOp::Equal => Ok(RuntimeValue::Bool(values_equal(left, right, self.params))),
            BinaryOp::NotEqual => Ok(RuntimeValue::Bool(!values_equal(left, right, self.params))),
            BinaryOp::And | BinaryOp::Or => Err(RuntimeError::new("invalid boolean operator path")),
        }
    }

    fn emit_generated(
        &mut self,
        effect: &Identifier,
        fields: &[(Identifier, RegisterId)],
    ) -> Result<(), RuntimeError> {
        let mut start_seconds = None;
        let mut duration_seconds = None;
        let mut target = None;
        let mut params = IndexMap::new();
        for (field, register) in fields {
            let value = self.get(*register)?.clone();
            match field.as_str() {
                "start" => start_seconds = Some(to_float_runtime(&value, self.params)?),
                "duration" => duration_seconds = Some(to_float_runtime(&value, self.params)?),
                "target" => {
                    target = Some(match value {
                        RuntimeValue::TargetItem(item) => item,
                        RuntimeValue::TargetItems(items) => target_item_from_groups(&items.groups),
                        RuntimeValue::Target(target) => target_item_from_groups(&target.groups),
                        _ => return Err(RuntimeError::new("emit target must be target items")),
                    });
                }
                _ => {
                    params.insert(field.clone(), runtime_to_value(value));
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

fn bind_param_value(ty: &Type, value: Value) -> BoundParamValue {
    match (ty, value) {
        (Type::Float, Value::Int(value)) => BoundParamValue::Float(value as f64),
        (ty, value) => BoundParamValue::from_value(ty, value),
    }
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

fn runtime_to_value(value: RuntimeValue) -> Value {
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
        RuntimeValue::PreparedCurve(value) => Value::Curve(value.raw()),
        RuntimeValue::Array(value) => Value::Array(value),
        RuntimeValue::Enum(value) => Value::Enum(value),
    }
}

fn member_value(target: &RuntimeValue, member: &Identifier) -> Result<RuntimeValue, RuntimeError> {
    let RuntimeValue::TargetItem(item) = target else {
        return Err(RuntimeError::new("member access requires TargetItem"));
    };
    let Some(pixel) = item.pixels.first() else {
        return Err(RuntimeError::new("empty TargetItem has no fields"));
    };
    Ok(match member.as_str() {
        "fixture_index" => RuntimeValue::Int(pixel.fixture_index),
        "fixture_pixel_index" => RuntimeValue::Int(pixel.fixture_pixel_index),
        "pixel_index" => RuntimeValue::Int(pixel.pixel_index),
        "pixel_count" => RuntimeValue::Int(pixel.pixel_count),
        "pixel_fraction" => RuntimeValue::Float(pixel.pixel_fraction),
        _ => return Err(RuntimeError::new("unknown TargetItem member")),
    })
}

fn target_item_from_groups(groups: &[Arc<TargetItemValue>]) -> Arc<TargetItemValue> {
    if groups.len() == 1 {
        return Arc::clone(&groups[0]);
    }
    let pixels = groups
        .iter()
        .flat_map(|item| item.pixels.iter().copied())
        .collect();
    Arc::new(TargetItemValue {
        pixels: Arc::new(pixels),
    })
}

fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}

fn sample_context(context: VmContext<'_>) -> Result<&RunContext, RuntimeError> {
    match context {
        VmContext::Sample(context) => Ok(context),
        VmContext::Generator(_) => Err(RuntimeError::new("sample context is unavailable")),
    }
}

fn to_bool_runtime(value: &RuntimeValue, params: &BoundEffectParams) -> Result<bool, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Bool(value) => Ok(*value),
        _ => Err(RuntimeError::new("expected bool")),
    }
}

fn to_int_runtime(value: &RuntimeValue, params: &BoundEffectParams) -> Result<i64, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Int(value) => Ok(*value),
        RuntimeValue::Float(value) => Ok(*value as i64),
        _ => Err(RuntimeError::new("expected int")),
    }
}

fn to_float_runtime(value: &RuntimeValue, params: &BoundEffectParams) -> Result<f64, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Int(value) => Ok(*value as f64),
        RuntimeValue::Float(value) => Ok(*value),
        _ => Err(RuntimeError::new("expected float")),
    }
}

fn to_curve_runtime<'a>(
    value: &'a RuntimeValue,
    params: &'a BoundEffectParams,
) -> Result<&'a Curve, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Curve(curve) => Ok(curve),
        RuntimeValue::PreparedCurve(curve) => match curve.as_ref() {
            PreparedCurve::Float { raw, .. } | PreparedCurve::Color { raw, .. } => Ok(raw),
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
    let _ = params;
    matches!(value, RuntimeValue::Int(_))
}

fn values_equal(left: &RuntimeValue, right: &RuntimeValue, params: &BoundEffectParams) -> bool {
    let _ = params;
    match (left, right) {
        (RuntimeValue::Void, RuntimeValue::Void) => true,
        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left == right,
        (RuntimeValue::Float(left), RuntimeValue::Float(right)) => left == right,
        (RuntimeValue::Int(left), RuntimeValue::Float(right)) => (*left as f64) == *right,
        (RuntimeValue::Float(left), RuntimeValue::Int(right)) => *left == (*right as f64),
        (RuntimeValue::Bool(left), RuntimeValue::Bool(right)) => left == right,
        (RuntimeValue::Color(left), RuntimeValue::Color(right)) => left == right,
        (RuntimeValue::Enum(left), RuntimeValue::Enum(right)) => left == right,
        _ => false,
    }
}

fn prepare_float_segments(curve: &Curve) -> Vec<FloatCurveSegment> {
    let Some(first) = curve.points.first() else {
        return Vec::new();
    };
    let mut previous = first;
    let mut segments = Vec::with_capacity(curve.points.len());
    for point in &curve.points {
        let (CurveValue::Float(start_value), CurveValue::Float(end_value)) =
            (&previous.value, &point.value)
        else {
            previous = point;
            continue;
        };
        segments.push(FloatCurveSegment {
            start_position: previous.position,
            end_position: point.position,
            start_value: *start_value,
            end_value: *end_value,
        });
        previous = point;
    }
    segments
}

fn prepare_color_segments(curve: &Curve) -> Vec<ColorCurveSegment> {
    let Some(first) = curve.points.first() else {
        return Vec::new();
    };
    let mut previous = first;
    let mut segments = Vec::with_capacity(curve.points.len());
    for point in &curve.points {
        let (CurveValue::Color(start_value), CurveValue::Color(end_value)) =
            (&previous.value, &point.value)
        else {
            previous = point;
            continue;
        };
        segments.push(ColorCurveSegment {
            start_position: previous.position,
            end_position: point.position,
            start_value: *start_value,
            end_value: *end_value,
        });
        previous = point;
    }
    segments
}

fn prepare_curve_crossings(segments: &[FloatCurveSegment]) -> PreparedCurveCrossings {
    let crossings = segments
        .iter()
        .map(|segment| CrossingSegment {
            start_position: segment.start_position,
            end_position: segment.end_position,
            start_value: segment.start_value,
            end_value: segment.end_value,
        })
        .collect::<Vec<_>>();
    let increasing = crossings
        .windows(2)
        .all(|pair| pair[0].end_value <= pair[1].end_value);
    let decreasing = crossings
        .windows(2)
        .all(|pair| pair[0].end_value >= pair[1].end_value);
    if increasing {
        PreparedCurveCrossings::Increasing(crossings)
    } else if decreasing {
        PreparedCurveCrossings::Decreasing(crossings)
    } else {
        PreparedCurveCrossings::Mixed(crossings)
    }
}

fn sample_prepared_curve(
    curve: &PreparedCurve,
    position: f64,
) -> Result<RuntimeValue, RuntimeError> {
    match curve {
        PreparedCurve::Float { .. } => {
            sample_prepared_float_curve(curve, position).map(RuntimeValue::Float)
        }
        PreparedCurve::Color { .. } => {
            sample_prepared_color_curve(curve, position).map(RuntimeValue::Color)
        }
    }
}

fn sample_prepared_float_curve(curve: &PreparedCurve, position: f64) -> Result<f64, RuntimeError> {
    let PreparedCurve::Float { segments, .. } = curve else {
        return Err(RuntimeError::new(
            "curve_float_clamped requires float curve",
        ));
    };
    let Some(segment) = find_position_segment(segments, position) else {
        return Ok(0.0);
    };
    Ok(mix_float_segment(
        segment.start_position,
        segment.end_position,
        segment.start_value,
        segment.end_value,
        position,
    ))
}

fn sample_prepared_color_curve(
    curve: &PreparedCurve,
    position: f64,
) -> Result<Color, RuntimeError> {
    let PreparedCurve::Color { segments, .. } = curve else {
        return Err(RuntimeError::new("curve_color_scaled requires color curve"));
    };
    let Some(segment) = find_position_segment(segments, position) else {
        return Err(RuntimeError::new("cannot sample empty color curve"));
    };
    mix_colors(
        segment.start_value,
        segment.end_value,
        segment_t(segment.start_position, segment.end_position, position),
    )
}

trait PositionSegment {
    fn end_position(&self) -> f64;
}

impl PositionSegment for FloatCurveSegment {
    fn end_position(&self) -> f64 {
        self.end_position
    }
}

impl PositionSegment for ColorCurveSegment {
    fn end_position(&self) -> f64 {
        self.end_position
    }
}

fn find_position_segment<T: PositionSegment>(segments: &[T], position: f64) -> Option<&T> {
    if segments.is_empty() {
        return None;
    }
    let index = segments.partition_point(|segment| segment.end_position() < position);
    segments.get(index).or_else(|| segments.last())
}

fn mix_float_segment(
    start_position: f64,
    end_position: f64,
    start_value: f64,
    end_value: f64,
    position: f64,
) -> f64 {
    start_value + (end_value - start_value) * segment_t(start_position, end_position, position)
}

fn segment_t(start_position: f64, end_position: f64, position: f64) -> f64 {
    let span = (end_position - start_position).max(0.000000001);
    ((position - start_position) / span).clamp(0.0, 1.0)
}

fn curve_crossing(curve: &PreparedCurve, value: f64, fallback: f64) -> Result<f64, RuntimeError> {
    let PreparedCurve::Float { crossings, .. } = curve else {
        return Err(RuntimeError::new("curve_crossing requires float curve"));
    };
    Ok(match crossings {
        PreparedCurveCrossings::Increasing(segments) => {
            let index = segments.partition_point(|segment| segment.end_value < value);
            crossing_at(segments.get(index), value).unwrap_or(fallback)
        }
        PreparedCurveCrossings::Decreasing(segments) => {
            let index = segments.partition_point(|segment| segment.end_value > value);
            crossing_at(segments.get(index), value).unwrap_or(fallback)
        }
        PreparedCurveCrossings::Mixed(segments) => segments
            .iter()
            .find_map(|segment| crossing_at(Some(segment), value))
            .unwrap_or(fallback),
    })
}

fn curve_crossing_raw(curve: &Curve, value: f64, fallback: f64) -> f64 {
    let segments = prepare_float_segments(curve);
    let crossings = prepare_curve_crossings(&segments);
    match crossings {
        PreparedCurveCrossings::Increasing(segments) => {
            let index = segments.partition_point(|segment| segment.end_value < value);
            crossing_at(segments.get(index), value).unwrap_or(fallback)
        }
        PreparedCurveCrossings::Decreasing(segments) => {
            let index = segments.partition_point(|segment| segment.end_value > value);
            crossing_at(segments.get(index), value).unwrap_or(fallback)
        }
        PreparedCurveCrossings::Mixed(segments) => segments
            .iter()
            .find_map(|segment| crossing_at(Some(segment), value))
            .unwrap_or(fallback),
    }
}

fn crossing_at(segment: Option<&CrossingSegment>, value: f64) -> Option<f64> {
    let segment = segment?;
    let min = segment.start_value.min(segment.end_value);
    let max = segment.start_value.max(segment.end_value);
    if value < min || value > max {
        return None;
    }
    let span = segment.end_value - segment.start_value;
    if span.abs() <= 0.000000001 {
        return Some(segment.start_position);
    }
    let t = ((value - segment.start_value) / span).clamp(0.0, 1.0);
    Some(segment.start_position + (segment.end_position - segment.start_position) * t)
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
    let _ = params;
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
    let _ = params;
    let Some(value) = args.first() else {
        return Err(RuntimeError::new(
            "mark builtin requires marks as the first argument",
        ));
    };
    match value {
        RuntimeValue::Marks(marks) => Ok(marks),
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
    let _ = params;
    matches!(args.first(), Some(RuntimeValue::Marks(_)))
}
