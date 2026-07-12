pub(in crate::gui) fn effect_params(
    session: &ProjectSession,
    sequence: &dawn_language::sequence::Sequence,
    effect: &dawn_language::effect::EffectInst,
) -> Vec<SequenceEffectParam> {
    let Some(definition) = session.project.definitions.effects.get(&effect.definition) else {
        return Vec::new();
    };
    definition
        .compiled
        .params()
        .iter()
        .filter_map(|param| {
            let kind = param_kind(&param.ty)?;
            let override_value = effect.param_overrides.get(&param.name);
            let value = override_value
                .map(|value| effect_param_value(session, value))
                .or_else(|| param.default.as_ref().and_then(default_param_value))
                .or_else(|| default_value_for_type(&param.ty))?;
            Some(SequenceEffectParam {
                name: param.name.as_str().to_string(),
                kind,
                options: param_options(&param.ty),
                editable: automation_for_param(sequence, effect.id.0, param.name.as_str())
                    .is_none(),
                curve_source: override_value.and_then(curve_source),
                automation: automation_for_param(sequence, effect.id.0, param.name.as_str()),
                value,
            })
        })
        .collect()
}

pub(in crate::gui) fn sequence_composition_graph_node(
    session: &ProjectSession,
    sequence: &dawn_language::sequence::Sequence,
    node: &CompositionGraphNode,
) -> SequenceGraphNode {
    SequenceGraphNode {
        id: graph_node_id(&node.id),
        x: node.position.x,
        y: node.position.y,
        inputs: graph_node_inputs(session, &node.kind),
        outputs: graph_node_outputs(session, &node.kind),
        kind: match &node.kind {
            CompositionGraphNodeKind::Layer { layer_id } => {
                let layer = sequence.layers.iter().find(|layer| layer.id == *layer_id);
                SequenceGraphNodeKind::Layer {
                    layer_id: layer_id.0,
                    layer_name: layer
                        .map(|layer| layer.name.clone())
                        .unwrap_or_else(|| format!("Layer {}", layer_id.0)),
                    layer_color: layer
                        .map(|layer| layer.color.to_hex())
                        .unwrap_or_else(|| "#808080".to_string()),
                    enabled: layer.map(|layer| layer.enabled).unwrap_or(false),
                }
            }
            CompositionGraphNodeKind::Operator(operator) => SequenceGraphNodeKind::Operator {
                operator: graph_operator_to_gui(&operator.operator),
                params: graph_operator_params(session, sequence, &node.id, operator),
            },
            CompositionGraphNodeKind::Output => SequenceGraphNodeKind::Output,
        },
    }
}

pub(in crate::gui) fn graph_node_id(node_id: &CompositionGraphNodeId) -> String {
    format!("node:{}", node_id.0)
}

fn graph_operator_params(
    session: &ProjectSession,
    sequence: &dawn_language::sequence::Sequence,
    node_id: &CompositionGraphNodeId,
    operator: &GraphOperatorNode,
) -> Vec<SequenceEffectParam> {
    let Some(definition) = session
        .project
        .definitions
        .operators
        .resolve(&operator.operator)
    else {
        return Vec::new();
    };
    definition
        .params
        .iter()
        .filter_map(|declaration| {
            let kind = param_kind(&declaration.ty)?;
            let override_value = operator.params.get(&declaration.name);
            let value = override_value
                .map(|value| effect_param_value(session, value))
                .or_else(|| declaration.default.as_ref().and_then(default_param_value))?;
            Some(SequenceEffectParam {
                name: declaration.name.as_str().to_string(),
                kind,
                options: param_options(&declaration.ty),
                editable: automation_for_composition_param(
                    sequence,
                    node_id,
                    declaration.name.as_str(),
                )
                .is_none(),
                value,
                curve_source: override_value.and_then(curve_source),
                automation: automation_for_composition_param(
                    sequence,
                    node_id,
                    declaration.name.as_str(),
                ),
            })
        })
        .collect()
}

pub(in crate::gui) fn graph_operator_definition_to_gui(
    operator: OperatorRef,
    definition: &OperatorDefinition,
) -> SequenceGraphOperatorDefinition {
    SequenceGraphOperatorDefinition {
        operator: graph_operator_to_gui(&operator),
        source_name: definition.source_name.clone(),
        display_name: definition.display_name.clone(),
        inputs: definition.inputs.iter().map(graph_port_to_gui).collect(),
        outputs: vec![graph_port_to_gui(&definition.output)],
        params: definition
            .params
            .iter()
            .filter_map(|param| {
                Some(crate::dto::SequenceEffectScriptParam {
                    name: param.name.as_str().to_string(),
                    kind: param_kind(&param.ty)?,
                })
            })
            .collect(),
    }
}

fn graph_port_to_gui(port: &OperatorPortDefinition) -> SequenceGraphPortDefinition {
    SequenceGraphPortDefinition {
        source_name: port.source_name.to_string(),
        display_name: port.display_name.to_string(),
        cardinality: match port.cardinality {
            OperatorPortCardinality::One => SequenceGraphPortCardinality::One,
            OperatorPortCardinality::Many => SequenceGraphPortCardinality::Many,
        },
    }
}

fn graph_node_inputs(
    session: &ProjectSession,
    kind: &CompositionGraphNodeKind,
) -> Vec<SequenceGraphPortDefinition> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => vec![],
        CompositionGraphNodeKind::Operator(operator) => session
            .project
            .definitions
            .operators
            .resolve(&operator.operator)
            .into_iter()
            .flat_map(|definition| definition.inputs.iter())
            .map(graph_port_to_gui)
            .collect(),
        CompositionGraphNodeKind::Output => vec![SequenceGraphPortDefinition {
            source_name: "input".to_string(),
            display_name: "Input".to_string(),
            cardinality: SequenceGraphPortCardinality::Many,
        }],
    }
}

fn graph_node_outputs(
    session: &ProjectSession,
    kind: &CompositionGraphNodeKind,
) -> Vec<SequenceGraphPortDefinition> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => {
            vec![SequenceGraphPortDefinition {
                source_name: "output".to_string(),
                display_name: "Output".to_string(),
                cardinality: SequenceGraphPortCardinality::Many,
            }]
        }
        CompositionGraphNodeKind::Operator(operator) => session
            .project
            .definitions
            .operators
            .resolve(&operator.operator)
            .map(|definition| vec![graph_port_to_gui(&definition.output)])
            .unwrap_or_default(),
        CompositionGraphNodeKind::Output => vec![],
    }
}

fn graph_operator_to_gui(operator: &OperatorRef) -> SequenceGraphOperator {
    match operator {
        OperatorRef::Builtin(operator) => SequenceGraphOperator::Builtin {
            operator: match operator {
                BuiltinOperator::Max => SequenceBuiltinOperator::Max,
                BuiltinOperator::Add => SequenceBuiltinOperator::Add,
                BuiltinOperator::Multiply => SequenceBuiltinOperator::Multiply,
                BuiltinOperator::IntensityModulate => SequenceBuiltinOperator::IntensityModulate,
                BuiltinOperator::Dim => SequenceBuiltinOperator::Dim,
                BuiltinOperator::Invert => SequenceBuiltinOperator::Invert,
                BuiltinOperator::Colorize => SequenceBuiltinOperator::Colorize,
                BuiltinOperator::Delay => SequenceBuiltinOperator::Delay,
                BuiltinOperator::Echo => SequenceBuiltinOperator::Echo,
            },
        },
        OperatorRef::Custom(id) => SequenceGraphOperator::Custom {
            path: id.0.document().to_string(),
            object_key: id.0.object().to_string(),
        },
    }
}

fn automation_for_param(
    sequence: &dawn_language::sequence::Sequence,
    effect_id: u32,
    param: &str,
) -> Option<SequenceParamAutomation> {
    sequence.automation_clips.iter().find_map(|clip| {
        clip.bindings
            .iter()
            .find(|binding| {
                binding
                    .effect_param()
                    .is_some_and(|(target_effect, target_param)| {
                        target_effect.0 == effect_id && target_param.as_str() == param
                    })
            })
            .map(|binding| SequenceParamAutomation {
                clip_id: clip.id.0,
                mapping: automation_mapping_to_gui(&binding.mapping),
            })
    })
}

fn automation_for_composition_param(
    sequence: &dawn_language::sequence::Sequence,
    node_id: &CompositionGraphNodeId,
    param: &str,
) -> Option<SequenceParamAutomation> {
    sequence.automation_clips.iter().find_map(|clip| {
        clip.bindings
            .iter()
            .find(|binding| {
                matches!(
                    &binding.target,
                    AutomationTarget::CompositionNodeParam {
                        node_id: target_node_id,
                        param: target_param,
                    } if target_node_id == node_id && target_param.as_str() == param
                )
            })
            .map(|binding| SequenceParamAutomation {
                clip_id: clip.id.0,
                mapping: automation_mapping_to_gui(&binding.mapping),
            })
    })
}

pub(in crate::gui) fn automation_mapping_to_gui(
    mapping: &AutomationMapping,
) -> SequenceAutomationMapping {
    match mapping {
        AutomationMapping::Float { min, max } => SequenceAutomationMapping::Float {
            min: *min,
            max: *max,
        },
        AutomationMapping::Int { min, max } => SequenceAutomationMapping::Int {
            min: *min as f64,
            max: *max as f64,
        },
        AutomationMapping::Bool => SequenceAutomationMapping::Bool,
        AutomationMapping::Enum { values } => SequenceAutomationMapping::Enum {
            values: values
                .iter()
                .map(|value| value.as_str().to_string())
                .collect(),
        },
        AutomationMapping::FloatCurve { min, max } => SequenceAutomationMapping::FloatCurve {
            min: *min,
            max: *max,
        },
    }
}

pub(in crate::gui) fn curve_library(session: &ProjectSession) -> Vec<SequenceCurveLibraryItem> {
    session
        .project
        .definitions
        .curves
        .definitions
        .iter()
        .filter_map(|(id, definition)| {
            let points = curve_points(&definition.curve.points)?;
            Some(SequenceCurveLibraryItem {
                path: id.0.document().to_string(),
                object_key: id.0.object().to_string(),
                display_name: id.0.object().to_string(),
                value_type: match &points {
                    SequenceCurveLibraryPoints::Float { .. } => SequenceCurveValueType::Float,
                    SequenceCurveLibraryPoints::Color { .. } => SequenceCurveValueType::Color,
                },
                points,
            })
        })
        .collect()
}

pub(in crate::gui) fn fixture_source_ref(id: &FixtureDefinitionId) -> Option<GuiObjectRef> {
    Some(GuiObjectRef {
        path: id.0.document().to_string(),
        object_key: id.0.object().to_string(),
        kind: ObjectKind::Fixture,
        id: id.0.object().to_string(),
    })
}

pub(in crate::gui) fn param_kind(ty: &Type) -> Option<SequenceEffectParamKind> {
    Some(match ty {
        Type::Int => SequenceEffectParamKind::Int,
        Type::Float => SequenceEffectParamKind::Float,
        Type::Bool => SequenceEffectParamKind::Bool,
        Type::Color => SequenceEffectParamKind::Color,
        Type::Enum(_) => SequenceEffectParamKind::Enum,
        Type::Marks => SequenceEffectParamKind::Marks,
        Type::Curve(inner) => match inner.as_ref() {
            Type::Color => SequenceEffectParamKind::ColorCurve,
            _ => SequenceEffectParamKind::FloatCurve,
        },
        Type::Array(inner) => match inner.as_ref() {
            Type::Int => SequenceEffectParamKind::IntArray,
            Type::Float => SequenceEffectParamKind::FloatArray,
            Type::Bool => SequenceEffectParamKind::BoolArray,
            Type::Color => SequenceEffectParamKind::ColorArray,
            Type::Curve(curve_inner) => match curve_inner.as_ref() {
                Type::Color => SequenceEffectParamKind::ColorCurveArray,
                _ => SequenceEffectParamKind::FloatCurveArray,
            },
            _ => SequenceEffectParamKind::FloatArray,
        },
        Type::Void
        | Type::Signal
        | Type::Timeline
        | Type::Target
        | Type::TargetItems
        | Type::TargetItem => {
            return None;
        }
    })
}

fn param_options(ty: &Type) -> Vec<String> {
    match ty {
        Type::Enum(options) => options
            .iter()
            .map(|option| option.as_str().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

pub(in crate::gui) fn effect_param_value(
    session: &ProjectSession,
    value: &EffectParamValue,
) -> SequenceEffectParamValue {
    match value {
        EffectParamValue::Int(value) => SequenceEffectParamValue::Int {
            value: *value as f64,
        },
        EffectParamValue::Float(value) => SequenceEffectParamValue::Float { value: *value },
        EffectParamValue::Bool(value) => SequenceEffectParamValue::Bool { value: *value },
        EffectParamValue::Color(value) => SequenceEffectParamValue::Color {
            value: value.to_hex(),
        },
        EffectParamValue::Enum(value) => SequenceEffectParamValue::Enum {
            value: value.as_str().to_string(),
        },
        EffectParamValue::Marks(value) => SequenceEffectParamValue::Marks {
            key: value.name.clone(),
        },
        EffectParamValue::Curve(source) => match source {
            CurveSource::Inline(curve) => curve_points(&curve.points)
                .map(curve_points_param_value)
                .unwrap_or_else(|| SequenceEffectParamValue::FloatCurve { points: Vec::new() }),
            CurveSource::Reference(id) => session
                .project
                .definitions
                .curves
                .get(id)
                .and_then(|definition| curve_points(&definition.curve.points))
                .map(curve_points_param_value)
                .unwrap_or_else(|| SequenceEffectParamValue::FloatCurve { points: Vec::new() }),
        },
        EffectParamValue::Array(values) => array_param_value(session, values),
    }
}

pub(in crate::gui) fn default_param_value(value: &EffectValue) -> Option<SequenceEffectParamValue> {
    Some(match value {
        EffectValue::Int(value) => SequenceEffectParamValue::Int {
            value: *value as f64,
        },
        EffectValue::Float(value) => SequenceEffectParamValue::Float { value: *value },
        EffectValue::Bool(value) => SequenceEffectParamValue::Bool { value: *value },
        EffectValue::Color(value) => SequenceEffectParamValue::Color {
            value: value.to_hex(),
        },
        EffectValue::Enum(value) => SequenceEffectParamValue::Enum {
            value: value.as_str().to_string(),
        },
        EffectValue::Marks(_) => SequenceEffectParamValue::Marks { key: String::new() },
        EffectValue::Curve(curve) => curve_points(&curve.points)
            .map(curve_points_param_value)
            .unwrap_or_else(|| SequenceEffectParamValue::FloatCurve { points: Vec::new() }),
        EffectValue::Array(values) => {
            let converted = values
                .iter()
                .map(default_param_value)
                .collect::<Option<Vec<_>>>()?;
            array_param_from_sequence_values(&converted)
        }
        EffectValue::Void
        | EffectValue::Target(_)
        | EffectValue::TargetItems(_)
        | EffectValue::TargetItem(_) => return None,
    })
}

fn default_value_for_type(ty: &Type) -> Option<SequenceEffectParamValue> {
    default_param_value(&ty.default_value())
}

fn curve_source(value: &EffectParamValue) -> Option<SequenceEffectParamCurveSource> {
    match value {
        EffectParamValue::Curve(CurveSource::Inline(_)) => {
            Some(SequenceEffectParamCurveSource::Inline)
        }
        EffectParamValue::Curve(CurveSource::Reference(id)) => {
            Some(SequenceEffectParamCurveSource::Library {
                reference: id.0.object().to_string(),
                path: Some(id.0.document().to_string()),
                object_key: Some(id.0.object().to_string()),
                display_name: Some(id.0.object().to_string()),
            })
        }
        _ => None,
    }
}

fn curve_points(
    points: &[dawn_language::values::CurvePoint],
) -> Option<SequenceCurveLibraryPoints> {
    let first = points.first()?;
    match first.value {
        CurveValue::Float(_) => Some(SequenceCurveLibraryPoints::Float {
            points: points
                .iter()
                .filter_map(|point| match point.value {
                    CurveValue::Float(value) => Some(FloatCurvePoint {
                        time: point.position,
                        value,
                    }),
                    CurveValue::Color(_) => None,
                })
                .collect(),
        }),
        CurveValue::Color(_) => Some(SequenceCurveLibraryPoints::Color {
            points: points
                .iter()
                .filter_map(|point| match point.value {
                    CurveValue::Color(value) => Some(ColorCurvePoint {
                        time: point.position,
                        value: value.to_hex(),
                    }),
                    CurveValue::Float(_) => None,
                })
                .collect(),
        }),
    }
}

fn curve_points_param_value(points: SequenceCurveLibraryPoints) -> SequenceEffectParamValue {
    match points {
        SequenceCurveLibraryPoints::Float { points } => {
            SequenceEffectParamValue::FloatCurve { points }
        }
        SequenceCurveLibraryPoints::Color { points } => {
            SequenceEffectParamValue::ColorCurve { points }
        }
    }
}

fn array_param_value(
    session: &ProjectSession,
    values: &[EffectParamValue],
) -> SequenceEffectParamValue {
    let converted = values
        .iter()
        .map(|value| effect_param_value(session, value))
        .collect::<Vec<_>>();
    array_param_from_sequence_values(&converted)
}

fn array_param_from_sequence_values(
    values: &[SequenceEffectParamValue],
) -> SequenceEffectParamValue {
    match values.first() {
        Some(SequenceEffectParamValue::Int { .. }) => SequenceEffectParamValue::IntArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Int { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
        Some(SequenceEffectParamValue::Bool { .. }) => SequenceEffectParamValue::BoolArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Bool { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
        Some(SequenceEffectParamValue::Color { .. }) => SequenceEffectParamValue::ColorArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Color { value } => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        },
        Some(SequenceEffectParamValue::ColorCurve { .. }) => {
            SequenceEffectParamValue::ColorCurveArray {
                values: values
                    .iter()
                    .filter_map(|value| match value {
                        SequenceEffectParamValue::ColorCurve { points } => Some(points.clone()),
                        _ => None,
                    })
                    .collect(),
            }
        }
        Some(SequenceEffectParamValue::FloatCurve { .. }) => {
            SequenceEffectParamValue::FloatCurveArray {
                values: values
                    .iter()
                    .filter_map(|value| match value {
                        SequenceEffectParamValue::FloatCurve { points } => Some(points.clone()),
                        _ => None,
                    })
                    .collect(),
            }
        }
        _ => SequenceEffectParamValue::FloatArray {
            values: values
                .iter()
                .filter_map(|value| match value {
                    SequenceEffectParamValue::Float { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        },
    }
}
use dawn_language::dsl::{Type, Value as EffectValue};
use dawn_language::effect::{CurveSource, EffectParamValue};
use dawn_language::operator::{
    BuiltinOperator, GraphOperatorNode, OperatorDefinition, OperatorPortCardinality,
    OperatorPortDefinition, OperatorRef,
};
use dawn_language::sequence::{
    AutomationMapping, AutomationTarget, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind,
};
use dawn_language::setup::FixtureDefinitionId;
use dawn_language::values::CurveValue;
use dawn_project_io::ProjectSession;

use crate::dto::{
    ColorCurvePoint, FloatCurvePoint, GuiObjectRef, ObjectKind, SequenceAutomationMapping,
    SequenceBuiltinOperator, SequenceCurveLibraryItem, SequenceCurveLibraryPoints,
    SequenceCurveValueType, SequenceEffectParam, SequenceEffectParamCurveSource,
    SequenceEffectParamKind, SequenceEffectParamValue, SequenceGraphNode, SequenceGraphNodeKind,
    SequenceGraphOperator, SequenceGraphOperatorDefinition, SequenceGraphPortCardinality,
    SequenceGraphPortDefinition, SequenceParamAutomation,
};
