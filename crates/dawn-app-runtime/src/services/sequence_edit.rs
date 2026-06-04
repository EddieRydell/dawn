use dawn_language::analysis::ProjectAnalysis;
use dawn_language::document::{
    apply_sequence_document_edit, DocumentEditOutcome, SequenceDocument, SequenceDocumentEdit,
};
use dawn_language::fs::WorkspaceFs;
use dawn_language::path::Utf8PathBuf;

use crate::contracts::{Revision, RuntimeError, RuntimeResult, ServiceName};

#[derive(Debug, Clone)]
pub struct SequenceEditCommand {
    pub path: Utf8PathBuf,
    pub object_key: String,
    pub expected_revision: Revision,
    pub current_revision: Revision,
    pub edit: SequenceDocumentEdit,
    pub base_content: String,
    pub overlays: Vec<dawn_language::analysis::ProjectOverlay>,
    pub fs: WorkspaceFs,
    pub analysis: ProjectAnalysis,
}

#[derive(Debug, Default, Clone)]
pub struct SequenceEditCore;

impl SequenceEditCore {
    pub fn handle(
        &mut self,
        command: SequenceEditCommand,
    ) -> RuntimeResult<DocumentEditOutcome<SequenceDocument>> {
        if command.expected_revision != command.current_revision {
            return Err(RuntimeError::stale(
                ServiceName::SequenceEdit,
                command.expected_revision,
                command.current_revision,
            ));
        }
        apply_sequence_document_edit(
            &command.fs,
            command.path,
            &command.object_key,
            command.edit,
            command.base_content,
            command.overlays,
            &command.analysis,
        )
        .map_err(|message| {
            RuntimeError::new(
                ServiceName::SequenceEdit,
                crate::contracts::RuntimeErrorKind::InvalidCommand,
                message,
            )
        })
    }
}
