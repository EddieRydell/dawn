use std::collections::BTreeMap;

use dawn_project::path::Utf8PathBuf;

use crate::contracts::{
    Event, Revision, RuntimeError, RuntimeErrorKind, RuntimeResult, ServiceName,
};
use crate::runtime::ServiceCore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewMode {
    Text,
    Gui,
}

#[derive(Debug, Clone)]
pub struct BufferState {
    pub path: Utf8PathBuf,
    pub text: String,
    pub saved_text: String,
    pub revision: Revision,
    pub disk_revision: Revision,
    pub view_mode: ViewMode,
    pub conflicted: bool,
}

impl BufferState {
    pub fn dirty(&self) -> bool {
        self.text != self.saved_text
    }
}

#[derive(Debug, Clone)]
pub enum DocumentStoreCommand {
    OpenProject {
        root: String,
    },
    OpenBuffer {
        path: Utf8PathBuf,
        text: String,
        disk_revision: Revision,
    },
    UpdateBufferText {
        path: Utf8PathBuf,
        expected_revision: Revision,
        text: String,
    },
    MarkSaved {
        path: Utf8PathBuf,
        expected_revision: Revision,
        disk_revision: Revision,
    },
    ExternalDiskChanged {
        path: Utf8PathBuf,
        disk_revision: Revision,
        text: String,
    },
    SetActiveBuffer {
        path: Utf8PathBuf,
    },
    SetViewMode {
        path: Utf8PathBuf,
        mode: ViewMode,
    },
}

#[derive(Debug, Default, Clone)]
pub struct DocumentStoreCore {
    project_root: Option<String>,
    buffers: BTreeMap<Utf8PathBuf, BufferState>,
    active_file: Option<Utf8PathBuf>,
    revision: Revision,
}

impl DocumentStoreCore {
    pub fn buffer(&self, path: &Utf8PathBuf) -> Option<&BufferState> {
        self.buffers.get(path)
    }

    pub fn buffers(&self) -> impl Iterator<Item = &BufferState> {
        self.buffers.values()
    }

    pub fn active_file(&self) -> Option<&Utf8PathBuf> {
        self.active_file.as_ref()
    }

    pub fn handle(&mut self, command: DocumentStoreCommand) -> RuntimeResult<Vec<Event>> {
        match command {
            DocumentStoreCommand::OpenProject { root } => {
                self.project_root = Some(root.clone());
                self.buffers.clear();
                self.active_file = None;
                self.revision = self.revision.next();
                Ok(vec![Event::ProjectOpened {
                    root,
                    revision: self.revision,
                }])
            }
            DocumentStoreCommand::OpenBuffer {
                path,
                text,
                disk_revision,
            } => {
                let revision = self.revision.next();
                self.revision = revision;
                self.buffers.insert(
                    path.clone(),
                    BufferState {
                        path: path.clone(),
                        saved_text: text.clone(),
                        text,
                        revision,
                        disk_revision,
                        view_mode: ViewMode::Text,
                        conflicted: false,
                    },
                );
                self.active_file = Some(path.clone());
                Ok(vec![Event::BufferOpened { path, revision }])
            }
            DocumentStoreCommand::UpdateBufferText {
                path,
                expected_revision,
                text,
            } => {
                let buffer = self.buffer_mut(&path)?;
                if buffer.conflicted {
                    return Err(RuntimeError::new(
                        ServiceName::DocumentStore,
                        RuntimeErrorKind::Conflict,
                        format!("buffer has external disk changes: {path}"),
                    ));
                }
                if buffer.revision != expected_revision {
                    return Err(RuntimeError::stale(
                        ServiceName::DocumentStore,
                        expected_revision,
                        buffer.revision,
                    ));
                }
                buffer.text = text;
                buffer.revision = expected_revision.next();
                let event = Event::BufferUpdated {
                    path,
                    revision: buffer.revision,
                    dirty: buffer.dirty(),
                };
                Ok(vec![event])
            }
            DocumentStoreCommand::MarkSaved {
                path,
                expected_revision,
                disk_revision,
            } => {
                let buffer = self.buffer_mut(&path)?;
                if buffer.revision != expected_revision {
                    return Err(RuntimeError::stale(
                        ServiceName::DocumentStore,
                        expected_revision,
                        buffer.revision,
                    ));
                }
                buffer.saved_text = buffer.text.clone();
                buffer.disk_revision = disk_revision;
                buffer.conflicted = false;
                Ok(vec![Event::BufferUpdated {
                    path,
                    revision: buffer.revision,
                    dirty: false,
                }])
            }
            DocumentStoreCommand::ExternalDiskChanged {
                path,
                disk_revision,
                text,
            } => {
                let buffer = self.buffer_mut(&path)?;
                if buffer.dirty() {
                    let clean_revision = buffer.revision;
                    buffer.conflicted = true;
                    buffer.disk_revision = disk_revision;
                    return Ok(vec![Event::BufferConflict {
                        path,
                        clean_revision,
                        disk_revision,
                    }]);
                }
                buffer.text = text.clone();
                buffer.saved_text = text;
                buffer.disk_revision = disk_revision;
                buffer.revision = buffer.revision.next();
                Ok(vec![Event::BufferUpdated {
                    path,
                    revision: buffer.revision,
                    dirty: false,
                }])
            }
            DocumentStoreCommand::SetActiveBuffer { path } => {
                if !self.buffers.contains_key(&path) {
                    return Err(RuntimeError::new(
                        ServiceName::DocumentStore,
                        RuntimeErrorKind::NotFound,
                        format!("buffer not open: {path}"),
                    ));
                }
                self.active_file = Some(path);
                Ok(Vec::new())
            }
            DocumentStoreCommand::SetViewMode { path, mode } => {
                let buffer = self.buffer_mut(&path)?;
                buffer.view_mode = mode;
                Ok(Vec::new())
            }
        }
    }

    fn buffer_mut(&mut self, path: &Utf8PathBuf) -> RuntimeResult<&mut BufferState> {
        self.buffers.get_mut(path).ok_or_else(|| {
            RuntimeError::new(
                ServiceName::DocumentStore,
                RuntimeErrorKind::NotFound,
                format!("buffer not open: {path}"),
            )
        })
    }
}

impl ServiceCore for DocumentStoreCore {
    type Command = DocumentStoreCommand;

    fn service_name(&self) -> ServiceName {
        ServiceName::DocumentStore
    }

    fn revision(&self) -> Revision {
        self.revision
    }

    fn handle(&mut self, command: Self::Command) -> RuntimeResult<Vec<Event>> {
        DocumentStoreCore::handle(self, command)
    }
}
