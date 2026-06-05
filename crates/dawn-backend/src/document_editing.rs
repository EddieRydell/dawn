use camino::Utf8PathBuf;
use dawn_language::{
    analysis::ProjectOverlay,
    document::{self, DocumentEditOutcome, FixtureDocument, LayoutDocument, SequenceDocumentEdit},
    fs::WorkspaceFs,
};

use crate::{BackendError, BackendErrorKind, BackendResult};

pub(crate) fn apply_sequence_document_text_edit(
    fs: &WorkspaceFs,
    path: Utf8PathBuf,
    object_key: &str,
    edit: SequenceDocumentEdit,
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

fn invalid_document_error(message: String) -> BackendError {
    BackendError::new(BackendErrorKind::InvalidInput, message)
}
