use dawn_language::path::Utf8PathBuf;

use crate::editor::document_store::{
    DocumentStoreCommand, ExternalDiskSnapshot, RuntimeSessionBuffer,
};
use crate::preview::session::PreviewSyncMode;
use crate::runtime::contracts::{RuntimeNotice, RuntimeStatus};
use crate::workspace::project_root_label_for_path;

use super::AppBackend;

impl AppBackend {
    pub(super) fn open_project_root_label(&mut self, root: String) -> Result<(), String> {
        self.handle_document_store(DocumentStoreCommand::OpenProject { root })
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(super) fn sync_project_opened(
        &mut self,
        path: std::path::PathBuf,
        _remember: bool,
        notice: RuntimeNotice,
    ) -> Result<(), String> {
        self.open_project_path(path)?;
        self.status = RuntimeStatus::notice(notice);
        Ok(())
    }

    pub(super) fn sync_session_opened(
        &mut self,
        path: std::path::PathBuf,
        buffers: Vec<RuntimeSessionBuffer>,
        active_file: Option<Utf8PathBuf>,
        notice: RuntimeNotice,
    ) -> Result<(), String> {
        self.workspace.open_project(&path)?;
        self.workers.sync_filesystem_root(Some(path))?;
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
        self.status = RuntimeStatus::notice(notice);
        Ok(())
    }

    pub(super) fn project_root(&self) -> Option<String> {
        self.workspace.project_root()
    }

    pub(super) fn restore_last_project_command(&mut self) -> Result<(), String> {
        let Some(path) = self.last_project_root() else {
            return Ok(());
        };
        match self.open_project_path(path) {
            Ok(()) => {
                self.status = RuntimeStatus::notice(RuntimeNotice::ProjectRestored);
                Ok(())
            }
            Err(error) => {
                self.status =
                    RuntimeStatus::error(format!("Could not restore last project: {error}"));
                Ok(())
            }
        }
    }

    pub(super) fn open_project_from_path(
        &mut self,
        path: std::path::PathBuf,
        remember: bool,
        notice: RuntimeNotice,
    ) -> Result<(), String> {
        self.prepare_for_backend_project_open()?;
        let root = project_root_label_for_path(&path)?;
        self.open_project_root_label(root)?;
        self.sync_project_opened(path.clone(), remember, notice)?;
        if remember {
            self.remember_project_root(path)?;
        }
        Ok(())
    }

    pub(super) fn current_analysis(&self) -> Option<dawn_language::analysis::ProjectAnalysis> {
        self.workspace.analysis_cloned()
    }

    pub(super) fn open_file_command(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        let (text, disk_version) = self.workspace.read_file_with_version(path.clone())?;
        self.open_buffer(path, text, Some(disk_version))
    }

    pub(super) fn create_file_command(
        &mut self,
        parent: Utf8PathBuf,
        name: String,
    ) -> Result<(), String> {
        self.flush_autosave_internal()?;
        let created = self.workspace.create_file_for_runtime_open(parent, &name)?;
        self.open_buffer(created.path, created.text, Some(created.disk_version))
    }

    pub(super) fn reload_project_command(&mut self) -> Result<(), String> {
        let paths = self
            .document_store
            .buffer_tabs()
            .into_iter()
            .map(|buffer| buffer.path)
            .collect();
        self.reconcile_filesystem_changes(paths)?;
        self.status = RuntimeStatus::notice(RuntimeNotice::ProjectChecked);
        Ok(())
    }

    pub(super) fn handle_filesystem_changes(
        &mut self,
        paths: Vec<Utf8PathBuf>,
    ) -> Result<(), String> {
        self.reconcile_filesystem_changes(paths)?;
        self.status = RuntimeStatus::notice(RuntimeNotice::FilesystemRefreshed);
        Ok(())
    }

    pub(super) fn reload_active_buffer_from_disk_command(&mut self) -> Result<(), String> {
        self.reload_active_buffer_from_disk_internal()?;
        self.status = RuntimeStatus::notice(RuntimeNotice::ReloadedFromDisk);
        Ok(())
    }

    pub(super) fn keep_active_buffer_command(&mut self) -> Result<(), String> {
        self.keep_active_buffer_internal()?;
        self.status = RuntimeStatus::notice(RuntimeNotice::KeptIdeChanges);
        Ok(())
    }

    pub(super) fn create_directory_command(
        &mut self,
        parent: Utf8PathBuf,
        name: String,
    ) -> Result<(), String> {
        self.flush_autosave_internal()?;
        self.workspace.create_directory(parent, &name)?;
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(super) fn rename_path_command(
        &mut self,
        path: Utf8PathBuf,
        new_name: String,
    ) -> Result<(), String> {
        self.flush_autosave_internal()?;
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

    pub(super) fn delete_path_command(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        self.flush_autosave_internal()?;
        self.workspace.delete_path(path.clone())?;
        self.document_store
            .handle(DocumentStoreCommand::ReconcileDeletedPath { path })
            .map_err(|error| error.to_string())?;
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub(super) fn open_project_path(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        self.workspace.open_project(&path)?;
        self.workers.sync_filesystem_root(Some(path.clone()))?;
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
        let paths = self.document_store.buffer_paths_for_external_changes(paths);
        for path in paths {
            let disk = self.external_disk_snapshot(path.clone());
            if let Some(command) = self.document_store.external_disk_command_for(path, disk) {
                self.document_store
                    .handle(command)
                    .map_err(|error| error.to_string())?;
            }
        }
        self.workspace.refresh_project_entries()?;
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn reload_active_buffer_from_disk_internal(&mut self) -> Result<(), String> {
        let Some(buffer) = self.document_store.active_tab() else {
            return Ok(());
        };
        let disk = self.external_disk_snapshot(buffer.path);
        if let Some(command) = self.document_store.active_reload_from_disk_command(disk) {
            self.document_store
                .handle(command)
                .map_err(|error| error.to_string())?;
        }
        self.workspace.refresh_project_entries()?;
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn keep_active_buffer_internal(&mut self) -> Result<(), String> {
        let Some(buffer) = self.document_store.active_tab() else {
            return Ok(());
        };
        let version = self
            .workspace
            .write_text_file_with_version(buffer.path, &buffer.text)?;
        if let Some(command) = self.document_store.active_mark_saved_command(version) {
            self.document_store
                .handle(command)
                .map_err(|error| error.to_string())?;
        }
        self.workspace.refresh_project_entries()?;
        self.refresh_analysis_from_document_store()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn external_disk_snapshot(&self, path: Utf8PathBuf) -> ExternalDiskSnapshot {
        match self.workspace.read_file_with_version(path) {
            Ok((text, disk_version)) => ExternalDiskSnapshot::Present { text, disk_version },
            Err(_) => ExternalDiskSnapshot::Missing,
        }
    }
}
