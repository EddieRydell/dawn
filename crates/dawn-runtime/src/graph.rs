use dawn_language::dsl::Identifier;
use dawn_language::dsl::{BoundParams, Value};
use dawn_language::operator::{
    BuiltinOperator, OperatorDefinition, OperatorImplementation, validate_composition_graph,
};
use dawn_language::sequence::{
    AutomationMapping, AutomationTarget, CompositionGraphNodeId, CompositionGraphNodeKind,
    GraphPortId, Sequence, SequenceCompositionGraph,
};
use indexmap::{IndexMap, IndexSet};
use std::sync::Arc;

use super::effect_preparation::{automation_param, prepare_automation};
use super::{
    EffectParamTiming, PreparedAutomation, PreparedElement, PreparedLayer, RenderError,
    prepare_operator_params,
};
use crate::sequence::targets::{PreparedTargetPixel, full_rig_target_pixels};
use dawn_language::model::DawnProject;

pub(crate) fn automation_for_composition_node(
    sequence: &Sequence,
    node_id: &CompositionGraphNodeId,
) -> Vec<PreparedAutomation> {
    sequence
        .automation_clips
        .iter()
        .flat_map(|clip| {
            clip.bindings
                .iter()
                .filter(move |binding| {
                    matches!(
                        &binding.target,
                        AutomationTarget::CompositionNodeParam {
                            node_id: target_node_id,
                            ..
                        } if target_node_id == node_id
                    )
                })
                .map(move |binding| prepare_automation(clip, binding))
        })
        .collect()
}

pub(crate) fn float_param(
    params: &IndexMap<Identifier, Value>,
    name: &str,
) -> Result<f64, RenderError> {
    let name = Identifier::new(name.to_string()).map_err(|_| RenderError::BadGraph {
        message: format!("invalid operator parameter name `{name}`"),
    })?;
    params
        .get(&name)
        .and_then(|value| match value {
            Value::Float(value) => Some(*value),
            _ => None,
        })
        .ok_or_else(|| RenderError::BadGraph {
            message: format!("missing or invalid operator parameter `{}`", name.as_str()),
        })
}

pub(crate) fn int_param(
    params: &IndexMap<Identifier, Value>,
    name: &str,
) -> Result<i64, RenderError> {
    let name = Identifier::new(name.to_string()).map_err(|_| RenderError::BadGraph {
        message: format!("invalid operator parameter name `{name}`"),
    })?;
    params
        .get(&name)
        .and_then(|value| match value {
            Value::Int(value) => Some(*value),
            _ => None,
        })
        .ok_or_else(|| RenderError::BadGraph {
            message: format!("missing or invalid operator parameter `{}`", name.as_str()),
        })
}

pub(crate) fn layer_cache_history_micros(
    graph: &PreparedCompositionGraph,
) -> Result<i64, RenderError> {
    let mut history_seconds = 0.0_f64;
    for node in &graph.nodes {
        let PreparedGraphNodeKind::Operator {
            definition,
            params,
            automation,
            ..
        } = &node.kind
        else {
            continue;
        };
        if !matches!(
            definition.implementation,
            OperatorImplementation::Native(BuiltinOperator::Echo)
        ) {
            continue;
        }
        let mut delay = float_param(params, "seconds")?.max(0.0);
        let mut repeats = int_param(params, "repeats")?.clamp(1, 32);
        for automation in automation {
            match (
                automation_param(&automation.binding).as_str(),
                &automation.binding.mapping,
            ) {
                ("seconds", AutomationMapping::Float { min, max }) => {
                    delay = delay.max(*min).max(*max).max(0.0);
                }
                ("repeats", AutomationMapping::Int { min, max }) => {
                    repeats = repeats.max(*min).max(*max).clamp(1, 32);
                }
                _ => {}
            }
        }
        history_seconds = history_seconds.max(delay * repeats as f64);
    }
    Ok(if !history_seconds.is_finite() || history_seconds <= 0.0 {
        0
    } else {
        (history_seconds * 1_000_000.0).ceil() as i64
    })
}

pub(crate) struct PrepareGraphContext<'a> {
    pub(crate) project: &'a DawnProject,
    pub(crate) sequence: &'a Sequence,
    pub(crate) elements: &'a [PreparedElement],
    pub(crate) layers: &'a [PreparedLayer],
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedCompositionGraph {
    pub(crate) output_index: usize,
    pub(crate) nodes: Vec<PreparedGraphNode>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedGraphNode {
    pub(crate) target: Arc<Vec<PreparedTargetPixel>>,
    pub(crate) kind: PreparedGraphNodeKind,
}

#[derive(Clone, Debug)]
pub(crate) enum PreparedGraphNodeKind {
    Layer {
        layer_index: usize,
    },
    Operator {
        definition: Box<OperatorDefinition>,
        inputs: Vec<usize>,
        params: IndexMap<dawn_language::dsl::Identifier, Value>,
        automation: Vec<PreparedAutomation>,
        bound_params: Option<BoundParams>,
    },
    Output {
        inputs: Vec<usize>,
    },
}

pub(crate) fn prepare_composition_graph(
    context: PrepareGraphContext<'_>,
    graph: &SequenceCompositionGraph,
) -> Result<PreparedCompositionGraph, RenderError> {
    let full_target = Arc::new(full_rig_target_pixels(context.elements)?);
    validate_composition_graph(graph, &context.project.definitions.operators).map_err(|error| {
        RenderError::BadGraph {
            message: error.message,
        }
    })?;
    validate_composition_graph_layers(context.sequence, graph)?;
    let node_ids = composition_graph_node_ids(graph)?;
    let node_indexes = node_ids
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, node_id)| (node_id, index))
        .collect::<IndexMap<_, _>>();
    let node_order = topological_composition_graph_order(&node_ids, &node_indexes, graph)?;
    let mut incoming = vec![Vec::<(GraphPortId, usize)>::new(); node_ids.len()];
    for edge in &graph.edges {
        let from = node_index(&node_indexes, &edge.from)?;
        let to = node_index(&node_indexes, &edge.to)?;
        incoming[to].push((edge.to_port.clone(), from));
    }

    let mut prepared_nodes = Vec::<PreparedGraphNode>::new();
    let mut prepared_index_by_node = vec![usize::MAX; node_ids.len()];
    for node_index in &node_order {
        let node_id = &node_ids[*node_index];
        let node = graph_node(graph, node_id)?;
        let prepared = match &node.kind {
            CompositionGraphNodeKind::Layer { layer_id } => {
                let layer_index = context
                    .layers
                    .iter()
                    .position(|layer| layer.id == *layer_id)
                    .ok_or_else(|| RenderError::BadGraph {
                        message: format!(
                            "composition graph references missing layer {}",
                            layer_id.0
                        ),
                    })?;
                PreparedGraphNode {
                    target: Arc::clone(&full_target),
                    kind: PreparedGraphNodeKind::Layer { layer_index },
                }
            }
            CompositionGraphNodeKind::Operator(operator_node) => {
                let definition = context
                    .project
                    .definitions
                    .operators
                    .resolve(&operator_node.operator)
                    .ok_or_else(|| RenderError::BadGraph {
                        message: "missing operator definition".to_string(),
                    })?
                    .clone();
                let inputs = definition
                    .inputs
                    .iter()
                    .map(|port| {
                        incoming[*node_index]
                            .iter()
                            .find_map(|(input_port, node)| {
                                (input_port.0 == port.source_name).then_some(*node)
                            })
                            .ok_or_else(|| RenderError::BadGraph {
                                message: format!(
                                    "composition graph input port `{}` is not connected",
                                    port.source_name
                                ),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|input| {
                        let prepared_index = prepared_index_by_node[input];
                        (prepared_index != usize::MAX)
                            .then_some(prepared_index)
                            .ok_or_else(|| RenderError::BadGraph {
                                message: "composition graph order did not prepare an input first"
                                    .to_string(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let params = prepare_operator_params(
                    context.project,
                    context.sequence,
                    &definition,
                    &operator_node.params,
                    EffectParamTiming {
                        start_seconds: 0.0,
                        duration_seconds: context.sequence.duration.as_seconds_f64(),
                    },
                )?;
                let automation = automation_for_composition_node(context.sequence, &node.id);
                let bound_params = match (&definition.implementation, automation.is_empty()) {
                    (OperatorImplementation::Dsl(compiled), true) => {
                        Some(compiled.bind_params(&params)?)
                    }
                    _ => None,
                };
                PreparedGraphNode {
                    target: Arc::clone(&full_target),
                    kind: PreparedGraphNodeKind::Operator {
                        definition: Box::new(definition),
                        inputs,
                        params,
                        automation,
                        bound_params,
                    },
                }
            }
            CompositionGraphNodeKind::Output => {
                let inputs = incoming[*node_index]
                    .iter()
                    .map(|(_, input)| {
                        let prepared_index = prepared_index_by_node[*input];
                        (prepared_index != usize::MAX)
                            .then_some(prepared_index)
                            .ok_or_else(|| RenderError::BadGraph {
                                message:
                                    "composition graph order did not prepare output input first"
                                        .to_string(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                PreparedGraphNode {
                    target: Arc::clone(&full_target),
                    kind: PreparedGraphNodeKind::Output { inputs },
                }
            }
        };
        prepared_index_by_node[*node_index] = prepared_nodes.len();
        prepared_nodes.push(prepared);
    }

    let output_candidates = node_order
        .iter()
        .filter(|index| {
            matches!(
                graph_node(graph, &node_ids[**index]).map(|node| &node.kind),
                Ok(CompositionGraphNodeKind::Output)
            )
        })
        .copied()
        .collect::<Vec<_>>();
    let [output_source_index] = output_candidates.as_slice() else {
        return Err(RenderError::BadGraph {
            message: "composition graph must have exactly one output node".to_string(),
        });
    };
    let output_index = prepared_index_by_node[*output_source_index];
    if output_index == usize::MAX {
        return Err(RenderError::BadGraph {
            message: "composition graph output node is not in render order".to_string(),
        });
    }

    Ok(PreparedCompositionGraph {
        output_index,
        nodes: prepared_nodes,
    })
}

fn composition_graph_node_ids(
    graph: &SequenceCompositionGraph,
) -> Result<Vec<CompositionGraphNodeId>, RenderError> {
    let mut ids = IndexSet::new();
    for node in &graph.nodes {
        if !ids.insert(node.id.clone()) {
            return Err(RenderError::BadGraph {
                message: format!("duplicate composition graph node {}", node.id.0),
            });
        }
    }
    Ok(ids.into_iter().collect())
}

fn validate_composition_graph_layers(
    sequence: &Sequence,
    graph: &SequenceCompositionGraph,
) -> Result<(), RenderError> {
    let mut graph_layer_ids = IndexSet::new();
    for node in &graph.nodes {
        let CompositionGraphNodeKind::Layer { layer_id } = &node.kind else {
            continue;
        };
        if !sequence.layers.iter().any(|layer| layer.id == *layer_id) {
            return Err(RenderError::BadGraph {
                message: format!(
                    "composition graph layer node references missing layer {}",
                    layer_id.0
                ),
            });
        }
        if !graph_layer_ids.insert(layer_id.clone()) {
            return Err(RenderError::BadGraph {
                message: format!(
                    "composition graph has duplicate layer node for layer {}",
                    layer_id.0
                ),
            });
        }
    }
    for layer in &sequence.layers {
        if !graph_layer_ids.contains(&layer.id) {
            return Err(RenderError::BadGraph {
                message: format!(
                    "composition graph is missing layer node for layer {}",
                    layer.id.0
                ),
            });
        }
    }
    Ok(())
}

fn node_index(
    indexes: &IndexMap<CompositionGraphNodeId, usize>,
    node_id: &CompositionGraphNodeId,
) -> Result<usize, RenderError> {
    indexes
        .get(node_id)
        .copied()
        .ok_or_else(|| RenderError::BadGraph {
            message: format!(
                "edge references missing composition graph node {}",
                node_id.0
            ),
        })
}

fn graph_node<'a>(
    graph: &'a SequenceCompositionGraph,
    id: &CompositionGraphNodeId,
) -> Result<&'a dawn_language::sequence::CompositionGraphNode, RenderError> {
    graph
        .nodes
        .iter()
        .find(|node| node.id == *id)
        .ok_or_else(|| RenderError::BadGraph {
            message: format!("missing composition graph node {}", id.0),
        })
}

fn topological_composition_graph_order(
    node_ids: &[CompositionGraphNodeId],
    node_indexes: &IndexMap<CompositionGraphNodeId, usize>,
    graph: &SequenceCompositionGraph,
) -> Result<Vec<usize>, RenderError> {
    let mut indegree = vec![0usize; node_ids.len()];
    let mut outgoing = vec![Vec::<usize>::new(); node_ids.len()];
    for edge in &graph.edges {
        let from = node_index(node_indexes, &edge.from)?;
        let to = node_index(node_indexes, &edge.to)?;
        outgoing[from].push(to);
        indegree[to] += 1;
    }
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut order = Vec::with_capacity(node_ids.len());
    while let Some(index) = ready.pop() {
        order.push(index);
        for next in &outgoing[index] {
            indegree[*next] = indegree[*next].saturating_sub(1);
            if indegree[*next] == 0 {
                ready.push(*next);
            }
        }
    }
    if order.len() != node_ids.len() {
        return Err(RenderError::BadGraph {
            message: "composition graph contains a cycle".to_string(),
        });
    }
    Ok(order)
}
