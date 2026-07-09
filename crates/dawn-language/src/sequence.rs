use crate::effect::{
    EffectDefinitionId, EffectInst, EffectInstId, EffectParamValue, EffectScope, EffectTarget,
};
use crate::effect_dsl::types::Identifier;
use crate::effect_dsl::{ParamDecl, Type, Value};
use crate::values::{Color, Curve, DawnDuration, DawnTime};
use indexmap::IndexMap;
use std::collections::HashSet;
use std::sync::LazyLock;

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
pub struct GraphOperatorNode {
    pub operator: GraphOperatorRef,
    pub params: IndexMap<Identifier, EffectParamValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphOperatorRef {
    Builtin(GraphOperator),
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphPortCardinality {
    One,
    Many,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphPortDefinition {
    pub source_name: &'static str,
    pub display_name: &'static str,
    pub cardinality: GraphPortCardinality,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphOperatorDefinition {
    pub operator: GraphOperator,
    pub source_name: &'static str,
    pub display_name: &'static str,
    pub inputs: &'static [GraphPortDefinition],
    pub outputs: &'static [GraphPortDefinition],
    pub params: Vec<ParamDecl>,
}

const OUTPUT_PORTS: &[GraphPortDefinition] = &[GraphPortDefinition {
    source_name: "output",
    display_name: "Output",
    cardinality: GraphPortCardinality::Many,
}];
const BINARY_INPUTS: &[GraphPortDefinition] = &[
    GraphPortDefinition {
        source_name: "a",
        display_name: "A",
        cardinality: GraphPortCardinality::One,
    },
    GraphPortDefinition {
        source_name: "b",
        display_name: "B",
        cardinality: GraphPortCardinality::One,
    },
];
const MODULATE_INPUTS: &[GraphPortDefinition] = &[
    GraphPortDefinition {
        source_name: "source",
        display_name: "Source",
        cardinality: GraphPortCardinality::One,
    },
    GraphPortDefinition {
        source_name: "mask",
        display_name: "Mask",
        cardinality: GraphPortCardinality::One,
    },
];
const UNARY_INPUTS: &[GraphPortDefinition] = &[GraphPortDefinition {
    source_name: "input",
    display_name: "Source",
    cardinality: GraphPortCardinality::One,
}];

impl GraphOperator {
    pub const ALL: [Self; 9] = [
        Self::Max,
        Self::Add,
        Self::Multiply,
        Self::IntensityModulate,
        Self::Dim,
        Self::Invert,
        Self::Colorize,
        Self::Delay,
        Self::Echo,
    ];

    pub fn definition(&self) -> &'static GraphOperatorDefinition {
        &GRAPH_OPERATOR_DEFINITIONS[graph_operator_index(self)]
    }

    pub fn from_source_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|operator| operator.definition().source_name == name)
            .cloned()
    }
}

fn graph_operator_index(operator: &GraphOperator) -> usize {
    match operator {
        GraphOperator::Max => 0,
        GraphOperator::Add => 1,
        GraphOperator::Multiply => 2,
        GraphOperator::IntensityModulate => 3,
        GraphOperator::Dim => 4,
        GraphOperator::Invert => 5,
        GraphOperator::Colorize => 6,
        GraphOperator::Delay => 7,
        GraphOperator::Echo => 8,
    }
}

fn identifier(name: &str) -> Identifier {
    Identifier::new(name.to_string()).unwrap_or_else(|_| unreachable!("static identifier is valid"))
}

fn param(name: &str, ty: Type, default: Value) -> ParamDecl {
    ParamDecl {
        name: identifier(name),
        ty,
        default: Some(default),
    }
}

static GRAPH_OPERATOR_DEFINITIONS: LazyLock<[GraphOperatorDefinition; 9]> = LazyLock::new(|| {
    [
        definition(GraphOperator::Max, "max", "Max", BINARY_INPUTS, vec![]),
        definition(GraphOperator::Add, "add", "Add", BINARY_INPUTS, vec![]),
        definition(
            GraphOperator::Multiply,
            "multiply",
            "Multiply",
            BINARY_INPUTS,
            vec![],
        ),
        definition(
            GraphOperator::IntensityModulate,
            "intensity_modulate",
            "Intensity Modulate",
            MODULATE_INPUTS,
            vec![],
        ),
        definition(
            GraphOperator::Dim,
            "dim",
            "Dim",
            UNARY_INPUTS,
            vec![param("amount", Type::Float, Value::Float(0.5))],
        ),
        definition(
            GraphOperator::Invert,
            "invert",
            "Invert",
            UNARY_INPUTS,
            vec![],
        ),
        definition(
            GraphOperator::Colorize,
            "colorize",
            "Colorize",
            UNARY_INPUTS,
            vec![param(
                "color",
                Type::Color,
                Value::Color(Color {
                    red: 255,
                    green: 255,
                    blue: 255,
                }),
            )],
        ),
        definition(
            GraphOperator::Delay,
            "delay",
            "Delay",
            UNARY_INPUTS,
            vec![param("seconds", Type::Float, Value::Float(0.1))],
        ),
        definition(
            GraphOperator::Echo,
            "echo",
            "Echo",
            UNARY_INPUTS,
            vec![
                param("seconds", Type::Float, Value::Float(0.1)),
                param("repeats", Type::Int, Value::Int(3)),
                param("decay", Type::Float, Value::Float(0.5)),
            ],
        ),
    ]
});

fn definition(
    operator: GraphOperator,
    source_name: &'static str,
    display_name: &'static str,
    inputs: &'static [GraphPortDefinition],
    params: Vec<ParamDecl>,
) -> GraphOperatorDefinition {
    GraphOperatorDefinition {
        operator,
        source_name,
        display_name,
        inputs,
        outputs: OUTPUT_PORTS,
        params,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphInterfaceError {
    pub message: String,
}

pub fn validate_graph_interface(
    graph: &SequenceCompositionGraph,
) -> Result<(), GraphInterfaceError> {
    let mut node_ids = HashSet::new();
    for node in &graph.nodes {
        if !node_ids.insert(&node.id) {
            return graph_error(format!("duplicate composition graph node {}", node.id.0));
        }
        if let CompositionGraphNodeKind::Operator(operator) = &node.kind {
            validate_operator_params(operator)?;
        }
    }

    let mut edges = HashSet::new();
    let mut occupied_inputs = HashSet::new();
    for edge in &graph.edges {
        let from = graph
            .nodes
            .iter()
            .find(|node| node.id == edge.from)
            .ok_or_else(|| GraphInterfaceError {
                message: format!(
                    "edge references missing composition graph node {}",
                    edge.from.0
                ),
            })?;
        let to = graph
            .nodes
            .iter()
            .find(|node| node.id == edge.to)
            .ok_or_else(|| GraphInterfaceError {
                message: format!(
                    "edge references missing composition graph node {}",
                    edge.to.0
                ),
            })?;
        if !output_ports(&from.kind)
            .iter()
            .any(|port| port.source_name == edge.from_port.0)
        {
            return graph_error(format!(
                "unknown composition graph output port `{}`",
                edge.from_port.0
            ));
        }
        let input = input_ports(&to.kind)
            .iter()
            .find(|port| port.source_name == edge.to_port.0)
            .ok_or_else(|| GraphInterfaceError {
                message: format!("unknown composition graph input port `{}`", edge.to_port.0),
            })?;
        let edge_key = (
            &edge.from,
            edge.from_port.0.as_str(),
            &edge.to,
            edge.to_port.0.as_str(),
        );
        if !edges.insert(edge_key) {
            return graph_error("duplicate composition graph edge".to_string());
        }
        if input.cardinality == GraphPortCardinality::One
            && !occupied_inputs.insert((&edge.to, edge.to_port.0.as_str()))
        {
            return graph_error(format!(
                "composition graph input port `{}` accepts one connection",
                edge.to_port.0
            ));
        }
    }
    Ok(())
}

fn graph_error<T>(message: String) -> Result<T, GraphInterfaceError> {
    Err(GraphInterfaceError { message })
}

fn input_ports(kind: &CompositionGraphNodeKind) -> &'static [GraphPortDefinition] {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => &[],
        CompositionGraphNodeKind::Operator(operator) => {
            let GraphOperatorRef::Builtin(operator) = &operator.operator;
            operator.definition().inputs
        }
        CompositionGraphNodeKind::Output => &[GraphPortDefinition {
            source_name: "input",
            display_name: "Input",
            cardinality: GraphPortCardinality::Many,
        }],
    }
}

fn output_ports(kind: &CompositionGraphNodeKind) -> &'static [GraphPortDefinition] {
    match kind {
        CompositionGraphNodeKind::Layer { .. } | CompositionGraphNodeKind::Operator(_) => {
            OUTPUT_PORTS
        }
        CompositionGraphNodeKind::Output => &[],
    }
}

fn validate_operator_params(operator: &GraphOperatorNode) -> Result<(), GraphInterfaceError> {
    let GraphOperatorRef::Builtin(builtin) = &operator.operator;
    for (name, value) in &operator.params {
        let declaration = builtin
            .definition()
            .params
            .iter()
            .find(|declaration| declaration.name == *name)
            .ok_or_else(|| GraphInterfaceError {
                message: format!(
                    "unknown parameter `{}` for operator {}",
                    name.as_str(),
                    builtin.definition().source_name
                ),
            })?;
        if !effect_param_matches_type(value, &declaration.ty) {
            return graph_error(format!(
                "parameter `{}` has the wrong type for operator {}",
                name.as_str(),
                builtin.definition().source_name
            ));
        }
    }
    Ok(())
}

pub fn effect_param_matches_type(value: &EffectParamValue, ty: &Type) -> bool {
    match (value, ty) {
        (EffectParamValue::Int(_), Type::Int)
        | (EffectParamValue::Float(_), Type::Float)
        | (EffectParamValue::Bool(_), Type::Bool)
        | (EffectParamValue::Color(_), Type::Color)
        | (EffectParamValue::Marks(_), Type::Marks)
        | (EffectParamValue::Curve(_), Type::Curve(_)) => true,
        (EffectParamValue::Enum(value), Type::Enum(options)) => options.contains(value),
        (EffectParamValue::Array(values), Type::Array(item_type)) => values
            .iter()
            .all(|value| effect_param_matches_type(value, item_type)),
        _ => false,
    }
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
