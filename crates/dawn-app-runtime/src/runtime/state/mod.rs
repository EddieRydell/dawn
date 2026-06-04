mod active_document;
mod autosave;
mod editor;
mod export;
mod gui;
mod preview;
mod snapshot;
mod workspace;

use crate::editor::document_store::DocumentStoreCore;
use crate::preview::session::PreviewController;
use crate::runtime::contracts::RuntimeStatus;
use crate::workspace::WorkspaceSession;

pub(crate) use snapshot::CoordinatorSnapshot;

#[derive(Debug)]
pub(crate) struct CoordinatorState {
    pub(super) workspace: WorkspaceSession,
    pub(super) document_store: DocumentStoreCore,
    pub(super) preview: PreviewController,
    pub(super) status: RuntimeStatus,
}

impl Default for CoordinatorState {
    fn default() -> Self {
        Self {
            workspace: WorkspaceSession::default(),
            document_store: DocumentStoreCore::default(),
            preview: PreviewController::default(),
            status: RuntimeStatus::NoProjectOpen,
        }
    }
}
