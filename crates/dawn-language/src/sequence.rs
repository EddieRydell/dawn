use crate::effect::{
    EffectDefinitionId, EffectInst, EffectInstId, EffectParamValue, EffectScope, EffectTarget,
};
use crate::effect_dsl::types::Identifier;
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
    pub effects: Vec<EffectInst>,
    pub automation_clips: Vec<AutomationClip>,
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
    Graph(EffectGraphClip),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectClip {
    pub definition: EffectDefinitionId,
    pub param_overrides: IndexMap<Identifier, EffectParamValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectGraphClip {
    pub nodes: Vec<EffectGraphNode>,
    pub edges: Vec<EffectGraphEdge>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectGraphNode {
    pub id: EffectGraphNodeId,
    pub position: GraphNodePosition,
    pub kind: EffectGraphNodeKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectGraphNodeId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct GraphNodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectGraphNodeKind {
    Source(GraphSourceNode),
    Operator(GraphOperatorNode),
    Output,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphSourceNode {
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub target: EffectTarget,
    pub scope: EffectScope,
    pub definition: EffectDefinitionId,
    pub param_overrides: IndexMap<Identifier, EffectParamValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphOperatorNode {
    pub operator: GraphOperator,
    pub params: IndexMap<Identifier, EffectParamValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphOperator {
    Max,
    Add,
    Multiply,
    IntensityModulate,
    Dim,
    Invert,
    Colorize,
    Delay,
    Echo,
    RemapNearest,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectGraphEdge {
    pub from_node: EffectGraphNodeId,
    pub from_port: GraphPortId,
    pub to_node: EffectGraphNodeId,
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
    EffectClipParam {
        clip_id: SequenceClipId,
        param: Identifier,
    },
    GraphNodeParam {
        clip_id: SequenceClipId,
        node_id: EffectGraphNodeId,
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
