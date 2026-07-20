use crate::dsl::{Identifier, Type};
use crate::identity::DocumentId;
use crate::model::DawnProject;
use crate::operator::{
    OperatorDefinition, OperatorDefinitionId, OperatorDefinitionStore, OperatorRef,
};
use crate::sequence::{CompositionGraphNodeId, CompositionGraphNodeKind, SequenceId};

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorRewriteAnalysis {
    pub source_document: DocumentId,
    pub definitions: Vec<OperatorDefinitionRewrite>,
    pub breaking: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorDefinitionRewrite {
    pub old_definition: OperatorDefinitionId,
    pub exact_replacement: Option<OperatorDefinitionId>,
    pub compatible_replacements: Vec<OperatorDefinitionId>,
    pub usages: Vec<OperatorUsage>,
    pub removed_or_changed_params: Vec<Identifier>,
    pub new_required_params: Vec<(Identifier, Type)>,
    pub removed_ports: Vec<String>,
    pub new_ports: Vec<String>,
    pub breaking: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorUsage {
    pub sequence_id: SequenceId,
    pub node_id: CompositionGraphNodeId,
}

pub fn analyze_operator_rewrite(
    project: &DawnProject,
    source_document: &DocumentId,
    candidate: &OperatorDefinitionStore,
    editable_module_id: uuid::Uuid,
) -> OperatorRewriteAnalysis {
    let candidates = candidate.definitions.values().collect::<Vec<_>>();
    let definitions = project
        .definitions
        .operators
        .definitions
        .iter()
        .filter(|(id, _)| id.0.document_id() == source_document)
        .map(|(old_id, old)| {
            analyze_definition(project, old_id, old, &candidates, editable_module_id)
        })
        .collect::<Vec<_>>();
    OperatorRewriteAnalysis {
        source_document: source_document.clone(),
        breaking: definitions.iter().any(|definition| definition.breaking),
        definitions,
    }
}

fn analyze_definition(
    project: &DawnProject,
    old_id: &OperatorDefinitionId,
    old: &OperatorDefinition,
    candidates: &[&OperatorDefinition],
    editable_module_id: uuid::Uuid,
) -> OperatorDefinitionRewrite {
    let exact = candidates
        .iter()
        .copied()
        .find(|candidate| candidate.declaration_name == old.declaration_name);
    let replacement = exact;
    let usages = operator_usages(project, old_id, editable_module_id);
    let changes = replacement
        .map(|replacement| schema_changes(old, replacement))
        .unwrap_or_else(|| SchemaChanges {
            removed_or_changed_params: old.params.iter().map(|param| param.name.clone()).collect(),
            new_required_params: Vec::new(),
            removed_ports: old
                .inputs
                .iter()
                .map(|port| port.source_name.clone())
                .collect(),
            new_ports: Vec::new(),
        });
    let SchemaChanges {
        removed_or_changed_params,
        new_required_params,
        removed_ports,
        new_ports,
    } = changes;
    let changed_param_is_used = usages.iter().any(|usage| {
        project
            .sequences
            .get(&usage.sequence_id)
            .is_some_and(|sequence| {
                let has_override = sequence
                    .composition_graph
                    .nodes
                    .iter()
                    .find(|node| node.id == usage.node_id)
                    .and_then(|node| match &node.kind {
                        CompositionGraphNodeKind::Operator(operator) => Some(operator),
                        _ => None,
                    })
                    .is_some_and(|operator| {
                        removed_or_changed_params
                            .iter()
                            .any(|name| operator.params.contains_key(name))
                    });
                let has_automation = sequence.automation_clips.iter().any(|clip| {
                    clip.bindings.iter().any(|binding| {
                        matches!(
                            &binding.target,
                            crate::sequence::AutomationTarget::CompositionNodeParam { node_id, param }
                                if node_id == &usage.node_id && removed_or_changed_params.contains(param)
                        )
                    })
                });
                has_override || has_automation
            })
    });
    let removed_port_is_connected =
        usages.iter().any(|usage| {
            project
                .sequences
                .get(&usage.sequence_id)
                .is_some_and(|sequence| {
                    sequence.composition_graph.edges.iter().any(|edge| {
                        edge.to == usage.node_id && removed_ports.contains(&edge.to_port.0)
                    })
                })
        });
    let breaking = !usages.is_empty()
        && (replacement.is_none()
            || changed_param_is_used
            || !new_required_params.is_empty()
            || removed_port_is_connected
            || !new_ports.is_empty());
    OperatorDefinitionRewrite {
        old_definition: old_id.clone(),
        exact_replacement: exact.and_then(custom_definition_id),
        compatible_replacements: candidates
            .iter()
            .filter_map(|candidate| custom_definition_id(candidate))
            .collect(),
        usages,
        removed_or_changed_params,
        new_required_params,
        removed_ports,
        new_ports,
        breaking,
    }
}

struct SchemaChanges {
    removed_or_changed_params: Vec<Identifier>,
    new_required_params: Vec<(Identifier, Type)>,
    removed_ports: Vec<String>,
    new_ports: Vec<String>,
}

fn schema_changes(old: &OperatorDefinition, new: &OperatorDefinition) -> SchemaChanges {
    let removed_or_changed_params = old
        .params
        .iter()
        .filter(|old_param| {
            new.params
                .iter()
                .find(|new_param| new_param.name == old_param.name)
                .is_none_or(|new_param| new_param.ty != old_param.ty)
        })
        .map(|param| param.name.clone())
        .collect();
    let new_required_params = new
        .params
        .iter()
        .filter(|new_param| {
            new_param.default.is_none()
                && old
                    .params
                    .iter()
                    .find(|old_param| old_param.name == new_param.name)
                    .is_none_or(|old_param| old_param.ty != new_param.ty)
        })
        .map(|param| (param.name.clone(), param.ty.clone()))
        .collect();
    let removed_ports = old
        .inputs
        .iter()
        .filter(|old_port| {
            !new.inputs
                .iter()
                .any(|new_port| new_port.source_name == old_port.source_name)
        })
        .map(|port| port.source_name.clone())
        .collect();
    let new_ports = new
        .inputs
        .iter()
        .filter(|new_port| {
            !old.inputs
                .iter()
                .any(|old_port| old_port.source_name == new_port.source_name)
        })
        .map(|port| port.source_name.clone())
        .collect();
    SchemaChanges {
        removed_or_changed_params,
        new_required_params,
        removed_ports,
        new_ports,
    }
}

fn operator_usages(
    project: &DawnProject,
    id: &OperatorDefinitionId,
    editable_module_id: uuid::Uuid,
) -> Vec<OperatorUsage> {
    project
        .sequences
        .iter()
        .filter(|(sequence_id, _)| sequence_id.0.module_id() == editable_module_id)
        .flat_map(|(sequence_id, sequence)| {
            sequence.composition_graph.nodes.iter().filter_map(|node| {
                let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
                    return None;
                };
                (operator.operator == OperatorRef::Custom(id.clone())).then(|| OperatorUsage {
                    sequence_id: sequence_id.clone(),
                    node_id: node.id.clone(),
                })
            })
        })
        .collect()
}

fn custom_definition_id(definition: &OperatorDefinition) -> Option<OperatorDefinitionId> {
    match &definition.id {
        OperatorRef::Custom(id) => Some(id.clone()),
        OperatorRef::Builtin(_) => None,
    }
}
