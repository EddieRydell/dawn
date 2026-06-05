use camino::Utf8PathBuf;
use dawn_language::{
    analysis::{
        DiagnosticCode, DiagnosticSeverity, ProjectAnalysis, ProjectDiagnostic, ProjectOverlay,
    },
    document::{self, DocumentDescriptor, DocumentViewId, SequenceDocument},
    fs::WorkspaceFs,
};

use crate::{
    types::{ActiveGuiDocument, ActiveGuiDocumentBlocked, ActiveGuiDocumentCacheKey},
    BackendError, BackendErrorKind, BackendResult,
};

pub(crate) fn inspect_active_document(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    overlays: Vec<ProjectOverlay>,
) -> BackendResult<DocumentDescriptor> {
    document::inspect_document(fs, path, overlays).map_err(invalid_document_error)
}

pub(crate) fn active_gui_document_cache_key(
    project_root: &Utf8PathBuf,
    path: &Utf8PathBuf,
    descriptor: &DocumentDescriptor,
) -> Result<ActiveGuiDocumentCacheKey, ActiveGuiDocumentBlocked> {
    let Some(view_id) = preferred_gui_view(descriptor) else {
        return Err(blocked_gui_document(
            path,
            "No GUI view is available for this document".to_string(),
        ));
    };
    let Some(object_key) = descriptor.default_object_keys.get(&view_id).cloned() else {
        return Err(blocked_gui_document(
            path,
            "No default object is available for this GUI view".to_string(),
        ));
    };
    Ok(ActiveGuiDocumentCacheKey {
        project_root: project_root.clone(),
        path: path.clone(),
        view_id,
        object_key,
    })
}

pub(crate) fn build_active_gui_document_from_analysis(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    descriptor: &DocumentDescriptor,
    overlays: Vec<ProjectOverlay>,
    analysis: &ProjectAnalysis,
) -> ActiveGuiDocument {
    let Some(view_id) = preferred_gui_view(descriptor) else {
        return ActiveGuiDocument::Blocked(blocked_gui_document(
            &path,
            "No GUI view is available for this document".to_string(),
        ));
    };
    let Some(object_key) = descriptor.default_object_keys.get(&view_id).cloned() else {
        return ActiveGuiDocument::Blocked(blocked_gui_document(
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
        .unwrap_or_else(|reason| ActiveGuiDocument::Blocked(blocked_gui_document(&path, reason))),
        DocumentViewId::Layout => document::get_layout_document_with_analysis(
            fs,
            path.clone(),
            &object_key,
            overlays,
            analysis,
        )
        .map(ActiveGuiDocument::Layout)
        .unwrap_or_else(|reason| ActiveGuiDocument::Blocked(blocked_gui_document(&path, reason))),
        DocumentViewId::Fixture => {
            document::get_fixture_document(fs, path.clone(), Some(&object_key), overlays)
                .map(ActiveGuiDocument::Fixture)
                .unwrap_or_else(|reason| {
                    ActiveGuiDocument::Blocked(blocked_gui_document(&path, reason))
                })
        }
        DocumentViewId::Text => ActiveGuiDocument::Blocked(blocked_gui_document(
            &path,
            "Text-only documents do not have a GUI view".to_string(),
        )),
    }
}

pub(crate) fn blocked_gui_document(path: &Utf8PathBuf, reason: String) -> ActiveGuiDocumentBlocked {
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

pub(crate) fn validate_sequence_preview_request(
    active_path: &Utf8PathBuf,
    active_object_key: &str,
    active_document: &SequenceDocument,
    path: &Utf8PathBuf,
    object_key: &str,
) -> BackendResult<()> {
    if active_object_key != object_key {
        return Err(invalid_input_error(format!(
            "active sequence object '{active_object_key}' does not match requested object '{object_key}'"
        )));
    }
    if active_path != path && active_document.path != path.as_str() {
        return Err(invalid_input_error(format!(
            "active sequence path '{active_path}' does not match requested path '{path}'"
        )));
    }
    Ok(())
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

fn invalid_document_error(message: String) -> BackendError {
    BackendError::new(BackendErrorKind::InvalidInput, message)
}

fn invalid_input_error(message: impl Into<String>) -> BackendError {
    BackendError::new(BackendErrorKind::InvalidInput, message)
}
