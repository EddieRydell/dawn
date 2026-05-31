use std::collections::BTreeMap;

use crate::model::Color;

use super::ast::Expr;
use super::bytecode::{BytecodeStats, RegisterCounts};
use super::{
    EffectParamSchema, FixtureContext, ParamDefault, PixelContext, RuntimeError, RuntimeValue,
    ScriptDiagnostic, ScriptType,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedEffectParams {
    pub(super) values: Vec<RuntimeValue>,
}

#[derive(Debug, Clone)]
pub struct EffectSampleScratch {
    pub(super) floats: Vec<f64>,
    pub(super) ints: Vec<i64>,
    pub(super) bools: Vec<bool>,
    pub(super) colors: Vec<Color>,
    pub(super) refs: Vec<RefValue>,
    pub(super) fixtures: Vec<FixtureContext>,
    pub(super) pixels: Vec<PixelContext>,
}

impl EffectSampleScratch {
    pub fn new(stats: BytecodeStats) -> Self {
        Self {
            floats: vec![0.0; stats.float_slots],
            ints: vec![0; stats.int_slots],
            bools: vec![false; stats.bool_slots],
            colors: vec![Color::new(0, 0, 0); stats.color_slots],
            refs: vec![RefValue::Unset; stats.ref_slots],
            fixtures: vec![FixtureContext { index: 0 }; stats.fixture_slots],
            pixels: vec![PixelContext { index: 0, count: 0 }; stats.pixel_slots],
        }
    }

    pub(super) fn resize_for(&mut self, counts: RegisterCounts) {
        self.floats.resize(counts.floats, 0.0);
        self.ints.resize(counts.ints, 0);
        self.bools.resize(counts.bools, false);
        self.colors.resize(counts.colors, Color::new(0, 0, 0));
        self.refs.resize(counts.refs, RefValue::Unset);
        self.fixtures
            .resize(counts.fixtures, FixtureContext { index: 0 });
        self.pixels
            .resize(counts.pixels, PixelContext { index: 0, count: 0 });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum RefValue {
    Param(usize),
    Constant(usize),
    Unset,
}

pub(super) fn prepare_params(
    params: &[EffectParamSchema],
    values: &BTreeMap<String, RuntimeValue>,
) -> Result<PreparedEffectParams, RuntimeError> {
    prepare_params_with(params, |name| values.get(name).cloned())
}

pub(super) fn prepare_params_with(
    params: &[EffectParamSchema],
    mut value_for: impl FnMut(&str) -> Option<RuntimeValue>,
) -> Result<PreparedEffectParams, RuntimeError> {
    let mut prepared = Vec::with_capacity(params.len());
    for param in params {
        let value = value_for(&param.name)
            .or_else(|| {
                param
                    .default
                    .as_ref()
                    .map(|ParamDefault::Value(value)| value.clone())
            })
            .ok_or_else(|| RuntimeError {
                message: format!("missing parameter `{}`", param.name),
            })?;
        prepared.push(coerce_value(value, param.value_type)?);
    }
    Ok(PreparedEffectParams { values: prepared })
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
