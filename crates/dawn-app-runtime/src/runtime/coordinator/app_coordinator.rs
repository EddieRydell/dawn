use std::collections::VecDeque;
use std::time::SystemTime;

use crate::app_shell::LayoutPrefsCore;
use crate::app_shell::WindowLayout;
use crate::dto::{AppSnapshotDto, SequenceSelectionEditDto, SequenceSelectionEditResultDto};
use crate::editor::document_store::{DocumentStoreCommand, RuntimeSessionBuffer};
use crate::editor::{EditorViewMode, FileVersion};
use crate::gui_edits::selection::SequenceClipboard;
use crate::output::live_output::{LiveOutputCore, LiveOutputReadout};
use crate::preview::audio_engine::{AudioEngineCommand, AudioEngineCore};
use crate::preview::engine_service::{PreviewEngineCommand, PreviewEngineCore};
use crate::runtime::autosave_service::{AutosaveCommand, AutosaveCore};
use crate::runtime::contracts::{
    CommandAck, Event, EventEnvelope, RequestId, Revision, RuntimeError, RuntimeErrorKind,
    RuntimeResult, ServiceName,
};
use crate::runtime::file_watcher_service::{FileWatcherCommand, FileWatcherCore};
use crate::runtime::read_model::{AppReadModels, ReadModelCore};
use crate::runtime::state::CoordinatorState;
use crate::workspace::project_index::{ProjectIndexCommand, ProjectIndexCore};
use dawn_language::path::Utf8PathBuf;

pub use crate::workspace::CreatedRuntimeFile;

#[allow(dead_code)]
pub struct AppCoordinator {
    state: CoordinatorState,
    stopped: bool,
    sequence_clipboard: Option<SequenceClipboard>,
    next_request_id: u64,
    next_event_sequence: u64,
    events: VecDeque<EventEnvelope>,
    read_model: ReadModelCore,
    command_failures: Vec<(RequestId, RuntimeError)>,
    buffer_text_updates: Vec<(RequestId, Utf8PathBuf, String, Revision)>,
    command_completions: Vec<RequestId>,
    live_output: LiveOutputCore,
    layout_prefs: LayoutPrefsCore,
    project_index: ProjectIndexCore,
    preview_engine: PreviewEngineCore,
    audio_engine: AudioEngineCore,
    autosave: AutosaveCore,
    file_watcher: FileWatcherCore,
}

impl Default for AppCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl AppCoordinator {
    pub fn new() -> Self {
        Self {
            state: CoordinatorState::default(),
            stopped: false,
            sequence_clipboard: None,
            next_request_id: 1,
            next_event_sequence: 0,
            events: VecDeque::new(),
            read_model: ReadModelCore::default(),
            command_failures: Vec::new(),
            buffer_text_updates: Vec::new(),
            command_completions: Vec::new(),
            live_output: LiveOutputCore::default(),
            layout_prefs: LayoutPrefsCore::default(),
            project_index: ProjectIndexCore::default(),
            preview_engine: PreviewEngineCore::default(),
            audio_engine: AudioEngineCore::default(),
            autosave: AutosaveCore::default(),
            file_watcher: FileWatcherCore::default(),
        }
    }

    pub fn prepare_for_runtime_project_open(&mut self) -> Result<(), String> {
        self.state.prepare_for_runtime_project_open()
    }

    pub fn sync_project_opened(
        &mut self,
        path: std::path::PathBuf,
        remember: bool,
        status: impl Into<String>,
    ) -> Result<(), String> {
        self.state.sync_project_opened(path, remember, status)
    }

    pub fn sync_session_opened(
        &mut self,
        path: std::path::PathBuf,
        buffers: Vec<RuntimeSessionBuffer>,
        active_file: Option<Utf8PathBuf>,
        status: impl Into<String>,
    ) -> Result<(), String> {
        self.state
            .sync_session_opened(path, buffers, active_file, status)
    }

    pub fn create_file_for_runtime_open(
        &mut self,
        parent: Utf8PathBuf,
        name: String,
    ) -> Result<CreatedRuntimeFile, String> {
        self.state.create_file_for_runtime_open(parent, name)
    }

    pub fn reload_project(&mut self) -> Result<(), String> {
        self.state.reload_project()
    }

    pub fn apply_sequence_gui_edit_and_autosave(
        &mut self,
        edit: crate::dto::SequenceGuiEditDto,
    ) -> Result<(), String> {
        self.state.apply_sequence_gui_edit_and_autosave(edit)
    }

    pub fn apply_layout_gui_edit_and_autosave(
        &mut self,
        edit: crate::dto::LayoutGuiEditDto,
    ) -> Result<(), String> {
        self.state.apply_layout_gui_edit_and_autosave(edit)
    }

    pub fn apply_fixture_gui_edit_and_autosave(
        &mut self,
        edit: crate::dto::FixtureGuiEditDto,
    ) -> Result<(), String> {
        self.state.apply_fixture_gui_edit_and_autosave(edit)
    }

    pub fn flush_autosave_command(&mut self) -> Result<(), String> {
        self.state.flush_autosave_command()
    }

    pub fn handle_filesystem_changes(&mut self, paths: Vec<Utf8PathBuf>) -> Result<(), String> {
        self.state.handle_filesystem_changes(paths)
    }

    pub fn reload_active_buffer_from_disk_command(&mut self) -> Result<(), String> {
        self.state.reload_active_buffer_from_disk_command()
    }

    pub fn keep_active_buffer_command(&mut self) -> Result<(), String> {
        self.state.keep_active_buffer_command()
    }

    pub fn create_directory(&mut self, parent: Utf8PathBuf, name: String) -> Result<(), String> {
        self.state.create_directory(parent, name)
    }

    pub fn rename_path(&mut self, path: Utf8PathBuf, new_name: String) -> Result<(), String> {
        self.state.rename_path(path, new_name)
    }

    pub fn delete_path(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        self.state.delete_path(path)
    }

    pub fn preview_pause(&mut self) {
        self.state.preview_pause();
    }

    pub fn preview_stop(&mut self) {
        self.state.preview_stop();
    }

    pub fn preview_rewind_to_zero(&mut self) {
        self.state.preview_rewind_to_zero();
    }

    pub fn preview_seek(&mut self, position_seconds: f64) {
        self.state.preview_seek(position_seconds);
    }

    pub fn tick_preview(&mut self) {
        self.state.tick_preview();
    }

    pub fn tick_preview_clock(&mut self) {
        self.state.tick_preview_clock();
    }

    pub fn render_preview_frame(&mut self) {
        self.state.render_preview_frame();
    }

    pub fn begin_deferred_preview_render(
        &mut self,
    ) -> Option<crate::preview::session::PreviewRenderRequest> {
        self.state.begin_deferred_preview_render()
    }

    pub fn complete_deferred_preview_render(
        &mut self,
        result: crate::preview::session::PreviewRenderResult,
    ) -> bool {
        self.state.complete_deferred_preview_render(result)
    }

    pub fn preview_target_fps(&self) -> u32 {
        self.state.preview_target_fps()
    }

    pub fn project_root(&self) -> Option<String> {
        self.state.project_root()
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.state.set_status(status);
    }

    pub fn read_file_with_version(
        &self,
        path: &Utf8PathBuf,
    ) -> Result<(String, FileVersion), String> {
        self.state.read_file_with_version(path.clone())
    }

    pub fn preview_snapshot(&self) -> crate::preview::session::PreviewSnapshot {
        self.state.preview_snapshot()
    }

    pub fn preview_last_render_timing(&self) -> crate::preview::session::PreviewRenderTiming {
        self.state.preview_last_render_timing()
    }

    pub fn current_analysis(&self) -> Option<dawn_language::analysis::ProjectAnalysis> {
        self.state.current_analysis()
    }

    pub fn preview_pause_at_native_audio(
        &mut self,
        position_seconds: f64,
        status: crate::preview::session::AudioPlaybackStatus,
    ) {
        self.state
            .preview_pause_at_native_audio(position_seconds, status);
    }

    pub fn preview_stop_native_audio(
        &mut self,
        status: crate::preview::session::AudioPlaybackStatus,
    ) {
        self.state.preview_stop_native_audio(status);
    }

    pub fn preview_rewind_native_audio(
        &mut self,
        status: crate::preview::session::AudioPlaybackStatus,
    ) {
        self.state.preview_rewind_native_audio(status);
    }

    pub fn preview_seek_native_audio(
        &mut self,
        position_seconds: f64,
        playing: bool,
        status: crate::preview::session::AudioPlaybackStatus,
    ) {
        self.state
            .preview_seek_native_audio(position_seconds, playing, status);
    }

    pub fn apply_audio_clock_state(
        &mut self,
        position_seconds: f64,
        status: crate::preview::session::AudioPlaybackStatus,
        ended: bool,
        error: Option<&str>,
    ) {
        self.state
            .apply_audio_clock_state(position_seconds, status, ended, error);
    }

    pub fn active_sequence_export_source(
        &self,
    ) -> Result<
        (
            dawn_language::analysis::ProjectAnalysis,
            dawn_language::document::SequenceDocument,
            String,
        ),
        String,
    > {
        self.state.active_sequence_export_source()
    }

    pub fn active_sequence_audio_context(&self) -> Result<(Option<String>, Utf8PathBuf), String> {
        self.state.active_sequence_audio_context()
    }

    pub fn effect_preview_request_source(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
    ) -> Result<
        (
            dawn_language::analysis::ProjectAnalysis,
            dawn_language::document::SequenceDocument,
        ),
        String,
    > {
        self.state.effect_preview_request_source(path, object_key)
    }

    pub(crate) fn read_models(&self) -> &AppReadModels {
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

    pub(crate) fn take_command_failure(&mut self, request_id: RequestId) -> Option<RuntimeError> {
        let index = self
            .command_failures
            .iter()
            .position(|(candidate, _)| *candidate == request_id)?;
        Some(self.command_failures.remove(index).1)
    }

    pub(crate) fn take_buffer_text_update(
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

    pub(crate) fn take_command_completion(&mut self, request_id: RequestId) -> bool {
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

    pub(crate) fn submit_document_store(
        &mut self,
        command: DocumentStoreCommand,
    ) -> RuntimeResult<CommandAck> {
        self.ensure_running(ServiceName::DocumentStore)?;
        let target_revision = document_store_target_revision(&command);
        let request_id = self.next_request_id();
        let result = self.state.submit_document_store(command);
        self.push_service_result(request_id, ServiceName::DocumentStore, result);
        Ok(CommandAck {
            request_id,
            service: ServiceName::DocumentStore,
            target_revision,
        })
    }

    pub(crate) fn submit_project_index(
        &mut self,
        command: ProjectIndexCommand,
    ) -> RuntimeResult<CommandAck> {
        self.ensure_running(ServiceName::ProjectIndex)?;
        let target_revision = match &command {
            ProjectIndexCommand::Analyze {
                source_revision, ..
            } => Some(*source_revision),
        };
        let request_id = self.next_request_id();
        let result = self.project_index.handle(command);
        self.push_service_result(request_id, ServiceName::ProjectIndex, result);
        Ok(CommandAck {
            request_id,
            service: ServiceName::ProjectIndex,
            target_revision,
        })
    }

    pub(crate) fn submit_preview_engine(
        &mut self,
        command: PreviewEngineCommand,
    ) -> RuntimeResult<CommandAck> {
        self.ensure_running(ServiceName::PreviewEngine)?;
        let target_revision = match &command {
            PreviewEngineCommand::QueueRender {
                request_revision, ..
            }
            | PreviewEngineCommand::PublishFrame {
                request_revision, ..
            } => Some(*request_revision),
        };
        let request_id = self.next_request_id();
        let result = self.preview_engine.handle(command);
        self.push_service_result(request_id, ServiceName::PreviewEngine, result);
        Ok(CommandAck {
            request_id,
            service: ServiceName::PreviewEngine,
            target_revision,
        })
    }

    pub(crate) fn submit_audio_engine(
        &mut self,
        command: AudioEngineCommand,
    ) -> RuntimeResult<CommandAck> {
        self.ensure_running(ServiceName::AudioEngine)?;
        let target_revision = match &command {
            AudioEngineCommand::SetReadiness { revision, .. } => Some(*revision),
        };
        let request_id = self.next_request_id();
        let result = self.audio_engine.handle(command);
        self.push_service_result(request_id, ServiceName::AudioEngine, result);
        Ok(CommandAck {
            request_id,
            service: ServiceName::AudioEngine,
            target_revision,
        })
    }

    pub(crate) fn submit_autosave(
        &mut self,
        command: AutosaveCommand,
    ) -> RuntimeResult<CommandAck> {
        self.ensure_running(ServiceName::Autosave)?;
        let target_revision = match &command {
            AutosaveCommand::TagSelfWrite { revision, .. } => Some(*revision),
            AutosaveCommand::CompleteWrite { tag } => Some(tag.revision),
        };
        let request_id = self.next_request_id();
        let result = self.autosave.handle(command);
        self.push_service_result(request_id, ServiceName::Autosave, result);
        Ok(CommandAck {
            request_id,
            service: ServiceName::Autosave,
            target_revision,
        })
    }

    pub(crate) fn submit_file_watcher(
        &mut self,
        command: FileWatcherCommand,
    ) -> RuntimeResult<CommandAck> {
        self.ensure_running(ServiceName::FileWatcher)?;
        let target_revision = match &command {
            FileWatcherCommand::DiskChanged { .. } => None,
        };
        let request_id = self.next_request_id();
        let result = self.file_watcher.handle(command);
        self.push_service_result(request_id, ServiceName::FileWatcher, result);
        Ok(CommandAck {
            request_id,
            service: ServiceName::FileWatcher,
            target_revision,
        })
    }

    pub(crate) fn drain_events(&mut self) -> RuntimeResult<usize> {
        let mut drained = 0;
        while let Some(envelope) = self.events.pop_front() {
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

    pub fn update_active_text(&mut self, text: String) -> Result<(), String> {
        self.state.update_active_text(text)
    }

    pub fn open_project(&mut self, root: String) -> Result<(), String> {
        self.state
            .submit_document_store(DocumentStoreCommand::OpenProject { root })
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn open_buffer(
        &mut self,
        path: Utf8PathBuf,
        text: String,
        disk_version: Option<FileVersion>,
    ) -> Result<(), String> {
        self.state.open_buffer(path, text, disk_version)
    }

    pub fn set_active_buffer(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        self.state.set_active_buffer(path)
    }

    pub fn close_buffer(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        self.state.close_buffer(path)
    }

    pub fn set_active_view_mode(&mut self, mode: EditorViewMode) -> Result<(), String> {
        self.state.set_active_view_mode(mode)
    }

    pub fn undo_active_text(&mut self) -> Result<Option<String>, String> {
        self.state.undo_active_text()
    }

    pub fn redo_active_text(&mut self) -> Result<Option<String>, String> {
        self.state.redo_active_text()
    }

    pub fn shutdown(&mut self) -> RuntimeResult<()> {
        self.stopped = true;
        Ok(())
    }

    fn next_request_id(&mut self) -> RequestId {
        let request_id = RequestId::new(self.next_request_id);
        self.next_request_id = self.next_request_id.saturating_add(1);
        request_id
    }

    fn ensure_running(&self, service: ServiceName) -> RuntimeResult<()> {
        if self.stopped {
            return Err(RuntimeError::new(
                service,
                RuntimeErrorKind::Fatal,
                "coordinator is stopped",
            ));
        }
        Ok(())
    }

    fn push_service_result(
        &mut self,
        request_id: RequestId,
        service: ServiceName,
        result: RuntimeResult<Vec<Event>>,
    ) {
        match result {
            Ok(events) => {
                if events.is_empty() {
                    self.push_event(
                        Some(request_id),
                        service.clone(),
                        Event::CommandCompleted { service },
                    );
                    return;
                }
                for event in events {
                    self.push_event(Some(request_id), service.clone(), event);
                }
            }
            Err(error) => {
                let event = if error.kind == RuntimeErrorKind::Fatal {
                    Event::Fatal {
                        service: error.service,
                        message: error.message,
                    }
                } else {
                    Event::CommandFailed {
                        service: error.service,
                        kind: error.kind,
                        message: error.message,
                    }
                };
                self.push_event(Some(request_id), service, event);
            }
        }
    }

    fn push_event(&mut self, request_id: Option<RequestId>, service: ServiceName, event: Event) {
        let sequence = self.next_event_sequence;
        self.next_event_sequence = self.next_event_sequence.saturating_add(1);
        self.events.push_back(EventEnvelope {
            request_id,
            service,
            sequence,
            created_at: SystemTime::now(),
            event,
        });
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

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::editor::FileVersion;

    #[test]
    fn coordinator_assigns_monotonic_request_ids() {
        let mut coordinator = AppCoordinator::new();

        let first = coordinator
            .submit_document_store(DocumentStoreCommand::OpenProject {
                root: "C:/project".to_string(),
            })
            .expect("first command accepted");
        let second = coordinator
            .submit_document_store(DocumentStoreCommand::OpenProject {
                root: "C:/project-2".to_string(),
            })
            .expect("second command accepted");

        assert_eq!(first.request_id.get(), 1);
        assert_eq!(second.request_id.get(), 2);
        assert_eq!(first.target_revision, None);
        coordinator.shutdown().expect("shutdown joins workers");
    }

    #[test]
    fn coordinator_drains_project_opened_into_read_model() {
        let mut coordinator = AppCoordinator::new();
        coordinator
            .submit_document_store(DocumentStoreCommand::OpenProject {
                root: "C:/project".to_string(),
            })
            .expect("open project accepted");

        drain_until(&mut coordinator, |coordinator| {
            coordinator.read_models().workspace.project_root.as_deref() == Some("C:/project")
        });

        assert_eq!(
            coordinator.read_models().workspace.revision,
            Revision::new(1)
        );
        coordinator.shutdown().expect("shutdown joins workers");
    }

    #[test]
    fn coordinator_drains_buffer_revisions_into_editor_read_model() {
        let path = Utf8PathBuf::from("sequences/example.sequence.dawn");
        let mut coordinator = AppCoordinator::new();
        coordinator
            .submit_document_store(DocumentStoreCommand::OpenProject {
                root: "C:/project".to_string(),
            })
            .expect("open project accepted");
        coordinator
            .submit_document_store(DocumentStoreCommand::OpenBuffer {
                path: path.clone(),
                text: "first".to_string(),
                disk_version: Some(disk_version(5, 1)),
            })
            .expect("open buffer accepted");
        let edit_ack = coordinator
            .submit_document_store(DocumentStoreCommand::UpdateBufferText {
                path: path.clone(),
                expected_revision: Revision::new(2),
                text: "second".to_string(),
            })
            .expect("edit accepted");

        assert_eq!(edit_ack.target_revision, Some(Revision::new(2)));
        drain_until(&mut coordinator, |coordinator| {
            coordinator
                .read_models()
                .editor
                .buffers
                .get(&path)
                .is_some_and(|buffer| buffer.revision == Revision::new(3) && buffer.dirty)
        });

        let buffer = coordinator
            .read_models()
            .editor
            .buffers
            .get(&path)
            .expect("buffer is published");
        assert_eq!(buffer.revision, Revision::new(3));
        assert!(buffer.dirty);
        coordinator.shutdown().expect("shutdown joins workers");
    }

    #[test]
    fn coordinator_seeds_project_buffer_then_publishes_text_edit() {
        let path = Utf8PathBuf::from("sequences/example.sequence.dawn");
        let mut coordinator = AppCoordinator::new();
        coordinator
            .submit_document_store(DocumentStoreCommand::OpenProject {
                root: "C:/project".to_string(),
            })
            .expect("open project accepted");
        drain_until(&mut coordinator, |coordinator| {
            coordinator.read_models().workspace.project_root.as_deref() == Some("C:/project")
        });

        coordinator
            .submit_document_store(DocumentStoreCommand::OpenBuffer {
                path: path.clone(),
                text: "seed".to_string(),
                disk_version: Some(disk_version(4, 1)),
            })
            .expect("open buffer accepted");
        drain_until(&mut coordinator, |coordinator| {
            coordinator
                .read_models()
                .editor
                .buffers
                .get(&path)
                .is_some_and(|buffer| buffer.revision == Revision::new(2) && !buffer.dirty)
        });

        let revision = coordinator
            .read_models()
            .editor
            .buffers
            .get(&path)
            .expect("buffer is seeded")
            .revision;
        coordinator
            .submit_document_store(DocumentStoreCommand::UpdateBufferText {
                path: path.clone(),
                expected_revision: revision,
                text: "edited".to_string(),
            })
            .expect("edit accepted");
        drain_until(&mut coordinator, |coordinator| {
            coordinator
                .read_models()
                .editor
                .buffers
                .get(&path)
                .is_some_and(|buffer| buffer.revision == revision.next() && buffer.dirty)
        });

        assert_eq!(
            coordinator.read_models().editor.active_file.as_ref(),
            Some(&path)
        );
        coordinator.shutdown().expect("shutdown joins workers");
    }

    #[test]
    fn stale_buffer_edits_are_rejected_by_document_store_core() {
        let path = Utf8PathBuf::from("sequences/example.sequence.dawn");
        let mut coordinator = AppCoordinator::new();
        coordinator
            .submit_document_store(DocumentStoreCommand::OpenBuffer {
                path: path.clone(),
                text: "first".to_string(),
                disk_version: Some(disk_version(5, 1)),
            })
            .expect("open buffer accepted");
        drain_until(&mut coordinator, |coordinator| {
            coordinator.read_models().editor.buffers.contains_key(&path)
        });

        let ack = coordinator
            .submit_document_store(DocumentStoreCommand::UpdateBufferText {
                path,
                expected_revision: Revision::INITIAL,
                text: "second".to_string(),
            })
            .expect("stale edit reaches service runner");

        assert_eq!(ack.target_revision, Some(Revision::INITIAL));
        drain_until(&mut coordinator, |coordinator| {
            coordinator.read_models().status.fatal_error.is_some()
        });

        assert_eq!(
            coordinator.read_models().status.fatal_error.as_deref(),
            Some("DocumentStore: stale revision: expected 0, current 1")
        );
        coordinator.shutdown().expect("shutdown joins workers");
    }

    #[test]
    fn service_errors_are_reflected_as_fatal_status() {
        let mut coordinator = AppCoordinator::new();
        coordinator
            .submit_document_store(DocumentStoreCommand::SetActiveBuffer {
                path: Utf8PathBuf::from("missing.sequence.dawn"),
            })
            .expect("invalid command still reaches service runner");

        drain_until(&mut coordinator, |coordinator| {
            coordinator.read_models().status.fatal_error.is_some()
        });

        let fatal = coordinator
            .read_models()
            .status
            .fatal_error
            .as_deref()
            .expect("fatal status is published");
        assert!(fatal.starts_with("DocumentStore: buffer not open:"));
        coordinator.shutdown().expect("shutdown joins workers");
    }

    #[test]
    fn coordinator_shutdown_joins_service_workers() {
        let mut coordinator = AppCoordinator::new();
        coordinator.shutdown().expect("shutdown joins workers");

        let error = coordinator
            .submit_document_store(DocumentStoreCommand::OpenProject {
                root: "C:/project".to_string(),
            })
            .expect_err("stopped coordinator rejects commands");

        assert_eq!(error.kind, RuntimeErrorKind::Fatal);
        assert_eq!(error.service, ServiceName::DocumentStore);
    }

    fn drain_until(coordinator: &mut AppCoordinator, done: impl Fn(&AppCoordinator) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            coordinator.drain_events().expect("events drain");
            if done(coordinator) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        coordinator.drain_events().expect("events drain");
        assert!(
            done(coordinator),
            "condition was not reached before timeout"
        );
    }

    fn disk_version(len: u64, content_hash: u64) -> FileVersion {
        FileVersion {
            len,
            modified_millis: None,
            content_hash,
        }
    }
}
