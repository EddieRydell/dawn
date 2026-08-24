use std::collections::BTreeSet;

use camino::Utf8Path;
use dawn_language::effect::EffectInst;
use dawn_language::identity::SourceIdentity;
use dawn_language::sequence::SequenceId;
use dawn_project_io::{ProjectSession, SourceObjectKind};

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
    if !session
        .source
        .is_project_owned(resolved.identity.document_id())
    {
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
        DocumentViewId::Setup => project_setup(session, &resolved),
        DocumentViewId::Preview => project_layout(session, &resolved),
        DocumentViewId::Prop => project_fixture(session, &resolved),
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

pub fn project_recovery_gui_document(
    recovery: &dawn_project_io::ProjectRecovery,
    request: &GuiDocumentRequest,
    diagnostics: Vec<ProjectDiagnostic>,
) -> GuiDocument {
    if !matches!(request.view, DocumentViewId::Sequence) {
        return blocked(
            "This GUI requires a complete project model. Use the text editor and Problems while the project is recovering.",
            diagnostics,
        );
    }
    let path = Utf8Path::new(&request.path);
    let Some(document) = recovery.documents.get(path) else {
        return blocked(
            "The recovery analysis did not find this document.",
            diagnostics,
        );
    };
    let sequence = document.objects.iter().find(|object| {
        object.kind == SourceObjectKind::Sequence
            && request
                .object_key
                .as_deref()
                .is_none_or(|key| key == object.key)
    });
    let Some(object) = sequence else {
        return blocked(
            "This sequence does not have a trustworthy identity in the recovery analysis.",
            diagnostics,
        );
    };
    let Some(sequence) = &object.sequence else {
        return blocked(
            "Sequence identity, duration, frame rate, or canvas coordinates are invalid. Fix the first sequence diagnostic in text.",
            diagnostics,
        );
    };
    let Some(module_id) = recovery
        .manifest
        .as_ref()
        .map(|manifest| manifest.module_id)
    else {
        return blocked(
            "The manifest module identity is required for a sequence recovery view.",
            diagnostics,
        );
    };
    GuiDocument::Sequence {
        document: crate::dto::SequenceGuiDocument {
            path: request.path.clone(),
            source_ref: GuiObjectRef {
                module_id: module_id.to_string(),
                path: request.path.clone(),
                object_key: object.key.clone(),
                kind: ObjectKind::Sequence,
                id: object.key.clone(),
            },
            object_key: object.key.clone(),
            duration_seconds: sequence.duration_seconds,
            frame_rate: sequence.frame_rate,
            audio: None,
            mark_collections: sequence
                .mark_collections
                .iter()
                .map(|collection| crate::dto::SequenceMarkCollection {
                    key: collection.key.clone(),
                    name: collection.name.clone(),
                    color: collection.color.clone(),
                    marks_seconds: collection.marks_seconds.clone(),
                })
                .collect(),
            lanes: Vec::new(),
            effect_definitions: Vec::new(),
            curve_library: Vec::new(),
            gradient_library: Vec::new(),
            layers: sequence
                .layers
                .iter()
                .map(|layer| crate::dto::SequenceLayer {
                    id: layer.id,
                    name: layer.name.clone(),
                    color: layer.color.clone(),
                    enabled: layer.enabled,
                    is_default: layer.id == 0,
                })
                .collect(),
            effects: Vec::new(),
            control_clips: Vec::new(),
            composition_graph: crate::dto::SequenceCompositionGraph {
                id: 0,
                operator_catalog: Vec::new(),
                nodes: Vec::new(),
                edges: Vec::new(),
            },
            automation_clips: Vec::new(),
            mode: crate::dto::GuiDocumentMode::Recovery,
            recovery_items: sequence
                .items
                .iter()
                .map(|item| crate::dto::InvalidSequencePlaceholder {
                    kind: match item.kind {
                        dawn_project_io::RecoverySequenceItemKind::Effect => {
                            crate::dto::InvalidSequencePlaceholderKind::Effect
                        }
                        dawn_project_io::RecoverySequenceItemKind::AutomationClip => {
                            crate::dto::InvalidSequencePlaceholderKind::AutomationClip
                        }
                        dawn_project_io::RecoverySequenceItemKind::ControlClip => {
                            crate::dto::InvalidSequencePlaceholderKind::ControlClip
                        }
                        dawn_project_io::RecoverySequenceItemKind::GraphNode => {
                            crate::dto::InvalidSequencePlaceholderKind::GraphNode
                        }
                    },
                    id: item.id.clone(),
                    placement: match &item.placement {
                        dawn_project_io::RecoverySequencePlacement::Timeline {
                            start_seconds,
                            duration_seconds,
                            lane,
                        } => crate::dto::InvalidSequencePlacement::Timeline {
                            start_seconds: *start_seconds,
                            duration_seconds: *duration_seconds,
                            lane: match lane {
                                dawn_project_io::RecoveryTimelineLane::Layer(layer_id) => {
                                    crate::dto::InvalidSequenceLane::Layer {
                                        layer_id: *layer_id,
                                    }
                                }
                                dawn_project_io::RecoveryTimelineLane::Lane(lane_index) => {
                                    crate::dto::InvalidSequenceLane::Lane {
                                        lane_index: *lane_index,
                                    }
                                }
                            },
                        },
                        dawn_project_io::RecoverySequencePlacement::Graph { x, y } => {
                            crate::dto::InvalidSequencePlacement::Graph { x: *x, y: *y }
                        }
                    },
                    message: item.message.clone().or_else(|| {
                        Some("Read-only while the project model has errors.".to_string())
                    }),
                })
                .collect(),
        },
    }
}

pub fn affected_paths(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<BTreeSet<String>, GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    ensure_owned_gui_document(session, &resolved)?;
    if matches!(request.view, DocumentViewId::Setup) {
        return Ok(session
            .source
            .documents
            .keys()
            .filter(|document| session.source.is_project_owned(document))
            .map(|document| document.path().to_string())
            .collect());
    }
    Ok(BTreeSet::from([resolved.identity.document().to_string()]))
}

pub fn apply_edit(
    session: &mut ProjectSession,
    request: &GuiDocumentRequest,
    edit: GuiEditCommand,
) -> Result<(), GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    ensure_owned_gui_document(session, &resolved)?;
    match (request.view.clone(), edit) {
        (DocumentViewId::Sequence, GuiEditCommand::Sequence { edit }) => {
            edit_sequence(session, &resolved, edit)?;
            let sequence_id = SequenceId(resolved.identity.clone());
            crate::sequence_integrity::validate_sequence_integrity(session, &sequence_id)?;
            dawn_language::validation::validate_project(&session.project)
                .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))?;
        }
        (DocumentViewId::Setup, GuiEditCommand::Setup { edit }) => {
            edit_setup(session, edit)?;
        }
        (DocumentViewId::Preview, GuiEditCommand::Preview { edit }) => {
            edit_layout(session, &resolved, edit)?;
        }
        (DocumentViewId::Prop, GuiEditCommand::Prop { edit }) => {
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
    let sequence_id = SequenceId(resolved.identity.clone());
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
    ensure_owned_gui_document(session, &resolved)?;
    let sequence_id = SequenceId(resolved.identity.clone());
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
            module_id: self.identity.module_id().to_string(),
            path: self.identity.document().to_string(),
            object_key: self.identity.object().to_string(),
            kind: ObjectKind::from(&self.kind),
            id: self.identity.object().to_string(),
        }
    }
}

fn ensure_owned_gui_document(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> Result<(), GuiMutationError> {
    if session
        .source
        .is_project_owned(resolved.identity.document_id())
    {
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
    let mut documents = session
        .source
        .documents
        .iter()
        .filter(|(document_id, _)| document_id.path() == path);
    let Some((document_id, document)) = documents.next() else {
        return Err("No matching GUI document was found for this request.".to_string());
    };
    if documents.next().is_some() {
        return Err("The document path is ambiguous across package modules.".to_string());
    }
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
        identity: SourceIdentity::from_document(document_id.clone(), source_id.id().to_string()),
        kind: source_id.kind().clone(),
    })
}

fn source_kind_for_view(view: &DocumentViewId) -> Result<SourceObjectKind, String> {
    match view {
        DocumentViewId::Sequence => Ok(SourceObjectKind::Sequence),
        DocumentViewId::Setup => Ok(SourceObjectKind::Setup),
        DocumentViewId::Preview => Ok(SourceObjectKind::PreviewLayout),
        DocumentViewId::Prop => Ok(SourceObjectKind::PropDefinition),
        DocumentViewId::Text => Err("Text view has no source GUI object kind.".to_string()),
    }
}

mod edit;
pub(crate) mod model;
mod projection;
mod selection;
mod setup;

use edit::{edit_fixture, edit_layout, edit_sequence};
use projection::{project_fixture, project_layout, project_sequence};
use selection::{
    copy_sequence_selection, delete_sequence_selection, move_effect_selection, move_mark_selection,
    paste_sequence_clipboard, resize_effect_selection,
};
use setup::{edit_setup, project_setup};

fn gui_diagnostic(path: &str, code: &str, message: &str) -> ProjectDiagnostic {
    ProjectDiagnostic {
        path: path.to_string(),
        code: code.to_string(),
        severity: DiagnosticSeverity::Error,
        message: message.to_string(),
        range: None,
        detail: None,
        related: Vec::new(),
    }
}
