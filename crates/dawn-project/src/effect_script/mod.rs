use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::diagnostics::TextRange;
use crate::model::{ArrayElementType, Color, Curve, EffectParam, EffectParamArrayValue, Flags};

mod ast;
mod builtins;
mod bytecode;
mod compile;
#[cfg(test)]
mod generator;
mod lexer;
mod params;
mod parser;
mod runtime;
mod type_check;

#[cfg(test)]
mod tests;

pub(crate) use ast::EffectEntrypoint;
pub use ast::EffectVisibility;
pub use ast::{EffectAst, EffectModuleAst};
pub(crate) use ast::{EffectImport, Stmt};
pub use lexer::lex;
pub use lexer::Token;
#[cfg(test)]
use parser::parse;
pub use parser::parse_module;
#[cfg(test)]
use type_check::type_check;
pub(crate) use type_check::{type_check_with_imports, ImportedEffect};

#[cfg(test)]
use generator::run_generator;

use ast::BinaryOp;
use bytecode::{specialize_for_params, BytecodeProgram};
pub use params::PreparedEffectParams;

#[derive(Debug, Clone)]
pub struct ScriptDiagnostic {
    pub range: Option<TextRange>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEffect {
    pub(crate) name: String,
    pub(crate) visibility: EffectVisibility,
    pub(crate) kind: EffectScriptKind,
    pub(crate) imports: Vec<EffectImport>,
    pub(crate) params: Vec<EffectParamSchema>,
    bytecode: Option<BytecodeProgram>,
    generator: Option<Vec<Stmt>>,
}

impl CompiledEffect {
    pub fn param(&self, name: &str) -> Option<&EffectParamSchema> {
        self.params.iter().find(|param| param.name == name)
    }

    pub fn sample(
        &self,
        progress: f64,
        seconds: f64,
        fixture: FixtureContext,
        pixel: PixelContext,
        params: &BTreeMap<String, RuntimeValue>,
    ) -> Result<Color, RuntimeError> {
        let prepared = self.prepare_params(params)?;
        self.sample_prepared(progress, seconds, fixture, pixel, &prepared)
    }

    pub fn prepare_params(
        &self,
        params: &BTreeMap<String, RuntimeValue>,
    ) -> Result<PreparedEffectParams, RuntimeError> {
        self.specialize_prepared_params(params::prepare_params(&self.params, params)?)
    }

    pub fn prepare_params_with(
        &self,
        value_for: impl FnMut(&str) -> Option<RuntimeValue>,
    ) -> Result<PreparedEffectParams, RuntimeError> {
        self.specialize_prepared_params(params::prepare_params_with(&self.params, value_for)?)
    }

    fn specialize_prepared_params(
        &self,
        prepared: PreparedEffectParams,
    ) -> Result<PreparedEffectParams, RuntimeError> {
        Ok(match &self.bytecode {
            Some(bytecode) => {
                let specialized = specialize_for_params(bytecode, prepared.values());
                prepared.with_specialized_bytecode(specialized)
            }
            None => prepared,
        })
    }

    pub fn sample_prepared(
        &self,
        progress: f64,
        seconds: f64,
        fixture: FixtureContext,
        pixel: PixelContext,
        params: &PreparedEffectParams,
    ) -> Result<Color, RuntimeError> {
        let bytecode = self.bytecode.as_ref().ok_or_else(|| RuntimeError {
            message: format!("effect `{}` is not a sample effect", self.name),
        })?;
        runtime::run(bytecode, progress, seconds, fixture, pixel, params)
    }

    #[cfg(test)]
    fn generator_statements(&self) -> Option<&[Stmt]> {
        self.generator.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectScriptKind {
    Sample,
    Generator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixtureContext {
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelContext {
    pub index: usize,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectParamSchema {
    pub name: String,
    pub value_type: ScriptType,
    pub options: Vec<String>,
    pub default: Option<ParamDefault>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamDefault {
    Value(RuntimeValue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptType {
    Float,
    Int,
    Bool,
    Color,
    Marks,
    CurveFloat,
    CurveColor,
    Array(ArrayElementType),
    Enum,
    Flags,
    Fixture,
    Pixel,
    Timeline,
    Target,
    TargetItems,
    TargetItem,
    Void,
}

impl ScriptType {
    pub fn matches_param(self, param: &EffectParam<crate::model::Resolved>) -> bool {
        match (&self, param) {
            (Self::Float, EffectParam::Float { .. }) => true,
            (Self::Int, EffectParam::Integer { .. }) => true,
            (Self::Bool, EffectParam::Boolean { .. }) => true,
            (Self::Color, EffectParam::Color { .. }) => true,
            (Self::Marks, EffectParam::Marks { .. }) => true,
            (Self::Enum, EffectParam::Enum { .. }) => true,
            (Self::Flags, EffectParam::Flags { .. }) => true,
            (Self::CurveFloat, EffectParam::Curve { curve }) => {
                resolved_curve_value_type(&curve.curve) == Some(crate::model::CurveValueType::Float)
            }
            (Self::CurveColor, EffectParam::Curve { curve }) => {
                resolved_curve_value_type(&curve.curve) == Some(crate::model::CurveValueType::Color)
            }
            (
                Self::Array(expected),
                EffectParam::Array {
                    element_type,
                    values,
                },
            ) => {
                *expected == *element_type
                    && values
                        .iter()
                        .all(|value| array_param_value_matches(*expected, value))
            }
            _ => false,
        }
    }
}

fn array_param_value_matches(
    expected: ArrayElementType,
    value: &EffectParamArrayValue<crate::model::Resolved>,
) -> bool {
    match (expected, value) {
        (ArrayElementType::Int, EffectParamArrayValue::Integer(_)) => true,
        (ArrayElementType::Float, EffectParamArrayValue::Float(_))
        | (ArrayElementType::Float, EffectParamArrayValue::Integer(_)) => true,
        (ArrayElementType::Bool, EffectParamArrayValue::Boolean(_)) => true,
        (ArrayElementType::Color, EffectParamArrayValue::Color(_)) => true,
        (ArrayElementType::CurveFloat, EffectParamArrayValue::Curve(curve)) => {
            resolved_curve_value_type(&curve.curve) == Some(crate::model::CurveValueType::Float)
        }
        (ArrayElementType::CurveColor, EffectParamArrayValue::Curve(curve)) => {
            resolved_curve_value_type(&curve.curve) == Some(crate::model::CurveValueType::Color)
        }
        _ => false,
    }
}

fn resolved_curve_value_type(
    curve: &crate::model::ResolvedInlineOrRef<
        crate::model::Curve,
        crate::model::CurveDefinitionKey,
    >,
) -> Option<crate::model::CurveValueType> {
    match curve {
        crate::model::ResolvedInlineOrRef::Inline(curve) => Some(curve.value_type),
        crate::model::ResolvedInlineOrRef::Ref(_) => None,
    }
}

fn is_float_compatible(value_type: &ScriptType) -> bool {
    matches!(value_type, ScriptType::Float | ScriptType::Int)
}

fn is_assignable(expected: &ScriptType, actual: &ScriptType) -> bool {
    expected == actual || (*expected == ScriptType::Float && *actual == ScriptType::Int)
}

fn binary_result_type(left: &ScriptType, op: BinaryOp, right: &ScriptType) -> Option<ScriptType> {
    if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
        return (*left == ScriptType::Bool && *right == ScriptType::Bool)
            .then_some(ScriptType::Bool);
    }

    if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) {
        return ((is_float_compatible(left) && is_float_compatible(right))
            || (*left == ScriptType::Bool && *right == ScriptType::Bool)
            || (*left == ScriptType::Enum && *right == ScriptType::Enum))
            .then_some(ScriptType::Bool);
    }

    if matches!(
        op,
        BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual
    ) {
        return (is_float_compatible(left) && is_float_compatible(right))
            .then_some(ScriptType::Bool);
    }

    match (left, op, right) {
        (
            ScriptType::Float,
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide,
            ScriptType::Float,
        )
        | (
            ScriptType::Float,
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide,
            ScriptType::Int,
        )
        | (
            ScriptType::Int,
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide,
            ScriptType::Float,
        ) => Some(ScriptType::Float),
        (
            ScriptType::Int,
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide,
            ScriptType::Int,
        ) => Some(ScriptType::Int),
        (ScriptType::Color, BinaryOp::Multiply, factor)
        | (factor, BinaryOp::Multiply, ScriptType::Color)
            if is_float_compatible(factor) =>
        {
            Some(ScriptType::Color)
        }
        (ScriptType::Int, BinaryOp::Modulo, ScriptType::Int) => Some(ScriptType::Int),
        _ => None,
    }
}

impl fmt::Display for ScriptType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Float => "float",
            Self::Int => "int",
            Self::Bool => "bool",
            Self::Color => "color",
            Self::Marks => "marks",
            Self::CurveFloat => "curve<float>",
            Self::CurveColor => "curve<color>",
            Self::Array(element_type) => {
                return write!(
                    formatter,
                    "array<{}>",
                    array_element_type_label(*element_type)
                )
            }
            Self::Enum => "enum",
            Self::Flags => "flags",
            Self::Fixture => "Fixture",
            Self::Pixel => "Pixel",
            Self::Timeline => "Timeline",
            Self::Target => "Target",
            Self::TargetItems => "TargetItems",
            Self::TargetItem => "TargetItem",
            Self::Void => "void",
        })
    }
}

fn array_element_type_label(element_type: ArrayElementType) -> &'static str {
    match element_type {
        ArrayElementType::Int => "int",
        ArrayElementType::Float => "float",
        ArrayElementType::Bool => "bool",
        ArrayElementType::Color => "color",
        ArrayElementType::CurveFloat => "curve<float>",
        ArrayElementType::CurveColor => "curve<color>",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeValue {
    Float(f64),
    Int(i64),
    Bool(bool),
    Color(Color),
    Marks(RuntimeMarks),
    Curve(Curve),
    Array(RuntimeArrayValue),
    Enum(String),
    Flags(Flags),
    Fixture(FixtureContext),
    Pixel(PixelContext),
    Target(GeneratorTarget),
    TargetItems(Vec<GeneratorTargetItem>),
    TargetItem(GeneratorTargetItem),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeMarks {
    pub windowed: Vec<f64>,
    pub global: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeArrayValue {
    pub element_type: ArrayElementType,
    pub values: Vec<RuntimeValue>,
}

impl RuntimeValue {
    pub(super) fn value_type(&self) -> ScriptType {
        match self {
            Self::Float(_) => ScriptType::Float,
            Self::Int(_) => ScriptType::Int,
            Self::Bool(_) => ScriptType::Bool,
            Self::Color(_) => ScriptType::Color,
            Self::Marks(_) => ScriptType::Marks,
            Self::Curve(curve) => match curve.value_type {
                crate::model::CurveValueType::Float => ScriptType::CurveFloat,
                crate::model::CurveValueType::Color => ScriptType::CurveColor,
            },
            Self::Array(value) => ScriptType::Array(value.element_type),
            Self::Enum(_) => ScriptType::Enum,
            Self::Flags(_) => ScriptType::Flags,
            Self::Fixture(_) => ScriptType::Fixture,
            Self::Pixel(_) => ScriptType::Pixel,
            Self::Target(_) => ScriptType::Target,
            Self::TargetItems(_) => ScriptType::TargetItems,
            Self::TargetItem(_) => ScriptType::TargetItem,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorTarget {
    pub pixels: Vec<GeneratorTargetPixel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorTargetPixel {
    pub fixture_index: usize,
    pub pixel_index: usize,
    pub pixel_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorTargetItem {
    pub target: GeneratorTarget,
    pub index: usize,
    pub count: usize,
    pub position: usize,
    pub fixture_index: usize,
    pub pixel_start: usize,
    pub pixel_count: usize,
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RuntimeError {}

#[cfg(test)]
fn compile(text: &str) -> Result<CompiledEffect, Vec<ScriptDiagnostic>> {
    let tokens = lex(text)?;
    let effect = parse(&tokens)?;
    type_check(&effect)?;
    Ok(compile_ast(effect))
}

#[cfg(test)]
fn compile_with_imports(
    text: &str,
    imports: &[ImportedEffect<'_>],
) -> Result<CompiledEffect, Vec<ScriptDiagnostic>> {
    let tokens = lex(text)?;
    let effect = parse(&tokens)?;
    type_check_with_imports(&effect, imports)?;
    Ok(compile_ast(effect))
}

pub(crate) fn compile_module_with_imports(
    text: &str,
    imports: &[ImportedEffect<'_>],
) -> Result<Vec<CompiledEffect>, Vec<ScriptDiagnostic>> {
    let tokens = lex(text)?;
    let module = parse_module(&tokens)?;
    let mut available = imports.to_vec();
    for effect in &module.effects {
        available.push(ImportedEffect {
            alias: None,
            name: effect.name.as_str(),
            params: &effect.params,
        });
    }
    for effect in &module.effects {
        type_check_with_imports(effect, &available)?;
    }
    Ok(module.effects.into_iter().map(compile_ast).collect())
}

pub(crate) fn compile_ast(effect: EffectAst) -> CompiledEffect {
    let kind = kind_for_entrypoint(&effect.entrypoint);
    let bytecode = if kind == EffectScriptKind::Sample {
        Some(compile::compile_effect(&effect))
    } else {
        None
    };
    let generator = match &effect.entrypoint {
        EffectEntrypoint::Generator(statements) => Some(statements.clone()),
        EffectEntrypoint::Sample(_) => None,
    };
    CompiledEffect {
        name: effect.name,
        visibility: effect.visibility,
        kind,
        imports: effect.imports,
        params: effect.params,
        bytecode,
        generator,
    }
}

fn kind_for_entrypoint(entrypoint: &EffectEntrypoint) -> EffectScriptKind {
    match entrypoint {
        EffectEntrypoint::Sample(_) => EffectScriptKind::Sample,
        EffectEntrypoint::Generator(_) => EffectScriptKind::Generator,
    }
}
