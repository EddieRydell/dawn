use crossbeam_channel::{unbounded, Receiver};

use crate::contracts::{
    CommandAck, EventEnvelope, RequestId, Revision, RuntimeError, RuntimeErrorKind, RuntimeResult,
    ServiceName,
};
use crate::read_model::{AppReadModels, ReadModelCore};
use crate::runtime::{spawn_service, BackpressurePolicy, ServiceHandle};
use crate::services::audio_engine::{AudioEngineCommand, AudioEngineCore};
use crate::services::autosave::{AutosaveCommand, AutosaveCore};
use crate::services::document_store::{DocumentStoreCommand, DocumentStoreCore};
use crate::services::file_watcher::{FileWatcherCommand, FileWatcherCore};
use crate::services::layout_prefs::LayoutPrefsCore;
use crate::services::live_output::LiveOutputCore;
use crate::services::preview_engine::{PreviewEngineCommand, PreviewEngineCore};
use crate::services::project_index::{ProjectIndexCommand, ProjectIndexCore};

const SERVICE_QUEUE_CAPACITY: usize = 128;

pub struct AppCoordinator {
    next_request_id: u64,
    events: Receiver<EventEnvelope>,
    read_model: ReadModelCore,
    live_output: LiveOutputCore,
    layout_prefs: LayoutPrefsCore,
    document_store: Option<ServiceHandle<DocumentStoreCommand>>,
    project_index: Option<ServiceHandle<ProjectIndexCommand>>,
    preview_engine: Option<ServiceHandle<PreviewEngineCommand>>,
    audio_engine: Option<ServiceHandle<AudioEngineCommand>>,
    autosave: Option<ServiceHandle<AutosaveCommand>>,
    file_watcher: Option<ServiceHandle<FileWatcherCommand>>,
}

impl Default for AppCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl AppCoordinator {
    pub fn new() -> Self {
        let (tx, events) = unbounded();
        let policy = BackpressurePolicy::Reject;
        Self {
            next_request_id: 1,
            events,
            read_model: ReadModelCore::default(),
            live_output: LiveOutputCore::default(),
            layout_prefs: LayoutPrefsCore::default(),
            document_store: Some(spawn_service(
                DocumentStoreCore::default(),
                SERVICE_QUEUE_CAPACITY,
                policy,
                tx.clone(),
            )),
            project_index: Some(spawn_service(
                ProjectIndexCore::default(),
                SERVICE_QUEUE_CAPACITY,
                policy,
                tx.clone(),
            )),
            preview_engine: Some(spawn_service(
                PreviewEngineCore::default(),
                SERVICE_QUEUE_CAPACITY,
                policy,
                tx.clone(),
            )),
            audio_engine: Some(spawn_service(
                AudioEngineCore::default(),
                SERVICE_QUEUE_CAPACITY,
                policy,
                tx.clone(),
            )),
            autosave: Some(spawn_service(
                AutosaveCore::default(),
                SERVICE_QUEUE_CAPACITY,
                policy,
                tx.clone(),
            )),
            file_watcher: Some(spawn_service(
                FileWatcherCore::default(),
                SERVICE_QUEUE_CAPACITY,
                policy,
                tx,
            )),
        }
    }

    pub fn read_models(&self) -> &AppReadModels {
        self.read_model.models()
    }

    pub fn live_output(&self) -> &LiveOutputCore {
        &self.live_output
    }

    pub fn layout_prefs(&self) -> &LayoutPrefsCore {
        &self.layout_prefs
    }

    pub fn submit_document_store(
        &mut self,
        command: DocumentStoreCommand,
    ) -> RuntimeResult<CommandAck> {
        let target_revision = document_store_target_revision(&command);
        let request_id = self.next_request_id();
        submit(
            self.document_store.as_ref(),
            ServiceName::DocumentStore,
            request_id,
            target_revision,
            command,
        )
    }

    pub fn submit_project_index(
        &mut self,
        command: ProjectIndexCommand,
    ) -> RuntimeResult<CommandAck> {
        let target_revision = match &command {
            ProjectIndexCommand::Analyze {
                source_revision, ..
            } => Some(*source_revision),
        };
        let request_id = self.next_request_id();
        submit(
            self.project_index.as_ref(),
            ServiceName::ProjectIndex,
            request_id,
            target_revision,
            command,
        )
    }

    pub fn submit_preview_engine(
        &mut self,
        command: PreviewEngineCommand,
    ) -> RuntimeResult<CommandAck> {
        let target_revision = match &command {
            PreviewEngineCommand::QueueRender {
                request_revision, ..
            }
            | PreviewEngineCommand::PublishFrame {
                request_revision, ..
            } => Some(*request_revision),
        };
        let request_id = self.next_request_id();
        submit(
            self.preview_engine.as_ref(),
            ServiceName::PreviewEngine,
            request_id,
            target_revision,
            command,
        )
    }

    pub fn submit_audio_engine(
        &mut self,
        command: AudioEngineCommand,
    ) -> RuntimeResult<CommandAck> {
        let target_revision = match &command {
            AudioEngineCommand::SetReadiness { revision, .. } => Some(*revision),
        };
        let request_id = self.next_request_id();
        submit(
            self.audio_engine.as_ref(),
            ServiceName::AudioEngine,
            request_id,
            target_revision,
            command,
        )
    }

    pub fn submit_autosave(&mut self, command: AutosaveCommand) -> RuntimeResult<CommandAck> {
        let target_revision = match &command {
            AutosaveCommand::TagSelfWrite { revision, .. } => Some(*revision),
            AutosaveCommand::CompleteWrite { tag } => Some(tag.revision),
        };
        let request_id = self.next_request_id();
        submit(
            self.autosave.as_ref(),
            ServiceName::Autosave,
            request_id,
            target_revision,
            command,
        )
    }

    pub fn submit_file_watcher(
        &mut self,
        command: FileWatcherCommand,
    ) -> RuntimeResult<CommandAck> {
        let target_revision = match &command {
            FileWatcherCommand::DiskChanged { disk_revision, .. } => Some(*disk_revision),
        };
        let request_id = self.next_request_id();
        submit(
            self.file_watcher.as_ref(),
            ServiceName::FileWatcher,
            request_id,
            target_revision,
            command,
        )
    }

    pub fn drain_events(&mut self) -> RuntimeResult<usize> {
        let mut drained = 0;
        while let Ok(envelope) = self.events.try_recv() {
            if let Some(live_event) = self.live_output.consume(&envelope.event)? {
                self.read_model.apply(&EventEnvelope {
                    event: live_event,
                    ..envelope.clone()
                })?;
            }
            self.read_model.apply(&envelope)?;
            drained += 1;
        }
        Ok(drained)
    }

    pub fn shutdown(&mut self) -> RuntimeResult<()> {
        shutdown_handle(&mut self.document_store)?;
        shutdown_handle(&mut self.project_index)?;
        shutdown_handle(&mut self.preview_engine)?;
        shutdown_handle(&mut self.audio_engine)?;
        shutdown_handle(&mut self.autosave)?;
        shutdown_handle(&mut self.file_watcher)?;
        Ok(())
    }

    fn next_request_id(&mut self) -> RequestId {
        let request_id = RequestId::new(self.next_request_id);
        self.next_request_id = self.next_request_id.saturating_add(1);
        request_id
    }
}

impl Drop for AppCoordinator {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn document_store_target_revision(command: &DocumentStoreCommand) -> Option<Revision> {
    match command {
        DocumentStoreCommand::UpdateBufferText {
            expected_revision, ..
        }
        | DocumentStoreCommand::MarkSaved {
            expected_revision, ..
        } => Some(*expected_revision),
        DocumentStoreCommand::OpenProject { .. }
        | DocumentStoreCommand::OpenBuffer { .. }
        | DocumentStoreCommand::ExternalDiskChanged { .. }
        | DocumentStoreCommand::SetActiveBuffer { .. }
        | DocumentStoreCommand::SetViewMode { .. } => None,
    }
}

fn submit<C: Send + 'static>(
    handle: Option<&ServiceHandle<C>>,
    service: ServiceName,
    request_id: RequestId,
    target_revision: Option<Revision>,
    command: C,
) -> RuntimeResult<CommandAck> {
    let Some(handle) = handle else {
        return Err(RuntimeError::new(
            service,
            RuntimeErrorKind::Fatal,
            "service runner is stopped",
        ));
    };
    handle.submit(request_id, target_revision, command)
}

fn shutdown_handle<C: Send + 'static>(handle: &mut Option<ServiceHandle<C>>) -> RuntimeResult<()> {
    if let Some(handle) = handle.take() {
        handle.shutdown()?;
    }
    Ok(())
}
