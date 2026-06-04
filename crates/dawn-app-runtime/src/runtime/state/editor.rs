use dawn_language::path::Utf8PathBuf;

use crate::editor::{EditorViewMode, SessionBufferState};
use crate::preview::session::PreviewSyncMode;
use crate::runtime::contracts::RuntimeStatus;

use super::CoordinatorState;

impl CoordinatorState {
    pub(crate) fn sync_project_opened(
        &mut self,
        path: std::path::PathBuf,
        _remember: bool,
        status: impl Into<String>,
    ) -> Result<(), String> {
        self.open_project(path)?;
        self.status = RuntimeStatus::message(status);
        Ok(())
    }

    pub(crate) fn sync_session_opened(
        &mut self,
        path: std::path::PathBuf,
        buffers: Vec<SessionBufferState>,
        active_file: Option<Utf8PathBuf>,
        status: impl Into<String>,
    ) -> Result<(), String> {
        self.workspace.open_project(&path)?;
        self.editor.restore(buffers, active_file);
        self.preview.reset();
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        self.status = RuntimeStatus::message(status);
        Ok(())
    }

    pub(crate) fn sync_file_opened(
        &mut self,
        path: Utf8PathBuf,
        text: String,
        disk_version: crate::editor::FileVersion,
        view_mode: EditorViewMode,
    ) -> Result<(), String> {
        self.editor.open_file(path, text, disk_version, view_mode);
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(crate) fn sync_file_closed(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        self.editor.close_file(&path);
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(crate) fn sync_active_file(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        let active_changed = self.editor.active_file() != Some(&path);
        self.editor.set_active_file(path);
        if active_changed {
            self.preview.pause(self.workspace.analysis());
            self.sync_preview_source(PreviewSyncMode::RenderNow);
        }
        Ok(())
    }

    pub(crate) fn sync_active_view_mode(&mut self, mode: EditorViewMode) -> Result<(), String> {
        self.editor.set_active_view_mode(mode);
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(crate) fn sync_active_text_update(&mut self, text: String) -> Result<(), String> {
        self.editor.ensure_active_buffer_not_conflicted()?;
        self.editor.update_active_text(text);
        self.refresh_analysis_after_memory_edit();
        self.status = RuntimeStatus::message("Edited");
        Ok(())
    }

    pub(crate) fn sync_active_history_text(&mut self, text: String, status: impl Into<String>) {
        self.editor.replace_active_text_from_runtime(text);
        self.refresh_analysis_after_memory_edit();
        self.status = RuntimeStatus::message(status);
    }

    pub(crate) fn project_root(&self) -> Option<String> {
        self.workspace.project_root()
    }

    pub(crate) fn set_status(&mut self, status: impl Into<String>) {
        self.status = RuntimeStatus::message(status);
    }

    pub(crate) fn read_file_with_version(
        &self,
        path: Utf8PathBuf,
    ) -> Result<(String, crate::editor::FileVersion), String> {
        self.workspace.read_file_with_version(path)
    }

    pub(crate) fn current_analysis(&self) -> Option<dawn_language::analysis::ProjectAnalysis> {
        self.workspace.analysis_cloned()
    }

    pub(super) fn refresh_analysis_after_memory_edit(&mut self) {
        match self.workspace.refresh_analysis_from_editor(&self.editor) {
            Ok(()) => {
                self.sync_preview_source(PreviewSyncMode::RenderNow);
            }
            Err(error) => {
                self.status = RuntimeStatus::message(error);
                self.sync_preview_source(PreviewSyncMode::RenderNow);
            }
        }
    }
}
