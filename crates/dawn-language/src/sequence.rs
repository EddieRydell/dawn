use crate::dsl::types::Identifier;
use crate::effect::{
    EffectDefinitionId, EffectInst, EffectInstId, EffectParamValue, EffectScope, EffectTarget,
};
use crate::identity::SourceIdentity;
use crate::operator::GraphOperatorNode;
use crate::values::{Color, Curve, CurvePoint, CurveValue, DawnDuration, DawnTime};
use indexmap::IndexMap;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SequenceId(pub SourceIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct Sequence {
    pub id: SequenceId,
    pub duration: DawnDuration,
    pub frame_rate: u32,
    pub audio: SequenceAudio,
    pub mark_collections: Vec<MarkCollection>,
    pub layers: Vec<SequenceLayer>,
    pub effects: Vec<EffectInst>,
    pub composition_graph: SequenceCompositionGraph,
    pub automation_clips: Vec<AutomationClip>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SequenceLayerId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct SequenceLayer {
    pub id: SequenceLayerId,
    pub name: String,
    pub color: Color,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SequenceCompositionGraph {
    pub nodes: Vec<CompositionGraphNode>,
    pub edges: Vec<EffectGraphEdge>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompositionGraphNode {
    pub id: CompositionGraphNodeId,
    pub position: GraphNodePosition,
    pub kind: CompositionGraphNodeKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CompositionGraphNodeKind {
    Layer { layer_id: SequenceLayerId },
    Operator(GraphOperatorNode),
    Output,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SequenceClip {
    pub id: SequenceClipId,
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub target: EffectTarget,
    pub scope: EffectScope,
    pub kind: SequenceClipKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SequenceClipId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub enum SequenceClipKind {
    Effect(EffectClip),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectClip {
    pub definition: EffectDefinitionId,
    pub param_overrides: IndexMap<Identifier, EffectParamValue>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct CompositionGraphNodeId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct GraphNodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectGraphEdge {
    pub from: CompositionGraphNodeId,
    pub from_port: GraphPortId,
    pub to: CompositionGraphNodeId,
    pub to_port: GraphPortId,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GraphPortId(pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct MarkCollection {
    pub key: MarkCollectionKey,
    pub name: String,
    pub display_color: Color,
    pub marks: Vec<DawnTime>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct MarkCollectionKey {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AutomationClip {
    pub id: AutomationClipId,
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub anchor_lane_index: u32,
    pub lane_index: u32,
    pub curve: Curve,
    pub bindings: Vec<AutomationBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationClipId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct AutomationBinding {
    pub target: AutomationTarget,
    pub mapping: AutomationMapping,
}

impl AutomationBinding {
    pub fn effect_param(&self) -> Option<(&EffectInstId, &Identifier)> {
        match &self.target {
            AutomationTarget::EffectParam { effect_id, param } => Some((effect_id, param)),
            AutomationTarget::CompositionNodeParam { .. } => None,
        }
    }

    pub fn composition_node_param(&self) -> Option<(&CompositionGraphNodeId, &Identifier)> {
        match &self.target {
            AutomationTarget::CompositionNodeParam { node_id, param } => Some((node_id, param)),
            AutomationTarget::EffectParam { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum AutomationTarget {
    EffectParam {
        effect_id: EffectInstId,
        param: Identifier,
    },
    CompositionNodeParam {
        node_id: CompositionGraphNodeId,
        param: Identifier,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum AutomationMapping {
    Float { min: f64, max: f64 },
    Int { min: i64, max: i64 },
    Bool,
    Enum { values: Vec<Identifier> },
    FloatCurve { min: f64, max: f64 },
}

pub enum AutomationValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Enum(Identifier),
    FloatCurve(Curve),
}

pub fn automation_value_at(
    clip: &AutomationClip,
    binding: &AutomationBinding,
    sample_seconds: f64,
) -> Option<AutomationValue> {
    let normalized = sample_automation_clip(clip, sample_seconds);
    Some(match &binding.mapping {
        AutomationMapping::Float { min, max } => {
            AutomationValue::Float(lerp(*min, *max, normalized))
        }
        AutomationMapping::Int { min, max } => {
            AutomationValue::Int(lerp(*min as f64, *max as f64, normalized).round() as i64)
        }
        AutomationMapping::Bool => AutomationValue::Bool(normalized >= 0.5),
        AutomationMapping::Enum { values } => {
            let index = ((normalized.clamp(0.0, 1.0) * values.len() as f64).floor() as usize)
                .min(values.len().checked_sub(1)?);
            AutomationValue::Enum(values[index].clone())
        }
        AutomationMapping::FloatCurve { min, max } => {
            AutomationValue::FloatCurve(float_curve_window(clip, *min, *max, sample_seconds))
        }
    })
}

fn sample_automation_clip(clip: &AutomationClip, sample_seconds: f64) -> f64 {
    let duration = clip.duration.as_seconds_f64();
    let position = if duration <= 0.0 {
        0.0
    } else {
        ((sample_seconds - clip.start.as_seconds_f64()) / duration).clamp(0.0, 1.0)
    };
    sample_float_curve(&clip.curve, position).clamp(0.0, 1.0)
}

fn float_curve_window(clip: &AutomationClip, min: f64, max: f64, sample_seconds: f64) -> Curve {
    let duration = clip.duration.as_seconds_f64().max(f64::EPSILON);
    let sample_position =
        ((sample_seconds - clip.start.as_seconds_f64()) / duration).clamp(0.0, 1.0);
    let points = clip
        .curve
        .points
        .iter()
        .filter_map(|point| {
            let position = point.position - sample_position;
            (0.0..=1.0).contains(&position).then(|| CurvePoint {
                position,
                value: CurveValue::Float(lerp(min, max, curve_point_float(point))),
            })
        })
        .collect::<Vec<_>>();
    Curve {
        points: if points.is_empty() {
            vec![CurvePoint {
                position: 0.0,
                value: CurveValue::Float(lerp(
                    min,
                    max,
                    sample_automation_clip(clip, sample_seconds),
                )),
            }]
        } else {
            points
        },
    }
}

fn sample_float_curve(curve: &Curve, position: f64) -> f64 {
    let Some(first) = curve.points.first() else {
        return 0.0;
    };
    if position <= first.position {
        return curve_point_float(first);
    }
    for pair in curve.points.windows(2) {
        let (left, right) = (&pair[0], &pair[1]);
        if position <= right.position {
            let span = right.position - left.position;
            let amount = if span <= 0.0 {
                0.0
            } else {
                (position - left.position) / span
            };
            return lerp(curve_point_float(left), curve_point_float(right), amount);
        }
    }
    curve.points.last().map(curve_point_float).unwrap_or(0.0)
}

fn curve_point_float(point: &CurvePoint) -> f64 {
    match point.value {
        CurveValue::Float(value) => value,
        CurveValue::Color(_) => 0.0,
    }
}

fn lerp(min: f64, max: f64, amount: f64) -> f64 {
    min + (max - min) * amount.clamp(0.0, 1.0)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SequenceAudio {
    None,
    Asset(AssetId),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AssetId(pub u32);
