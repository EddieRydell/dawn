use dawn_language::document::{DocumentViewId, SequenceDocument, SequenceDocumentEdit};
use dawn_language::path::Utf8PathBuf;

use crate::gui_edits::selection::plan_sequence_selection_edit;
use crate::gui_edits::types::{
    FixtureGuiEdit, LayoutGuiEdit, SequenceGuiEdit, SequenceSelectionEdit,
    SequenceSelectionEditResult,
};
use crate::preview::session::PreviewSyncMode;
use crate::runtime::contracts::{RuntimeNotice, RuntimeStatus};

use super::AppBackend;

impl AppBackend {
    pub(super) fn apply_sequence_gui_edit_and_autosave(
        &mut self,
        edit: SequenceGuiEdit,
    ) -> Result<(), String> {
        self.apply_sequence_gui_edit_internal(edit)?;
        self.flush_autosave_without_analysis()?;
        self.status = RuntimeStatus::notice(RuntimeNotice::Autosaved);
        Ok(())
    }

    pub(super) fn apply_layout_gui_edit_and_autosave(
        &mut self,
        edit: LayoutGuiEdit,
    ) -> Result<(), String> {
        self.apply_layout_gui_edit_internal(edit)?;
        self.flush_autosave_internal()?;
        self.status = RuntimeStatus::notice(RuntimeNotice::Autosaved);
        Ok(())
    }

    pub(super) fn apply_fixture_gui_edit_and_autosave(
        &mut self,
        edit: FixtureGuiEdit,
    ) -> Result<(), String> {
        self.apply_fixture_gui_edit_internal(edit)?;
        self.flush_autosave_internal()?;
        self.status = RuntimeStatus::notice(RuntimeNotice::Autosaved);
        Ok(())
    }

    pub(super) fn apply_sequence_selection_edit_command(
        &mut self,
        edit: SequenceSelectionEdit,
    ) -> Result<SequenceSelectionEditResult, String> {
        let before = self.active_sequence_authored()?;
        let before_document = self.active_sequence_document()?;
        let outcome = plan_sequence_selection_edit(
            edit,
            &mut self.sequence_clipboard,
            &before,
            &before_document,
        )?;

        if let Some(document_edit) = outcome.document_edit {
            self.apply_sequence_document_edit(document_edit)?;
            self.flush_autosave_without_analysis()?;
        }
        self.status = RuntimeStatus::notice(RuntimeNotice::Selection {
            message: outcome.status_message,
        });
        Ok(outcome.result)
    }

    fn apply_sequence_gui_edit_internal(&mut self, edit: SequenceGuiEdit) -> Result<(), String> {
        self.document_store.ensure_active_buffer_not_conflicted()?;
        let path = self.document_store.active_path()?;
        let descriptor_overlays = self.document_store.dirty_overlays();
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), descriptor_overlays)?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .ok_or_else(|| "active document is not a sequence".to_string())?
            .clone();
        let edit = crate::gui_edits::sequence::sequence_document_edit_from_gui(edit);
        let base_content = self.document_store.active_text()?;
        let edit_overlays = self.document_store.dirty_overlays();
        let outcome = self.workspace.apply_sequence_edit(
            path.clone(),
            &object_key,
            edit,
            base_content,
            edit_overlays,
        )?;
        self.save_active_sequence_gui_text(
            path,
            outcome.serialized_content,
            outcome.refreshed_document,
            PreviewSyncMode::DeferRender,
        )?;
        Ok(())
    }

    fn apply_sequence_document_edit(&mut self, edit: SequenceDocumentEdit) -> Result<(), String> {
        self.document_store.ensure_active_buffer_not_conflicted()?;
        let path = self.document_store.active_path()?;
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), self.document_store.dirty_overlays())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .ok_or_else(|| "active document is not a sequence".to_string())?
            .clone();
        let outcome = self.workspace.apply_sequence_edit(
            path.clone(),
            &object_key,
            edit,
            self.document_store.active_text()?,
            self.document_store.dirty_overlays(),
        )?;
        self.save_active_sequence_gui_text(
            path,
            outcome.serialized_content,
            outcome.refreshed_document,
            PreviewSyncMode::DeferRender,
        )
    }

    fn apply_layout_gui_edit_internal(&mut self, edit: LayoutGuiEdit) -> Result<(), String> {
        self.document_store.ensure_active_buffer_not_conflicted()?;
        let path = self.document_store.active_path()?;
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), self.document_store.dirty_overlays())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Layout)
            .ok_or_else(|| "active document is not a layout".to_string())?
            .clone();
        let mut document = self.workspace.layout_document(
            path.clone(),
            &object_key,
            self.document_store.dirty_overlays(),
        )?;
        crate::gui_edits::layout::apply_layout_gui_edit(&mut document, edit)?;
        let outcome = self.workspace.apply_layout_edit(
            path,
            &object_key,
            document,
            self.document_store.active_text()?,
            self.document_store.dirty_overlays(),
        )?;
        self.save_active_gui_text(outcome.serialized_content)
    }

    fn apply_fixture_gui_edit_internal(&mut self, edit: FixtureGuiEdit) -> Result<(), String> {
        self.document_store.ensure_active_buffer_not_conflicted()?;
        let path = self.document_store.active_path()?;
        let mut document = self.workspace.fixture_document(
            path.clone(),
            None,
            self.document_store.dirty_overlays(),
        )?;
        crate::gui_edits::fixture::apply_fixture_gui_edit(&mut document, edit)?;
        let outcome = self.workspace.apply_fixture_edit(
            path,
            document,
            self.document_store.active_text()?,
            self.document_store.dirty_overlays(),
        )?;
        self.save_active_gui_text(outcome.serialized_content)
    }

    fn save_active_gui_text(&mut self, text: String) -> Result<(), String> {
        self.replace_active_gui_text(text)?;
        self.refresh_analysis_after_memory_edit();
        Ok(())
    }

    fn save_active_sequence_gui_text(
        &mut self,
        path: Utf8PathBuf,
        text: String,
        document: SequenceDocument,
        mode: PreviewSyncMode,
    ) -> Result<(), String> {
        self.replace_active_gui_text(text)?;
        self.sync_preview_source_from_document(path, document, mode);
        Ok(())
    }
}
