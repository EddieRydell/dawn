use dawn_language::path::Utf8PathBuf;

use crate::editor::document_store::{DocumentStoreCommand, RuntimeSessionBuffer};
use crate::editor::{EditorViewMode, FileVersion};
use crate::preview::session::PreviewSyncMode;
use crate::runtime::contracts::{Event, RuntimeResult, RuntimeStatus};

use super::CoordinatorState;

impl CoordinatorState {
    pub(crate) fn submit_document_store(
        &mut self,
        command: DocumentStoreCommand,
    ) -> RuntimeResult<Vec<Event>> {
        self.document_store.handle(command)
    }

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
        buffers: Vec<RuntimeSessionBuffer>,
        active_file: Option<Utf8PathBuf>,
        status: impl Into<String>,
    ) -> Result<(), String> {
        self.workspace.open_project(&path)?;
        let root = self
            .workspace
            .project_root()
            .ok_or_else(|| "project root was not opened".to_string())?;
        self.document_store
            .handle(DocumentStoreCommand::OpenSession {
                root,
                buffers,
                active_file,
            })
            .map_err(|error| error.to_string())?;
        self.preview.reset();
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        self.status = RuntimeStatus::message(status);
        Ok(())
    }

    pub(crate) fn open_buffer(
        &mut self,
        path: Utf8PathBuf,
        text: String,
        disk_version: Option<FileVersion>,
    ) -> Result<(), String> {
        self.document_store
            .handle(DocumentStoreCommand::OpenBuffer {
                path,
                text,
                disk_version,
            })
            .map_err(|error| error.to_string())?;
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(crate) fn close_buffer(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        self.document_store
            .handle(DocumentStoreCommand::CloseBuffer { path })
            .map_err(|error| error.to_string())?;
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(crate) fn set_active_buffer(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        let active_changed = self.document_store.active_file() != Some(&path);
        self.document_store
            .handle(DocumentStoreCommand::SetActiveBuffer { path })
            .map_err(|error| error.to_string())?;
        if active_changed {
            self.preview.pause(self.workspace.analysis());
            self.sync_preview_source(PreviewSyncMode::RenderNow);
        }
        Ok(())
    }

    pub(crate) fn set_active_view_mode(&mut self, mode: EditorViewMode) -> Result<(), String> {
        let Some(path) = self.document_store.active_file().cloned() else {
            return Ok(());
        };
        self.document_store
            .handle(DocumentStoreCommand::SetViewMode { path, mode })
            .map_err(|error| error.to_string())?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(crate) fn update_active_text(&mut self, text: String) -> Result<(), String> {
        self.document_store.ensure_active_buffer_not_conflicted()?;
        let Some(buffer) = self.document_store.active_buffer() else {
            return Ok(());
        };
        if buffer.text == text {
            return Ok(());
        }
        self.document_store
            .handle(DocumentStoreCommand::UpdateBufferText {
                path: buffer.path.clone(),
                expected_revision: buffer.revision,
                text,
            })
            .map_err(|error| error.to_string())?;
        self.refresh_analysis_after_memory_edit();
        self.status = RuntimeStatus::message("Edited");
        Ok(())
    }

    pub(crate) fn replace_active_gui_text(&mut self, text: String) -> Result<(), String> {
        self.document_store.ensure_active_buffer_not_conflicted()?;
        let Some(buffer) = self.document_store.active_buffer() else {
            return Ok(());
        };
        if buffer.text == text {
            return Ok(());
        }
        self.document_store
            .handle(DocumentStoreCommand::UpdateBufferText {
                path: buffer.path.clone(),
                expected_revision: buffer.revision,
                text,
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(crate) fn undo_buffer_text(&mut self, path: Utf8PathBuf) -> Result<Option<String>, String> {
        self.apply_history_edit(path, HistoryCommand::Undo)
    }

    pub(crate) fn redo_buffer_text(&mut self, path: Utf8PathBuf) -> Result<Option<String>, String> {
        self.apply_history_edit(path, HistoryCommand::Redo)
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
    ) -> Result<(String, FileVersion), String> {
        self.workspace.read_file_with_version(path)
    }

    pub(crate) fn current_analysis(&self) -> Option<dawn_language::analysis::ProjectAnalysis> {
        self.workspace.analysis_cloned()
    }

    pub(crate) fn refresh_analysis_after_memory_edit(&mut self) {
        match self.refresh_analysis_from_document_store() {
            Ok(()) => self.sync_preview_source(PreviewSyncMode::RenderNow),
            Err(error) => {
                self.status = RuntimeStatus::message(error);
                self.sync_preview_source(PreviewSyncMode::RenderNow);
            }
        }
    }

    pub(super) fn refresh_analysis_from_document_store(&mut self) -> Result<(), String> {
        self.workspace
            .refresh_analysis_from_overlays(self.document_store.dirty_overlays())
    }

    fn apply_history_edit(
        &mut self,
        path: Utf8PathBuf,
        command: HistoryCommand,
    ) -> Result<Option<String>, String> {
        let revision = self
            .document_store
            .buffer(&path)
            .map(|buffer| buffer.revision)
            .ok_or_else(|| format!("runtime buffer is not open: {path}"))?;
        let events = match command {
            HistoryCommand::Undo => {
                self.document_store
                    .handle(DocumentStoreCommand::UndoBufferText {
                        path: path.clone(),
                        expected_revision: revision,
                    })
            }
            HistoryCommand::Redo => {
                self.document_store
                    .handle(DocumentStoreCommand::RedoBufferText {
                        path: path.clone(),
                        expected_revision: revision,
                    })
            }
        }
        .map_err(|error| error.to_string())?;
        let text = events.into_iter().find_map(|event| match event {
            Event::BufferTextUpdated { text, .. } => Some(text),
            _ => None,
        });
        if text.is_some() {
            self.refresh_analysis_after_memory_edit();
        }
        Ok(text)
    }
}

#[derive(Debug, Clone, Copy)]
enum HistoryCommand {
    Undo,
    Redo,
}
