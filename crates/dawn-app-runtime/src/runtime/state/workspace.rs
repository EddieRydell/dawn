use dawn_language::path::{path_matches_any, Utf8PathBuf};

use crate::editor::document_store::DocumentStoreCommand;
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
            .document_store
            .buffer_tabs()
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
        self.refresh_analysis_from_document_store()?;
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
        for (old_path, new_path) in moves {
            self.document_store
                .handle(DocumentStoreCommand::ReconcileMovedPath { old_path, new_path })
                .map_err(|error| error.to_string())?;
        }
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(crate) fn delete_path(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        self.flush_autosave()?;
        self.workspace.delete_path(path.clone())?;
        self.document_store
            .handle(DocumentStoreCommand::ReconcileDeletedPath { path })
            .map_err(|error| error.to_string())?;
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(super) fn open_project(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        self.workspace.open_project(&path)?;
        let root = self
            .workspace
            .project_root()
            .ok_or_else(|| "project root was not opened".to_string())?;
        self.document_store
            .handle(DocumentStoreCommand::OpenProject { root })
            .map_err(|error| error.to_string())?;
        self.preview.reset();
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(super) fn reconcile_filesystem_changes(
        &mut self,
        paths: Vec<Utf8PathBuf>,
    ) -> Result<(), String> {
        let watched_paths = if paths.is_empty() {
            self.document_store
                .buffer_tabs()
                .into_iter()
                .map(|buffer| buffer.path)
                .collect()
        } else {
            paths
        };
        let buffers = self.document_store.buffer_tabs();
        for buffer in buffers {
            if !path_matches_any(&buffer.path, &watched_paths) {
                continue;
            }
            match self.workspace.read_file_with_version(buffer.path.clone()) {
                Ok((disk_text, disk_version)) => {
                    if buffer.disk_version.as_ref() == Some(&disk_version) {
                        continue;
                    }
                    self.document_store
                        .handle(DocumentStoreCommand::ExternalDiskChanged {
                            path: buffer.path,
                            disk_version,
                            text: disk_text,
                        })
                        .map_err(|error| error.to_string())?;
                }
                Err(_) => {
                    self.document_store
                        .handle(DocumentStoreCommand::ExternalDiskDeleted { path: buffer.path })
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        self.workspace.refresh_project_entries()?;
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn reload_active_buffer_from_disk(&mut self) -> Result<(), String> {
        let Some(buffer) = self.document_store.active_tab() else {
            return Ok(());
        };
        match self.workspace.read_file_with_version(buffer.path.clone()) {
            Ok((text, disk_version)) => {
                self.document_store
                    .handle(DocumentStoreCommand::ReloadBufferFromDisk {
                        path: buffer.path,
                        text,
                        disk_version,
                    })
                    .map_err(|error| error.to_string())?;
            }
            Err(_) => {
                self.document_store
                    .handle(DocumentStoreCommand::CloseBuffer { path: buffer.path })
                    .map_err(|error| error.to_string())?;
            }
        }
        self.workspace.refresh_project_entries()?;
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn keep_active_buffer(&mut self) -> Result<(), String> {
        let Some(buffer) = self.document_store.active_tab() else {
            return Ok(());
        };
        let version = self
            .workspace
            .write_text_file_with_version(buffer.path.clone(), &buffer.text)?;
        self.document_store
            .handle(DocumentStoreCommand::MarkSaved {
                path: buffer.path,
                expected_revision: buffer.revision,
                disk_version: version,
            })
            .map_err(|error| error.to_string())?;
        self.workspace.refresh_project_entries()?;
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }
}
