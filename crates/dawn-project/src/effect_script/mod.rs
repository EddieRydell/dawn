use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{Color, Curve, EffectParam, Flags};

mod ast;
mod lexer;
mod parser;
mod type_check;
mod vm;

#[cfg(test)]
mod tests;

pub use ast::EffectAst;
pub use lexer::{lex, Token};
pub use parser::parse;
pub use type_check::type_check;

use ast::{BinaryOp, Stmt};
use vm::Vm;

#[derive(Debug, Clone)]
pub struct ScriptDiagnostic {
    pub range: Option<SourceRange>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEffect {
    pub name: String,
    pub params: Vec<EffectParamSchema>,
    sample: Vec<Stmt>,
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
        Vm::new(self, progress, seconds, fixture, pixel, params).run()
    }
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
    Enum,
    Flags,
    Fixture,
    Pixel,
    Void,
}

impl ScriptType {
    pub fn matches_param(self, param: &EffectParam<crate::model::Resolved>) -> bool {
        match (self, param) {
            (Self::Float, EffectParam::Float { .. }) => true,
            (Self::Int, EffectParam::Integer { .. }) => true,
            (Self::Bool, EffectParam::Boolean { .. }) => true,
            (Self::Color, EffectParam::Color { .. }) => true,
            (Self::Marks, EffectParam::Marks { .. }) => true,
            (Self::Enum, EffectParam::Enum { .. }) => true,
            (Self::Flags, EffectParam::Flags { .. }) => true,
            (Self::CurveFloat, EffectParam::Curve { curve }) => {
                curve.value_type == crate::model::CurveValueType::Float
            }
            (Self::CurveColor, EffectParam::Curve { curve }) => {
                curve.value_type == crate::model::CurveValueType::Color
            }
            _ => false,
        }
    }
}

fn is_float_compatible(value_type: ScriptType) -> bool {
    matches!(value_type, ScriptType::Float | ScriptType::Int)
}

fn is_assignable(expected: ScriptType, actual: ScriptType) -> bool {
    expected == actual || (expected == ScriptType::Float && actual == ScriptType::Int)
}

fn binary_result_type(left: ScriptType, op: BinaryOp, right: ScriptType) -> Option<ScriptType> {
    if matches!(
        op,
        BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual
    ) {
        return (is_float_compatible(left) && is_float_compatible(right))
            .then_some(ScriptType::Bool);
    }

    match (left, op, right) {
        (ScriptType::Float, _, ScriptType::Float)
        | (ScriptType::Float, _, ScriptType::Int)
        | (ScriptType::Int, _, ScriptType::Float) => Some(ScriptType::Float),
        (ScriptType::Int, _, ScriptType::Int) => Some(ScriptType::Int),
        (ScriptType::Color, BinaryOp::Multiply, factor)
        | (factor, BinaryOp::Multiply, ScriptType::Color)
            if is_float_compatible(factor) =>
        {
            Some(ScriptType::Color)
        }
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
            Self::Enum => "enum",
            Self::Flags => "flags",
            Self::Fixture => "Fixture",
            Self::Pixel => "Pixel",
            Self::Void => "void",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeValue {
    Float(f64),
    Int(i64),
    Bool(bool),
    Color(Color),
    Marks(Vec<f64>),
    Curve(Curve),
    Enum(String),
    Flags(Flags),
    Fixture(FixtureContext),
    Pixel(PixelContext),
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
            Self::Enum(_) => ScriptType::Enum,
            Self::Flags(_) => ScriptType::Flags,
            Self::Fixture(_) => ScriptType::Fixture,
            Self::Pixel(_) => ScriptType::Pixel,
        }
    }
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

pub fn compile(text: &str) -> Result<CompiledEffect, Vec<ScriptDiagnostic>> {
    let tokens = lex(text)?;
    let effect = parse(&tokens)?;
    type_check(&effect)?;
    Ok(compile_ast(effect))
}

pub fn compile_ast(effect: EffectAst) -> CompiledEffect {
    CompiledEffect {
        name: effect.name,
        params: effect.params,
        sample: effect.sample,
    }
}
