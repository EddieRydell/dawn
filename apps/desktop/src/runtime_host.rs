use std::thread;
use std::time::{Duration, Instant};

use dawn_app_runtime::app_model::AppRuntimeModel;
use dawn_app_runtime::contracts::{CommandAck, DiskVersion, RequestId};
use dawn_app_runtime::coordinator::AppCoordinator;
use dawn_app_runtime::dto::{
    AppSnapshotDto, SequenceSelectionEditDto, SequenceSelectionEditResultDto,
};
use dawn_app_runtime::layout_persistence::WindowLayout;
use dawn_app_runtime::read_model::AppReadModels;
use dawn_app_runtime::services::document_store::{DocumentStoreCommand, ViewMode};
use dawn_app_runtime::services::live_output::LiveOutputReadout;
use dawn_project::path::Utf8PathBuf;

const RUNTIME_DOCUMENT_TIMEOUT: Duration = Duration::from_millis(500);
const RUNTIME_DOCUMENT_POLL_INTERVAL: Duration = Duration::from_millis(5);

pub(crate) struct BufferTextEdit {
    pub(crate) project_root: Option<String>,
    pub(crate) path: Utf8PathBuf,
    pub(crate) text: String,
    pub(crate) conflicted: bool,
}

#[derive(Default)]
pub(crate) struct RuntimeHost {
    coordinator: AppCoordinator,
}

impl RuntimeHost {
    pub(crate) fn runtime_model(&self) -> &AppRuntimeModel {
        self.coordinator.runtime_model()
    }

    pub(crate) fn runtime_model_mut(&mut self) -> &mut AppRuntimeModel {
        self.coordinator.runtime_model_mut()
    }

    pub(crate) fn read_models(&self) -> &AppReadModels {
        self.coordinator.read_models()
    }

    pub(crate) fn app_snapshot(&self) -> AppSnapshotDto {
        self.coordinator.app_snapshot()
    }

    pub(crate) fn sync_live_output_readout(&mut self, readout: LiveOutputReadout) {
        self.coordinator.sync_live_output_readout(readout);
    }

    pub(crate) fn live_output_readout(&self) -> LiveOutputReadout {
        self.coordinator.live_output_readout()
    }

    pub(crate) fn last_project_root(&self) -> Option<std::path::PathBuf> {
        self.coordinator.last_project_root()
    }

    pub(crate) fn remember_project_root(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        self.coordinator.remember_project_root(path)
    }

    pub(crate) fn toggle_project_tree(&mut self) -> Result<(), String> {
        self.coordinator.toggle_project_tree()
    }

    pub(crate) fn set_effect_preview_enabled(&mut self, enabled: bool) -> Result<(), String> {
        self.coordinator.set_effect_preview_enabled(enabled)
    }

    pub(crate) fn set_effect_preview_effects(&mut self, ids: Vec<u32>) {
        self.coordinator.set_effect_preview_effects(ids);
    }

    pub(crate) fn effect_preview_enabled(&self) -> bool {
        self.coordinator.effect_preview_enabled()
    }

    pub(crate) fn preview_play(&mut self) -> Result<(), String> {
        self.coordinator.preview_play()
    }

    pub(crate) fn preview_window_should_open(&self) -> bool {
        self.coordinator.preview_window_should_open()
    }

    pub(crate) fn preview_window_layout(&self) -> WindowLayout {
        self.coordinator.preview_window_layout()
    }

    pub(crate) fn main_window_layout(&self) -> WindowLayout {
        self.coordinator.main_window_layout()
    }

    pub(crate) fn set_main_window_layout(&mut self, layout: WindowLayout) -> Result<(), String> {
        self.coordinator.set_main_window_layout(layout)
    }

    pub(crate) fn set_preview_window_layout(&mut self, layout: WindowLayout) -> Result<(), String> {
        self.coordinator.set_preview_window_layout(layout)
    }

    pub(crate) fn set_preview_window_open(&mut self, open: bool) -> Result<(), String> {
        self.coordinator.set_preview_window_open(open)
    }

    pub(crate) fn apply_sequence_selection_edit(
        &mut self,
        edit: SequenceSelectionEditDto,
    ) -> Result<SequenceSelectionEditResultDto, String> {
        self.coordinator.apply_sequence_selection_edit(edit)
    }

    pub(crate) fn drain_events(&mut self) -> Result<usize, String> {
        self.coordinator
            .drain_events()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn update_active_text(
        &mut self,
        active_buffer: BufferTextEdit,
        text: String,
    ) -> Result<(), String> {
        if active_buffer.conflicted {
            return Err("active document has external disk changes".to_string());
        }
        if active_buffer.text == text {
            return Ok(());
        }

        self.ensure_project_open(active_buffer.project_root.as_deref())?;
        self.ensure_buffer_open(&active_buffer)?;

        let revision = self
            .coordinator
            .read_models()
            .editor
            .buffers
            .get(&active_buffer.path)
            .map(|buffer| buffer.revision)
            .ok_or_else(|| format!("runtime buffer is not open: {}", active_buffer.path))?;
        let expected_revision = revision.next();
        let ack = self
            .coordinator
            .submit_document_store(DocumentStoreCommand::UpdateBufferText {
                path: active_buffer.path.clone(),
                expected_revision: revision,
                text,
            })
            .map_err(|error| error.to_string())?;
        self.drain_until_request(ack, |host| {
            host.coordinator
                .read_models()
                .editor
                .buffers
                .get(&active_buffer.path)
                .is_some_and(|buffer| buffer.revision == expected_revision)
        })
    }

    pub(crate) fn open_project(&mut self, root: String) -> Result<(), String> {
        let ack = self
            .coordinator
            .submit_document_store(DocumentStoreCommand::OpenProject { root: root.clone() })
            .map_err(|error| error.to_string())?;
        self.drain_until_request(ack, |host| {
            host.coordinator
                .read_models()
                .workspace
                .project_root
                .as_deref()
                == Some(root.as_str())
        })
    }

    pub(crate) fn open_buffer(
        &mut self,
        path: Utf8PathBuf,
        text: String,
        disk_version: Option<DiskVersion>,
    ) -> Result<(), String> {
        let ack = self
            .coordinator
            .submit_document_store(DocumentStoreCommand::OpenBuffer {
                path: path.clone(),
                text,
                disk_version,
            })
            .map_err(|error| error.to_string())?;
        self.drain_until_request(ack, |host| {
            host.coordinator
                .read_models()
                .editor
                .buffers
                .contains_key(&path)
        })
    }

    pub(crate) fn set_active_buffer(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        let ack = self
            .coordinator
            .submit_document_store(DocumentStoreCommand::SetActiveBuffer { path: path.clone() })
            .map_err(|error| error.to_string())?;
        self.drain_until_request(ack, |host| {
            host.coordinator.read_models().editor.active_file.as_ref() == Some(&path)
        })
    }

    pub(crate) fn close_buffer(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        let previous_active = self.coordinator.read_models().editor.active_file.clone();
        let ack = self
            .coordinator
            .submit_document_store(DocumentStoreCommand::CloseBuffer { path: path.clone() })
            .map_err(|error| error.to_string())?;
        self.drain_until_request(ack, |host| {
            let editor = &host.coordinator.read_models().editor;
            if editor.buffers.contains_key(&path) {
                return false;
            }
            if previous_active.as_ref() == Some(&path) {
                return editor.active_file.as_ref() != Some(&path);
            }
            editor.active_file == previous_active
        })
    }

    pub(crate) fn set_view_mode(
        &mut self,
        path: Utf8PathBuf,
        mode: ViewMode,
    ) -> Result<(), String> {
        let ack = self
            .coordinator
            .submit_document_store(DocumentStoreCommand::SetViewMode {
                path: path.clone(),
                mode,
            })
            .map_err(|error| error.to_string())?;
        self.drain_until_request(ack, |host| {
            host.coordinator
                .read_models()
                .editor
                .buffers
                .get(&path)
                .is_some_and(|buffer| buffer.view_mode == mode)
        })
    }

    pub(crate) fn undo_buffer_text(&mut self, path: Utf8PathBuf) -> Result<Option<String>, String> {
        self.apply_history_edit(path, HistoryCommand::Undo)
    }

    pub(crate) fn redo_buffer_text(&mut self, path: Utf8PathBuf) -> Result<Option<String>, String> {
        self.apply_history_edit(path, HistoryCommand::Redo)
    }

    fn ensure_project_open(&mut self, project_root: Option<&str>) -> Result<(), String> {
        if self
            .coordinator
            .read_models()
            .workspace
            .project_root
            .as_deref()
            == project_root
        {
            return Ok(());
        }
        let Some(project_root) = project_root else {
            return Ok(());
        };
        self.open_project(project_root.to_string())
    }

    fn ensure_buffer_open(&mut self, active_buffer: &BufferTextEdit) -> Result<(), String> {
        if self
            .coordinator
            .read_models()
            .editor
            .buffers
            .contains_key(&active_buffer.path)
        {
            return Ok(());
        }
        self.open_buffer(active_buffer.path.clone(), active_buffer.text.clone(), None)
    }

    fn apply_history_edit(
        &mut self,
        path: Utf8PathBuf,
        command: HistoryCommand,
    ) -> Result<Option<String>, String> {
        let revision = self
            .coordinator
            .read_models()
            .editor
            .buffers
            .get(&path)
            .map(|buffer| buffer.revision)
            .ok_or_else(|| format!("runtime buffer is not open: {path}"))?;
        let ack = match command {
            HistoryCommand::Undo => {
                self.coordinator
                    .submit_document_store(DocumentStoreCommand::UndoBufferText {
                        path: path.clone(),
                        expected_revision: revision,
                    })
            }
            HistoryCommand::Redo => {
                self.coordinator
                    .submit_document_store(DocumentStoreCommand::RedoBufferText {
                        path: path.clone(),
                        expected_revision: revision,
                    })
            }
        }
        .map_err(|error| error.to_string())?;
        let request_id = ack.request_id;
        self.drain_until_request(ack, |host| {
            host.coordinator
                .read_models()
                .editor
                .buffers
                .get(&path)
                .is_some_and(|buffer| buffer.revision == revision.next())
                || host.has_request_completion(request_id)
        })?;
        Ok(self
            .coordinator
            .take_buffer_text_update(request_id)
            .map(|(_, text, _)| text))
    }

    fn drain_until_request(
        &mut self,
        ack: CommandAck,
        done: impl Fn(&mut Self) -> bool,
    ) -> Result<(), String> {
        let deadline = Instant::now() + RUNTIME_DOCUMENT_TIMEOUT;
        loop {
            self.drain_events()?;
            if let Some(error) = self.coordinator.take_command_failure(ack.request_id) {
                return Err(error.to_string());
            }
            if let Some(error) = self.coordinator.read_models().status.fatal_error.as_ref() {
                return Err(error.clone());
            }
            if done(self) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("runtime timed out waiting for document store update".to_string());
            }
            thread::sleep(RUNTIME_DOCUMENT_POLL_INTERVAL);
        }
    }

    fn has_request_completion(&mut self, request_id: RequestId) -> bool {
        self.coordinator.take_command_completion(request_id)
    }
}

#[derive(Debug, Clone, Copy)]
enum HistoryCommand {
    Undo,
    Redo,
}

impl Drop for RuntimeHost {
    fn drop(&mut self) {
        let _ = self.coordinator.shutdown();
    }
}
