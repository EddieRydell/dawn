use dawn_language::document::{DocumentViewId, SequenceDocument, SequenceDocumentEdit};
use dawn_language::path::Utf8PathBuf;

use crate::dto::{
    FixtureGuiEditDto, LayoutGuiEditDto, SequenceGuiEditDto, SequenceSelectionEditDto,
    SequenceSelectionEditResultDto,
};
use crate::gui_edits::selection::{plan_sequence_selection_edit, SequenceClipboard};
use crate::preview::session::PreviewSyncMode;
use crate::runtime::contracts::RuntimeStatus;

use super::CoordinatorState;

impl CoordinatorState {
    pub(crate) fn apply_sequence_gui_edit_and_autosave(
        &mut self,
        edit: SequenceGuiEditDto,
    ) -> Result<(), String> {
        self.apply_sequence_gui_edit(edit)?;
        self.flush_autosave_without_analysis()?;
        self.status = RuntimeStatus::message("Autosaved");
        Ok(())
    }

    pub(crate) fn apply_layout_gui_edit_and_autosave(
        &mut self,
        edit: LayoutGuiEditDto,
    ) -> Result<(), String> {
        self.apply_layout_gui_edit(edit)?;
        self.flush_autosave()?;
        self.status = RuntimeStatus::message("Autosaved");
        Ok(())
    }

    pub(crate) fn apply_fixture_gui_edit_and_autosave(
        &mut self,
        edit: FixtureGuiEditDto,
    ) -> Result<(), String> {
        self.apply_fixture_gui_edit(edit)?;
        self.flush_autosave()?;
        self.status = RuntimeStatus::message("Autosaved");
        Ok(())
    }

    pub(crate) fn apply_sequence_selection_edit(
        &mut self,
        edit: SequenceSelectionEditDto,
        sequence_clipboard: &mut Option<SequenceClipboard>,
    ) -> Result<SequenceSelectionEditResultDto, String> {
        let before = self.active_sequence_authored()?;
        let before_document = self.active_sequence_document()?;
        let outcome =
            plan_sequence_selection_edit(edit, sequence_clipboard, &before, &before_document)?;

        if let Some(document_edit) = outcome.document_edit {
            self.apply_sequence_document_edit(document_edit)?;
            self.flush_autosave_without_analysis()?;
        }
        self.status = RuntimeStatus::message(outcome.status_message);
        Ok(outcome.result)
    }

    fn apply_sequence_gui_edit(&mut self, edit: SequenceGuiEditDto) -> Result<(), String> {
        self.editor.ensure_active_buffer_not_conflicted()?;
        let path = self.editor.active_path()?;
        let descriptor_overlays = self.editor.dirty_overlays();
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), descriptor_overlays)?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .ok_or_else(|| "active document is not a sequence".to_string())?
            .clone();
        let edit = crate::gui_edits::sequence::sequence_document_edit_from_gui(edit);
        let base_content = self.editor.active_text()?;
        let edit_overlays = self.editor.dirty_overlays();
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
        self.editor.ensure_active_buffer_not_conflicted()?;
        let path = self.editor.active_path()?;
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), self.editor.dirty_overlays())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .ok_or_else(|| "active document is not a sequence".to_string())?
            .clone();
        let outcome = self.workspace.apply_sequence_edit(
            path.clone(),
            &object_key,
            edit,
            self.editor.active_text()?,
            self.editor.dirty_overlays(),
        )?;
        self.save_active_sequence_gui_text(
            path,
            outcome.serialized_content,
            outcome.refreshed_document,
            PreviewSyncMode::DeferRender,
        )
    }

    fn apply_layout_gui_edit(&mut self, edit: LayoutGuiEditDto) -> Result<(), String> {
        self.editor.ensure_active_buffer_not_conflicted()?;
        let path = self.editor.active_path()?;
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), self.editor.dirty_overlays())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Layout)
            .ok_or_else(|| "active document is not a layout".to_string())?
            .clone();
        let mut document = self.workspace.layout_document(
            path.clone(),
            &object_key,
            self.editor.dirty_overlays(),
        )?;
        crate::gui_edits::layout::apply_layout_gui_edit(&mut document, edit)?;
        let outcome = self.workspace.apply_layout_edit(
            path,
            &object_key,
            document,
            self.editor.active_text()?,
            self.editor.dirty_overlays(),
        )?;
        self.save_active_gui_text(outcome.serialized_content)
    }

    fn apply_fixture_gui_edit(&mut self, edit: FixtureGuiEditDto) -> Result<(), String> {
        self.editor.ensure_active_buffer_not_conflicted()?;
        let path = self.editor.active_path()?;
        let mut document =
            self.workspace
                .fixture_document(path.clone(), None, self.editor.dirty_overlays())?;
        crate::gui_edits::fixture::apply_fixture_gui_edit(&mut document, edit)?;
        let outcome = self.workspace.apply_fixture_edit(
            path,
            document,
            self.editor.active_text()?,
            self.editor.dirty_overlays(),
        )?;
        self.save_active_gui_text(outcome.serialized_content)
    }

    fn save_active_gui_text(&mut self, text: String) -> Result<(), String> {
        self.editor.replace_active_text_from_gui(text);
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
        self.editor.replace_active_text_from_gui(text);
        self.sync_preview_source_from_document(path, document, mode);
        Ok(())
    }
}
