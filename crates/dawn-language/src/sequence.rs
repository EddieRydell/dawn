use crate::control::ControlClip;
use crate::dsl::types::Identifier;
use crate::effect::{EffectInst, EffectInstId};
use crate::identity::SourceIdentity;
use crate::operator::GraphOperatorNode;
use crate::sampling::sample_curve;
use crate::values::{Color, Curve, CurvePoint, DawnDuration, DawnTime};

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
    pub control_clips: Vec<ControlClip>,
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

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct CompositionGraphNodeId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct GraphNodePosition {
    pub x: f32,
    pub y: f32,
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
    pub detached_bindings: Vec<DetachedAutomationBinding>,
}

impl AutomationClip {
    pub fn detach_bindings(
        &mut self,
        reason: AutomationDetachmentReason,
        matches: impl Fn(&AutomationTarget) -> bool,
    ) {
        let mut retained = Vec::with_capacity(self.bindings.len());
        for binding in self.bindings.drain(..) {
            if matches(&binding.target) {
                self.detached_bindings.push(DetachedAutomationBinding {
                    target: binding.target,
                    mapping: binding.mapping,
                    reason: reason.clone(),
                });
            } else {
                retained.push(binding);
            }
        }
        self.bindings = retained;
    }

    pub fn bind(&mut self, target: AutomationTarget, mapping: AutomationMapping) {
        self.detached_bindings
            .retain(|binding| binding.target != target);
        self.bindings.push(AutomationBinding { target, mapping });
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationClipId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct AutomationBinding {
    pub target: AutomationTarget,
    pub mapping: AutomationMapping,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetachedAutomationBinding {
    pub target: AutomationTarget,
    pub mapping: AutomationMapping,
    pub reason: AutomationDetachmentReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationDetachmentReason {
    TargetDeleted,
    DefinitionChanged,
    OperatorSchemaChanged,
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
    Float { min: f32, max: f32 },
    Int { min: i32, max: i32 },
    Bool,
    Enum { values: Vec<Identifier> },
    Curve { min: f32, max: f32 },
}

pub enum AutomationValue {
    Int(i32),
    Float(f32),
    Bool(bool),
    Enum(Identifier),
    Curve(Curve),
}

pub fn automation_value_at(
    clip: &AutomationClip,
    binding: &AutomationBinding,
    sample_seconds: f32,
) -> Option<AutomationValue> {
    let normalized = sample_automation_clip(clip, sample_seconds);
    Some(match &binding.mapping {
        AutomationMapping::Float { min, max } => {
            AutomationValue::Float(lerp(*min, *max, normalized))
        }
        AutomationMapping::Int { min, max } => {
            AutomationValue::Int(lerp(*min as f32, *max as f32, normalized).round() as i32)
        }
        AutomationMapping::Bool => AutomationValue::Bool(normalized >= 0.5),
        AutomationMapping::Enum { values } => {
            let index = ((normalized.clamp(0.0, 1.0) * values.len() as f32).floor() as usize)
                .min(values.len().checked_sub(1)?);
            AutomationValue::Enum(values[index].clone())
        }
        AutomationMapping::Curve { min, max } => {
            AutomationValue::Curve(curve_window(clip, *min, *max, sample_seconds))
        }
    })
}

fn sample_automation_clip(clip: &AutomationClip, sample_seconds: f32) -> f32 {
    let duration = clip.duration.as_seconds_f32();
    let position = if duration <= 0.0 {
        0.0
    } else {
        ((sample_seconds - clip.start.as_seconds_f32()) / duration).clamp(0.0, 1.0)
    };
    sample_curve(&clip.curve, position).clamp(0.0, 1.0)
}

fn curve_window(clip: &AutomationClip, min: f32, max: f32, sample_seconds: f32) -> Curve {
    let duration = clip.duration.as_seconds_f32().max(f32::EPSILON);
    let sample_position =
        ((sample_seconds - clip.start.as_seconds_f32()) / duration).clamp(0.0, 1.0);
    let points = clip
        .curve
        .points
        .iter()
        .filter_map(|point| {
            let position = point.position - sample_position;
            (0.0..=1.0).contains(&position).then(|| CurvePoint {
                position,
                value: lerp(min, max, point.value),
            })
        })
        .collect::<Vec<_>>();
    Curve {
        points: if points.is_empty() {
            vec![CurvePoint {
                position: 0.0,
                value: lerp(min, max, sample_automation_clip(clip, sample_seconds)),
            }]
        } else {
            points
        },
    }
}

fn lerp(min: f32, max: f32, amount: f32) -> f32 {
    min + (max - min) * amount.clamp(0.0, 1.0)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SequenceAudio {
    None,
    Asset(AssetId),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AssetId(pub u32);
