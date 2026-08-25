use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::dsl::Identifier;
use dawn_language::identity::{DocumentId, SourceIdentity};
use dawn_language::operator::{
    OperatorDefinition, OperatorDefinitionId, OperatorDefinitionStore, OperatorRef,
    validate_composition_graph,
};
use dawn_language::sequence::{
    AutomationDetachmentReason, AutomationTarget, CompositionGraphNodeId, CompositionGraphNodeKind,
    EffectGraphEdge, GraphPortId, SequenceId,
};
use dawn_project_io::{
    ProjectSession, apply_compiled_operator_document, compile_operator_document, save_project,
};
use indexmap::IndexMap;

use super::{
    DesktopState, PendingOperatorRewriteKind, PendingOperatorRewriteState,
    decorate_deprecation_status, generated_source_texts, lock_unpoisoned, package_status,
    project_diagnostic,
};
use crate::dto::{
    AppSnapshot, OperatorDefinitionCandidate, OperatorDefinitionKey,
    OperatorDefinitionRewriteDescription, OperatorRequiredParamDescription,
    OperatorRewriteResolution, OperatorRewriteUsageDescription, OperatorRewriteValidation,
    OperatorSchemaParam, OperatorUpstreamSourceDescription, PackageDependencySource,
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
        let document_id = project.source.project_document(relative.to_path_buf());
        let compiled = match compile_operator_document(&document_id, source) {
            Ok(compiled) => compiled,
            Err(diagnostics) => {
                *lock_unpoisoned(&self.pending_operator_rewrite) = None;
                let root = project.source.project_root();
                let stable_diagnostics = dawn_project_io::check_package(root).diagnostics;
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
            &compiled.document_id,
            &compiled.definitions,
            project.source.project_module_id(),
        );
        if !analysis.breaking {
            let mut edited = (*project).clone();
            apply_compiled_operator_document(&mut edited, compiled);
            dawn_project_io::save_project(&edited).map_err(|error| error.to_string())?;
            self.after_file_saved(path);
            let root = edited.source.project_root().to_path_buf();
            return Ok(Some(self.apply_project_refresh_check(
                root.as_str(),
                dawn_project_io::check_package(&root),
            )));
        }

        let mut next_token = lock_unpoisoned(&self.next_operator_rewrite_token);
        let token = *next_token;
        *next_token = next_token.saturating_add(1);
        let description = pending_description(
            token,
            path,
            &[CandidateRewrite {
                analysis,
                candidates: compiled.definitions.clone(),
            }],
            &project,
        );
        let target_documents = std::iter::once(compiled.document_id.clone()).collect();
        *lock_unpoisoned(&self.pending_operator_rewrite) = Some(PendingOperatorRewriteState {
            token,
            project_revision: self.snapshot().project_revision,
            target_documents,
            kind: PendingOperatorRewriteKind::Document {
                path: relative.to_path_buf(),
                compiled: Box::new(compiled),
            },
        });
        let root = project.source.project_root();
        let stable_diagnostics = dawn_project_io::check_package(root).diagnostics;
        Ok(Some(self.update_snapshot(|snapshot| {
            snapshot.pending_operator_rewrite = Some(description);
            snapshot.diagnostics = stable_diagnostics.iter().map(project_diagnostic).collect();
            snapshot.status = format!("Operator rewrite for {path} needs reconciliation");
        })))
    }

    pub(super) fn stage_package_operator_rewrite(
        &self,
        root: &Utf8Path,
        candidate: dawn_package::PreparedPackageCandidate,
        candidate_session: ProjectSession,
    ) -> Result<bool, String> {
        let current = self
            .project_session()
            .ok_or_else(|| "No project is open".to_string())?;
        let rewrites = package_candidate_rewrites(&current, &candidate_session);
        if rewrites.is_empty() {
            return Ok(false);
        }

        let mut next_token = lock_unpoisoned(&self.next_operator_rewrite_token);
        let token = *next_token;
        *next_token = next_token.saturating_add(1);
        drop(next_token);

        let subject = package_update_subject(&candidate);
        let target_documents = rewrites
            .iter()
            .map(|rewrite| rewrite.analysis.source_document.clone())
            .collect();
        let description = pending_description(token, &subject, &rewrites, &current);
        *lock_unpoisoned(&self.pending_operator_rewrite) = Some(PendingOperatorRewriteState {
            token,
            project_revision: self.snapshot().project_revision,
            target_documents,
            kind: PendingOperatorRewriteKind::PackageUpdate {
                root: root.to_path_buf(),
                candidate: Box::new(candidate),
                session: Box::new(candidate_session),
            },
        });
        self.update_snapshot(|snapshot| {
            snapshot.pending_operator_rewrite = Some(description);
            snapshot.status = "Package update needs operator reconciliation".to_string();
        });
        Ok(true)
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
        let (target_documents, commit) = {
            let pending = lock_unpoisoned(&self.pending_operator_rewrite);
            let Some(pending) = pending.as_ref().filter(|pending| pending.token == token) else {
                return self.snapshot_with_error(
                    "operator.rewrite",
                    "",
                    "Operator rewrite is no longer pending.",
                );
            };
            let commit = match &pending.kind {
                PendingOperatorRewriteKind::Document { path, .. } => {
                    RewriteCommit::Document { path: path.clone() }
                }
                PendingOperatorRewriteKind::PackageUpdate {
                    root, candidate, ..
                } => RewriteCommit::PackageUpdate {
                    root: root.clone(),
                    candidate: candidate.clone(),
                },
            };
            (pending.target_documents.clone(), commit)
        };
        let affected_paths = affected_rewrite_paths(&before, &target_documents);
        let status_path = commit.status_path();
        let generated_text = match generated_source_texts(&edited, &affected_paths) {
            Ok(text) => text,
            Err(message) => {
                return self.snapshot_with_error("operator.rewrite", &status_path, &message);
            }
        };
        match commit {
            RewriteCommit::Document { path } => {
                let path = path.to_string();
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
                self.update_snapshot(|snapshot| {
                    snapshot.pending_operator_rewrite = None;
                });
                self.apply_gui_project_update(edited, "Operator rewrite applied", generated_text)
            }
            RewriteCommit::PackageUpdate { root, candidate } => self.apply_package_operator_update(
                &before,
                edited,
                affected_paths,
                generated_text,
                &root,
                &candidate,
            ),
        }
    }

    pub fn cancel_operator_rewrite(&self, token: u32) -> AppSnapshot {
        let mut pending = lock_unpoisoned(&self.pending_operator_rewrite);
        let package_update = pending.as_ref().is_some_and(|pending| {
            pending.token == token
                && matches!(
                    pending.kind,
                    PendingOperatorRewriteKind::PackageUpdate { .. }
                )
        });
        let matched = pending
            .as_ref()
            .is_some_and(|pending| pending.token == token);
        if matched {
            *pending = None;
        }
        drop(pending);
        if !matched {
            return self.snapshot();
        }
        self.update_snapshot(|snapshot| {
            snapshot.pending_operator_rewrite = None;
            snapshot.status = if package_update {
                "Package update canceled; the current lock and project remain unchanged".to_string()
            } else {
                "Operator rewrite canceled; the draft remains unsaved".to_string()
            };
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
        let revision = self.snapshot().project_revision;
        let Some(project) = self.project_session() else {
            return Err(vec!["No project is open.".to_string()]);
        };
        let edited = {
            let pending = lock_unpoisoned(&self.pending_operator_rewrite);
            let Some(pending) = pending.as_ref() else {
                return Err(vec!["Operator rewrite is no longer pending.".to_string()]);
            };
            if pending.token != token || pending.project_revision != revision {
                return Err(vec!["Operator rewrite is stale.".to_string()]);
            }
            let mut edited = match &pending.kind {
                PendingOperatorRewriteKind::Document { compiled, .. } => {
                    let mut edited = (*project).clone();
                    apply_compiled_operator_document(&mut edited, compiled.as_ref().clone());
                    edited
                }
                PendingOperatorRewriteKind::PackageUpdate { session, .. } => {
                    session.as_ref().clone()
                }
            };
            apply_rewrite(
                &mut edited,
                &project.project.definitions.operators,
                &pending.target_documents,
                resolution,
            )?;
            edited
        };
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
            if let Err(error) =
                dawn_language::validation::validate_sequence_by_id(&edited.project, &id)
            {
                errors.push(format!("{}: {}", id.0.object(), error.message));
            }
        }
        if errors.is_empty() {
            Ok(edited)
        } else {
            Err(errors)
        }
    }

    fn apply_package_operator_update(
        &self,
        before: &ProjectSession,
        edited: ProjectSession,
        affected_paths: BTreeSet<String>,
        generated_text: BTreeMap<String, String>,
        root: &Utf8Path,
        candidate: &dawn_package::PreparedPackageCandidate,
    ) -> AppSnapshot {
        if let Err(error) = save_project(&edited) {
            return self.snapshot_with_error(
                "package.update.save",
                root.as_str(),
                &format!(
                    "Package update was not accepted because rewritten project files could not be saved: {error}"
                ),
            );
        }
        if let Err(error) = candidate.lock.write(root) {
            let rollback = save_project(before);
            let message = match rollback {
                Ok(_) => format!(
                    "Package lockfile was not accepted: {error}. Rewritten project files were restored."
                ),
                Err(rollback_error) => format!(
                    "Package lockfile was not accepted: {error}. Restoring project files also failed: {rollback_error}"
                ),
            };
            return self.snapshot_with_error("package.update.lock", root.as_str(), &message);
        }

        lock_unpoisoned(&self.gui_history).clear();
        *lock_unpoisoned(&self.pending_operator_rewrite) = None;
        let edited = Arc::new(edited);
        let _ = self.apply_gui_project_update(
            Arc::clone(&edited),
            "Package operator rewrite applied",
            generated_text,
        );
        self.refresh_saved_tabs(&affected_paths);

        let mut status = package_status(root, Some(&edited));
        decorate_deprecation_status(&mut status, candidate);
        status.update_checked = true;
        for dependency in &mut status.dependencies {
            if matches!(dependency.source, PackageDependencySource::Registry) {
                dependency.update_available = Some(false);
            }
        }
        let change_count = candidate.changes.len();
        self.update_snapshot(|snapshot| {
            snapshot.package = status;
            snapshot.status =
                format!("Accepted {change_count} package change(s) after operator reconciliation");
        })
    }
}

struct CandidateRewrite {
    analysis: dawn_language::operator_rewrite::OperatorRewriteAnalysis,
    candidates: OperatorDefinitionStore,
}

enum RewriteCommit {
    Document {
        path: Utf8PathBuf,
    },
    PackageUpdate {
        root: Utf8PathBuf,
        candidate: Box<dawn_package::PreparedPackageCandidate>,
    },
}

impl RewriteCommit {
    fn status_path(&self) -> String {
        match self {
            Self::Document { path } => path.to_string(),
            Self::PackageUpdate { candidate, .. } => package_update_subject(candidate),
        }
    }
}

fn package_candidate_rewrites(
    current: &ProjectSession,
    candidate: &ProjectSession,
) -> Vec<CandidateRewrite> {
    let editable_module_id = current.source.project_module_id();
    let changed_documents = current
        .project
        .definitions
        .operators
        .definitions
        .iter()
        .filter(|(id, definition)| {
            id.0.module_id() != editable_module_id
                && candidate.project.definitions.operators.get(id) != Some(*definition)
        })
        .map(|(id, _)| id.0.document_id().clone())
        .collect::<BTreeSet<_>>();

    changed_documents
        .into_iter()
        .filter_map(|document| {
            let mut candidates = OperatorDefinitionStore::default();
            for (id, definition) in &candidate.project.definitions.operators.definitions {
                if id.0.document_id() == &document {
                    candidates.insert(id.clone(), definition.clone());
                }
            }
            let analysis = dawn_language::operator_rewrite::analyze_operator_rewrite(
                &current.project,
                &document,
                &candidates,
                editable_module_id,
            );
            analysis.breaking.then_some(CandidateRewrite {
                analysis,
                candidates,
            })
        })
        .collect()
}

fn package_update_subject(candidate: &dawn_package::PreparedPackageCandidate) -> String {
    if candidate.changes.is_empty() {
        return "Package synchronization".to_string();
    }
    let changes = candidate
        .changes
        .iter()
        .map(|change| match &change.candidate_version {
            Some(version) => format!("{}@{version}", change.package),
            None => format!("{} (removed)", change.package),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("Package update: {changes}")
}

fn pending_description(
    token: u32,
    path: &str,
    rewrites: &[CandidateRewrite],
    project: &ProjectSession,
) -> PendingOperatorRewrite {
    let mut definitions = Vec::new();
    for rewrite in rewrites {
        let candidates = rewrite
            .candidates
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
        definitions.extend(
            rewrite
                .analysis
                .definitions
                .iter()
                .filter(|definition| definition.breaking)
                .map(|definition| OperatorDefinitionRewriteDescription {
                    definition: operator_definition_key(&definition.old_definition),
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
                }),
        );
    }
    PendingOperatorRewrite {
        token,
        path: path.to_string(),
        definitions,
    }
}

fn operator_definition_key(id: &OperatorDefinitionId) -> OperatorDefinitionKey {
    OperatorDefinitionKey {
        module_id: id.0.module_id().to_string(),
        document: id.0.document().to_string(),
        name: id.0.object().to_string(),
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
    old_definitions: &OperatorDefinitionStore,
    target_documents: &BTreeSet<DocumentId>,
    resolution: &OperatorRewriteResolution,
) -> Result<(), Vec<String>> {
    let candidate_definitions = session.project.definitions.operators.clone();
    let editable_module_id = session.source.project_module_id();
    let mut errors = Vec::new();
    for (sequence_id, sequence) in &mut session.project.sequences {
        if sequence_id.0.module_id() != editable_module_id {
            continue;
        }
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
            if !target_documents.contains(old_id.0.document_id()) {
                continue;
            }
            let old_name = old_id.0.object();
            let old_key = operator_definition_key(&old_id);
            let replacement_name =
                match replacement_for_usage(resolution, sequence_id, &node.id, &old_key) {
                    ReplacementDecision::Replace(name) => Some(name),
                    ReplacementDecision::Delete => None,
                    ReplacementDecision::Unspecified => {
                        candidate_definitions.get(&old_id).map(|_| old_name)
                    }
                };
            let Some(replacement_name) = replacement_name else {
                deleted_nodes.insert(node.id.clone());
                continue;
            };
            let replacement_id = OperatorDefinitionId(SourceIdentity::from_document(
                old_id.0.document_id().clone(),
                replacement_name.to_string(),
            ));
            let Some(replacement) = candidate_definitions.get(&replacement_id) else {
                errors.push(format!(
                    "Replacement operator `{replacement_name}` does not exist."
                ));
                continue;
            };
            operator.operator = OperatorRef::Custom(replacement_id);
            rewrite_params(
                sequence_id,
                &node.id,
                &old_key,
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
            old_definitions,
            &candidate_definitions,
            target_documents,
            resolution,
            &mut errors,
        );
        rewrite_automation(
            sequence_id,
            sequence,
            &old_nodes,
            &deleted_nodes,
            old_definitions,
            &candidate_definitions,
            target_documents,
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
    old_definition: &OperatorDefinitionKey,
    replacement: &OperatorDefinition,
    params: &mut IndexMap<Identifier, dawn_language::effect::EffectParamValue>,
    resolution: &OperatorRewriteResolution,
    errors: &mut Vec<String>,
) {
    let old_params = std::mem::take(params);
    for (name, value) in old_params {
        let mapped = parameter_name_for_usage(
            resolution,
            sequence_id,
            node_id,
            old_definition,
            name.as_str(),
        )
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
}

#[allow(clippy::too_many_arguments)]
fn rewrite_edges(
    sequence_id: &SequenceId,
    old_nodes: &[dawn_language::sequence::CompositionGraphNode],
    edges: &mut Vec<EffectGraphEdge>,
    deleted_nodes: &BTreeSet<CompositionGraphNodeId>,
    old_definitions: &OperatorDefinitionStore,
    new_definitions: &OperatorDefinitionStore,
    target_documents: &BTreeSet<DocumentId>,
    resolution: &OperatorRewriteResolution,
    errors: &mut Vec<String>,
) {
    edges.retain(|edge| !deleted_nodes.contains(&edge.from) && !deleted_nodes.contains(&edge.to));
    for edge in edges.iter_mut() {
        let Some((old_definition, replacement)) = replacement_for_node(
            sequence_id,
            old_nodes,
            &edge.to,
            old_definitions,
            new_definitions,
            target_documents,
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
            &old_definition,
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
        let Some((_old_definition, replacement)) = replacement_for_node(
            sequence_id,
            old_nodes,
            &node.id,
            old_definitions,
            new_definitions,
            target_documents,
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
    old_definitions: &OperatorDefinitionStore,
    new_definitions: &OperatorDefinitionStore,
    target_documents: &BTreeSet<DocumentId>,
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
            let Some((old_definition, replacement)) = replacement_for_node(
                sequence_id,
                old_nodes,
                node_id,
                old_definitions,
                new_definitions,
                target_documents,
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
                &old_definition,
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

#[allow(clippy::too_many_arguments)]
fn replacement_for_node<'a>(
    sequence_id: &SequenceId,
    nodes: &[dawn_language::sequence::CompositionGraphNode],
    node_id: &CompositionGraphNodeId,
    old_definitions: &OperatorDefinitionStore,
    new_definitions: &'a OperatorDefinitionStore,
    target_documents: &BTreeSet<DocumentId>,
    resolution: &OperatorRewriteResolution,
) -> Option<(OperatorDefinitionKey, &'a OperatorDefinition)> {
    let node = nodes.iter().find(|node| &node.id == node_id)?;
    let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
        return None;
    };
    let OperatorRef::Custom(old_id) = &operator.operator else {
        return None;
    };
    if !target_documents.contains(old_id.0.document_id()) {
        return None;
    }
    let old = old_definitions.get(old_id)?;
    let old_definition = operator_definition_key(old_id);
    let replacement_name =
        match replacement_for_usage(resolution, sequence_id, node_id, &old_definition) {
            ReplacementDecision::Replace(name) => name,
            ReplacementDecision::Delete => return None,
            ReplacementDecision::Unspecified => {
                new_definitions.get(old_id)?;
                old.declaration_name.as_str()
            }
        };
    let replacement_id = OperatorDefinitionId(SourceIdentity::from_document(
        old_id.0.document_id().clone(),
        replacement_name.to_string(),
    ));
    Some((old_definition, new_definitions.get(&replacement_id)?))
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
    old_definition: &OperatorDefinitionKey,
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
        .find(|item| &item.definition == old_definition)
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
    old_definition: &OperatorDefinitionKey,
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
        .find(|item| &item.definition == old_definition && item.old_name == old_name)
        .and_then(|item| item.new_name.as_deref())
}

fn port_name_for_usage<'a>(
    resolution: &'a OperatorRewriteResolution,
    sequence_id: &SequenceId,
    node_id: &CompositionGraphNodeId,
    old_definition: &OperatorDefinitionKey,
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
        .find(|item| &item.definition == old_definition && item.old_name == old_name)
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

fn affected_rewrite_paths(
    session: &ProjectSession,
    target_documents: &BTreeSet<DocumentId>,
) -> BTreeSet<String> {
    target_documents
        .iter()
        .filter(|document| session.source.is_project_owned(document))
        .map(|document| document.path().to_string())
        .chain(
            session
                .project
                .sequences
                .iter()
                .filter(|(id, sequence)| {
                    id.0.module_id() == session.source.project_module_id()
                        && sequence.composition_graph.nodes.iter().any(|node| {
                            matches!(
                                &node.kind,
                                CompositionGraphNodeKind::Operator(operator)
                                    if matches!(
                                        &operator.operator,
                                        OperatorRef::Custom(id)
                                            if target_documents
                                                .contains(id.0.document_id())
                                    )
                            )
                        })
                })
                .map(|(id, _)| id.0.document().to_string()),
        )
        .collect()
}
