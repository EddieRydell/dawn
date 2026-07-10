use crate::dsl::types::Identifier;
use crate::effect::{
    EffectDefinitionId, EffectInst, EffectInstId, EffectParamValue, EffectScope, EffectTarget,
};
use crate::operator::GraphOperatorNode;
use crate::values::{Color, Curve, DawnDuration, DawnTime};
use indexmap::IndexMap;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SequenceId(pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct Sequence {
    pub id: SequenceId,
    pub duration: DawnDuration,
    pub frame_rate: u32,
    pub audio: SequenceAudio,
    pub mark_collections: Vec<MarkCollection>,
    pub clips: Vec<SequenceClip>,
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
    pub effect_id: EffectInstId,
    pub param: Identifier,
    pub mapping: AutomationMapping,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SequenceAudio {
    None,
    Asset(AssetId),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AssetId(pub u32);
