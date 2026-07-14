pub(super) fn sequence_mut<'a>(
    session: &'a mut ProjectSession,
    id: &SequenceId,
) -> Result<&'a mut dawn_language::sequence::Sequence, GuiMutationError> {
    session
        .project
        .sequences
        .get_mut(id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))
}

pub(super) fn register_sequence_audio_asset(
    session: &mut ProjectSession,
    document: &Utf8Path,
    import_path: &str,
) -> Result<AssetId, GuiMutationError> {
    if let Some(asset) = session
        .source
        .referenced_assets
        .iter()
        .find(|asset| asset.relative_path.as_str() == import_path)
    {
        return Ok(asset.id.clone());
    }

    let document_path = session.source.source_root.join(document);
    let document_dir = document_path
        .parent()
        .unwrap_or(&session.source.source_root)
        .to_path_buf();
    let selected_path = document_dir.join(import_path);
    let absolute_path = fs::canonicalize(&selected_path)
        .map_err(|error| GuiMutationError::Invalid(format!("Audio file was not found: {error}")))?;
    let absolute_path = Utf8PathBuf::from_path_buf(absolute_path).map_err(|path| {
        GuiMutationError::Invalid(format!("Audio path is not valid UTF-8: {}", path.display()))
    })?;
    if !absolute_path.is_file() {
        return Err(GuiMutationError::Invalid(
            "Selected audio path is not a file.".to_string(),
        ));
    }

    if let Some(asset) = session
        .source
        .referenced_assets
        .iter()
        .find(|asset| asset.absolute_path == absolute_path)
    {
        return Ok(asset.id.clone());
    }

    let relative_path = absolute_path
        .strip_prefix(&session.source.source_root)
        .map(Utf8Path::to_path_buf)
        .unwrap_or_else(|_| Utf8PathBuf::from(import_path));
    let next_id = session
        .source
        .referenced_assets
        .iter()
        .map(|asset| asset.id.0)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let id = AssetId(next_id);
    session.source.referenced_assets.push(ReferencedAsset {
        id: id.clone(),
        relative_path,
        absolute_path,
    });
    Ok(id)
}

pub(super) fn fixture_definition_mut<'a>(
    session: &'a mut ProjectSession,
    document_path: &Utf8Path,
    object_key: &str,
) -> Result<&'a mut dawn_language::preview::PropDefinition, GuiMutationError> {
    let id = session
        .source
        .documents
        .get(document_path)
        .into_iter()
        .flat_map(|document| document.objects())
        .find_map(|object| {
            (object.kind() == &SourceObjectKind::PropDefinition && object.id() == object_key).then(
                || {
                    PropDefinitionId(SourceIdentity::new(
                        document_path.to_path_buf(),
                        object.id().to_string(),
                    ))
                },
            )
        })
        .ok_or_else(|| {
            GuiMutationError::Invalid("Fixture definition was not found.".to_string())
        })?;
    session
        .project
        .definitions
        .props
        .definitions
        .get_mut(&id)
        .ok_or_else(|| GuiMutationError::Invalid("Fixture definition was not loaded.".to_string()))
}

pub(super) fn effect_mut(
    sequence: &mut dawn_language::sequence::Sequence,
    id: u32,
) -> Result<&mut EffectInst, GuiMutationError> {
    sequence
        .effects
        .iter_mut()
        .find(|effect| effect.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))
}

pub(super) fn composition_graph_node_mut<'a>(
    sequence: &'a mut dawn_language::sequence::Sequence,
    id: &CompositionGraphNodeId,
) -> Result<&'a mut CompositionGraphNode, GuiMutationError> {
    sequence
        .composition_graph
        .nodes
        .iter_mut()
        .find(|node| node.id == *id)
        .ok_or_else(|| GuiMutationError::Invalid("Graph node was not found.".to_string()))
}

pub(super) fn parse_graph_node_id(value: &str) -> Result<CompositionGraphNodeId, GuiMutationError> {
    if let Some(id) = value.strip_prefix("node:") {
        return id
            .parse::<u32>()
            .map(CompositionGraphNodeId)
            .map_err(|_| GuiMutationError::Invalid("Invalid graph node id.".to_string()));
    }
    Err(GuiMutationError::Invalid(
        "Invalid graph node id.".to_string(),
    ))
}

pub(super) fn ensure_graph_node_exists(
    sequence: &dawn_language::sequence::Sequence,
    node_id: &CompositionGraphNodeId,
) -> Result<(), GuiMutationError> {
    if sequence
        .composition_graph
        .nodes
        .iter()
        .any(|node| node.id == *node_id)
    {
        Ok(())
    } else {
        Err(GuiMutationError::Invalid(
            "Graph node was not found.".to_string(),
        ))
    }
}

pub(super) fn graph_input_cardinality(
    definitions: &dawn_language::operator::OperatorDefinitionStore,
    kind: &CompositionGraphNodeKind,
    source_name: &str,
) -> Option<OperatorPortCardinality> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => None,
        CompositionGraphNodeKind::Operator(operator) => definitions
            .resolve(&operator.operator)?
            .inputs
            .iter()
            .find(|port| port.source_name == source_name)
            .map(|port| port.cardinality.clone()),
        CompositionGraphNodeKind::Output => {
            (source_name == "input").then_some(OperatorPortCardinality::Many)
        }
    }
}

pub(super) fn next_composition_node_id(sequence: &dawn_language::sequence::Sequence) -> u32 {
    sequence
        .composition_graph
        .nodes
        .iter()
        .map(|node| node.id.0)
        .max()
        .unwrap_or(0)
        + 1
}

pub(super) fn create_sequence_layer(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    name: String,
    color: String,
    position: Option<(f64, f64)>,
    connect_to_output: bool,
) -> Result<(), GuiMutationError> {
    let sequence = sequence_mut(session, sequence_id)?;
    let next_layer_id = sequence
        .layers
        .iter()
        .map(|layer| layer.id.0)
        .max()
        .unwrap_or(0)
        + 1;
    let output_node_id = sequence
        .composition_graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Output))
        .map(|node| node.id.clone())
        .ok_or_else(|| {
            GuiMutationError::Invalid("Composition graph output was not found.".to_string())
        })?;
    let layer_node_id = CompositionGraphNodeId(next_composition_node_id(sequence));
    sequence
        .layers
        .push(dawn_language::sequence::SequenceLayer {
            id: SequenceLayerId(next_layer_id),
            name,
            color: parse_color(&color)?,
            enabled: true,
        });
    let (x, y) = position.unwrap_or((80.0, 120.0 + f64::from(next_layer_id) * 80.0));
    sequence.composition_graph.nodes.push(CompositionGraphNode {
        id: layer_node_id.clone(),
        position: GraphNodePosition { x, y },
        kind: CompositionGraphNodeKind::Layer {
            layer_id: SequenceLayerId(next_layer_id),
        },
    });
    if connect_to_output {
        sequence.composition_graph.edges.push(EffectGraphEdge {
            from: layer_node_id,
            from_port: GraphPortId("output".to_string()),
            to: output_node_id,
            to_port: GraphPortId("input".to_string()),
        });
    }
    Ok(())
}

pub(super) fn graph_operator_from_gui(
    operator: &SequenceGraphOperator,
) -> Result<OperatorRef, GuiMutationError> {
    Ok(match operator {
        SequenceGraphOperator::Builtin { operator } => OperatorRef::Builtin(match operator {
            SequenceBuiltinOperator::Max => BuiltinOperator::Max,
            SequenceBuiltinOperator::Add => BuiltinOperator::Add,
            SequenceBuiltinOperator::Multiply => BuiltinOperator::Multiply,
            SequenceBuiltinOperator::IntensityModulate => BuiltinOperator::IntensityModulate,
            SequenceBuiltinOperator::Dim => BuiltinOperator::Dim,
            SequenceBuiltinOperator::Invert => BuiltinOperator::Invert,
            SequenceBuiltinOperator::Colorize => BuiltinOperator::Colorize,
            SequenceBuiltinOperator::Delay => BuiltinOperator::Delay,
            SequenceBuiltinOperator::Echo => BuiltinOperator::Echo,
        }),
        SequenceGraphOperator::Custom { path, object_key } => {
            OperatorRef::Custom(OperatorDefinitionId(SourceIdentity::new(
                Utf8PathBuf::from(path),
                identifier(object_key)?.as_str().to_string(),
            )))
        }
    })
}

pub(super) fn mark_collection_mut<'a>(
    sequence: &'a mut dawn_language::sequence::Sequence,
    key: &str,
) -> Result<&'a mut MarkCollection, GuiMutationError> {
    sequence
        .mark_collections
        .iter_mut()
        .find(|collection| collection.key.name == key)
        .ok_or_else(|| GuiMutationError::Invalid("Mark collection was not found.".to_string()))
}

pub(super) fn automation_clip_mut(
    sequence: &mut dawn_language::sequence::Sequence,
    id: u32,
) -> Result<&mut AutomationClip, GuiMutationError> {
    sequence
        .automation_clips
        .iter_mut()
        .find(|clip| clip.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Automation clip was not found.".to_string()))
}

pub(super) fn identifier(value: &str) -> Result<Identifier, GuiMutationError> {
    Identifier::new(value.to_string())
        .map_err(|_| GuiMutationError::Invalid(format!("Invalid identifier `{value}`.")))
}

pub(super) fn effect_scope(scope: SequenceEffectScope) -> EffectScope {
    match scope {
        SequenceEffectScope::PerFixture => EffectScope::PerFixture,
        SequenceEffectScope::WholeTarget => EffectScope::WholeTarget,
    }
}

pub(super) fn layout_target_to_effect_target(
    tree: &ElementTreeId,
    target: ElementTarget,
) -> Result<ElementSelection, GuiMutationError> {
    let id = target
        .name
        .parse::<u32>()
        .map_err(|_| GuiMutationError::Invalid("Layout target id must be numeric.".to_string()))?;
    Ok(ElementSelection {
        tree: tree.clone(),
        node: ElementNodeId(id),
        cells: None,
    })
}

pub(super) fn effect_param_value_from_gui(
    value: SequenceEffectParamValue,
) -> Result<EffectParamValue, GuiMutationError> {
    Ok(match value {
        SequenceEffectParamValue::Int { value } => EffectParamValue::Int(value as i64),
        SequenceEffectParamValue::Float { value } => EffectParamValue::Float(value),
        SequenceEffectParamValue::Bool { value } => EffectParamValue::Bool(value),
        SequenceEffectParamValue::Color { value } => EffectParamValue::Color(parse_color(&value)?),
        SequenceEffectParamValue::Enum { value } => EffectParamValue::Enum(identifier(&value)?),
        SequenceEffectParamValue::Marks { key } => {
            EffectParamValue::Marks(MarkCollectionKey { name: key })
        }
        SequenceEffectParamValue::Curve { points } => {
            EffectParamValue::Curve(CurveSource::Inline(curve_from_points(points)))
        }
        SequenceEffectParamValue::Gradient { stops } => {
            EffectParamValue::Gradient(GradientSource::Inline(gradient_from_stops(stops)?))
        }
        SequenceEffectParamValue::IntArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| EffectParamValue::Int(value as i64))
                .collect(),
        ),
        SequenceEffectParamValue::FloatArray { values } => {
            EffectParamValue::Array(values.into_iter().map(EffectParamValue::Float).collect())
        }
        SequenceEffectParamValue::BoolArray { values } => {
            EffectParamValue::Array(values.into_iter().map(EffectParamValue::Bool).collect())
        }
        SequenceEffectParamValue::ColorArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| parse_color(&value).map(EffectParamValue::Color))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        SequenceEffectParamValue::CurveArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|points| {
                    EffectParamValue::Curve(CurveSource::Inline(curve_from_points(points)))
                })
                .collect(),
        ),
        SequenceEffectParamValue::GradientArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|stops| gradient_from_stops(stops).map(GradientSource::Inline))
                .map(|source| source.map(EffectParamValue::Gradient))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    })
}

pub(super) fn automation_mapping_from_gui(
    mapping: SequenceAutomationMapping,
) -> Result<AutomationMapping, GuiMutationError> {
    Ok(match mapping {
        SequenceAutomationMapping::Float { min, max } => AutomationMapping::Float { min, max },
        SequenceAutomationMapping::Int { min, max } => AutomationMapping::Int {
            min: min.round() as i64,
            max: max.round() as i64,
        },
        SequenceAutomationMapping::Bool => AutomationMapping::Bool,
        SequenceAutomationMapping::Enum { values } => AutomationMapping::Enum {
            values: values
                .into_iter()
                .map(|value| identifier(&value))
                .collect::<Result<Vec<_>, _>>()?,
        },
        SequenceAutomationMapping::Curve { min, max } => AutomationMapping::Curve { min, max },
    })
}

pub(super) fn automation_binding_value_at(
    clip: &AutomationClip,
    binding: &AutomationBinding,
    seconds: f64,
) -> Result<EffectParamValue, GuiMutationError> {
    automation_value_at(clip, binding, seconds)
        .map(|value| match value {
            AutomationValue::Int(value) => EffectParamValue::Int(value),
            AutomationValue::Float(value) => EffectParamValue::Float(value),
            AutomationValue::Bool(value) => EffectParamValue::Bool(value),
            AutomationValue::Enum(value) => EffectParamValue::Enum(value),
            AutomationValue::Curve(value) => EffectParamValue::Curve(CurveSource::Inline(value)),
        })
        .ok_or_else(|| {
            GuiMutationError::Invalid("Enum automation mapping has no values.".to_string())
        })
}

pub(super) fn default_automation_curve() -> Curve {
    Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 1.0,
                value: 1.0,
            },
        ],
    }
}

pub(super) fn curve_from_points(points: Vec<SequenceCurvePoint>) -> Curve {
    Curve {
        points: points
            .into_iter()
            .map(|point| CurvePoint {
                position: point.time,
                value: point.value,
            })
            .collect(),
    }
}

fn gradient_from_stops(stops: Vec<SequenceGradientStop>) -> Result<Gradient, GuiMutationError> {
    Ok(Gradient {
        stops: stops
            .into_iter()
            .map(|stop| {
                Ok(GradientStop {
                    position: stop.time,
                    color: parse_color(&stop.value)?,
                })
            })
            .collect::<Result<Vec<_>, GuiMutationError>>()?,
    })
}

pub(super) fn parse_color(value: &str) -> Result<Color, GuiMutationError> {
    Color::from_hex(value)
        .ok_or_else(|| GuiMutationError::Invalid(format!("Invalid color `{value}`.")))
}

pub(super) fn domain_point3_meters(point: Point3Meters) -> Point3 {
    Point3 {
        x: Distance::from_meters(point.x_meters),
        y: Distance::from_meters(point.y_meters),
        z: Distance::from_meters(point.z_meters),
    }
}

pub(super) fn rotation3_degrees(rotation: Rotation3Degrees) -> DomainRotation3 {
    DomainRotation3 {
        x: rotation.x_degrees,
        y: rotation.y_degrees,
        z: rotation.z_degrees,
    }
}

pub(super) fn scale3(scale: Scale3) -> DomainScale3 {
    DomainScale3 {
        x: scale.x,
        y: scale.y,
        z: scale.z,
    }
}

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::dsl::Identifier;
use dawn_language::effect::{
    CurveSource, EffectInst, EffectParamValue, EffectScope, GradientSource,
};
use dawn_language::element::{ElementNodeId, ElementSelection, ElementTreeId};
use dawn_language::identity::SourceIdentity;
use dawn_language::operator::{
    BuiltinOperator, OperatorDefinitionId, OperatorPortCardinality, OperatorRef,
};
use dawn_language::preview::PropDefinitionId;
use dawn_language::sequence::{
    AssetId, AutomationBinding, AutomationClip, AutomationMapping, AutomationValue,
    CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
    GraphNodePosition, GraphPortId, MarkCollection, MarkCollectionKey, SequenceId, SequenceLayerId,
    automation_value_at,
};
use dawn_language::values::{
    Color, Curve, CurvePoint, Distance, Gradient, GradientStop, Point3,
    Rotation3 as DomainRotation3, Scale3 as DomainScale3,
};
use dawn_project_io::{ProjectSession, ReferencedAsset, SourceObjectKind};

use super::GuiMutationError;
use crate::dto::{
    ElementTarget, Point3Meters, Rotation3Degrees, Scale3, SequenceAutomationMapping,
    SequenceBuiltinOperator, SequenceCurvePoint, SequenceEffectParamValue, SequenceEffectScope,
    SequenceGradientStop, SequenceGraphOperator,
};
