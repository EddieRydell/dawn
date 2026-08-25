use dawn_language::dsl::{Identifier, Value};
use dawn_language::effect::{CurveSource, EffectParamValue, GradientSource};
use dawn_language::model::DawnProject;
use dawn_language::operator::OperatorDefinition;
use dawn_language::sequence::Sequence;
use dawn_language::values::{DawnTime, Marks};
use indexmap::IndexMap;
use std::sync::Arc;

use crate::RenderError;

#[derive(Clone, Copy)]
pub(crate) struct EffectParamTiming {
    pub(crate) start_seconds: f64,
    pub(crate) duration_seconds: f64,
}

pub(crate) fn prepare_params(
    project: &DawnProject,
    sequence: &Sequence,
    overrides: &IndexMap<Identifier, EffectParamValue>,
    timing: EffectParamTiming,
) -> Result<IndexMap<Identifier, Value>, RenderError> {
    overrides
        .iter()
        .map(|(key, value)| {
            Ok((
                key.clone(),
                prepare_param_value(project, sequence, value, timing)?,
            ))
        })
        .collect()
}

pub(crate) fn prepare_operator_params(
    project: &DawnProject,
    sequence: &Sequence,
    definition: &OperatorDefinition,
    overrides: &IndexMap<Identifier, EffectParamValue>,
    timing: EffectParamTiming,
) -> Result<IndexMap<Identifier, Value>, RenderError> {
    let mut params = definition
        .params
        .iter()
        .filter_map(|param| {
            param
                .default
                .as_ref()
                .map(|default| (param.name.clone(), default.clone()))
        })
        .collect::<IndexMap<_, _>>();
    for (name, value) in prepare_params(project, sequence, overrides, timing)? {
        params.insert(name, value);
    }
    Ok(params)
}

fn prepare_param_value(
    project: &DawnProject,
    sequence: &Sequence,
    value: &EffectParamValue,
    timing: EffectParamTiming,
) -> Result<Value, RenderError> {
    match value {
        EffectParamValue::Int(value) => Ok(Value::Int(*value)),
        EffectParamValue::Float(value) => Ok(Value::Float(*value)),
        EffectParamValue::Bool(value) => Ok(Value::Bool(*value)),
        EffectParamValue::Color(value) => Ok(Value::Color(*value)),
        EffectParamValue::Enum(value) => Ok(Value::Enum(value.clone())),
        EffectParamValue::Marks(key) => {
            let collection = sequence
                .mark_collections
                .iter()
                .find(|collection| collection.key == *key)
                .ok_or_else(|| RenderError::MissingMarkCollection { key: key.clone() })?;
            let end_seconds = timing.start_seconds + timing.duration_seconds;
            Ok(Value::Marks(Arc::new(Marks {
                marks: collection
                    .marks
                    .iter()
                    .filter_map(|mark| {
                        let seconds = mark.as_seconds_f64();
                        (seconds >= timing.start_seconds && seconds < end_seconds)
                            .then(|| DawnTime::from_seconds_f64(seconds - timing.start_seconds))
                    })
                    .collect(),
            })))
        }
        EffectParamValue::Curve(source) => Ok(Value::Curve(Arc::new(match source {
            CurveSource::Inline(curve) => curve.clone(),
            CurveSource::Reference(id) => project
                .definitions
                .curves
                .get(id)
                .ok_or(RenderError::MissingCurve)?
                .curve
                .clone(),
        }))),
        EffectParamValue::Gradient(source) => Ok(Value::Gradient(Arc::new(match source {
            GradientSource::Inline(gradient) => gradient.clone(),
            GradientSource::Reference(id) => project
                .definitions
                .gradients
                .get(id)
                .ok_or(RenderError::MissingGradient)?
                .gradient
                .clone(),
        }))),
        EffectParamValue::Array(values) => values
            .iter()
            .map(|value| prepare_param_value(project, sequence, value, timing))
            .collect::<Result<Vec<_>, _>>()
            .map(Arc::new)
            .map(Value::Array),
    }
}
