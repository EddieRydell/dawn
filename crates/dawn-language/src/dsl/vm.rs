use super::GeneratedEffectRef;
use super::bytecode::{
    ArithmeticOp, BoolSlot, ColorBinary, ColorSlot, CompareOp, ContextRead, FloatBinary, FloatSlot,
    FloatUnary, GeneratorContextId, Instruction, IntArithmeticOp, IntSlot, MarkOp, RefSlot,
    RegisterFunction, SlotLayout, TargetItemsOp, ValueSlot,
};
use super::types::{Identifier, Type, Value};
use super::types::{TargetItemValue, TargetItemsValue, TargetPixelValue, TargetValue};
use super::{CompiledEffect, CompiledOperator, EffectKind, ParamDecl};
use crate::values::{Color, Curve, Gradient, Marks};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

pub const MAX_VM_INSTRUCTIONS_PER_INVOCATION: usize = 10_000;

#[derive(Clone, Debug)]
pub struct RunContext {
    pub progress: f64,
    pub seconds: f64,
    pub duration: f64,
    pub pixel_index: i64,
    pub pixel_count: i64,
    pub pixel_fraction: f64,
    pub global_marks: Marks,
}

pub type OperatorRunContext = RunContext;

pub trait SignalSampler {
    fn sample_signal(
        &mut self,
        input: usize,
        seconds: f64,
        pixel_index: usize,
    ) -> Result<Color, RuntimeError>;
}

#[derive(Clone, Debug)]
pub struct GeneratorContext {
    pub duration: f64,
    pub target: Arc<TargetValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedEffect {
    pub definition: GeneratedEffectRef,
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
pub struct BoundParams {
    values: Vec<BoundParamValue>,
}

#[derive(Debug, Default)]
pub struct DslBindCache {
    curves: HashMap<usize, Arc<PreparedCurve>>,
    gradients: HashMap<usize, Arc<PreparedGradient>>,
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
    Gradient(Arc<PreparedGradient>),
    Array(Arc<Vec<Value>>),
    Enum(Identifier),
}

impl BoundParamValue {
    fn from_value(_ty: &Type, value: Value, cache: &mut DslBindCache) -> Self {
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
            Value::Curve(value) => Self::Curve(cache.prepared_curve(value)),
            Value::Gradient(value) => Self::Gradient(cache.prepared_gradient(value)),
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
            Self::Gradient(value) => RuntimeValue::PreparedGradient(Arc::clone(value)),
            Self::Array(value) => RuntimeValue::Array(Arc::clone(value)),
            Self::Enum(value) => RuntimeValue::Enum(value.clone()),
        }
    }
}

impl DslBindCache {
    fn prepared_curve(&mut self, raw: Arc<Curve>) -> Arc<PreparedCurve> {
        let key = Arc::as_ptr(&raw).cast::<()>() as usize;
        if let Some(curve) = self.curves.get(&key) {
            return Arc::clone(curve);
        }
        let curve = Arc::new(PreparedCurve::new(raw));
        self.curves.insert(key, Arc::clone(&curve));
        curve
    }

    fn prepared_gradient(&mut self, raw: Arc<Gradient>) -> Arc<PreparedGradient> {
        let key = Arc::as_ptr(&raw).cast::<()>() as usize;
        if let Some(gradient) = self.gradients.get(&key) {
            return Arc::clone(gradient);
        }
        let gradient = Arc::new(PreparedGradient::new(raw));
        self.gradients.insert(key, Arc::clone(&gradient));
        gradient
    }
}

#[derive(Clone, Debug)]
struct PreparedCurve {
    raw: Arc<Curve>,
    segments: Vec<CurveSegment>,
    crossings: PreparedCurveCrossings,
}

#[derive(Clone, Debug)]
struct PreparedGradient {
    raw: Arc<Gradient>,
    segments: Vec<GradientSegment>,
}

#[derive(Clone, Copy, Debug)]
struct CurveSegment {
    start_position: f64,
    end_position: f64,
    start_value: f64,
    end_value: f64,
}

#[derive(Clone, Copy, Debug)]
struct GradientSegment {
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
    fn new(raw: Arc<Curve>) -> Self {
        let segments = prepare_float_segments(&raw);
        let crossings = prepare_curve_crossings(&segments);
        Self {
            raw,
            segments,
            crossings,
        }
    }

    fn raw(&self) -> Arc<Curve> {
        Arc::clone(&self.raw)
    }
}

impl PreparedGradient {
    fn new(raw: Arc<Gradient>) -> Self {
        let segments = prepare_gradient_segments(&raw);
        Self { raw, segments }
    }

    fn raw(&self) -> Arc<Gradient> {
        Arc::clone(&self.raw)
    }
}

#[derive(Clone, Debug, Default)]
pub struct DslVmScratch {
    registers: VmRegisters,
    param_overrides: Vec<Option<RuntimeValue>>,
    dirty_param_overrides: Vec<usize>,
    generated: Vec<GeneratedEffect>,
}

#[derive(Clone, Debug, Default)]
struct VmRegisters {
    layout: Option<VmRegisterLayout>,
    ints: Vec<i64>,
    floats: Vec<f64>,
    bools: Vec<bool>,
    colors: Vec<Color>,
    refs: Vec<RuntimeValue>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VmRegisterLayout {
    id: u64,
    layout: SlotLayout,
}

impl VmRegisters {
    fn prepare(&mut self, function: &RegisterFunction) {
        let layout = VmRegisterLayout {
            id: function.layout_id,
            layout: function.layout,
        };
        if self.layout == Some(layout)
            && self.ints.len() == function.layout.ints
            && self.floats.len() == function.layout.floats
            && self.bools.len() == function.layout.bools
            && self.colors.len() == function.layout.colors
            && self.refs.len() == function.layout.refs
        {
            return;
        }
        self.layout = Some(layout);
        self.ints.clear();
        self.ints.resize(function.layout.ints, 0);
        self.floats.clear();
        self.floats.resize(function.layout.floats, 0.0);
        self.bools.clear();
        self.bools.resize(function.layout.bools, false);
        self.colors.clear();
        self.colors.resize(function.layout.colors, black());
        self.refs.clear();
        self.refs.resize(function.layout.refs, RuntimeValue::Void);
    }
}

pub(crate) fn bind_effect_params(
    effect: &CompiledEffect,
    params: &IndexMap<Identifier, Value>,
) -> Result<BoundParams, RuntimeError> {
    let mut cache = DslBindCache::default();
    bind_effect_params_cached(effect, params, &mut cache)
}

pub(crate) fn bind_effect_params_cached(
    effect: &CompiledEffect,
    params: &IndexMap<Identifier, Value>,
    cache: &mut DslBindCache,
) -> Result<BoundParams, RuntimeError> {
    Ok(BoundParams {
        values: effect
            .params
            .iter()
            .map(|param| {
                Ok(bind_param_value(
                    &param.ty,
                    resolve_param(param, params)?,
                    cache,
                ))
            })
            .collect::<Result<_, RuntimeError>>()?,
    })
}

pub(crate) fn bind_operator_params(
    operator: &CompiledOperator,
    params: &IndexMap<Identifier, Value>,
) -> Result<BoundParams, RuntimeError> {
    let mut cache = DslBindCache::default();
    bind_operator_params_cached(operator, params, &mut cache)
}

pub(crate) fn bind_operator_params_cached(
    operator: &CompiledOperator,
    params: &IndexMap<Identifier, Value>,
    cache: &mut DslBindCache,
) -> Result<BoundParams, RuntimeError> {
    Ok(BoundParams {
        values: operator
            .params
            .iter()
            .map(|param| {
                Ok(bind_param_value(
                    &param.ty,
                    resolve_param(param, params)?,
                    cache,
                ))
            })
            .collect::<Result<_, RuntimeError>>()?,
    })
}

pub(crate) fn run_sample_effect(
    effect: &CompiledEffect,
    params: &BoundParams,
    context: &RunContext,
    scratch: &mut DslVmScratch,
) -> Result<Color, RuntimeError> {
    if effect.kind != EffectKind::Sample {
        return Err(RuntimeError::new("cannot sample generator effect"));
    }
    let mut vm = Vm::new(
        &effect.function,
        params,
        VmContext::Sample(context),
        scratch,
        None,
    );
    vm.run_color()
}

pub(crate) fn run_generator_effect(
    effect: &CompiledEffect,
    params: &BoundParams,
    context: &GeneratorContext,
    scratch: &mut DslVmScratch,
) -> Result<Vec<GeneratedEffect>, RuntimeError> {
    if effect.kind != EffectKind::Generator {
        return Err(RuntimeError::new("cannot generate sample effect"));
    }
    let mut vm = Vm::new(
        &effect.function,
        params,
        VmContext::Generator(context),
        scratch,
        None,
    );
    let _ = vm.run()?;
    Ok(std::mem::take(&mut vm.scratch.generated))
}

pub(crate) fn run_operator(
    operator: &CompiledOperator,
    params: &BoundParams,
    context: &OperatorRunContext,
    sampler: &mut dyn SignalSampler,
    scratch: &mut DslVmScratch,
) -> Result<Color, RuntimeError> {
    let mut vm = Vm::new(
        &operator.function,
        params,
        VmContext::Sample(context),
        scratch,
        Some(sampler),
    );
    vm.run_color()
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
    Gradient(Arc<Gradient>),
    PreparedCurve(Arc<PreparedCurve>),
    PreparedGradient(Arc<PreparedGradient>),
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
            Value::Gradient(value) => Self::Gradient(Arc::clone(value)),
            Value::Array(value) => Self::Array(Arc::clone(value)),
            Value::Enum(value) => Self::Enum(value.clone()),
        }
    }
}

fn clone_runtime(value: &RuntimeValue) -> RuntimeValue {
    match value {
        RuntimeValue::Void => RuntimeValue::Void,
        RuntimeValue::Int(value) => RuntimeValue::Int(*value),
        RuntimeValue::Float(value) => RuntimeValue::Float(*value),
        RuntimeValue::Bool(value) => RuntimeValue::Bool(*value),
        RuntimeValue::Color(value) => RuntimeValue::Color(*value),
        RuntimeValue::Marks(value) => RuntimeValue::Marks(Arc::clone(value)),
        RuntimeValue::Timeline => RuntimeValue::Timeline,
        RuntimeValue::Target(value) => RuntimeValue::Target(Arc::clone(value)),
        RuntimeValue::TargetItems(value) => RuntimeValue::TargetItems(Arc::clone(value)),
        RuntimeValue::TargetItem(value) => RuntimeValue::TargetItem(Arc::clone(value)),
        RuntimeValue::Curve(value) => RuntimeValue::Curve(Arc::clone(value)),
        RuntimeValue::Gradient(value) => RuntimeValue::Gradient(Arc::clone(value)),
        RuntimeValue::PreparedCurve(value) => RuntimeValue::PreparedCurve(Arc::clone(value)),
        RuntimeValue::PreparedGradient(value) => RuntimeValue::PreparedGradient(Arc::clone(value)),
        RuntimeValue::Array(value) => RuntimeValue::Array(Arc::clone(value)),
        RuntimeValue::Enum(value) => RuntimeValue::Enum(value.clone()),
    }
}

struct Vm<'a> {
    function: &'a RegisterFunction,
    params: &'a BoundParams,
    context: VmContext<'a>,
    scratch: &'a mut DslVmScratch,
    ip: usize,
    loop_iterations: usize,
    signal_sampler: Option<&'a mut (dyn SignalSampler + 'a)>,
}

#[derive(Clone, Copy)]
enum VmContext<'a> {
    Sample(&'a RunContext),
    Generator(&'a GeneratorContext),
}

impl<'a> Vm<'a> {
    fn new(
        function: &'a RegisterFunction,
        params: &'a BoundParams,
        context: VmContext<'a>,
        scratch: &'a mut DslVmScratch,
        signal_sampler: Option<&'a mut (dyn SignalSampler + 'a)>,
    ) -> Self {
        scratch.registers.prepare(function);
        if scratch.param_overrides.len() != params.values.len() {
            scratch.param_overrides.clear();
            scratch.param_overrides.resize(params.values.len(), None);
            scratch.dirty_param_overrides.clear();
        } else {
            for index in scratch.dirty_param_overrides.drain(..) {
                if let Some(slot) = scratch.param_overrides.get_mut(index) {
                    *slot = None;
                }
            }
        }
        if matches!(context, VmContext::Generator(_)) {
            scratch.generated.clear();
        }
        Self {
            function,
            params,
            context,
            scratch,
            ip: 0,
            loop_iterations: 0,
            signal_sampler,
        }
    }

    fn run_color(&mut self) -> Result<Color, RuntimeError> {
        match self.run()? {
            RuntimeValue::Color(color) => Ok(color),
            other => Err(RuntimeError::new(format!(
                "`sample` returned non-color value {other:?}"
            ))),
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
                    self.set_const_value(*dst, value)?;
                }
                Instruction::LoadDefault { dst, ty } => {
                    self.set_value(*dst, RuntimeValue::from_value(&ty.default_value()))?;
                }
                Instruction::LoadIntParam { dst, param } => {
                    self.load_int_param(*dst, *param)?;
                }
                Instruction::LoadFloatParam { dst, param } => {
                    self.load_float_param(*dst, *param)?;
                }
                Instruction::LoadBoolParam { dst, param } => {
                    self.load_bool_param(*dst, *param)?;
                }
                Instruction::LoadColorParam { dst, param } => {
                    self.load_color_param(*dst, *param)?;
                }
                Instruction::LoadRefParam { dst, param } => {
                    self.load_ref_param(*dst, *param)?;
                }
                Instruction::LoadGeneratorContext { dst, slot } => {
                    let value = self.generator_context_value(*slot)?;
                    self.set_value(*dst, value)?;
                }
                Instruction::StoreParam { param, src } => {
                    let value = self.value(*src)?;
                    let Some(slot) = self.scratch.param_overrides.get_mut(*param) else {
                        return Err(RuntimeError::new("invalid param slot"));
                    };
                    if slot.is_none() {
                        self.scratch.dirty_param_overrides.push(*param);
                    }
                    *slot = Some(value);
                }
                Instruction::Move { dst, src } => {
                    self.copy_slot(*dst, *src)?;
                }
                Instruction::MakeArray { dst, items } => {
                    let values = items
                        .iter()
                        .map(|item| self.value(*item).map(runtime_to_value))
                        .collect::<Result<Vec<_>, _>>()?;
                    self.set_ref(*dst, RuntimeValue::Array(Arc::new(values)))?;
                }
                Instruction::Index { dst, target, index } => {
                    let target = self.value(*target)?;
                    let index = self.value(*index)?;
                    let value = self.index_value(&target, &index)?;
                    self.set_value(*dst, value)?;
                }
                Instruction::CurveParamSample {
                    dst,
                    param,
                    position,
                } => {
                    let position = self.float(*position)?;
                    let curve = self.prepared_curve_param(*param)?;
                    self.set_float(*dst, sample_prepared_curve(curve, position)?)?;
                }
                Instruction::SignalSample {
                    dst,
                    input,
                    seconds,
                } => {
                    let seconds = self.float(*seconds)?;
                    let pixel_index = usize::try_from(sample_context(self.context)?.pixel_index)
                        .map_err(|_| RuntimeError::new("pixel index cannot be negative"))?;
                    let sampler = self
                        .signal_sampler
                        .as_deref_mut()
                        .ok_or_else(|| RuntimeError::new("Signal sampler is unavailable"))?;
                    let color = sampler.sample_signal(*input, seconds, pixel_index)?;
                    self.set_color(*dst, color)?;
                }
                Instruction::Member {
                    dst,
                    target,
                    member,
                } => {
                    let value = member_value(self.ref_value(*target)?, member)?;
                    self.set_value(*dst, value)?;
                }
                Instruction::IntToFloat { dst, src } => {
                    self.set_float(*dst, self.int(*src)? as f64)?;
                }
                Instruction::Not { dst, src } => self.set_bool(*dst, !self.bool(*src)?)?,
                Instruction::NegInt { dst, src } => {
                    let value = self
                        .int(*src)?
                        .checked_neg()
                        .ok_or_else(|| RuntimeError::new("integer negation overflow"))?;
                    self.set_int(*dst, value)?;
                }
                Instruction::NegFloat { dst, src } => {
                    self.set_float(*dst, -self.float(*src)?)?;
                }
                Instruction::FloatArithmetic {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.float(*left)?;
                    let right = self.float(*right)?;
                    let value = match op {
                        ArithmeticOp::Add => left + right,
                        ArithmeticOp::Subtract => left - right,
                        ArithmeticOp::Multiply => left * right,
                        ArithmeticOp::Divide => left / right,
                        ArithmeticOp::Remainder => left % right,
                    };
                    self.set_float(*dst, value)?;
                }
                Instruction::FloatArithmeticConst {
                    dst,
                    op,
                    value,
                    constant_bits,
                    constant_left,
                } => {
                    let value = self.float(*value)?;
                    let constant = f64::from_bits(*constant_bits);
                    let (left, right) = if *constant_left {
                        (constant, value)
                    } else {
                        (value, constant)
                    };
                    let value = match op {
                        ArithmeticOp::Add => left + right,
                        ArithmeticOp::Subtract => left - right,
                        ArithmeticOp::Multiply => left * right,
                        ArithmeticOp::Divide => left / right,
                        ArithmeticOp::Remainder => left % right,
                    };
                    self.set_float(*dst, value)?;
                }
                Instruction::IntArithmetic {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.int(*left)?;
                    let right = self.int(*right)?;
                    let value = match op {
                        IntArithmeticOp::Add => left.checked_add(right),
                        IntArithmeticOp::Subtract => left.checked_sub(right),
                        IntArithmeticOp::Multiply => left.checked_mul(right),
                        IntArithmeticOp::Remainder => left.checked_rem(right),
                    };
                    let value = value.ok_or_else(|| {
                        RuntimeError::new("integer arithmetic overflow or division by zero")
                    })?;
                    self.set_int(*dst, value)?;
                }
                Instruction::FloatCompare {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.float(*left)?;
                    let right = self.float(*right)?;
                    let value = match op {
                        CompareOp::Less => left < right,
                        CompareOp::LessEqual => left <= right,
                        CompareOp::Greater => left > right,
                        CompareOp::GreaterEqual => left >= right,
                    };
                    self.set_bool(*dst, value)?;
                }
                Instruction::FloatCompareConst {
                    dst,
                    op,
                    value,
                    constant_bits,
                    constant_left,
                } => {
                    let value = self.float(*value)?;
                    let constant = f64::from_bits(*constant_bits);
                    let (left, right) = if *constant_left {
                        (constant, value)
                    } else {
                        (value, constant)
                    };
                    let value = match op {
                        CompareOp::Less => left < right,
                        CompareOp::LessEqual => left <= right,
                        CompareOp::Greater => left > right,
                        CompareOp::GreaterEqual => left >= right,
                    };
                    self.set_bool(*dst, value)?;
                }
                Instruction::ValueEqual {
                    dst,
                    negate,
                    left,
                    right,
                } => {
                    let equal = self.slots_equal(*left, *right)?;
                    self.set_bool(*dst, if *negate { !equal } else { equal })?;
                }
                Instruction::EnumParamEqualConst {
                    dst,
                    param,
                    constant,
                    negate,
                } => {
                    let equal = self.enum_param_equal_const(*param, *constant)?;
                    self.set_bool(*dst, if *negate { !equal } else { equal })?;
                }
                Instruction::Jump(target) => self.ip = *target,
                Instruction::JumpIfFalse { condition, target } => {
                    if !self.bool(*condition)? {
                        self.ip = *target;
                    }
                }
                Instruction::JumpIfTrue { condition, target } => {
                    if self.bool(*condition)? {
                        self.ip = *target;
                    }
                }
                Instruction::ContextRead { dst, read } => {
                    self.context_read(*dst, *read)?;
                }
                Instruction::SectionPosition { dst, width } => {
                    let width = self.float(*width)?.max(1.0);
                    let index = sample_context(self.context)?.pixel_index as f64;
                    self.set_float(*dst, (index - (index / width).floor() * width) / width)?;
                }
                Instruction::FloatUnary { dst, op, value } => {
                    let value = self.float(*value)?;
                    let result = match op {
                        FloatUnary::Sin => value.sin(),
                        FloatUnary::Cos => value.cos(),
                        FloatUnary::Abs => value.abs(),
                        FloatUnary::Floor => value.floor(),
                    };
                    self.set_float(*dst, result)?;
                }
                Instruction::FloatBinary {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.float(*left)?;
                    let right = self.float(*right)?;
                    let result = match op {
                        FloatBinary::Min => left.min(right),
                        FloatBinary::Max => left.max(right),
                    };
                    self.set_float(*dst, result)?;
                }
                Instruction::FloatBinaryConst {
                    dst,
                    op,
                    value,
                    constant_bits,
                } => {
                    let value = self.float(*value)?;
                    let constant = f64::from_bits(*constant_bits);
                    let result = match op {
                        FloatBinary::Min => value.min(constant),
                        FloatBinary::Max => value.max(constant),
                    };
                    self.set_float(*dst, result)?;
                }
                Instruction::Clamp {
                    dst,
                    value,
                    min,
                    max,
                } => {
                    let value = self.float(*value)?;
                    let min = self.float(*min)?;
                    let max = self.float(*max)?;
                    self.set_float(*dst, value.clamp(min, max))?;
                }
                Instruction::ClampConst {
                    dst,
                    value,
                    min_bits,
                    max_bits,
                } => {
                    let value = self.float(*value)?;
                    self.set_float(
                        *dst,
                        value.clamp(f64::from_bits(*min_bits), f64::from_bits(*max_bits)),
                    )?;
                }
                Instruction::Smoothstep {
                    dst,
                    edge0,
                    edge1,
                    value,
                } => {
                    let edge0 = self.float(*edge0)?;
                    let edge1 = self.float(*edge1)?;
                    let value = self.float(*value)?;
                    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                    self.set_float(*dst, t * t * (3.0 - 2.0 * t))?;
                }
                Instruction::MixFloat {
                    dst,
                    left,
                    right,
                    amount,
                } => {
                    let amount = self.float(*amount)?;
                    let left = self.float(*left)?;
                    let right = self.float(*right)?;
                    self.set_float(*dst, left + (right - left) * amount)?;
                }
                Instruction::MixColor {
                    dst,
                    left,
                    right,
                    amount,
                } => {
                    let amount = self.float(*amount)?;
                    let left = self.color(*left)?;
                    let right = self.color(*right)?;
                    self.set_color(*dst, mix_colors(left, right, amount))?;
                }
                Instruction::ColorBinary {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.color(*left)?;
                    let right = self.color(*right)?;
                    let color = match op {
                        ColorBinary::Add => add_colors(left, right),
                        ColorBinary::Multiply => multiply_colors(left, right),
                        ColorBinary::Max => max_colors(left, right),
                    };
                    self.set_color(*dst, color)?;
                }
                Instruction::ColorScale { dst, color, scale } => {
                    let color = self.color(*color)?;
                    let scale = self.float(*scale)?;
                    self.set_color(*dst, scale_color(color, scale))?;
                }
                Instruction::ColorIntensity { dst, color } => {
                    let color = self.color(*color)?;
                    self.set_float(*dst, color_intensity(color))?;
                }
                Instruction::ColorInvert { dst, color } => {
                    let color = self.color(*color)?;
                    self.set_color(*dst, invert_color(color))?;
                }
                Instruction::Rgb {
                    dst,
                    red,
                    green,
                    blue,
                } => {
                    self.set_color(
                        *dst,
                        Color {
                            red: channel(self.float(*red)?),
                            green: channel(self.float(*green)?),
                            blue: channel(self.float(*blue)?),
                        },
                    )?;
                }
                Instruction::Hsv {
                    dst,
                    hue,
                    saturation,
                    value,
                } => {
                    self.set_color(
                        *dst,
                        hsv(
                            self.float(*hue)?,
                            self.float(*saturation)?,
                            self.float(*value)?,
                        ),
                    )?;
                }
                Instruction::Rand { dst, args } => {
                    self.set_float(*dst, self.random(args))?;
                }
                Instruction::CurveFloatClamped {
                    dst,
                    curve,
                    position,
                    min,
                    max,
                } => {
                    let curve = to_curve_runtime(self.ref_value(*curve)?, self.params)?;
                    let position = self.float(*position)?;
                    let min = self.float(*min)?;
                    let max = self.float(*max)?;
                    self.set_float(*dst, sample_curve(curve, position).clamp(min, max))?;
                }
                Instruction::CurveParamFloatClamped {
                    dst,
                    param,
                    position,
                    min,
                    max,
                } => {
                    let position = self.float(*position)?;
                    let min = self.float(*min)?;
                    let max = self.float(*max)?;
                    let curve = self.prepared_curve_param(*param)?;
                    let value = sample_prepared_curve(curve, position)?.clamp(min, max);
                    self.set_float(*dst, value)?;
                }
                Instruction::GradientColorScaled {
                    dst,
                    gradient,
                    position,
                    scale,
                } => {
                    let scale = self.float(*scale)?.clamp(0.0, 1.0);
                    if scale <= 0.0 {
                        self.set_color(*dst, black())?;
                    } else {
                        let gradient =
                            to_gradient_runtime(self.ref_value(*gradient)?, self.params)?;
                        let position = self.float(*position)?;
                        let color = sample_gradient(gradient, position)?;
                        self.set_color(*dst, scale_color(color, scale))?;
                    }
                }
                Instruction::GradientParamColorScaled {
                    dst,
                    param,
                    position,
                    scale,
                } => {
                    let scale = self.float(*scale)?.clamp(0.0, 1.0);
                    if scale <= 0.0 {
                        self.set_color(*dst, black())?;
                    } else {
                        let position = self.float(*position)?;
                        let gradient = self.prepared_gradient_param(*param)?;
                        let color = sample_prepared_gradient(gradient, position)?;
                        self.set_color(*dst, scale_color(color, scale))?;
                    }
                }
                Instruction::CurveCrossing {
                    dst,
                    curve,
                    value,
                    fallback,
                } => {
                    let curve = to_curve_runtime(self.ref_value(*curve)?, self.params)?;
                    let value = self.float(*value)?;
                    let fallback = fallback
                        .map(|fallback| self.float(fallback))
                        .transpose()?
                        .unwrap_or(value);
                    self.set_float(*dst, curve_crossing_raw(curve, value, fallback))?;
                }
                Instruction::CurveParamCrossing {
                    dst,
                    param,
                    value,
                    fallback,
                } => {
                    let value = self.float(*value)?;
                    let fallback = fallback
                        .map(|fallback| self.float(fallback))
                        .transpose()?
                        .unwrap_or(value);
                    let curve = self.prepared_curve_param(*param)?;
                    self.set_float(*dst, curve_crossing(curve, value, fallback)?)?;
                }
                Instruction::Len { dst, value } => {
                    let runtime_value = self.value(*value)?;
                    let value = match &runtime_value {
                        RuntimeValue::Array(items) => i64::try_from(items.len())
                            .map_err(|_| RuntimeError::new("array length exceeds int range"))?,
                        RuntimeValue::Marks(marks) => i64::try_from(marks.marks.len())
                            .map_err(|_| RuntimeError::new("mark count exceeds int range"))?,
                        _ => return Err(RuntimeError::new("len requires array or marks")),
                    };
                    self.set_int(*dst, value)?;
                }
                Instruction::Mark { dst, op, args } => match op {
                    MarkOp::Count => {
                        let marks = self.mark_arg(args, 0)?;
                        let value = i64::try_from(marks.marks.len())
                            .map_err(|_| RuntimeError::new("mark count exceeds int range"))?;
                        self.set_int(self.int_slot_value(*dst)?, value)?;
                    }
                    MarkOp::At => {
                        let marks = self.mark_arg(args, 0)?;
                        let index = self.int_arg(args, 1)?;
                        let fallback = self.optional_float_arg(args, 2)?.unwrap_or(0.0);
                        let value = mark_at_from(marks, index, fallback)?;
                        self.set_float(self.float_slot_value(*dst)?, value)?;
                    }
                    MarkOp::Prev => {
                        let marks = self.mark_arg(args, 0)?;
                        let seconds = self.optional_float_arg(args, 1)?.unwrap_or(
                            sample_context(self.context)
                                .map(|context| context.seconds)
                                .unwrap_or(0.0),
                        );
                        let fallback = self.optional_float_arg(args, 2)?.unwrap_or(0.0);
                        let index = prev_index(marks, seconds)?;
                        let value = if index < 0 {
                            fallback
                        } else {
                            mark_at_from(marks, index, fallback)?
                        };
                        self.set_float(self.float_slot_value(*dst)?, value)?;
                    }
                    MarkOp::PrevIndex => {
                        let marks = self.mark_arg(args, 0)?;
                        let seconds = self.optional_float_arg(args, 1)?.unwrap_or(
                            sample_context(self.context)
                                .map(|context| context.seconds)
                                .unwrap_or(0.0),
                        );
                        let value = prev_index(marks, seconds)?;
                        self.set_int(self.int_slot_value(*dst)?, value)?;
                    }
                    MarkOp::NextIndex => {
                        let marks = self.mark_arg(args, 0)?;
                        let seconds = self.optional_float_arg(args, 1)?.unwrap_or(
                            sample_context(self.context)
                                .map(|context| context.seconds)
                                .unwrap_or(0.0),
                        );
                        let value = next_index(marks, seconds)?;
                        self.set_int(self.int_slot_value(*dst)?, value)?;
                    }
                    MarkOp::Elapsed => {
                        let marks = self.mark_arg(args, 0)?;
                        let seconds = self.optional_float_arg(args, 1)?.unwrap_or(
                            sample_context(self.context)
                                .map(|context| context.seconds)
                                .unwrap_or(0.0),
                        );
                        let value = elapsed(marks, seconds)?;
                        self.set_float(self.float_slot_value(*dst)?, value)?;
                    }
                    MarkOp::Phase => {
                        let marks = self.mark_arg(args, 0)?;
                        let seconds = self.optional_float_arg(args, 1)?.unwrap_or(
                            sample_context(self.context)
                                .map(|context| context.seconds)
                                .unwrap_or(0.0),
                        );
                        let duration = match self.context {
                            VmContext::Sample(context) => context.duration,
                            VmContext::Generator(context) => context.duration,
                        };
                        let value = phase(marks, seconds, duration)?;
                        self.set_float(self.float_slot_value(*dst)?, value)?;
                    }
                },
                Instruction::TargetItems { dst, op, args } => match op {
                    TargetItemsOp::Fixtures => {
                        let target = self.ref_arg(args, 0)?;
                        self.set_value(
                            *dst,
                            RuntimeValue::TargetItems(Arc::new(fixtures(target)?)),
                        )?
                    }
                    TargetItemsOp::Pixels => {
                        let target = self.ref_arg(args, 0)?;
                        self.set_value(*dst, RuntimeValue::TargetItems(Arc::new(pixels(target)?)))?
                    }
                    TargetItemsOp::Sections => {
                        let target = self.ref_arg(args, 0)?;
                        let width = self.float_arg(args, 1)?;
                        self.set_value(
                            *dst,
                            RuntimeValue::TargetItems(Arc::new(sections(target, width)?)),
                        )?
                    }
                    TargetItemsOp::Count => {
                        let target = self.ref_arg(args, 0)?;
                        self.set_value(
                            *dst,
                            RuntimeValue::Int(target_items(target)?.groups.len() as i64),
                        )?
                    }
                    TargetItemsOp::Pick => {
                        let target = self.ref_arg(args, 0)?;
                        let items = target_items(target)?;
                        let index = usize::try_from(self.int_arg(args, 1)?).map_err(|_| {
                            RuntimeError::new("target item index cannot be negative")
                        })?;
                        self.set_value(
                            *dst,
                            RuntimeValue::TargetItem(items.groups.get(index).cloned().ok_or_else(
                                || RuntimeError::new("target item index out of bounds"),
                            )?),
                        )?;
                    }
                },
                Instruction::CheckLoopLimit => {
                    self.loop_iterations += 1;
                    if self.loop_iterations > MAX_VM_INSTRUCTIONS_PER_INVOCATION {
                        return Err(RuntimeError::new("loop iteration limit exceeded"));
                    }
                }
                Instruction::Emit { effect, fields } => self.emit_generated(effect, fields)?,
                Instruction::Return(src) => return self.return_value(*src),
                Instruction::ReturnColor(src) => return Ok(RuntimeValue::Color(self.color(*src)?)),
            }
        }
    }

    fn return_value(&self, slot: ValueSlot) -> Result<RuntimeValue, RuntimeError> {
        self.value(slot)
    }

    fn int(&self, slot: IntSlot) -> Result<i64, RuntimeError> {
        Ok(self.scratch.registers.ints[slot.0])
    }

    fn float(&self, slot: FloatSlot) -> Result<f64, RuntimeError> {
        Ok(self.scratch.registers.floats[slot.0])
    }

    fn bool(&self, slot: BoolSlot) -> Result<bool, RuntimeError> {
        Ok(self.scratch.registers.bools[slot.0])
    }

    fn color(&self, slot: ColorSlot) -> Result<Color, RuntimeError> {
        Ok(self.scratch.registers.colors[slot.0])
    }

    fn ref_value(&self, slot: RefSlot) -> Result<&RuntimeValue, RuntimeError> {
        Ok(&self.scratch.registers.refs[slot.0])
    }

    fn value(&self, slot: ValueSlot) -> Result<RuntimeValue, RuntimeError> {
        match slot {
            ValueSlot::Int(slot) => self.int(slot).map(RuntimeValue::Int),
            ValueSlot::Float(slot) => self.float(slot).map(RuntimeValue::Float),
            ValueSlot::Bool(slot) => self.bool(slot).map(RuntimeValue::Bool),
            ValueSlot::Color(slot) => self.color(slot).map(RuntimeValue::Color),
            ValueSlot::Ref(slot) => self.ref_value(slot).map(clone_runtime),
        }
    }

    fn float_value(&self, slot: ValueSlot) -> Result<f64, RuntimeError> {
        match slot {
            ValueSlot::Int(slot) => self.int(slot).map(|value| value as f64),
            ValueSlot::Float(slot) => self.float(slot),
            _ => Err(RuntimeError::new("expected float")),
        }
    }

    fn target_item_value(&self, slot: ValueSlot) -> Result<Arc<TargetItemValue>, RuntimeError> {
        let ValueSlot::Ref(slot) = slot else {
            return Err(RuntimeError::new("emit target must be target items"));
        };
        match self.ref_value(slot)? {
            RuntimeValue::TargetItem(item) => Ok(Arc::clone(item)),
            RuntimeValue::TargetItems(items) => Ok(target_item_from_groups(&items.groups)),
            RuntimeValue::Target(target) => Ok(target_item_from_groups(&target.groups)),
            _ => Err(RuntimeError::new("emit target must be target items")),
        }
    }

    fn int_slot_value(&self, slot: ValueSlot) -> Result<IntSlot, RuntimeError> {
        match slot {
            ValueSlot::Int(slot) => Ok(slot),
            _ => Err(RuntimeError::new("expected int slot")),
        }
    }

    fn float_slot_value(&self, slot: ValueSlot) -> Result<FloatSlot, RuntimeError> {
        match slot {
            ValueSlot::Float(slot) => Ok(slot),
            _ => Err(RuntimeError::new("expected float slot")),
        }
    }

    fn mark_arg(&self, args: &[ValueSlot], index: usize) -> Result<&Marks, RuntimeError> {
        let Some(slot) = args.get(index) else {
            return Err(RuntimeError::new("missing argument"));
        };
        let ValueSlot::Ref(slot) = slot else {
            return Err(RuntimeError::new("mark builtin first arg must be marks"));
        };
        match self.ref_value(*slot)? {
            RuntimeValue::Marks(marks) => Ok(marks),
            _ => Err(RuntimeError::new("mark builtin first arg must be marks")),
        }
    }

    fn int_arg(&self, args: &[ValueSlot], index: usize) -> Result<i64, RuntimeError> {
        let Some(slot) = args.get(index) else {
            return Err(RuntimeError::new("missing argument"));
        };
        match *slot {
            ValueSlot::Int(slot) => self.int(slot),
            ValueSlot::Float(slot) => self.float(slot).map(|value| value as i64),
            _ => Err(RuntimeError::new("expected int")),
        }
    }

    fn float_arg(&self, args: &[ValueSlot], index: usize) -> Result<f64, RuntimeError> {
        let Some(slot) = args.get(index) else {
            return Err(RuntimeError::new("missing argument"));
        };
        match *slot {
            ValueSlot::Int(slot) => self.int(slot).map(|value| value as f64),
            ValueSlot::Float(slot) => self.float(slot),
            _ => Err(RuntimeError::new("expected float")),
        }
    }

    fn optional_float_arg(
        &self,
        args: &[ValueSlot],
        index: usize,
    ) -> Result<Option<f64>, RuntimeError> {
        let Some(slot) = args.get(index) else {
            return Ok(None);
        };
        match *slot {
            ValueSlot::Int(slot) => self.int(slot).map(|value| Some(value as f64)),
            ValueSlot::Float(slot) => self.float(slot).map(Some),
            _ => Err(RuntimeError::new("expected float")),
        }
    }

    fn ref_arg(&self, args: &[ValueSlot], index: usize) -> Result<&RuntimeValue, RuntimeError> {
        let Some(slot) = args.get(index) else {
            return Err(RuntimeError::new("missing argument"));
        };
        let ValueSlot::Ref(slot) = *slot else {
            return Err(RuntimeError::new("expected reference-like value"));
        };
        self.ref_value(slot)
    }

    fn set_int(&mut self, slot: IntSlot, value: i64) -> Result<(), RuntimeError> {
        self.scratch.registers.ints[slot.0] = value;
        Ok(())
    }

    fn set_float(&mut self, slot: FloatSlot, value: f64) -> Result<(), RuntimeError> {
        self.scratch.registers.floats[slot.0] = value;
        Ok(())
    }

    fn set_bool(&mut self, slot: BoolSlot, value: bool) -> Result<(), RuntimeError> {
        self.scratch.registers.bools[slot.0] = value;
        Ok(())
    }

    fn set_color(&mut self, slot: ColorSlot, value: Color) -> Result<(), RuntimeError> {
        self.scratch.registers.colors[slot.0] = value;
        Ok(())
    }

    fn set_ref(&mut self, slot: RefSlot, value: RuntimeValue) -> Result<(), RuntimeError> {
        self.scratch.registers.refs[slot.0] = value;
        Ok(())
    }

    fn set_value(&mut self, slot: ValueSlot, value: RuntimeValue) -> Result<(), RuntimeError> {
        match (slot, value) {
            (ValueSlot::Int(slot), RuntimeValue::Int(value)) => self.set_int(slot, value),
            (ValueSlot::Int(slot), RuntimeValue::Float(value)) => self.set_int(slot, value as i64),
            (ValueSlot::Float(slot), RuntimeValue::Float(value)) => self.set_float(slot, value),
            (ValueSlot::Float(slot), RuntimeValue::Int(value)) => {
                self.set_float(slot, value as f64)
            }
            (ValueSlot::Bool(slot), RuntimeValue::Bool(value)) => self.set_bool(slot, value),
            (ValueSlot::Color(slot), RuntimeValue::Color(value)) => self.set_color(slot, value),
            (ValueSlot::Ref(slot), value) => self.set_ref(slot, value),
            _ => Err(RuntimeError::new("type mismatch writing VM slot")),
        }
    }

    fn set_const_value(&mut self, slot: ValueSlot, value: &Value) -> Result<(), RuntimeError> {
        match (slot, value) {
            (ValueSlot::Int(slot), Value::Int(value)) => self.set_int(slot, *value),
            (ValueSlot::Float(slot), Value::Float(value)) => self.set_float(slot, *value),
            (ValueSlot::Float(slot), Value::Int(value)) => self.set_float(slot, *value as f64),
            (ValueSlot::Bool(slot), Value::Bool(value)) => self.set_bool(slot, *value),
            (ValueSlot::Color(slot), Value::Color(value)) => self.set_color(slot, *value),
            (ValueSlot::Ref(slot), value) => self.set_ref(slot, RuntimeValue::from_value(value)),
            _ => self.set_value(slot, RuntimeValue::from_value(value)),
        }
    }

    fn load_int_param(&mut self, slot: IntSlot, index: usize) -> Result<(), RuntimeError> {
        if let Some(Some(value)) = self.scratch.param_overrides.get(index) {
            return match value {
                RuntimeValue::Int(value) => self.set_int(slot, *value),
                RuntimeValue::Float(value) => self.set_int(slot, *value as i64),
                _ => Err(RuntimeError::new("expected int")),
            };
        }
        match self.params.values.get(index) {
            Some(BoundParamValue::Int(value)) => self.set_int(slot, *value),
            Some(BoundParamValue::Float(value)) => self.set_int(slot, *value as i64),
            Some(_) => Err(RuntimeError::new("expected int")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn load_float_param(&mut self, slot: FloatSlot, index: usize) -> Result<(), RuntimeError> {
        if let Some(Some(value)) = self.scratch.param_overrides.get(index) {
            return match value {
                RuntimeValue::Float(value) => self.set_float(slot, *value),
                RuntimeValue::Int(value) => self.set_float(slot, *value as f64),
                _ => Err(RuntimeError::new("expected float")),
            };
        }
        match self.params.values.get(index) {
            Some(BoundParamValue::Float(value)) => self.set_float(slot, *value),
            Some(BoundParamValue::Int(value)) => self.set_float(slot, *value as f64),
            Some(_) => Err(RuntimeError::new("expected float")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn load_bool_param(&mut self, slot: BoolSlot, index: usize) -> Result<(), RuntimeError> {
        if let Some(Some(value)) = self.scratch.param_overrides.get(index) {
            return match value {
                RuntimeValue::Bool(value) => self.set_bool(slot, *value),
                _ => Err(RuntimeError::new("expected bool")),
            };
        }
        match self.params.values.get(index) {
            Some(BoundParamValue::Bool(value)) => self.set_bool(slot, *value),
            Some(_) => Err(RuntimeError::new("expected bool")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn load_color_param(&mut self, slot: ColorSlot, index: usize) -> Result<(), RuntimeError> {
        if let Some(Some(value)) = self.scratch.param_overrides.get(index) {
            return match value {
                RuntimeValue::Color(value) => self.set_color(slot, *value),
                _ => Err(RuntimeError::new("expected color")),
            };
        }
        match self.params.values.get(index) {
            Some(BoundParamValue::Color(value)) => self.set_color(slot, *value),
            Some(_) => Err(RuntimeError::new("expected color")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn load_ref_param(&mut self, slot: RefSlot, index: usize) -> Result<(), RuntimeError> {
        if let Some(Some(value)) = self.scratch.param_overrides.get(index) {
            return self.set_ref(slot, clone_runtime(value));
        }
        match self.params.values.get(index) {
            Some(value) => self.set_ref(slot, value.to_runtime()),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn copy_slot(&mut self, dst: ValueSlot, src: ValueSlot) -> Result<(), RuntimeError> {
        match (dst, src) {
            (ValueSlot::Int(dst), ValueSlot::Int(src)) => self.set_int(dst, self.int(src)?),
            (ValueSlot::Int(dst), ValueSlot::Float(src)) => {
                self.set_int(dst, self.float(src)? as i64)
            }
            (ValueSlot::Float(dst), ValueSlot::Float(src)) => self.set_float(dst, self.float(src)?),
            (ValueSlot::Float(dst), ValueSlot::Int(src)) => {
                self.set_float(dst, self.int(src)? as f64)
            }
            (ValueSlot::Bool(dst), ValueSlot::Bool(src)) => self.set_bool(dst, self.bool(src)?),
            (ValueSlot::Color(dst), ValueSlot::Color(src)) => self.set_color(dst, self.color(src)?),
            (ValueSlot::Ref(dst), ValueSlot::Ref(src)) => {
                self.set_ref(dst, clone_runtime(self.ref_value(src)?))
            }
            _ => Err(RuntimeError::new("type mismatch copying VM slot")),
        }
    }

    fn random(&self, args: &[FloatSlot]) -> f64 {
        let mut seed = 0.0;
        for slot in args {
            seed = seed * 31.0 + self.scratch.registers.floats[slot.0];
        }
        crate::sampling::deterministic_random_seed(seed)
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

    fn context_read(&mut self, dst: ValueSlot, read: ContextRead) -> Result<(), RuntimeError> {
        match read {
            ContextRead::Progress => {
                self.set_context_float(dst, sample_context(self.context)?.progress)
            }
            ContextRead::Seconds => {
                self.set_context_float(dst, sample_context(self.context)?.seconds)
            }
            ContextRead::Duration => self.set_context_float(
                dst,
                match self.context {
                    VmContext::Sample(context) => context.duration,
                    VmContext::Generator(context) => context.duration,
                },
            ),
            ContextRead::PixelIndex => {
                self.set_context_int(dst, sample_context(self.context)?.pixel_index)
            }
            ContextRead::PixelCount => {
                self.set_context_int(dst, sample_context(self.context)?.pixel_count)
            }
            ContextRead::PixelFraction => {
                self.set_context_float(dst, sample_context(self.context)?.pixel_fraction)
            }
        }
    }

    fn set_context_float(&mut self, dst: ValueSlot, value: f64) -> Result<(), RuntimeError> {
        match dst {
            ValueSlot::Float(slot) => self.set_float(slot, value),
            _ => self.set_value(dst, RuntimeValue::Float(value)),
        }
    }

    fn set_context_int(&mut self, dst: ValueSlot, value: i64) -> Result<(), RuntimeError> {
        match dst {
            ValueSlot::Int(slot) => self.set_int(slot, value),
            ValueSlot::Float(slot) => self.set_float(slot, value as f64),
            _ => self.set_value(dst, RuntimeValue::Int(value)),
        }
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
                Ok(RuntimeValue::Float(sample_curve(curve, position)))
            }
            RuntimeValue::PreparedCurve(curve) => {
                let position = to_float_runtime(index, self.params)?;
                Ok(RuntimeValue::Float(sample_prepared_curve(curve, position)?))
            }
            RuntimeValue::Gradient(gradient) => {
                let position = to_float_runtime(index, self.params)?;
                Ok(RuntimeValue::Color(sample_gradient(gradient, position)?))
            }
            RuntimeValue::PreparedGradient(gradient) => {
                let position = to_float_runtime(index, self.params)?;
                Ok(RuntimeValue::Color(sample_prepared_gradient(
                    gradient, position,
                )?))
            }
            _ => Err(RuntimeError::new(
                "index target is not an array, curve, or gradient",
            )),
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

    fn prepared_gradient_param(&self, param: usize) -> Result<&PreparedGradient, RuntimeError> {
        match self.scratch.param_overrides.get(param) {
            Some(Some(RuntimeValue::PreparedGradient(gradient))) => return Ok(gradient),
            Some(Some(RuntimeValue::Gradient(_))) => {
                return Err(RuntimeError::new("unprepared gradient param override"));
            }
            Some(Some(_)) => return Err(RuntimeError::new("expected gradient")),
            _ => {}
        }
        match self.params.values.get(param) {
            Some(BoundParamValue::Gradient(gradient)) => Ok(gradient),
            Some(_) => Err(RuntimeError::new("expected gradient")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn enum_param_equal_const(&self, param: usize, constant: usize) -> Result<bool, RuntimeError> {
        let expected = match self.function.constants.get(constant) {
            Some(Value::Enum(value)) => value,
            Some(_) => return Err(RuntimeError::new("expected enum constant")),
            None => return Err(RuntimeError::new("invalid constant slot")),
        };
        match self.scratch.param_overrides.get(param) {
            Some(Some(RuntimeValue::Enum(value))) => return Ok(value == expected),
            Some(Some(_)) => return Err(RuntimeError::new("expected enum")),
            _ => {}
        }
        match self.params.values.get(param) {
            Some(BoundParamValue::Enum(value)) => Ok(value == expected),
            Some(_) => Err(RuntimeError::new("expected enum")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn slots_equal(&self, left: ValueSlot, right: ValueSlot) -> Result<bool, RuntimeError> {
        Ok(match (left, right) {
            (ValueSlot::Int(left), ValueSlot::Int(right)) => self.int(left)? == self.int(right)?,
            (ValueSlot::Float(left), ValueSlot::Float(right)) => {
                self.float(left)? == self.float(right)?
            }
            (ValueSlot::Int(left), ValueSlot::Float(right)) => {
                self.int(left)? as f64 == self.float(right)?
            }
            (ValueSlot::Float(left), ValueSlot::Int(right)) => {
                self.float(left)? == self.int(right)? as f64
            }
            (ValueSlot::Bool(left), ValueSlot::Bool(right)) => {
                self.bool(left)? == self.bool(right)?
            }
            (ValueSlot::Color(left), ValueSlot::Color(right)) => {
                self.color(left)? == self.color(right)?
            }
            (ValueSlot::Ref(left), ValueSlot::Ref(right)) => {
                runtime_refs_equal(self.ref_value(left)?, self.ref_value(right)?)
            }
            _ => {
                let left = self.value(left)?;
                let right = self.value(right)?;
                values_equal(&left, &right, self.params)
            }
        })
    }

    fn emit_generated(
        &mut self,
        effect: &GeneratedEffectRef,
        fields: &[(Identifier, ValueSlot)],
    ) -> Result<(), RuntimeError> {
        let mut start_seconds = None;
        let mut duration_seconds = None;
        let mut target = None;
        let mut params = IndexMap::with_capacity(fields.len());
        for (field, slot) in fields {
            match field.as_str() {
                "start" => start_seconds = Some(self.float_value(*slot)?),
                "duration" => duration_seconds = Some(self.float_value(*slot)?),
                "target" => {
                    target = Some(self.target_item_value(*slot)?);
                }
                _ => {
                    let value = self.value(*slot)?;
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

fn resolve_param(
    param: &ParamDecl,
    params: &IndexMap<Identifier, Value>,
) -> Result<Value, RuntimeError> {
    if let Some(value) = params.get(&param.name) {
        return Ok(value.clone());
    }
    if let Some(default) = &param.default {
        return Ok(default.clone());
    }
    Err(RuntimeError::new(format!(
        "missing required parameter `{}`",
        param.name.as_str()
    )))
}

fn bind_param_value(ty: &Type, value: Value, cache: &mut DslBindCache) -> BoundParamValue {
    match (ty, value) {
        (Type::Float, Value::Int(value)) => BoundParamValue::Float(value as f64),
        (ty, value) => BoundParamValue::from_value(ty, value, cache),
    }
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
        RuntimeValue::Gradient(value) => Value::Gradient(value),
        RuntimeValue::PreparedGradient(value) => Value::Gradient(value.raw()),
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
        "element_index" => RuntimeValue::Int(pixel.element_index),
        "element_cell_index" => RuntimeValue::Int(pixel.element_cell_index),
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

fn to_int_runtime(value: &RuntimeValue, params: &BoundParams) -> Result<i64, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Int(value) => Ok(*value),
        RuntimeValue::Float(value) => Ok(*value as i64),
        _ => Err(RuntimeError::new("expected int")),
    }
}

fn to_float_runtime(value: &RuntimeValue, params: &BoundParams) -> Result<f64, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Int(value) => Ok(*value as f64),
        RuntimeValue::Float(value) => Ok(*value),
        _ => Err(RuntimeError::new("expected float")),
    }
}

fn to_curve_runtime<'a>(
    value: &'a RuntimeValue,
    params: &'a BoundParams,
) -> Result<&'a Curve, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Curve(curve) => Ok(curve),
        RuntimeValue::PreparedCurve(curve) => Ok(&curve.raw),
        _ => Err(RuntimeError::new("expected curve")),
    }
}

fn to_gradient_runtime<'a>(
    value: &'a RuntimeValue,
    params: &'a BoundParams,
) -> Result<&'a Gradient, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Gradient(gradient) => Ok(gradient),
        RuntimeValue::PreparedGradient(gradient) => Ok(&gradient.raw),
        _ => Err(RuntimeError::new("expected gradient")),
    }
}

fn target_items(value: &RuntimeValue) -> Result<&TargetItemsValue, RuntimeError> {
    match value {
        RuntimeValue::TargetItems(items) => Ok(items),
        _ => Err(RuntimeError::new("expected TargetItems")),
    }
}

fn for_each_target_group(
    value: &RuntimeValue,
    mut visit: impl FnMut(&Arc<TargetItemValue>),
) -> Result<(), RuntimeError> {
    match value {
        RuntimeValue::Target(target) => {
            for group in &target.groups {
                visit(group);
            }
            Ok(())
        }
        RuntimeValue::TargetItems(items) => {
            for group in &items.groups {
                visit(group);
            }
            Ok(())
        }
        RuntimeValue::TargetItem(item) => {
            visit(item);
            Ok(())
        }
        _ => Err(RuntimeError::new("expected target")),
    }
}

fn fixtures(value: &RuntimeValue) -> Result<TargetItemsValue, RuntimeError> {
    let mut raw_groups: Vec<Vec<TargetPixelValue>> = Vec::new();
    for_each_target_pixel(value, |pixel| {
        if raw_groups
            .last()
            .and_then(|group| group.first())
            .is_some_and(|first| first.element_index == pixel.element_index)
        {
            if let Some(group) = raw_groups.last_mut() {
                group.push(pixel);
            }
        } else {
            raw_groups.push(vec![pixel]);
        }
    })?;
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
    let mut groups = Vec::new();
    for_each_target_pixel(value, |pixel| {
        groups.push(Arc::new(TargetItemValue {
            pixels: Arc::new(vec![pixel]),
        }));
    })?;
    Ok(TargetItemsValue { groups })
}

fn sections(value: &RuntimeValue, width: f64) -> Result<TargetItemsValue, RuntimeError> {
    let width = width.max(1.0).floor() as i64;
    let mut raw_groups: Vec<Vec<TargetPixelValue>> = Vec::new();
    for_each_target_pixel(value, |pixel| {
        if raw_groups
            .last()
            .and_then(|group| group.first())
            .is_some_and(|first| {
                first.element_index == pixel.element_index
                    && first.element_cell_index / width == pixel.element_cell_index / width
            })
        {
            if let Some(group) = raw_groups.last_mut() {
                group.push(pixel);
            }
        } else {
            raw_groups.push(vec![pixel]);
        }
    })?;
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

fn for_each_target_pixel(
    value: &RuntimeValue,
    mut visit: impl FnMut(TargetPixelValue),
) -> Result<(), RuntimeError> {
    for_each_target_group(value, |item| {
        for pixel in item.pixels.iter().copied() {
            visit(pixel);
        }
    })
}

fn values_equal(left: &RuntimeValue, right: &RuntimeValue, params: &BoundParams) -> bool {
    let _ = params;
    runtime_refs_equal(left, right)
}

fn runtime_refs_equal(left: &RuntimeValue, right: &RuntimeValue) -> bool {
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

fn prepare_float_segments(curve: &Curve) -> Vec<CurveSegment> {
    let Some(first) = curve.points.first() else {
        return Vec::new();
    };
    let mut previous = first;
    let mut segments = Vec::with_capacity(curve.points.len());
    for point in &curve.points {
        segments.push(CurveSegment {
            start_position: previous.position,
            end_position: point.position,
            start_value: previous.value,
            end_value: point.value,
        });
        previous = point;
    }
    segments
}

fn prepare_gradient_segments(gradient: &Gradient) -> Vec<GradientSegment> {
    let Some(first) = gradient.stops.first() else {
        return Vec::new();
    };
    let mut previous = first;
    let mut segments = Vec::with_capacity(gradient.stops.len());
    for stop in &gradient.stops {
        segments.push(GradientSegment {
            start_position: previous.position,
            end_position: stop.position,
            start_value: previous.color,
            end_value: stop.color,
        });
        previous = stop;
    }
    segments
}

fn prepare_curve_crossings(segments: &[CurveSegment]) -> PreparedCurveCrossings {
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

fn sample_prepared_curve(curve: &PreparedCurve, position: f64) -> Result<f64, RuntimeError> {
    let Some(segment) = find_position_segment(&curve.segments, position) else {
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

fn sample_prepared_gradient(
    gradient: &PreparedGradient,
    position: f64,
) -> Result<Color, RuntimeError> {
    let Some(segment) = find_position_segment(&gradient.segments, position) else {
        return Err(RuntimeError::new("cannot sample empty gradient"));
    };
    Ok(mix_colors(
        segment.start_value,
        segment.end_value,
        segment_t(segment.start_position, segment.end_position, position),
    ))
}

trait PositionSegment {
    fn end_position(&self) -> f64;
}

impl PositionSegment for CurveSegment {
    fn end_position(&self) -> f64 {
        self.end_position
    }
}

impl PositionSegment for GradientSegment {
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
    Ok(match &curve.crossings {
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
    crate::sampling::curve_crossing(curve, value, fallback)
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

fn sample_curve(curve: &Curve, position: f64) -> f64 {
    crate::sampling::sample_curve(curve, position)
}

fn sample_gradient(gradient: &Gradient, position: f64) -> Result<Color, RuntimeError> {
    crate::sampling::sample_gradient(gradient, position)
        .ok_or_else(|| RuntimeError::new("cannot sample empty gradient"))
}

fn mix_colors(left: Color, right: Color, t: f64) -> Color {
    Color {
        red: channel_byte(left.red as f64 + (right.red as f64 - left.red as f64) * t),
        green: channel_byte(left.green as f64 + (right.green as f64 - left.green as f64) * t),
        blue: channel_byte(left.blue as f64 + (right.blue as f64 - left.blue as f64) * t),
    }
}

fn scale_color(color: Color, scale: f64) -> Color {
    Color {
        red: channel_byte(color.red as f64 * scale),
        green: channel_byte(color.green as f64 * scale),
        blue: channel_byte(color.blue as f64 * scale),
    }
}

fn add_colors(left: Color, right: Color) -> Color {
    Color {
        red: left.red.saturating_add(right.red),
        green: left.green.saturating_add(right.green),
        blue: left.blue.saturating_add(right.blue),
    }
}

fn multiply_colors(left: Color, right: Color) -> Color {
    Color {
        red: ((u16::from(left.red) * u16::from(right.red) + 127) / 255) as u8,
        green: ((u16::from(left.green) * u16::from(right.green) + 127) / 255) as u8,
        blue: ((u16::from(left.blue) * u16::from(right.blue) + 127) / 255) as u8,
    }
}

fn max_colors(left: Color, right: Color) -> Color {
    Color {
        red: left.red.max(right.red),
        green: left.green.max(right.green),
        blue: left.blue.max(right.blue),
    }
}

fn color_intensity(color: Color) -> f64 {
    f64::from(color.red.max(color.green).max(color.blue)) / 255.0
}

fn invert_color(color: Color) -> Color {
    Color {
        red: 255 - color.red,
        green: 255 - color.green,
        blue: 255 - color.blue,
    }
}

fn channel(value: f64) -> u8 {
    channel_byte(value * 255.0)
}

fn channel_byte(value: f64) -> u8 {
    (value.clamp(0.0, 255.0) + 0.5) as u8
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

fn mark_at_from(marks: &Marks, index: i64, fallback: f64) -> Result<f64, RuntimeError> {
    let index =
        usize::try_from(index).map_err(|_| RuntimeError::new("mark index cannot be negative"))?;
    Ok(marks
        .marks
        .get(index)
        .map(|mark| mark.as_seconds_f64())
        .unwrap_or(fallback))
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

fn elapsed(marks: &Marks, seconds: f64) -> Result<f64, RuntimeError> {
    let previous = prev_index(marks, seconds)?;
    if previous < 0 {
        return Ok(seconds);
    }
    Ok(seconds - mark_at_from(marks, previous, 0.0)?)
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
