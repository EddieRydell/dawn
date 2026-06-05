use camino::Utf8PathBuf;
use dawn_language::{
    analysis::{
        DiagnosticCode, DiagnosticSeverity, ProjectAnalysis, ProjectDiagnostic, ProjectOverlay,
    },
    document::{
        self, DocumentDescriptor, DocumentEditOutcome, DocumentViewId, FixtureDocument,
        LayoutDocument,
    },
    fs::WorkspaceFs,
};

use crate::{
    types::{ActiveGuiDocument, ActiveGuiDocumentBlocked},
    BackendError, BackendErrorKind, BackendResult,
};

pub(crate) fn inspect_active_document(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    overlays: Vec<ProjectOverlay>,
) -> BackendResult<DocumentDescriptor> {
    document::inspect_document(fs, path, overlays).map_err(invalid_document_error)
}

pub(crate) fn build_active_gui_document_from_analysis(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    descriptor: &DocumentDescriptor,
    overlays: Vec<ProjectOverlay>,
    analysis: &ProjectAnalysis,
) -> ActiveGuiDocument {
    let Some(view_id) = preferred_gui_view(descriptor) else {
        return ActiveGuiDocument::Blocked(blocked(
            &path,
            "No GUI view is available for this document".to_string(),
        ));
    };
    let Some(object_key) = descriptor.default_object_keys.get(&view_id).cloned() else {
        return ActiveGuiDocument::Blocked(blocked(
            &path,
            "No default object is available for this GUI view".to_string(),
        ));
    };

    match view_id {
        DocumentViewId::Sequence => document::get_sequence_document_with_analysis(
            fs,
            path.clone(),
            &object_key,
            overlays,
            analysis,
        )
        .map(ActiveGuiDocument::Sequence)
        .unwrap_or_else(|reason| ActiveGuiDocument::Blocked(blocked(&path, reason))),
        DocumentViewId::Layout => document::get_layout_document_with_analysis(
            fs,
            path.clone(),
            &object_key,
            overlays,
            analysis,
        )
        .map(ActiveGuiDocument::Layout)
        .unwrap_or_else(|reason| ActiveGuiDocument::Blocked(blocked(&path, reason))),
        DocumentViewId::Fixture => {
            document::get_fixture_document(fs, path.clone(), Some(&object_key), overlays)
                .map(ActiveGuiDocument::Fixture)
                .unwrap_or_else(|reason| ActiveGuiDocument::Blocked(blocked(&path, reason)))
        }
        DocumentViewId::Text => ActiveGuiDocument::Blocked(blocked(
            &path,
            "Text-only documents do not have a GUI view".to_string(),
        )),
    }
}

pub(crate) fn apply_sequence_document_text_edit(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    object_key: &str,
    edit: document::SequenceDocumentEdit,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
    analysis: &dawn_language::analysis::ProjectAnalysis,
) -> BackendResult<String> {
    document::apply_sequence_document_text_edit(
        fs,
        path,
        object_key,
        edit,
        base_content,
        overlays,
        analysis,
    )
    .map_err(invalid_document_error)
}

pub(crate) fn apply_layout_document_edit(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    object_key: &str,
    document: LayoutDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
) -> BackendResult<DocumentEditOutcome<LayoutDocument>> {
    document::apply_layout_document_edit(fs, path, object_key, document, base_content, overlays)
        .map_err(invalid_document_error)
}

pub(crate) fn apply_fixture_document_edit(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    document: FixtureDocument,
    base_content: String,
    overlays: Vec<ProjectOverlay>,
) -> BackendResult<DocumentEditOutcome<FixtureDocument>> {
    document::apply_fixture_document_edit(fs, path, document, base_content, overlays)
        .map_err(invalid_document_error)
}

fn preferred_gui_view(descriptor: &DocumentDescriptor) -> Option<DocumentViewId> {
    [
        DocumentViewId::Sequence,
        DocumentViewId::Layout,
        DocumentViewId::Fixture,
    ]
    .into_iter()
    .find(|view| descriptor.available_views.contains(view))
}

fn blocked(path: &Utf8PathBuf, reason: String) -> ActiveGuiDocumentBlocked {
    ActiveGuiDocumentBlocked {
        reason: reason.clone(),
        diagnostics: vec![ProjectDiagnostic {
            path: path.clone(),
            range: None,
            severity: DiagnosticSeverity::Error,
            code: DiagnosticCode::Yaml,
            message: reason,
        }],
    }
}

fn invalid_document_error(message: String) -> BackendError {
    BackendError::new(BackendErrorKind::InvalidInput, message)
}
