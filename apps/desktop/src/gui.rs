use std::collections::BTreeSet;

use camino::Utf8Path;
use dawn_language::effect::EffectInst;
use dawn_language::identity::SourceIdentity;
use dawn_language::sequence::SequenceId;
use dawn_project_io::{ProjectSession, SourceObjectKind, is_project_owned_path};

use crate::dto::{
    DiagnosticSeverity, DocumentViewId, GuiDocument, GuiDocumentRequest, GuiEditCommand,
    GuiObjectRef, ObjectKind, ProjectDiagnostic, SequenceSelection, SequenceSelectionEdit,
};

#[derive(Debug)]
pub enum GuiMutationError {
    Blocked(String),
    Invalid(String),
}

impl GuiMutationError {
    pub fn message(&self) -> &str {
        match self {
            Self::Blocked(message) | Self::Invalid(message) => message,
        }
    }
}

pub fn blocked(reason: impl Into<String>, diagnostics: Vec<ProjectDiagnostic>) -> GuiDocument {
    GuiDocument::Blocked {
        reason: reason.into(),
        diagnostics,
    }
}

pub fn project_gui_document(
    session: Option<&ProjectSession>,
    request: &GuiDocumentRequest,
) -> GuiDocument {
    let Some(session) = session else {
        return blocked("No project is loaded.", Vec::new());
    };
    let resolved = match resolve_request(session, request) {
        Ok(resolved) => resolved,
        Err(message) => {
            return blocked(
                message.clone(),
                vec![gui_diagnostic(&request.path, "gui.resolve", &message)],
            );
        }
    };
    if !is_project_owned_path(resolved.identity.document()) {
        return blocked(
            "Imported dependency documents are read-only.",
            vec![gui_diagnostic(
                &request.path,
                "gui.read_only_dependency",
                "Imported dependency documents are read-only.",
            )],
        );
    }
    match request.view {
        DocumentViewId::Sequence => project_sequence(session, &resolved),
        DocumentViewId::Layout => project_layout(session, &resolved),
        DocumentViewId::Fixture => project_fixture(session, &resolved),
        DocumentViewId::Text => blocked(
            "Text documents do not have a GUI projection.",
            vec![gui_diagnostic(
                &request.path,
                "gui.view",
                "Text documents do not have a GUI projection.",
            )],
        ),
    }
}

pub fn affected_paths(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<BTreeSet<String>, GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    ensure_owned_gui_document(&resolved)?;
    Ok(BTreeSet::from([resolved.identity.document().to_string()]))
}

pub fn apply_edit(
    session: &mut ProjectSession,
    request: &GuiDocumentRequest,
    edit: GuiEditCommand,
) -> Result<(), GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    ensure_owned_gui_document(&resolved)?;
    match (request.view.clone(), edit) {
        (DocumentViewId::Sequence, GuiEditCommand::Sequence { edit }) => {
            edit_sequence(session, &resolved, edit)?;
            let sequence_id = SequenceId(SourceIdentity::new(
                resolved.identity.document().to_path_buf(),
                resolved.identity.object().to_string(),
            ));
            crate::sequence_integrity::validate_sequence_integrity(session, &sequence_id)?;
        }
        (DocumentViewId::Layout, GuiEditCommand::Layout { edit }) => {
            edit_layout(session, &resolved, edit)?;
        }
        (DocumentViewId::Fixture, GuiEditCommand::Fixture { edit }) => {
            edit_fixture(session, &resolved.identity, edit)?;
        }
        _ => {
            return Err(GuiMutationError::Invalid(
                "GUI edit type does not match the requested document view.".to_string(),
            ));
        }
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) enum SequenceClipboard {
    Effects(Vec<ClipboardEffect>),
    Marks(Vec<ClipboardMark>),
}

#[derive(Clone)]
pub(crate) struct ClipboardEffect {
    effect: EffectInst,
    start_seconds: f64,
    lane_index: usize,
}

#[derive(Clone)]
pub(crate) struct ClipboardMark {
    collection_key: String,
    time_seconds: f64,
}

pub(crate) struct SequenceSelectionMutation {
    pub selection: Option<SequenceSelection>,
    pub copied_count: u32,
    pub skipped_count: u32,
}

pub(crate) fn apply_sequence_selection_edit(
    session: &mut ProjectSession,
    request: &GuiDocumentRequest,
    edit: SequenceSelectionEdit,
    clipboard: &mut Option<SequenceClipboard>,
) -> Result<SequenceSelectionMutation, GuiMutationError> {
    let mut candidate_clipboard = clipboard.clone();
    let result =
        apply_sequence_selection_edit_inner(session, request, edit, &mut candidate_clipboard)?;
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    let sequence_id = SequenceId(SourceIdentity::new(
        resolved.identity.document().to_path_buf(),
        resolved.identity.object().to_string(),
    ));
    crate::sequence_integrity::validate_sequence_integrity(session, &sequence_id)?;
    *clipboard = candidate_clipboard;
    Ok(result)
}

fn apply_sequence_selection_edit_inner(
    session: &mut ProjectSession,
    request: &GuiDocumentRequest,
    edit: SequenceSelectionEdit,
    clipboard: &mut Option<SequenceClipboard>,
) -> Result<SequenceSelectionMutation, GuiMutationError> {
    if !matches!(request.view, DocumentViewId::Sequence) {
        return Err(GuiMutationError::Invalid(
            "Sequence selection edits require a sequence GUI document.".to_string(),
        ));
    }
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    ensure_owned_gui_document(&resolved)?;
    let sequence_id = SequenceId(SourceIdentity::new(
        resolved.identity.document().to_path_buf(),
        resolved.identity.object().to_string(),
    ));
    match edit {
        SequenceSelectionEdit::Copy { selection } => {
            let (next_clipboard, copied_count, skipped_count) =
                copy_sequence_selection(session, &sequence_id, &selection)?;
            *clipboard = next_clipboard;
            Ok(SequenceSelectionMutation {
                selection: Some(selection),
                copied_count,
                skipped_count,
            })
        }
        SequenceSelectionEdit::Cut { selection } => {
            let (next_clipboard, copied_count, skipped_count) =
                copy_sequence_selection(session, &sequence_id, &selection)?;
            *clipboard = next_clipboard;
            delete_sequence_selection(session, &sequence_id, &selection)?;
            Ok(SequenceSelectionMutation {
                selection: None,
                copied_count,
                skipped_count,
            })
        }
        SequenceSelectionEdit::Delete { selection } => {
            delete_sequence_selection(session, &sequence_id, &selection)?;
            Ok(SequenceSelectionMutation {
                selection: None,
                copied_count: 0,
                skipped_count: 0,
            })
        }
        SequenceSelectionEdit::Paste { anchor } => {
            paste_sequence_clipboard(session, &sequence_id, anchor, clipboard.as_ref())
        }
        SequenceSelectionEdit::MoveEffects {
            ids,
            time_delta_seconds,
            lane_delta,
        } => {
            let moved =
                move_effect_selection(session, &sequence_id, &ids, time_delta_seconds, lane_delta)?;
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Effects { ids: moved }),
                copied_count: 0,
                skipped_count: 0,
            })
        }
        SequenceSelectionEdit::ResizeEffects {
            ids,
            edge,
            time_delta_seconds,
        } => {
            resize_effect_selection(session, &sequence_id, &ids, edge, time_delta_seconds)?;
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Effects { ids }),
                copied_count: 0,
                skipped_count: 0,
            })
        }
        SequenceSelectionEdit::MoveMarks {
            marks,
            time_delta_seconds,
        } => {
            let moved = move_mark_selection(session, &sequence_id, &marks, time_delta_seconds)?;
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Marks { marks: moved }),
                copied_count: 0,
                skipped_count: 0,
            })
        }
    }
}

struct ResolvedGuiObject {
    identity: SourceIdentity,
    kind: SourceObjectKind,
}

impl ResolvedGuiObject {
    fn source_ref(&self) -> GuiObjectRef {
        GuiObjectRef {
            path: self.identity.document().to_string(),
            object_key: self.identity.object().to_string(),
            kind: ObjectKind::from(&self.kind),
            id: self.identity.object().to_string(),
        }
    }
}

fn ensure_owned_gui_document(resolved: &ResolvedGuiObject) -> Result<(), GuiMutationError> {
    if is_project_owned_path(resolved.identity.document()) {
        Ok(())
    } else {
        Err(GuiMutationError::Blocked(
            "Imported dependency documents are read-only.".to_string(),
        ))
    }
}

fn resolve_request(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<ResolvedGuiObject, String> {
    let path = Utf8Path::new(&request.path);
    let kind = source_kind_for_view(&request.view)?;
    let requested_key = request.object_key.as_deref();
    let document = session
        .source
        .documents
        .get(path)
        .ok_or_else(|| "No matching GUI document was found for this request.".to_string())?;
    let mut matches = document
        .objects()
        .iter()
        .filter(|object| object.kind() == &kind)
        .filter(|object| requested_key.is_none_or(|key| object.id() == key));
    let Some(source_id) = matches.next() else {
        return Err("No matching GUI object was found for this request.".to_string());
    };
    if matches.next().is_some() && requested_key.is_none() {
        return Err("GUI request must include an object key for this document.".to_string());
    }
    Ok(ResolvedGuiObject {
        identity: SourceIdentity::new(path.to_path_buf(), source_id.id().to_string()),
        kind: source_id.kind().clone(),
    })
}

fn source_kind_for_view(view: &DocumentViewId) -> Result<SourceObjectKind, String> {
    match view {
        DocumentViewId::Sequence => Ok(SourceObjectKind::Sequence),
        DocumentViewId::Layout => Ok(SourceObjectKind::Layout),
        DocumentViewId::Fixture => Ok(SourceObjectKind::FixtureDefinition),
        DocumentViewId::Text => Err("Text view has no source GUI object kind.".to_string()),
    }
}

mod edit;
mod model;
mod projection;
mod selection;

use edit::{edit_fixture, edit_layout, edit_sequence};
#[cfg(test)]
use model::identifier;
use projection::{project_fixture, project_layout, project_sequence};
use selection::{
    copy_sequence_selection, delete_sequence_selection, move_effect_selection, move_mark_selection,
    paste_sequence_clipboard, resize_effect_selection,
};

fn gui_diagnostic(path: &str, code: &str, message: &str) -> ProjectDiagnostic {
    ProjectDiagnostic {
        path: path.to_string(),
        code: code.to_string(),
        severity: DiagnosticSeverity::Error,
        message: message.to_string(),
        range: None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use camino::Utf8PathBuf;
    use dawn_language::dsl::compile_operators;
    use dawn_language::effect::{CurveDefinition, CurveId, CurveSource, EffectParamValue};
    use dawn_language::identity::SourceIdentity;
    use dawn_language::model::{DawnProject, ProjectDefinitionStores, ProjectId, ProjectRoot};
    use dawn_language::operator::{
        BuiltinOperator, GraphOperatorNode, OperatorDefinitionId, OperatorRef,
        custom_operator_definition,
    };
    use dawn_language::sequence::{
        CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
        GraphNodePosition, GraphPortId, Sequence, SequenceAudio, SequenceCompositionGraph,
        SequenceId, SequenceLayer, SequenceLayerId,
    };
    use dawn_language::setup::{LayoutId, PatchId, Setup, SetupId};
    use dawn_language::values::{Color, Curve, CurvePoint, DawnDuration};
    use dawn_project_io::{
        ProjectSession, SourceDocument, SourceDocumentKind, SourceObjectId, SourceObjectKind,
        SourceProject,
    };
    use indexmap::IndexMap;

    use super::apply_edit;
    use crate::dto::{DocumentViewId, GuiDocumentRequest, GuiEditCommand, SequenceGuiEdit};

    fn source_identity(object: &str) -> SourceIdentity {
        SourceIdentity::new("sequences.dawn".into(), object.to_string())
    }

    #[test]
    fn create_layer_adds_layer_to_output_edge() {
        let mut session = test_session(test_sequence_with_graph(true));

        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::CreateLayer {
                name: "Front".to_string(),
                color: "#123456".to_string(),
            },
        )
        .unwrap();

        let sequence = test_sequence(&session);
        assert!(
            sequence
                .layers
                .iter()
                .any(|layer| layer.id == SequenceLayerId(1) && layer.name == "Front")
        );
        assert!(sequence.composition_graph.edges.iter().any(|edge| {
            edge.from == CompositionGraphNodeId(3)
                && edge.from_port.0 == "output"
                && edge.to == CompositionGraphNodeId(2)
                && edge.to_port.0 == "input"
        }));
    }

    #[test]
    fn create_layer_errors_when_output_node_is_missing() {
        let mut session = test_session(test_sequence_with_graph(false));

        let error = apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::CreateLayer {
                name: "Front".to_string(),
                color: "#123456".to_string(),
            },
        )
        .unwrap_err();

        assert_eq!(error.message(), "Composition graph output was not found.");
        assert_eq!(test_sequence(&session).layers.len(), 1);
        assert!(test_sequence(&session).composition_graph.edges.is_empty());
    }

    #[test]
    fn disconnect_graph_nodes_removes_only_matching_edge() {
        let mut sequence = test_sequence_with_graph(true);
        sequence.composition_graph.nodes.push(operator_node(3));
        sequence.composition_graph.edges.push(graph_edge(
            CompositionGraphNodeId(1),
            "output",
            CompositionGraphNodeId(3),
            "input",
        ));
        sequence.composition_graph.edges.push(graph_edge(
            CompositionGraphNodeId(3),
            "output",
            CompositionGraphNodeId(2),
            "input",
        ));
        let mut session = test_session(sequence);

        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::DisconnectGraphNodes {
                from_node: "node:1".to_string(),
                from_port: "output".to_string(),
                to_node: "node:2".to_string(),
                to_port: "input".to_string(),
            },
        )
        .unwrap();

        let edges = &test_sequence(&session).composition_graph.edges;
        assert_eq!(edges.len(), 2);
        assert!(!edges.iter().any(|edge| {
            edge.from == CompositionGraphNodeId(1) && edge.to == CompositionGraphNodeId(2)
        }));
        assert!(edges.iter().any(|edge| {
            edge.from == CompositionGraphNodeId(1) && edge.to == CompositionGraphNodeId(3)
        }));
        assert!(edges.iter().any(|edge| {
            edge.from == CompositionGraphNodeId(3) && edge.to == CompositionGraphNodeId(2)
        }));
    }

    #[test]
    fn delete_graph_node_removes_operator_edges_but_rejects_layer_and_output_nodes() {
        let mut sequence = test_sequence_with_graph(true);
        sequence.composition_graph.nodes.push(operator_node(3));
        sequence.composition_graph.edges.push(graph_edge(
            CompositionGraphNodeId(1),
            "output",
            CompositionGraphNodeId(3),
            "input",
        ));
        sequence.composition_graph.edges.push(graph_edge(
            CompositionGraphNodeId(3),
            "output",
            CompositionGraphNodeId(2),
            "input",
        ));
        let mut session = test_session(sequence);

        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::DeleteGraphNode {
                node_id: "node:1".to_string(),
            },
        )
        .unwrap_err();
        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::DeleteGraphNode {
                node_id: "node:2".to_string(),
            },
        )
        .unwrap_err();
        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::DeleteGraphNode {
                node_id: "node:3".to_string(),
            },
        )
        .unwrap();

        let sequence = test_sequence(&session);
        assert!(
            sequence
                .composition_graph
                .nodes
                .iter()
                .all(|node| node.id != CompositionGraphNodeId(3))
        );
        assert!(sequence.composition_graph.nodes.iter().any(|node| {
            node.id == CompositionGraphNodeId(2)
                && matches!(node.kind, CompositionGraphNodeKind::Output)
        }));
        assert!(sequence.composition_graph.edges.iter().all(|edge| {
            edge.from != CompositionGraphNodeId(3) && edge.to != CompositionGraphNodeId(3)
        }));
    }

    #[test]
    fn custom_operator_adds_import_and_supports_typed_and_curve_params() {
        let mut session = test_session(test_sequence_with_graph(true));
        let compiled = compile_operators(
            "operator Gain { input Signal source; param float amount; param curve shape; color sample() { return source.at(seconds()) * amount; } }",
        )
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
        let operator_id = OperatorDefinitionId(SourceIdentity::new(
            "operators/gain.operator.dawn".into(),
            "Gain".to_string(),
        ));
        session.project.definitions.operators.insert(
            operator_id.clone(),
            custom_operator_definition(operator_id.clone(), compiled),
        );
        session.project.definitions.curves.insert(
            CurveId(SourceIdentity::new(
                "curves/shape.curve.dawn".into(),
                "shape".to_string(),
            )),
            CurveDefinition {
                curve: Curve {
                    points: vec![CurvePoint {
                        position: 0.0,
                        value: 1.0,
                    }],
                },
            },
        );
        session.source.documents.insert(
            Utf8PathBuf::from("sequences.dawn"),
            SourceDocument::new(
                Vec::new(),
                vec![SourceObjectId::new(SourceObjectKind::Sequence, "seq".to_string()).unwrap()],
                SourceDocumentKind::Dawn {
                    original_value: yaml_serde::Value::Mapping(yaml_serde::Mapping::new()),
                },
            )
            .unwrap(),
        );
        session.source.documents.insert(
            Utf8PathBuf::from("operators/gain.operator.dawn"),
            SourceDocument::new(
                Vec::new(),
                vec![
                    SourceObjectId::new(SourceObjectKind::OperatorDefinition, "Gain".to_string())
                        .unwrap(),
                ],
                SourceDocumentKind::Operator {
                    source: String::new(),
                },
            )
            .unwrap(),
        );
        session.source.documents.insert(
            Utf8PathBuf::from("curves/shape.curve.dawn"),
            SourceDocument::new(
                Vec::new(),
                vec![SourceObjectId::new(SourceObjectKind::Curve, "shape".to_string()).unwrap()],
                SourceDocumentKind::Dawn {
                    original_value: yaml_serde::Value::Mapping(yaml_serde::Mapping::new()),
                },
            )
            .unwrap(),
        );

        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::AddGraphOperatorNode {
                operator: crate::dto::SequenceGraphOperator::Custom {
                    path: "operators/gain.operator.dawn".to_string(),
                    object_key: "Gain".to_string(),
                },
                x: 100.0,
                y: 100.0,
            },
        )
        .unwrap();
        assert!(
            session.source.documents[&Utf8PathBuf::from("sequences.dawn")]
                .imports()
                .iter()
                .any(|import| import.alias() == "operators")
        );
        let node_id = test_sequence(&session)
            .composition_graph
            .nodes
            .iter()
            .map(|node| node.id.0)
            .max()
            .unwrap();
        {
            let sequence = session
                .project
                .sequences
                .get_mut(&SequenceId(source_identity("seq")))
                .unwrap();
            sequence.composition_graph.edges.push(graph_edge(
                CompositionGraphNodeId(1),
                "output",
                CompositionGraphNodeId(node_id),
                "source",
            ));
            sequence.composition_graph.edges.push(graph_edge(
                CompositionGraphNodeId(node_id),
                "output",
                CompositionGraphNodeId(2),
                "input",
            ));
        }
        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::UpdateGraphOperatorParam {
                node_id: format!("node:{node_id}"),
                name: "amount".to_string(),
                value: crate::dto::SequenceEffectParamValue::Float { value: 0.75 },
            },
        )
        .unwrap();
        apply_sequence_edit(
            &mut session,
            SequenceGuiEdit::LinkGraphOperatorCurve {
                node_id: format!("node:{node_id}"),
                name: "shape".to_string(),
                source_path: "curves/shape.curve.dawn".to_string(),
                object_key: "shape".to_string(),
            },
        )
        .unwrap();
        let node = test_sequence(&session)
            .composition_graph
            .nodes
            .iter()
            .find(|node| node.id.0 == node_id)
            .unwrap();
        let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
            panic!("expected operator");
        };
        assert_eq!(
            operator.params[&super::identifier("amount").unwrap()],
            EffectParamValue::Float(0.75)
        );
        assert!(matches!(
            operator.params[&super::identifier("shape").unwrap()],
            EffectParamValue::Curve(CurveSource::Reference(_))
        ));
    }

    fn apply_sequence_edit(
        session: &mut ProjectSession,
        edit: SequenceGuiEdit,
    ) -> Result<(), super::GuiMutationError> {
        apply_edit(
            session,
            &GuiDocumentRequest {
                path: "sequences.dawn".to_string(),
                view: DocumentViewId::Sequence,
                object_key: Some("seq".to_string()),
            },
            GuiEditCommand::Sequence { edit },
        )
    }

    fn test_session(sequence: Sequence) -> ProjectSession {
        ProjectSession {
            project: DawnProject {
                root: ProjectRoot {
                    id: ProjectId(source_identity("project")),
                    setup: SetupId(source_identity("setup")),
                    sequences: vec![SequenceId(source_identity("seq"))],
                },
                setups: IndexMap::from([(
                    SetupId(source_identity("setup")),
                    Setup {
                        id: SetupId(source_identity("setup")),
                        layout: LayoutId(source_identity("layout")),
                        patch: PatchId(source_identity("patch")),
                        controllers: Vec::new(),
                    },
                )]),
                layouts: IndexMap::new(),
                patches: IndexMap::new(),
                controllers: IndexMap::new(),
                sequences: IndexMap::from([(SequenceId(source_identity("seq")), sequence)]),
                definitions: ProjectDefinitionStores::default(),
            },
            source: SourceProject {
                source_root: Utf8PathBuf::from("."),
                entrypoint: Utf8PathBuf::from("project.dawn"),
                documents: IndexMap::from([(
                    Utf8PathBuf::from("sequences.dawn"),
                    SourceDocument::new(
                        Vec::new(),
                        vec![
                            SourceObjectId::new(SourceObjectKind::Sequence, "seq".to_string())
                                .unwrap(),
                        ],
                        SourceDocumentKind::Dawn {
                            original_value: yaml_serde::Value::Mapping(yaml_serde::Mapping::new()),
                        },
                    )
                    .unwrap(),
                )]),
                referenced_assets: Vec::new(),
            },
        }
    }

    fn test_sequence(session: &ProjectSession) -> &Sequence {
        session
            .project
            .sequences
            .get(&SequenceId(source_identity("seq")))
            .unwrap()
    }

    fn test_sequence_with_graph(include_output: bool) -> Sequence {
        Sequence {
            id: SequenceId(source_identity("seq")),
            duration: DawnDuration(Duration::from_secs(1)),
            frame_rate: 30,
            audio: SequenceAudio::None,
            mark_collections: Vec::new(),
            layers: vec![SequenceLayer {
                id: SequenceLayerId(0),
                name: "Default".to_string(),
                color: Color {
                    red: 80,
                    green: 160,
                    blue: 255,
                },
                enabled: true,
            }],
            effects: Vec::new(),
            composition_graph: SequenceCompositionGraph {
                nodes: if include_output {
                    vec![
                        layer_node(1, 0),
                        CompositionGraphNode {
                            id: CompositionGraphNodeId(2),
                            position: GraphNodePosition { x: 240.0, y: 0.0 },
                            kind: CompositionGraphNodeKind::Output,
                        },
                    ]
                } else {
                    vec![layer_node(1, 0)]
                },
                edges: if include_output {
                    vec![graph_edge(
                        CompositionGraphNodeId(1),
                        "output",
                        CompositionGraphNodeId(2),
                        "input",
                    )]
                } else {
                    Vec::new()
                },
            },
            automation_clips: Vec::new(),
        }
    }

    fn operator_node(id: u32) -> CompositionGraphNode {
        CompositionGraphNode {
            id: CompositionGraphNodeId(id),
            position: GraphNodePosition { x: 120.0, y: 0.0 },
            kind: CompositionGraphNodeKind::Operator(GraphOperatorNode {
                operator: OperatorRef::Builtin(BuiltinOperator::Dim),
                params: IndexMap::new(),
            }),
        }
    }

    fn layer_node(id: u32, layer_id: u32) -> CompositionGraphNode {
        CompositionGraphNode {
            id: CompositionGraphNodeId(id),
            position: GraphNodePosition { x: 0.0, y: 0.0 },
            kind: CompositionGraphNodeKind::Layer {
                layer_id: SequenceLayerId(layer_id),
            },
        }
    }

    fn graph_edge(
        from: CompositionGraphNodeId,
        from_port: &str,
        to: CompositionGraphNodeId,
        to_port: &str,
    ) -> EffectGraphEdge {
        EffectGraphEdge {
            from,
            from_port: GraphPortId(from_port.to_string()),
            to,
            to_port: GraphPortId(to_port.to_string()),
        }
    }
}
