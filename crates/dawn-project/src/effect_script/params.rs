use std::collections::BTreeMap;

use crate::model::{ArrayElementType, Color, Curve, CurveValue, CurveValueType};

use super::ast::Expr;
use super::bytecode::{BytecodeProgram, BytecodeStats, RegisterCounts};
use super::{
    EffectParamSchema, FixtureContext, ParamDefault, PixelContext, RuntimeError, RuntimeValue,
    ScriptDiagnostic, ScriptType,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedEffectParams {
    pub(super) values: Vec<RuntimeValue>,
    pub(super) curve_crossings: Vec<Option<CurveCrossingTable>>,
    pub(super) specialized_bytecode: Option<BytecodeProgram>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CurveCrossingTable {
    segments: Vec<CurveCrossingSegment>,
    monotonic: Option<CurveMonotonicity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CurveMonotonicity {
    Increasing,
    Decreasing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CurveCrossingSegment {
    time: f64,
    value: f64,
    min_value: f64,
    max_value: f64,
    inverse_slope: f64,
}

impl CurveCrossingTable {
    fn from_curve(curve: &Curve) -> Option<Self> {
        if curve.value_type != CurveValueType::Float {
            return None;
        }
        let mut segments = Vec::with_capacity(curve.points.len().saturating_sub(1));
        let mut direction = None;
        let mut mixed = false;
        for pair in curve.points.windows(2) {
            let left_point = &pair[0];
            let right_point = &pair[1];
            let CurveValue::Float(left) = left_point.value else {
                return None;
            };
            let CurveValue::Float(right) = right_point.value else {
                return None;
            };
            let span = right - left;
            let segment_direction = if span > 0.0 {
                Some(CurveMonotonicity::Increasing)
            } else if span < 0.0 {
                Some(CurveMonotonicity::Decreasing)
            } else {
                None
            };
            direction = match (direction, segment_direction) {
                (None, direction) => direction,
                (Some(left), Some(right)) if left == right => Some(left),
                (Some(left), None) => Some(left),
                _ => {
                    mixed = true;
                    None
                }
            };
            segments.push(CurveCrossingSegment {
                time: left_point.time,
                value: left,
                min_value: left.min(right),
                max_value: left.max(right),
                inverse_slope: if span.abs() < f64::EPSILON {
                    0.0
                } else {
                    (right_point.time - left_point.time) / span
                },
            });
        }
        let monotonic = (!mixed).then_some(direction).flatten();
        Some(Self {
            segments,
            monotonic,
        })
    }

    pub(super) fn crossing(&self, value: f64) -> Option<f64> {
        if let Some(monotonic) = self.monotonic {
            return self.monotonic_crossing(value, monotonic);
        }
        self.segments.iter().find_map(|segment| {
            if value < segment.min_value || value > segment.max_value {
                return None;
            }
            Some(segment.time + (value - segment.value) * segment.inverse_slope)
        })
    }

    fn monotonic_crossing(&self, value: f64, monotonic: CurveMonotonicity) -> Option<f64> {
        let index = match monotonic {
            CurveMonotonicity::Increasing => self
                .segments
                .partition_point(|segment| segment.max_value < value),
            CurveMonotonicity::Decreasing => self
                .segments
                .partition_point(|segment| segment.min_value > value),
        };
        let segment = self.segments.get(index)?;
        if value < segment.min_value || value > segment.max_value {
            return None;
        }
        Some(segment.time + (value - segment.value) * segment.inverse_slope)
    }
}

#[derive(Debug, Clone)]
pub struct EffectSampleScratch {
    pub(super) floats: Vec<f64>,
    pub(super) ints: Vec<i64>,
    pub(super) bools: Vec<bool>,
    pub(super) colors: Vec<Color>,
    pub(super) refs: Vec<RefValue>,
    pub(super) ref_values: Vec<Option<RuntimeValue>>,
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
            ref_values: vec![None; stats.ref_slots],
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
        self.ref_values.resize(counts.refs, None);
        self.fixtures
            .resize(counts.fixtures, FixtureContext { index: 0 });
        self.pixels
            .resize(counts.pixels, PixelContext { index: 0, count: 0 });
    }
}

impl PreparedEffectParams {
    pub fn values(&self) -> &[RuntimeValue] {
        &self.values
    }

    pub(super) fn with_specialized_bytecode(mut self, bytecode: BytecodeProgram) -> Self {
        self.specialized_bytecode = Some(bytecode);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum RefValue {
    Param(usize),
    Constant(usize),
    Local,
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
    let mut curve_crossings = Vec::with_capacity(params.len());
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
        let value = coerce_value(value, &param.value_type)?;
        curve_crossings.push(match &value {
            RuntimeValue::Curve(curve) => CurveCrossingTable::from_curve(curve),
            _ => None,
        });
        prepared.push(value);
    }
    Ok(PreparedEffectParams {
        values: prepared,
        curve_crossings,
        specialized_bytecode: None,
    })
}

pub(super) fn eval_constant(expr: &Expr) -> Result<RuntimeValue, ScriptDiagnostic> {
    match expr {
        Expr::Float(value) => Ok(RuntimeValue::Float(*value)),
        Expr::Int(value) => Ok(RuntimeValue::Int(*value)),
        Expr::Bool(value) => Ok(RuntimeValue::Bool(*value)),
        Expr::Color(value) => Ok(RuntimeValue::Color(*value)),
        Expr::Array(items) => eval_constant_array(items),
        _ => Err(ScriptDiagnostic {
            range: None,
            message: "parameter defaults must be literals in Dawn v1".to_string(),
        }),
    }
}

fn eval_constant_array(items: &[Expr]) -> Result<RuntimeValue, ScriptDiagnostic> {
    let mut values = Vec::with_capacity(items.len());
    let mut element_type = None;
    for item in items {
        let value = eval_constant(item)?;
        let item_type = match value.value_type() {
            ScriptType::Int => ArrayElementType::Int,
            ScriptType::Float => ArrayElementType::Float,
            ScriptType::Bool => ArrayElementType::Bool,
            ScriptType::Color => ArrayElementType::Color,
            ScriptType::CurveFloat => ArrayElementType::CurveFloat,
            ScriptType::CurveColor => ArrayElementType::CurveColor,
            other => {
                return Err(ScriptDiagnostic {
                    range: None,
                    message: format!("array literal cannot contain {other} values"),
                });
            }
        };
        element_type = match element_type {
            None => Some(item_type),
            Some(ArrayElementType::Float) if item_type == ArrayElementType::Int => {
                Some(ArrayElementType::Float)
            }
            Some(ArrayElementType::Int) if item_type == ArrayElementType::Float => {
                Some(ArrayElementType::Float)
            }
            Some(expected) if expected == item_type => Some(expected),
            Some(expected) => {
                return Err(ScriptDiagnostic {
                    range: None,
                    message: format!(
                        "array literal elements must all be {}, but found {}",
                        array_element_label(expected),
                        array_element_label(item_type)
                    ),
                });
            }
        };
        values.push(value);
    }
    Ok(RuntimeValue::Array(super::RuntimeArrayValue {
        element_type: element_type.unwrap_or(ArrayElementType::Float),
        values,
    }))
}

pub(super) fn coerce_value(
    value: RuntimeValue,
    expected: &ScriptType,
) -> Result<RuntimeValue, RuntimeError> {
    match (expected, value) {
        (ScriptType::Float, RuntimeValue::Int(value)) => Ok(RuntimeValue::Float(value as f64)),
        (ScriptType::Array(expected), RuntimeValue::Array(value)) => {
            coerce_array_value(value, *expected)
        }
        (expected, value) if value.value_type() == *expected => Ok(value),
        (expected, value) => Err(RuntimeError {
            message: format!(
                "expected {expected} value, but found {}",
                value.value_type()
            ),
        }),
    }
}

fn coerce_array_value(
    value: super::RuntimeArrayValue,
    expected: ArrayElementType,
) -> Result<RuntimeValue, RuntimeError> {
    let mut values = Vec::with_capacity(value.values.len());
    for item in value.values {
        values.push(match expected {
            ArrayElementType::Float => match item {
                RuntimeValue::Int(value) => RuntimeValue::Float(value as f64),
                RuntimeValue::Float(_) => item,
                item => {
                    return Err(RuntimeError {
                        message: format!(
                            "expected array<float> element, but found {}",
                            item.value_type()
                        ),
                    });
                }
            },
            ArrayElementType::Int if matches!(item, RuntimeValue::Int(_)) => item,
            ArrayElementType::Bool if matches!(item, RuntimeValue::Bool(_)) => item,
            ArrayElementType::Color if matches!(item, RuntimeValue::Color(_)) => item,
            ArrayElementType::CurveFloat => match item {
                RuntimeValue::Curve(curve) if curve.value_type == CurveValueType::Float => {
                    RuntimeValue::Curve(curve)
                }
                item => {
                    return Err(RuntimeError {
                        message: format!(
                            "expected array<curve<float>> element, but found {}",
                            item.value_type()
                        ),
                    });
                }
            },
            ArrayElementType::CurveColor => match item {
                RuntimeValue::Curve(curve) if curve.value_type == CurveValueType::Color => {
                    RuntimeValue::Curve(curve)
                }
                item => {
                    return Err(RuntimeError {
                        message: format!(
                            "expected array<curve<color>> element, but found {}",
                            item.value_type()
                        ),
                    });
                }
            },
            expected => {
                return Err(RuntimeError {
                    message: format!(
                        "expected array<{}> element, but found {}",
                        array_element_label(expected),
                        item.value_type()
                    ),
                });
            }
        });
    }
    Ok(RuntimeValue::Array(super::RuntimeArrayValue {
        element_type: expected,
        values,
    }))
}

fn array_element_label(element_type: ArrayElementType) -> &'static str {
    match element_type {
        ArrayElementType::Int => "int",
        ArrayElementType::Float => "float",
        ArrayElementType::Bool => "bool",
        ArrayElementType::Color => "color",
        ArrayElementType::CurveFloat => "curve<float>",
        ArrayElementType::CurveColor => "curve<color>",
    }
}
