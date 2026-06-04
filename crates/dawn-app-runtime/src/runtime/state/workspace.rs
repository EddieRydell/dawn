use dawn_language::path::Utf8PathBuf;

use crate::preview::session::PreviewSyncMode;
use crate::runtime::contracts::RuntimeStatus;
use crate::workspace::CreatedRuntimeFile;

use super::CoordinatorState;

impl CoordinatorState {
    pub(crate) fn create_file_for_runtime_open(
        &mut self,
        parent: Utf8PathBuf,
        name: String,
    ) -> Result<CreatedRuntimeFile, String> {
        self.flush_autosave()?;
        self.workspace.create_file_for_runtime_open(parent, &name)
    }

    pub(crate) fn reload_project(&mut self) -> Result<(), String> {
        let paths = self
            .editor
            .buffers()
            .into_iter()
            .map(|buffer| buffer.path)
            .collect();
        self.reconcile_filesystem_changes(paths)?;
        self.status = RuntimeStatus::message("Project checked");
        Ok(())
    }

    pub(crate) fn handle_filesystem_changes(
        &mut self,
        paths: Vec<Utf8PathBuf>,
    ) -> Result<(), String> {
        self.reconcile_filesystem_changes(paths)?;
        self.status = RuntimeStatus::message("Filesystem refreshed");
        Ok(())
    }

    pub(crate) fn reload_active_buffer_from_disk_command(&mut self) -> Result<(), String> {
        self.reload_active_buffer_from_disk()?;
        self.status = RuntimeStatus::message("Reloaded from disk");
        Ok(())
    }

    pub(crate) fn keep_active_buffer_command(&mut self) -> Result<(), String> {
        self.keep_active_buffer()?;
        self.status = RuntimeStatus::message("Kept IDE changes");
        Ok(())
    }

    pub(crate) fn create_directory(
        &mut self,
        parent: Utf8PathBuf,
        name: String,
    ) -> Result<(), String> {
        self.flush_autosave()?;
        self.workspace.create_directory(parent, &name)?;
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(crate) fn rename_path(
        &mut self,
        path: Utf8PathBuf,
        new_name: String,
    ) -> Result<(), String> {
        self.flush_autosave()?;
        let moves = self.workspace.rename_path(path.clone(), &new_name)?;
        self.editor.reconcile_moved_paths(&moves);
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(crate) fn delete_path(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        self.flush_autosave()?;
        self.workspace.delete_path(path.clone())?;
        self.editor.reconcile_deleted_path(&path);
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(super) fn open_project(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        self.workspace.open_project(&path)?;
        self.editor.clear();
        self.preview.reset();
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(super) fn reconcile_filesystem_changes(
        &mut self,
        paths: Vec<Utf8PathBuf>,
    ) -> Result<(), String> {
        self.workspace
            .reconcile_filesystem_changes(&mut self.editor, paths)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn reload_active_buffer_from_disk(&mut self) -> Result<(), String> {
        self.workspace
            .reload_active_buffer_from_disk(&mut self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn keep_active_buffer(&mut self) -> Result<(), String> {
        self.workspace.keep_active_buffer(&mut self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }
}
