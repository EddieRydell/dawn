use std::collections::BTreeMap;

use dawn_project::path::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::contracts::{
    Event, EventEnvelope, Revision, RuntimeError, RuntimeResult, ServiceName, TaskRecord,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceModel {
    pub project_root: Option<String>,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorBufferModel {
    pub path: Utf8PathBuf,
    pub revision: Revision,
    pub dirty: bool,
    pub conflicted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorModel {
    pub buffers: BTreeMap<Utf8PathBuf, EditorBufferModel>,
    pub active_file: Option<Utf8PathBuf>,
    pub revision: Revision,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsModel {
    pub analysis_revision: Revision,
    pub diagnostic_count: usize,
    pub updating: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewModel {
    pub source: Option<String>,
    pub request_revision: Revision,
    pub frame_revision: Revision,
    pub updating: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportAudioModel {
    pub ready: bool,
    pub revision: Revision,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveOutputModel {
    pub enabled: bool,
    pub frame_revision: Revision,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusModel {
    pub tasks: Vec<TaskRecord>,
    pub fatal_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefsWindowLayoutModel {
    pub project_tree_visible: bool,
    pub preview_window_open: bool,
    pub revision: Revision,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppReadModels {
    pub workspace: WorkspaceModel,
    pub editor: EditorModel,
    pub diagnostics: DiagnosticsModel,
    pub preview: PreviewModel,
    pub transport_audio: TransportAudioModel,
    pub live_output: LiveOutputModel,
    pub status: StatusModel,
    pub prefs_window_layout: PrefsWindowLayoutModel,
}

#[derive(Debug, Clone, Default)]
pub struct ReadModelCore {
    models: AppReadModels,
}

impl ReadModelCore {
    pub fn models(&self) -> &AppReadModels {
        &self.models
    }

    pub fn apply(&mut self, envelope: &EventEnvelope) -> RuntimeResult<()> {
        match &envelope.event {
            Event::ProjectOpened { root, revision } => {
                self.models.workspace.project_root = Some(root.clone());
                self.models.workspace.revision = *revision;
                self.models.editor.buffers.clear();
                self.models.editor.active_file = None;
                self.models.editor.revision = revision.next();
                self.models.diagnostics.stale = true;
                self.models.preview.stale = true;
            }
            Event::BufferOpened { path, revision } => {
                self.models.editor.buffers.insert(
                    path.clone(),
                    EditorBufferModel {
                        path: path.clone(),
                        revision: *revision,
                        dirty: false,
                        conflicted: false,
                    },
                );
                self.models.editor.active_file = Some(path.clone());
                self.models.editor.revision = self.models.editor.revision.next();
            }
            Event::BufferUpdated {
                path,
                revision,
                dirty,
            } => {
                let Some(buffer) = self.models.editor.buffers.get_mut(path) else {
                    return Err(RuntimeError::new(
                        ServiceName::ReadModel,
                        crate::contracts::RuntimeErrorKind::NotFound,
                        format!("buffer not open: {path}"),
                    ));
                };
                buffer.revision = *revision;
                buffer.dirty = *dirty;
                self.models.editor.revision = self.models.editor.revision.next();
                self.models.diagnostics.stale = true;
                self.models.preview.stale = true;
            }
            Event::BufferConflict {
                path,
                clean_revision,
                ..
            } => {
                let Some(buffer) = self.models.editor.buffers.get_mut(path) else {
                    return Err(RuntimeError::new(
                        ServiceName::ReadModel,
                        crate::contracts::RuntimeErrorKind::NotFound,
                        format!("buffer not open: {path}"),
                    ));
                };
                buffer.revision = *clean_revision;
                buffer.conflicted = true;
            }
            Event::AnalysisUpdated {
                revision,
                diagnostic_count,
            } => {
                self.models.diagnostics.analysis_revision = *revision;
                self.models.diagnostics.diagnostic_count = *diagnostic_count;
                self.models.diagnostics.updating = false;
                self.models.diagnostics.stale = false;
            }
            Event::PreviewQueued {
                sequence,
                request_revision,
            } => {
                self.models.preview.source =
                    Some(format!("{}::{}", sequence.path, sequence.object_key));
                self.models.preview.request_revision = *request_revision;
                self.models.preview.updating = true;
                self.models.preview.stale = true;
            }
            Event::PreviewFramePublished {
                sequence,
                request_revision,
                frame_revision,
            } => {
                self.models.preview.source =
                    Some(format!("{}::{}", sequence.path, sequence.object_key));
                self.models.preview.request_revision = *request_revision;
                self.models.preview.frame_revision = *frame_revision;
                self.models.preview.updating = false;
                self.models.preview.stale = false;
                self.models.live_output.frame_revision = *frame_revision;
            }
            Event::AudioReadinessChanged {
                revision, ready, ..
            } => {
                self.models.transport_audio.ready = *ready;
                self.models.transport_audio.revision = *revision;
            }
            Event::AutosaveTagged { .. } => {}
            Event::TaskChanged(task) => {
                if let Some(existing) = self
                    .models
                    .status
                    .tasks
                    .iter_mut()
                    .find(|candidate| candidate.request_id == task.request_id)
                {
                    *existing = task.clone();
                } else {
                    self.models.status.tasks.push(task.clone());
                }
            }
            Event::Fatal { service, message } => {
                self.models.status.fatal_error = Some(format!("{service:?}: {message}"));
            }
        }
        Ok(())
    }
}
