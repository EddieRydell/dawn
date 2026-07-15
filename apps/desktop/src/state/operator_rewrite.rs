use std::collections::BTreeSet;
use std::sync::Arc;

use camino::Utf8Path;
use dawn_language::dsl::Identifier;
use dawn_language::identity::SourceIdentity;
use dawn_language::operator::{
    OperatorDefinition, OperatorDefinitionId, OperatorRef, validate_composition_graph,
};
use dawn_language::sequence::{
    AutomationDetachmentReason, AutomationTarget, CompositionGraphNodeId, CompositionGraphNodeKind,
    EffectGraphEdge, GraphPortId, SequenceId,
};
use dawn_project_io::{
    ProjectSession, apply_compiled_operator_document, compile_operator_document,
};
use indexmap::IndexMap;

use super::{
    DesktopState, PendingOperatorRewriteState, generated_source_texts, lock_unpoisoned,
    project_diagnostic,
};
use crate::dto::{
    AppSnapshot, OperatorDefinitionCandidate, OperatorDefinitionRewriteDescription,
    OperatorRequiredParamDescription, OperatorRewriteResolution, OperatorRewriteUsageDescription,
    OperatorRewriteValidation, OperatorSchemaParam, OperatorUpstreamSourceDescription,
    PendingOperatorRewrite,
};
use crate::state_tasks::GuiHistoryEntry;

impl DesktopState {
    pub(super) fn save_operator_draft(
        &self,
        path: &str,
        source: &str,
    ) -> Result<Option<AppSnapshot>, String> {
        if !path.ends_with(".operator.dawn") {
            return Ok(None);
        }
        let Some(project) = self.project_session() else {
            return Err("No project is open".to_string());
        };
        let relative = Utf8Path::new(path);
        let compiled = match compile_operator_document(relative, source) {
            Ok(compiled) => compiled,
            Err(diagnostics) => {
                *lock_unpoisoned(&self.pending_operator_rewrite) = None;
                let entrypoint = project.source.source_root.join(&project.source.entrypoint);
                let stable_diagnostics = dawn_project_io::check_project(&entrypoint).diagnostics;
                return Ok(Some(self.update_snapshot(|snapshot| {
                    snapshot.pending_operator_rewrite = None;
                    snapshot.diagnostics =
                        stable_diagnostics.iter().map(project_diagnostic).collect();
                    snapshot
                        .diagnostics
                        .extend(diagnostics.iter().map(project_diagnostic));
                    snapshot.status = format!("Operator draft {path} has compile errors");
                })));
            }
        };
        let analysis = dawn_language::operator_rewrite::analyze_operator_rewrite(
            &project.project,
            relative,
            &compiled.definitions,
        );
        if !analysis.breaking {
            let mut edited = (*project).clone();
            apply_compiled_operator_document(&mut edited, compiled);
            dawn_project_io::save_project(&edited).map_err(|error| error.to_string())?;
            self.after_file_saved(path);
            let entrypoint = edited.source.source_root.join(&edited.source.entrypoint);
            return Ok(Some(self.apply_project_refresh_check(
                entrypoint.as_str(),
                dawn_project_io::check_project(&entrypoint),
            )));
        }

        let mut next_token = lock_unpoisoned(&self.next_operator_rewrite_token);
        let token = *next_token;
        *next_token = next_token.saturating_add(1);
        let description = pending_description(token, path, &analysis, &compiled, &project);
        *lock_unpoisoned(&self.pending_operator_rewrite) = Some(PendingOperatorRewriteState {
            token,
            project_revision: self.snapshot().project_revision,
            path: relative.to_path_buf(),
            compiled,
        });
        let entrypoint = project.source.source_root.join(&project.source.entrypoint);
        let stable_diagnostics = dawn_project_io::check_project(&entrypoint).diagnostics;
        Ok(Some(self.update_snapshot(|snapshot| {
            snapshot.pending_operator_rewrite = Some(description);
            snapshot.diagnostics = stable_diagnostics.iter().map(project_diagnostic).collect();
            snapshot.status = format!("Operator rewrite for {path} needs reconciliation");
        })))
    }

    pub fn validate_operator_rewrite(
        &self,
        token: u32,
        resolution: OperatorRewriteResolution,
    ) -> OperatorRewriteValidation {
        match self.operator_rewrite_candidate(token, &resolution) {
            Ok(_) => OperatorRewriteValidation {
                valid: true,
                errors: Vec::new(),
            },
            Err(errors) => OperatorRewriteValidation {
                valid: false,
                errors,
            },
        }
    }

    pub fn apply_operator_rewrite(
        &self,
        token: u32,
        resolution: OperatorRewriteResolution,
    ) -> AppSnapshot {
        let before = match self.project_session() {
            Some(project) => project,
            None => return self.snapshot(),
        };
        let edited = match self.operator_rewrite_candidate(token, &resolution) {
            Ok(edited) => edited,
            Err(errors) => {
                return self.snapshot_with_error("operator.rewrite", "", &errors.join(" "));
            }
        };
        let path = {
            let pending = lock_unpoisoned(&self.pending_operator_rewrite);
            pending
                .as_ref()
                .map(|pending| pending.path.to_string())
                .unwrap_or_default()
        };
        let affected_paths = affected_rewrite_paths(&before, &path);
        let generated_text = match generated_source_texts(&edited, &affected_paths) {
            Ok(text) => text,
            Err(message) => return self.snapshot_with_error("operator.rewrite", &path, &message),
        };
        let edited = Arc::new(edited);
        lock_unpoisoned(&self.gui_history).push_undo(GuiHistoryEntry {
            before,
            after: Arc::clone(&edited),
            affected_paths: affected_paths.clone(),
            status_path: path.clone(),
        });
        self.after_file_saved(&path);
        self.schedule_gui_save(Arc::clone(&edited), affected_paths, path);
        *lock_unpoisoned(&self.pending_operator_rewrite) = None;
        self.update_snapshot(|snapshot| snapshot.pending_operator_rewrite = None);
        self.apply_gui_project_update(edited, "Operator rewrite applied", generated_text)
    }

    pub fn cancel_operator_rewrite(&self, token: u32) -> AppSnapshot {
        let mut pending = lock_unpoisoned(&self.pending_operator_rewrite);
        if pending
            .as_ref()
            .is_some_and(|pending| pending.token == token)
        {
            *pending = None;
        }
        drop(pending);
        self.update_snapshot(|snapshot| {
            snapshot.pending_operator_rewrite = None;
            snapshot.status = "Operator rewrite canceled; the draft remains unsaved".to_string();
        })
    }

    pub(super) fn invalidate_operator_rewrite(&self) {
        *lock_unpoisoned(&self.pending_operator_rewrite) = None;
        self.update_snapshot(|snapshot| snapshot.pending_operator_rewrite = None);
    }

    fn operator_rewrite_candidate(
        &self,
        token: u32,
        resolution: &OperatorRewriteResolution,
    ) -> Result<ProjectSession, Vec<String>> {
        let pending = lock_unpoisoned(&self.pending_operator_rewrite);
        let Some(pending) = pending.as_ref() else {
            return Err(vec!["Operator rewrite is no longer pending.".to_string()]);
        };
        if pending.token != token || pending.project_revision != self.snapshot().project_revision {
            return Err(vec!["Operator rewrite is stale.".to_string()]);
        }
        let Some(project) = self.project_session() else {
            return Err(vec!["No project is open.".to_string()]);
        };
        let mut edited = (*project).clone();
        apply_rewrite(&mut edited, pending, resolution)?;
        let sequence_ids = edited.project.sequences.keys().cloned().collect::<Vec<_>>();
        let mut errors = Vec::new();
        for id in sequence_ids {
            let Some(sequence) = edited.project.sequences.get(&id) else {
                continue;
            };
            if let Err(error) = validate_composition_graph(
                &sequence.composition_graph,
                &edited.project.definitions.operators,
            ) {
                errors.push(format!("{}: {}", id.0.object(), error.message));
            }
            if let Err(error) = crate::sequence_integrity::validate_sequence_integrity(&edited, &id)
            {
                errors.push(format!("{}: {}", id.0.object(), error.message()));
            }
        }
        if errors.is_empty() {
            Ok(edited)
        } else {
            Err(errors)
        }
    }
}

fn pending_description(
    token: u32,
    path: &str,
    analysis: &dawn_language::operator_rewrite::OperatorRewriteAnalysis,
    compiled: &dawn_project_io::CompiledOperatorDocument,
    project: &ProjectSession,
) -> PendingOperatorRewrite {
    let candidates = compiled
        .definitions
        .definitions
        .values()
        .map(|definition| OperatorDefinitionCandidate {
            name: definition.declaration_name.clone(),
            params: definition
                .params
                .iter()
                .map(|param| OperatorSchemaParam {
                    name: param.name.as_str().to_string(),
                    value_type: format!("{:?}", param.ty),
                    required: param.default.is_none(),
                })
                .collect(),
            input_ports: definition
                .inputs
                .iter()
                .map(|port| port.source_name.clone())
                .collect(),
        })
        .collect::<Vec<_>>();
    PendingOperatorRewrite {
        token,
        path: path.to_string(),
        definitions: analysis
            .definitions
            .iter()
            .filter(|definition| definition.breaking)
            .map(|definition| OperatorDefinitionRewriteDescription {
                old_name: definition.old_definition.0.object().to_string(),
                exact_replacement: definition
                    .exact_replacement
                    .as_ref()
                    .map(|id| id.0.object().to_string()),
                candidates: candidates.clone(),
                usage_count: definition.usages.len() as u32,
                usages: definition
                    .usages
                    .iter()
                    .map(|usage| OperatorRewriteUsageDescription {
                        sequence_path: usage.sequence_id.0.document().to_string(),
                        sequence_name: usage.sequence_id.0.object().to_string(),
                        node_id: usage.node_id.0.to_string(),
                        upstream_sources: upstream_sources(project, usage),
                    })
                    .collect(),
                removed_or_changed_params: definition
                    .removed_or_changed_params
                    .iter()
                    .map(|name| name.as_str().to_string())
                    .collect(),
                new_required_params: definition
                    .new_required_params
                    .iter()
                    .map(|(name, ty)| OperatorRequiredParamDescription {
                        name: name.as_str().to_string(),
                        value_type: format!("{ty:?}"),
                    })
                    .collect(),
                removed_ports: definition.removed_ports.clone(),
                new_ports: definition.new_ports.clone(),
            })
            .collect(),
    }
}

fn upstream_sources(
    project: &ProjectSession,
    usage: &dawn_language::operator_rewrite::OperatorUsage,
) -> Vec<OperatorUpstreamSourceDescription> {
    project
        .project
        .sequences
        .get(&usage.sequence_id)
        .into_iter()
        .flat_map(|sequence| &sequence.composition_graph.nodes)
        .filter(|node| node.id != usage.node_id)
        .filter_map(|node| match &node.kind {
            CompositionGraphNodeKind::Layer { .. } => Some(OperatorUpstreamSourceDescription {
                node_id: node.id.0.to_string(),
                port: "output".to_string(),
                label: format!("Layer node {}", node.id.0),
            }),
            CompositionGraphNodeKind::Operator(operator) => project
                .project
                .definitions
                .operators
                .resolve(&operator.operator)
                .map(|definition| OperatorUpstreamSourceDescription {
                    node_id: node.id.0.to_string(),
                    port: definition.output.source_name.clone(),
                    label: definition.display_name.clone(),
                }),
            CompositionGraphNodeKind::Output => None,
        })
        .collect()
}

fn apply_rewrite(
    session: &mut ProjectSession,
    pending: &PendingOperatorRewriteState,
    resolution: &OperatorRewriteResolution,
) -> Result<(), Vec<String>> {
    let old_definitions = session.project.definitions.operators.clone();
    apply_compiled_operator_document(session, pending.compiled.clone());
    let candidate_definitions = session.project.definitions.operators.clone();
    let mut errors = Vec::new();
    for (sequence_id, sequence) in &mut session.project.sequences {
        let old_nodes = sequence.composition_graph.nodes.clone();
        let mut deleted_nodes = BTreeSet::new();
        for node in &mut sequence.composition_graph.nodes {
            let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                continue;
            };
            let OperatorRef::Custom(old_id) = &operator.operator else {
                continue;
            };
            let old_id = old_id.clone();
            if old_id.0.document() != pending.path {
                continue;
            }
            let old_name = old_id.0.object();
            let replacement_name =
                match replacement_for_usage(resolution, sequence_id, &node.id, old_name) {
                    ReplacementDecision::Replace(name) => Some(name),
                    ReplacementDecision::Delete => None,
                    ReplacementDecision::Unspecified => candidate_definitions
                        .get(&OperatorDefinitionId(SourceIdentity::new(
                            pending.path.clone(),
                            old_name.to_string(),
                        )))
                        .map(|_| old_name),
                };
            let Some(replacement_name) = replacement_name else {
                deleted_nodes.insert(node.id.clone());
                continue;
            };
            let replacement_id = OperatorDefinitionId(SourceIdentity::new(
                pending.path.clone(),
                replacement_name.to_string(),
            ));
            let Some(replacement) = candidate_definitions.get(&replacement_id) else {
                errors.push(format!(
                    "Replacement operator `{replacement_name}` does not exist."
                ));
                continue;
            };
            let old_definition = old_definitions.get(&old_id);
            operator.operator = OperatorRef::Custom(replacement_id);
            rewrite_params(
                sequence_id,
                &node.id,
                old_name,
                old_definition,
                replacement,
                &mut operator.params,
                resolution,
                &mut errors,
            );
        }
        if !deleted_nodes.is_empty() {
            sequence
                .composition_graph
                .nodes
                .retain(|node| !deleted_nodes.contains(&node.id));
        }
        rewrite_edges(
            sequence_id,
            &old_nodes,
            &mut sequence.composition_graph.edges,
            &deleted_nodes,
            &old_definitions,
            &candidate_definitions,
            &pending.path,
            resolution,
            &mut errors,
        );
        rewrite_automation(
            sequence_id,
            sequence,
            &old_nodes,
            &deleted_nodes,
            &old_definitions,
            &candidate_definitions,
            &pending.path,
            resolution,
        );
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[allow(clippy::too_many_arguments)]
fn rewrite_params(
    sequence_id: &SequenceId,
    node_id: &CompositionGraphNodeId,
    old_name: &str,
    old_definition: Option<&OperatorDefinition>,
    replacement: &OperatorDefinition,
    params: &mut IndexMap<Identifier, dawn_language::effect::EffectParamValue>,
    resolution: &OperatorRewriteResolution,
    errors: &mut Vec<String>,
) {
    let old_params = std::mem::take(params);
    for (name, value) in old_params {
        let mapped =
            parameter_name_for_usage(resolution, sequence_id, node_id, old_name, name.as_str())
                .or_else(|| {
                    replacement
                        .params
                        .iter()
                        .any(|candidate| candidate.name == name)
                        .then_some(name.as_str())
                });
        if let Some(mapped) = mapped
            && let Ok(identifier) = Identifier::new(mapped.to_string())
        {
            params.insert(identifier, value);
        }
    }
    let required = replacement
        .params
        .iter()
        .filter(|declaration| {
            declaration.default.is_none() && !params.contains_key(&declaration.name)
        })
        .cloned()
        .collect::<Vec<_>>();
    for declaration in required {
        let value = resolution.required_values.iter().find(|item| {
            item.sequence_path == sequence_id.0.document().as_str()
                && item.sequence_name == sequence_id.0.object()
                && item.node_id == node_id.0.to_string()
                && item.name == declaration.name.as_str()
        });
        match value {
            Some(value) => {
                match crate::gui::model::effect_param_value_from_gui(value.value.clone()) {
                    Ok(value) => {
                        params.insert(declaration.name.clone(), value);
                    }
                    Err(error) => errors.push(error.message().to_string()),
                }
            }
            None => errors.push(format!(
                "Node {} requires a value for `{}`.",
                node_id.0,
                declaration.name.as_str()
            )),
        }
    }
    let _ = old_definition;
}

#[allow(clippy::too_many_arguments)]
fn rewrite_edges(
    sequence_id: &SequenceId,
    old_nodes: &[dawn_language::sequence::CompositionGraphNode],
    edges: &mut Vec<EffectGraphEdge>,
    deleted_nodes: &BTreeSet<CompositionGraphNodeId>,
    old_definitions: &dawn_language::operator::OperatorDefinitionStore,
    new_definitions: &dawn_language::operator::OperatorDefinitionStore,
    path: &Utf8Path,
    resolution: &OperatorRewriteResolution,
    errors: &mut Vec<String>,
) {
    edges.retain(|edge| !deleted_nodes.contains(&edge.from) && !deleted_nodes.contains(&edge.to));
    for edge in edges.iter_mut() {
        let Some((old_name, replacement)) = replacement_for_node(
            sequence_id,
            old_nodes,
            &edge.to,
            old_definitions,
            new_definitions,
            path,
            resolution,
        ) else {
            continue;
        };
        if replacement
            .inputs
            .iter()
            .any(|port| port.source_name == edge.to_port.0)
        {
            continue;
        }
        let mapped = port_name_for_usage(
            resolution,
            sequence_id,
            &edge.to,
            &old_name,
            &edge.to_port.0,
        )
        .map(ToString::to_string);
        if let Some(mapped) = mapped {
            edge.to_port = GraphPortId(mapped);
        } else {
            edge.to_port = GraphPortId(String::new());
        }
    }
    edges.retain(|edge| !edge.to_port.0.is_empty());
    for node in old_nodes {
        let Some((_old_name, replacement)) = replacement_for_node(
            sequence_id,
            old_nodes,
            &node.id,
            old_definitions,
            new_definitions,
            path,
            resolution,
        ) else {
            continue;
        };
        for input in &replacement.inputs {
            if edges
                .iter()
                .any(|edge| edge.to == node.id && edge.to_port.0 == input.source_name)
            {
                continue;
            }
            let connection = resolution.required_connections.iter().find(|item| {
                item.sequence_path == sequence_id.0.document().as_str()
                    && item.sequence_name == sequence_id.0.object()
                    && item.node_id == node.id.0.to_string()
                    && item.input_port == input.source_name
            });
            let Some(connection) = connection else {
                errors.push(format!(
                    "Node {} requires a connection for `{}`.",
                    node.id.0, input.source_name
                ));
                continue;
            };
            let Ok(from_node) = connection.from_node.parse::<u32>() else {
                errors.push(format!("Invalid upstream node `{}`.", connection.from_node));
                continue;
            };
            edges.push(EffectGraphEdge {
                from: CompositionGraphNodeId(from_node),
                from_port: GraphPortId(connection.from_port.clone()),
                to: node.id.clone(),
                to_port: GraphPortId(input.source_name.clone()),
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rewrite_automation(
    sequence_id: &SequenceId,
    sequence: &mut dawn_language::sequence::Sequence,
    old_nodes: &[dawn_language::sequence::CompositionGraphNode],
    deleted_nodes: &BTreeSet<CompositionGraphNodeId>,
    old_definitions: &dawn_language::operator::OperatorDefinitionStore,
    new_definitions: &dawn_language::operator::OperatorDefinitionStore,
    path: &Utf8Path,
    resolution: &OperatorRewriteResolution,
) {
    for clip in &mut sequence.automation_clips {
        clip.detach_bindings(AutomationDetachmentReason::TargetDeleted, |target| {
            matches!(target, AutomationTarget::CompositionNodeParam { node_id, .. } if deleted_nodes.contains(node_id))
        });
        let mut detached = Vec::new();
        for binding in &mut clip.bindings {
            let AutomationTarget::CompositionNodeParam { node_id, param } = &mut binding.target
            else {
                continue;
            };
            let Some((old_name, replacement)) = replacement_for_node(
                sequence_id,
                old_nodes,
                node_id,
                old_definitions,
                new_definitions,
                path,
                resolution,
            ) else {
                continue;
            };
            if replacement
                .params
                .iter()
                .any(|candidate| candidate.name == *param)
            {
                continue;
            }
            let mapped = parameter_name_for_usage(
                resolution,
                sequence_id,
                node_id,
                &old_name,
                param.as_str(),
            );
            if let Some(mapped) = mapped.and_then(|mapped| Identifier::new(mapped.to_string()).ok())
            {
                *param = mapped;
            } else {
                detached.push(binding.target.clone());
            }
        }
        for target in detached {
            clip.detach_bindings(
                AutomationDetachmentReason::OperatorSchemaChanged,
                |candidate| candidate == &target,
            );
        }
    }
}

fn replacement_for_node<'a>(
    sequence_id: &SequenceId,
    nodes: &[dawn_language::sequence::CompositionGraphNode],
    node_id: &CompositionGraphNodeId,
    old_definitions: &dawn_language::operator::OperatorDefinitionStore,
    new_definitions: &'a dawn_language::operator::OperatorDefinitionStore,
    path: &Utf8Path,
    resolution: &OperatorRewriteResolution,
) -> Option<(String, &'a OperatorDefinition)> {
    let node = nodes.iter().find(|node| &node.id == node_id)?;
    let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
        return None;
    };
    let OperatorRef::Custom(old_id) = &operator.operator else {
        return None;
    };
    if old_id.0.document() != path {
        return None;
    }
    let old = old_definitions.get(old_id)?;
    let replacement_name =
        match replacement_for_usage(resolution, sequence_id, node_id, &old.declaration_name) {
            ReplacementDecision::Replace(name) => name,
            ReplacementDecision::Delete => return None,
            ReplacementDecision::Unspecified => {
                let exact = OperatorDefinitionId(SourceIdentity::new(
                    path.to_path_buf(),
                    old.declaration_name.clone(),
                ));
                new_definitions.get(&exact)?;
                old.declaration_name.as_str()
            }
        };
    let replacement_id = OperatorDefinitionId(SourceIdentity::new(
        path.to_path_buf(),
        replacement_name.to_string(),
    ));
    Some((
        old.declaration_name.clone(),
        new_definitions.get(&replacement_id)?,
    ))
}

enum ReplacementDecision<'a> {
    Unspecified,
    Delete,
    Replace(&'a str),
}

fn replacement_for_usage<'a>(
    resolution: &'a OperatorRewriteResolution,
    sequence_id: &SequenceId,
    node_id: &CompositionGraphNodeId,
    old_definition: &str,
) -> ReplacementDecision<'a> {
    if let Some(item) = resolution.usage_definitions.iter().find(|item| {
        usage_matches(
            &item.sequence_path,
            &item.sequence_name,
            &item.node_id,
            sequence_id,
            node_id,
        )
    }) {
        return item
            .replacement_name
            .as_deref()
            .map_or(ReplacementDecision::Delete, ReplacementDecision::Replace);
    }
    resolution
        .definitions
        .iter()
        .find(|item| item.old_name == old_definition)
        .map_or(ReplacementDecision::Unspecified, |item| {
            item.replacement_name
                .as_deref()
                .map_or(ReplacementDecision::Delete, ReplacementDecision::Replace)
        })
}

fn parameter_name_for_usage<'a>(
    resolution: &'a OperatorRewriteResolution,
    sequence_id: &SequenceId,
    node_id: &CompositionGraphNodeId,
    old_definition: &str,
    old_name: &str,
) -> Option<&'a str> {
    if let Some(item) = resolution.usage_parameters.iter().find(|item| {
        item.old_name == old_name
            && usage_matches(
                &item.sequence_path,
                &item.sequence_name,
                &item.node_id,
                sequence_id,
                node_id,
            )
    }) {
        return item.new_name.as_deref();
    }
    resolution
        .parameters
        .iter()
        .find(|item| item.old_definition == old_definition && item.old_name == old_name)
        .and_then(|item| item.new_name.as_deref())
}

fn port_name_for_usage<'a>(
    resolution: &'a OperatorRewriteResolution,
    sequence_id: &SequenceId,
    node_id: &CompositionGraphNodeId,
    old_definition: &str,
    old_name: &str,
) -> Option<&'a str> {
    if let Some(item) = resolution.usage_ports.iter().find(|item| {
        item.old_name == old_name
            && usage_matches(
                &item.sequence_path,
                &item.sequence_name,
                &item.node_id,
                sequence_id,
                node_id,
            )
    }) {
        return item.new_name.as_deref();
    }
    resolution
        .ports
        .iter()
        .find(|item| item.old_definition == old_definition && item.old_name == old_name)
        .and_then(|item| item.new_name.as_deref())
}

fn usage_matches(
    path: &str,
    name: &str,
    node: &str,
    sequence_id: &SequenceId,
    node_id: &CompositionGraphNodeId,
) -> bool {
    path == sequence_id.0.document().as_str()
        && name == sequence_id.0.object()
        && node == node_id.0.to_string()
}

fn affected_rewrite_paths(session: &ProjectSession, operator_path: &str) -> BTreeSet<String> {
    std::iter::once(operator_path.to_string())
        .chain(
            session
                .project
                .sequences
                .iter()
                .filter(|(_, sequence)| {
                    sequence.composition_graph.nodes.iter().any(|node| {
                        matches!(
                            &node.kind,
                            CompositionGraphNodeKind::Operator(operator)
                                if matches!(&operator.operator, OperatorRef::Custom(id) if id.0.document().as_str() == operator_path)
                        )
                    })
                })
                .map(|(id, _)| id.0.document().to_string()),
        )
        .collect()
}
