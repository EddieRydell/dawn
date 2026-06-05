use crate::editor::document_store::DocumentStoreCommand;
use crate::runtime::contracts::{RuntimeNotice, RuntimeStatus};

use super::AppBackend;

impl AppBackend {
    pub(super) fn prepare_for_backend_project_open(&mut self) -> Result<(), String> {
        self.flush_autosave_internal()
    }

    pub(super) fn flush_autosave_command(&mut self) -> Result<(), String> {
        self.flush_autosave_internal()?;
        self.status = RuntimeStatus::notice(RuntimeNotice::Saved);
        Ok(())
    }

    pub(crate) fn flush_autosave_internal(&mut self) -> Result<(), String> {
        self.flush_autosave_with_preview_sync(true)
    }

    pub(super) fn flush_autosave_without_analysis(&mut self) -> Result<(), String> {
        self.flush_autosave_buffers(false).map(|_| ())
    }

    fn flush_autosave_with_preview_sync(&mut self, sync_preview: bool) -> Result<(), String> {
        let had_dirty_buffers = self.flush_autosave_buffers(true)?;
        if had_dirty_buffers && sync_preview {
            self.sync_preview_source(crate::preview::session::PreviewSyncMode::RenderNow);
        }
        Ok(())
    }

    fn flush_autosave_buffers(&mut self, refresh_analysis: bool) -> Result<bool, String> {
        let dirty_buffers = self.document_store.dirty_autosave_buffers();
        let had_dirty_buffers = !dirty_buffers.is_empty();
        let saved_versions = self
            .workspace
            .flush_autosave_buffers(dirty_buffers.clone())?;
        for (path, disk_version) in saved_versions {
            let Some(buffer) = dirty_buffers.iter().find(|buffer| buffer.path == path) else {
                continue;
            };
            self.document_store
                .handle(DocumentStoreCommand::MarkSaved {
                    path,
                    expected_revision: buffer.revision,
                    disk_version,
                })
                .map_err(|error| error.to_string())?;
        }
        if had_dirty_buffers && refresh_analysis {
            self.refresh_analysis_from_document_store()?;
        }
        Ok(had_dirty_buffers)
    }
}
