use crossbeam_channel::{unbounded, Receiver};

use crate::app_model::AppRuntimeModel;
use crate::contracts::{
    CommandAck, Event, EventEnvelope, RequestId, Revision, RuntimeError, RuntimeErrorKind,
    RuntimeResult, ServiceName,
};
use crate::dto::{AppSnapshotDto, SequenceSelectionEditDto, SequenceSelectionEditResultDto};
use crate::layout_persistence::WindowLayout;
use crate::read_model::{AppReadModels, ReadModelCore};
use crate::runtime::{spawn_service, BackpressurePolicy, ServiceHandle};
use crate::services::audio_engine::{AudioEngineCommand, AudioEngineCore};
use crate::services::autosave::{AutosaveCommand, AutosaveCore};
use crate::services::document_store::{DocumentStoreCommand, DocumentStoreCore};
use crate::services::file_watcher::{FileWatcherCommand, FileWatcherCore};
use crate::services::layout_prefs::LayoutPrefsCore;
use crate::services::live_output::{LiveOutputCore, LiveOutputReadout};
use crate::services::preview_engine::{PreviewEngineCommand, PreviewEngineCore};
use crate::services::project_index::{ProjectIndexCommand, ProjectIndexCore};
use dawn_project::document::SequenceMarkPasteDocumentEdit;
use dawn_project::model::{Authored, SequenceEffect};
use dawn_project::path::Utf8PathBuf;

const SERVICE_QUEUE_CAPACITY: usize = 128;

pub struct AppCoordinator {
    state: AppRuntimeModel,
    sequence_clipboard: Option<SequenceClipboard>,
    next_request_id: u64,
    events: Receiver<EventEnvelope>,
    read_model: ReadModelCore,
    command_failures: Vec<(RequestId, RuntimeError)>,
    buffer_text_updates: Vec<(RequestId, Utf8PathBuf, String, Revision)>,
    command_completions: Vec<RequestId>,
    live_output: LiveOutputCore,
    layout_prefs: LayoutPrefsCore,
    document_store: Option<ServiceHandle<DocumentStoreCommand>>,
    project_index: Option<ServiceHandle<ProjectIndexCommand>>,
    preview_engine: Option<ServiceHandle<PreviewEngineCommand>>,
    audio_engine: Option<ServiceHandle<AudioEngineCommand>>,
    autosave: Option<ServiceHandle<AutosaveCommand>>,
    file_watcher: Option<ServiceHandle<FileWatcherCommand>>,
}

#[derive(Debug, Clone)]
pub enum SequenceClipboard {
    Effects(Vec<SequenceEffect<Authored>>),
    Marks(Vec<SequenceMarkPasteDocumentEdit>),
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
            state: AppRuntimeModel::default(),
            sequence_clipboard: None,
            next_request_id: 1,
            events,
            read_model: ReadModelCore::default(),
            command_failures: Vec::new(),
            buffer_text_updates: Vec::new(),
            command_completions: Vec::new(),
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

    pub fn runtime_model(&self) -> &AppRuntimeModel {
        &self.state
    }

    pub fn runtime_model_mut(&mut self) -> &mut AppRuntimeModel {
        &mut self.state
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

    pub fn app_snapshot(&self) -> AppSnapshotDto {
        AppSnapshotDto::from(self.state.snapshot(
            self.layout_prefs.project_tree_visible(),
            self.layout_prefs.effect_preview_enabled(),
            self.live_output.readout(),
        ))
    }

    pub fn sync_live_output_readout(&mut self, readout: LiveOutputReadout) {
        self.live_output.sync_readout(readout);
    }

    pub fn live_output_readout(&self) -> LiveOutputReadout {
        self.live_output.readout()
    }

    pub fn last_project_root(&self) -> Option<std::path::PathBuf> {
        self.layout_prefs.last_project_root()
    }

    pub fn remember_project_root(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        self.layout_prefs.remember_project_root(path)
    }

    pub fn toggle_project_tree(&mut self) -> Result<(), String> {
        self.layout_prefs.toggle_project_tree()
    }

    pub fn set_effect_preview_enabled(&mut self, enabled: bool) -> Result<(), String> {
        self.layout_prefs.set_effect_preview_enabled(enabled)?;
        self.state.set_effect_preview_enabled(enabled)
    }

    pub fn set_effect_preview_effects(&mut self, ids: Vec<u32>) {
        self.state
            .set_effect_preview_effects(ids, self.layout_prefs.effect_preview_enabled());
    }

    pub fn preview_play(&mut self) -> Result<(), String> {
        if self.layout_prefs.effect_preview_enabled() {
            self.layout_prefs.set_effect_preview_enabled(false)?;
            self.state.set_effect_preview_enabled(false)?;
        }
        self.state.preview_play();
        Ok(())
    }

    pub fn effect_preview_enabled(&self) -> bool {
        self.layout_prefs.effect_preview_enabled()
    }

    pub fn preview_window_should_open(&self) -> bool {
        self.layout_prefs.preview_window_open()
    }

    pub fn preview_window_layout(&self) -> WindowLayout {
        self.layout_prefs.preview_window_layout()
    }

    pub fn main_window_layout(&self) -> WindowLayout {
        self.layout_prefs.main_window_layout()
    }

    pub fn set_main_window_layout(&mut self, layout: WindowLayout) -> Result<(), String> {
        self.layout_prefs.set_main_window_layout(layout)
    }

    pub fn set_preview_window_layout(&mut self, layout: WindowLayout) -> Result<(), String> {
        self.layout_prefs.set_preview_window_layout(layout)
    }

    pub fn set_preview_window_open(&mut self, open: bool) -> Result<(), String> {
        self.layout_prefs.set_preview_window_open(open)
    }

    pub fn apply_sequence_selection_edit(
        &mut self,
        edit: SequenceSelectionEditDto,
    ) -> Result<SequenceSelectionEditResultDto, String> {
        self.state
            .apply_sequence_selection_edit(edit, &mut self.sequence_clipboard)
    }

    pub fn take_command_failure(&mut self, request_id: RequestId) -> Option<RuntimeError> {
        let index = self
            .command_failures
            .iter()
            .position(|(candidate, _)| *candidate == request_id)?;
        Some(self.command_failures.remove(index).1)
    }

    pub fn take_buffer_text_update(
        &mut self,
        request_id: RequestId,
    ) -> Option<(Utf8PathBuf, String, Revision)> {
        let index = self
            .buffer_text_updates
            .iter()
            .position(|(candidate, _, _, _)| *candidate == request_id)?;
        let (_, path, text, revision) = self.buffer_text_updates.remove(index);
        Some((path, text, revision))
    }

    pub fn take_command_completion(&mut self, request_id: RequestId) -> bool {
        let Some(index) = self
            .command_completions
            .iter()
            .position(|candidate| *candidate == request_id)
        else {
            return false;
        };
        self.command_completions.remove(index);
        true
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
            FileWatcherCommand::DiskChanged { .. } => None,
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
            if let (
                Some(request_id),
                Event::CommandFailed {
                    service,
                    kind,
                    message,
                },
            ) = (envelope.request_id, &envelope.event)
            {
                self.command_failures.push((
                    request_id,
                    RuntimeError::new(service.clone(), kind.clone(), message.clone()),
                ));
            }
            if let (
                Some(request_id),
                Event::BufferTextUpdated {
                    path,
                    revision,
                    text,
                },
            ) = (envelope.request_id, &envelope.event)
            {
                self.buffer_text_updates
                    .push((request_id, path.clone(), text.clone(), *revision));
            }
            if let (Some(request_id), Event::CommandCompleted { .. }) =
                (envelope.request_id, &envelope.event)
            {
                self.command_completions.push(request_id);
            }
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
        | DocumentStoreCommand::OpenSession { .. }
        | DocumentStoreCommand::OpenBuffer { .. }
        | DocumentStoreCommand::ExternalDiskChanged { .. }
        | DocumentStoreCommand::ExternalDiskDeleted { .. }
        | DocumentStoreCommand::ReloadBufferFromDisk { .. }
        | DocumentStoreCommand::KeepBuffer { .. }
        | DocumentStoreCommand::ReconcileMovedPath { .. }
        | DocumentStoreCommand::ReconcileDeletedPath { .. }
        | DocumentStoreCommand::SetActiveBuffer { .. }
        | DocumentStoreCommand::SetViewMode { .. }
        | DocumentStoreCommand::CloseBuffer { .. } => None,
        DocumentStoreCommand::UndoBufferText {
            expected_revision, ..
        }
        | DocumentStoreCommand::RedoBufferText {
            expected_revision, ..
        } => Some(*expected_revision),
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
