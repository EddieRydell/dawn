mod ast;
mod bytecode;
mod checked;
mod compiler;
mod diagnostic;
mod parser;
mod typecheck;
mod vm;

use bytecode::{RegisterFunction, register_function_reads_only_written_slots};
use compiler::{compile_checked_effects, compile_checked_operators};
pub use diagnostic::Diagnostic;
use indexmap::IndexMap;
use parser::parse_module;
use std::hash::{Hash, Hasher};
use typecheck::check_module;
pub use vm::{
    BoundParams, DslBindCache, DslVmScratch, GeneratedEffect, GeneratorContext, OperatorRunContext,
    RunContext, RuntimeError, SignalSampler,
};

pub(crate) mod lexer;
pub mod types;

pub use crate::values::{Color, Curve, CurvePoint, Gradient, GradientStop, Marks};
pub use ast::{OperatorInputDecl, ParamDecl};
pub use types::{
    Identifier, TargetItemValue, TargetItemsValue, TargetPixelValue, TargetValue, Type, Value,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum GeneratedEffectRef {
    Local(Identifier),
    Builtin(crate::effect::BuiltinEffect),
}

pub fn compile_effects(source: &str) -> Result<Vec<CompiledEffect>, Vec<Diagnostic>> {
    let module = parse_module(source)?;
    if !module.operators.is_empty() {
        return Err(vec![Diagnostic::new(
            lexer::TextSpan { start: 0, end: 0 },
            "operator declarations are not allowed in effect sources",
        )]);
    }
    let module = check_module(module)?;
    Ok(compile_checked_effects(module))
}

pub fn compile_operators(source: &str) -> Result<Vec<CompiledOperator>, Vec<Diagnostic>> {
    compile_operators_inner(source, false)
}

pub(crate) fn compile_builtin_operators(
    source: &str,
) -> Result<Vec<CompiledOperator>, Vec<Diagnostic>> {
    compile_operators_inner(source, true)
}

fn compile_operators_inner(
    source: &str,
    allow_reserved_names: bool,
) -> Result<Vec<CompiledOperator>, Vec<Diagnostic>> {
    let module = parse_module(source)?;
    if !module.effects.is_empty() {
        return Err(vec![Diagnostic::new(
            lexer::TextSpan { start: 0, end: 0 },
            "effect declarations are not allowed in operator sources",
        )]);
    }
    if !allow_reserved_names {
        const RESERVED: &[&str] = &[
            "Max",
            "Add",
            "Multiply",
            "IntensityModulate",
            "Dim",
            "Invert",
            "Colorize",
            "Delay",
            "Echo",
            "max",
            "add",
            "multiply",
            "intensity_modulate",
            "dim",
            "invert",
            "colorize",
            "delay",
            "echo",
        ];
        if let Some(operator) = module
            .operators
            .iter()
            .find(|operator| RESERVED.contains(&operator.name.as_str()))
        {
            return Err(vec![Diagnostic::new(
                lexer::TextSpan { start: 0, end: 0 },
                format!("operator name `{}` is reserved", operator.name.as_str()),
            )]);
        }
    }
    let module = check_module(module)?;
    Ok(compile_checked_operators(module))
}

#[derive(Clone, Debug)]
pub struct CompiledEffect {
    name: Identifier,
    params: Vec<ParamDecl>,
    kind: EffectKind,
    function: RegisterFunction,
}

#[derive(Clone, Debug)]
pub struct CompiledOperator {
    pub(crate) name: Identifier,
    pub(crate) inputs: Vec<OperatorInputDecl>,
    pub(crate) params: Vec<ParamDecl>,
    pub(crate) function: RegisterFunction,
}

impl PartialEq for CompiledOperator {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.inputs == other.inputs
            && self.params == other.params
            && self.function == other.function
    }
}

impl CompiledOperator {
    pub fn name(&self) -> &Identifier {
        &self.name
    }

    pub fn inputs(&self) -> &[OperatorInputDecl] {
        &self.inputs
    }

    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }

    pub fn bind_params(
        &self,
        params: &IndexMap<Identifier, Value>,
    ) -> Result<BoundParams, RuntimeError> {
        vm::bind_operator_params(self, params)
    }

    pub fn bind_params_cached(
        &self,
        params: &IndexMap<Identifier, Value>,
        cache: &mut DslBindCache,
    ) -> Result<BoundParams, RuntimeError> {
        vm::bind_operator_params_cached(self, params, cache)
    }

    pub fn sample_bound(
        &self,
        params: &BoundParams,
        context: &OperatorRunContext,
        sampler: &mut dyn SignalSampler,
        scratch: &mut DslVmScratch,
    ) -> Result<Color, RuntimeError> {
        vm::run_operator(self, params, context, sampler, scratch)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectKind {
    Sample,
    Generator,
}

pub fn hash_compiled_effect<H: Hasher>(effect: &CompiledEffect, state: &mut H) {
    effect.name.hash(state);
    hash_param_decls(&effect.params, state);
    effect.kind.hash(state);
    hash_register_function(&effect.function, state);
}

impl PartialEq for CompiledEffect {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.params == other.params
            && self.kind == other.kind
            && self.function == other.function
    }
}

impl CompiledEffect {
    pub fn name(&self) -> &Identifier {
        &self.name
    }

    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }

    pub fn kind(&self) -> EffectKind {
        self.kind
    }

    pub fn sample_reads_only_written_slots(&self) -> bool {
        register_function_reads_only_written_slots(&self.function)
    }

    pub fn sample(
        &self,
        params: &IndexMap<Identifier, Value>,
        context: &RunContext,
    ) -> Result<Color, RuntimeError> {
        let bound = self.bind_params(params)?;
        let mut scratch = DslVmScratch::default();
        self.sample_bound(&bound, context, &mut scratch)
    }

    pub fn bind_params(
        &self,
        params: &IndexMap<Identifier, Value>,
    ) -> Result<BoundParams, RuntimeError> {
        vm::bind_effect_params(self, params)
    }

    pub fn bind_params_cached(
        &self,
        params: &IndexMap<Identifier, Value>,
        cache: &mut DslBindCache,
    ) -> Result<BoundParams, RuntimeError> {
        vm::bind_effect_params_cached(self, params, cache)
    }

    pub fn sample_bound(
        &self,
        params: &BoundParams,
        context: &RunContext,
        scratch: &mut DslVmScratch,
    ) -> Result<Color, RuntimeError> {
        vm::run_sample_effect(self, params, context, scratch)
    }

    pub fn generate_bound(
        &self,
        params: &BoundParams,
        context: &GeneratorContext,
        scratch: &mut DslVmScratch,
    ) -> Result<Vec<GeneratedEffect>, RuntimeError> {
        vm::run_generator_effect(self, params, context, scratch)
    }
}

impl Hash for EffectKind {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Sample => 0u8.hash(state),
            Self::Generator => 1u8.hash(state),
        }
    }
}

fn hash_register_function<H: Hasher>(function: &RegisterFunction, state: &mut H) {
    function.instructions.hash(state);
    hash_values(&function.constants, state);
    function.layout.hash(state);
    function.layout_id.hash(state);
}

fn hash_param_decls<H: Hasher>(params: &[ParamDecl], state: &mut H) {
    params.len().hash(state);
    for param in params {
        param.name.hash(state);
        param.ty.hash(state);
        hash_optional_value(&param.default, state);
    }
}

fn hash_optional_value<H: Hasher>(value: &Option<Value>, state: &mut H) {
    match value {
        Some(value) => {
            1u8.hash(state);
            hash_value(value, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_values<H: Hasher>(values: &[Value], state: &mut H) {
    values.len().hash(state);
    for value in values {
        hash_value(value, state);
    }
}

fn hash_value<H: Hasher>(value: &Value, state: &mut H) {
    match value {
        Value::Void => 0u8.hash(state),
        Value::Int(value) => {
            1u8.hash(state);
            value.hash(state);
        }
        Value::Float(value) => {
            2u8.hash(state);
            value.to_bits().hash(state);
        }
        Value::Bool(value) => {
            3u8.hash(state);
            value.hash(state);
        }
        Value::Color(value) => {
            4u8.hash(state);
            value.hash(state);
        }
        Value::Marks(value) => {
            5u8.hash(state);
            value.marks.len().hash(state);
            for mark in &value.marks {
                mark.0.hash(state);
            }
        }
        Value::Target(value) => {
            6u8.hash(state);
            hash_target_items(&value.groups, state);
        }
        Value::TargetItems(value) => {
            7u8.hash(state);
            hash_target_items(&value.groups, state);
        }
        Value::TargetItem(value) => {
            8u8.hash(state);
            hash_target_pixels(&value.pixels, state);
        }
        Value::Curve(value) => {
            9u8.hash(state);
            hash_curve(value, state);
        }
        Value::Gradient(value) => {
            10u8.hash(state);
            hash_gradient(value, state);
        }
        Value::Array(values) => {
            11u8.hash(state);
            hash_values(values, state);
        }
        Value::Enum(value) => {
            12u8.hash(state);
            value.hash(state);
        }
    }
}

fn hash_target_items<H: Hasher>(items: &[std::sync::Arc<TargetItemValue>], state: &mut H) {
    items.len().hash(state);
    for item in items {
        hash_target_pixels(&item.pixels, state);
    }
}

fn hash_target_pixels<H: Hasher>(pixels: &[TargetPixelValue], state: &mut H) {
    pixels.len().hash(state);
    for pixel in pixels {
        pixel.element_index.hash(state);
        pixel.element_cell_index.hash(state);
        pixel.pixel_index.hash(state);
        pixel.pixel_count.hash(state);
        pixel.pixel_fraction.to_bits().hash(state);
    }
}

fn hash_curve<H: Hasher>(curve: &Curve, state: &mut H) {
    curve.points.len().hash(state);
    for point in &curve.points {
        point.position.to_bits().hash(state);
        point.value.to_bits().hash(state);
    }
}

fn hash_gradient<H: Hasher>(gradient: &Gradient, state: &mut H) {
    gradient.stops.len().hash(state);
    for stop in &gradient.stops {
        stop.position.to_bits().hash(state);
        stop.color.hash(state);
    }
}
