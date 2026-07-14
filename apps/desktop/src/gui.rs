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

pub fn affected_paths(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<BTreeSet<String>, GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    ensure_owned_gui_document(&resolved)?;
    if matches!(request.view, DocumentViewId::Setup) {
        return Ok(session
            .source
            .documents
            .keys()
            .filter(|path| is_project_owned_path(path))
            .map(ToString::to_string)
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
    ensure_owned_gui_document(&resolved)?;
    match (request.view.clone(), edit) {
        (DocumentViewId::Sequence, GuiEditCommand::Sequence { edit }) => {
            edit_sequence(session, &resolved, edit)?;
            let sequence_id = SequenceId(SourceIdentity::new(
                resolved.identity.document().to_path_buf(),
                resolved.identity.object().to_string(),
            ));
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
        DocumentViewId::Setup => Ok(SourceObjectKind::Setup),
        DocumentViewId::Preview => Ok(SourceObjectKind::PreviewLayout),
        DocumentViewId::Prop => Ok(SourceObjectKind::PropDefinition),
        DocumentViewId::Text => Err("Text view has no source GUI object kind.".to_string()),
    }
}

mod edit;
mod model;
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
    }
}
