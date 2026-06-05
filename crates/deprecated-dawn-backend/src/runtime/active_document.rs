use dawn_language::document::{DocumentDescriptor, DocumentViewId, SequenceDocument};
use dawn_language::model::{Authored, DawnObject};
use dawn_language::parse::parse_dawn_file_with_source_map;

use crate::editor::EditorViewMode;
use crate::preview::session::SequenceKey;
use crate::workspace::ActiveGuiDocument;

use super::AppBackend;

impl AppBackend {
    pub(super) fn active_gui_document(&self) -> Option<ActiveGuiDocument> {
        let descriptor = self.active_document_descriptor();
        let active_buffer = self.document_store.active_tab();
        self.workspace.active_gui_document(
            active_buffer.as_ref(),
            descriptor.as_ref(),
            self.document_store.dirty_overlays(),
        )
    }

    pub(super) fn active_document_descriptor(&self) -> Option<DocumentDescriptor> {
        let path = self.document_store.active_file()?.clone();
        self.workspace
            .inspect_document(path, self.document_store.dirty_overlays())
            .ok()
    }

    pub(super) fn active_sequence_authored(
        &self,
    ) -> Result<dawn_language::model::Sequence<Authored>, String> {
        let object_key = self.active_sequence_object_key()?;
        let parsed = parse_dawn_file_with_source_map(&self.document_store.active_text()?)
            .map_err(|error| error.to_string())?;
        match parsed.file.get(&object_key) {
            Some(DawnObject::Sequence(sequence)) => Ok(sequence.clone()),
            _ => Err(format!("sequence object `{object_key}` was not found")),
        }
    }

    pub(super) fn active_sequence_document(&self) -> Result<SequenceDocument, String> {
        let path = self.document_store.active_path()?;
        let object_key = self.active_sequence_object_key()?;
        let Some(buffer) = self.document_store.active_buffer() else {
            return Err("no active document".to_string());
        };
        if buffer.view_mode != EditorViewMode::Gui || buffer.is_conflicted() {
            return Err("active sequence GUI document is not available".to_string());
        }
        self.workspace
            .sequence_document(path, &object_key, self.document_store.dirty_overlays())
    }

    pub(super) fn active_sequence_object_key(&self) -> Result<String, String> {
        let path = self.document_store.active_path()?;
        let descriptor = self
            .workspace
            .inspect_document(path, self.document_store.dirty_overlays())?;
        descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .cloned()
            .ok_or_else(|| "active document is not a sequence".to_string())
    }

    pub(super) fn active_sequence_source(&self) -> Option<(SequenceKey, SequenceDocument)> {
        let path = self.document_store.active_file()?.clone();
        let overlays = self.document_store.dirty_overlays();
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), overlays.clone())
            .ok()?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)?;
        let document = self
            .workspace
            .sequence_document(path.clone(), object_key, overlays)
            .ok()?;
        Some((
            SequenceKey {
                path,
                object_key: document.object_key.clone(),
            },
            document,
        ))
    }
}
