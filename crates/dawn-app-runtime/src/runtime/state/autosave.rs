use crate::runtime::contracts::RuntimeStatus;

use super::CoordinatorState;

impl CoordinatorState {
    pub(crate) fn prepare_for_runtime_project_open(&mut self) -> Result<(), String> {
        self.flush_autosave()
    }

    pub(crate) fn flush_autosave_command(&mut self) -> Result<(), String> {
        self.flush_autosave()?;
        self.status = RuntimeStatus::Saved;
        Ok(())
    }

    pub(crate) fn flush_autosave(&mut self) -> Result<(), String> {
        self.flush_autosave_with_preview_sync(true)
    }

    pub(super) fn flush_autosave_without_analysis(&mut self) -> Result<(), String> {
        self.workspace
            .flush_autosave_without_analysis(&mut self.editor)
    }

    fn flush_autosave_with_preview_sync(&mut self, sync_preview: bool) -> Result<(), String> {
        let had_dirty_buffers = self.workspace.flush_autosave(&mut self.editor)?;
        if had_dirty_buffers && sync_preview {
            self.sync_preview_source(crate::preview::session::PreviewSyncMode::RenderNow);
        }
        Ok(())
    }
}
