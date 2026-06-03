use std::collections::BTreeMap;

use dawn_project::path::Utf8PathBuf;

use crate::contracts::{
    Event, Revision, RuntimeError, RuntimeErrorKind, RuntimeResult, ServiceName,
};
use crate::runtime::ServiceCore;

pub use crate::contracts::ViewMode;

#[derive(Debug, Clone)]
pub struct BufferState {
    pub path: Utf8PathBuf,
    pub text: String,
    pub saved_text: String,
    pub revision: Revision,
    pub disk_revision: Revision,
    pub view_mode: ViewMode,
    pub conflicted: bool,
    undo_stack: Vec<String>,
    redo_stack: Vec<String>,
}

impl BufferState {
    pub fn dirty(&self) -> bool {
        self.text != self.saved_text
    }

    fn record_snapshot(&mut self) {
        if self.undo_stack.last() == Some(&self.text) {
            return;
        }
        self.undo_stack.push(self.text.clone());
        self.redo_stack.clear();
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeSessionBuffer {
    pub path: Utf8PathBuf,
    pub text: String,
    pub view_mode: ViewMode,
}

#[derive(Debug, Clone)]
pub enum DocumentStoreCommand {
    OpenProject {
        root: String,
    },
    OpenSession {
        root: String,
        buffers: Vec<RuntimeSessionBuffer>,
        active_file: Option<Utf8PathBuf>,
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
    CloseBuffer {
        path: Utf8PathBuf,
    },
    UndoBufferText {
        path: Utf8PathBuf,
        expected_revision: Revision,
    },
    RedoBufferText {
        path: Utf8PathBuf,
        expected_revision: Revision,
    },
}

#[derive(Debug, Default, Clone)]
pub struct DocumentStoreCore {
    project_root: Option<String>,
    buffers: BTreeMap<Utf8PathBuf, BufferState>,
    tab_order: Vec<Utf8PathBuf>,
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
                self.tab_order.clear();
                self.active_file = None;
                self.revision = self.revision.next();
                Ok(vec![Event::ProjectOpened {
                    root,
                    revision: self.revision,
                }])
            }
            DocumentStoreCommand::OpenSession {
                root,
                buffers,
                active_file,
            } => {
                self.project_root = Some(root.clone());
                self.buffers.clear();
                self.tab_order.clear();
                self.active_file = None;
                self.revision = self.revision.next();
                let mut events = vec![Event::ProjectOpened {
                    root,
                    revision: self.revision,
                }];

                for session_buffer in buffers {
                    self.revision = self.revision.next();
                    let revision = self.revision;
                    let path = session_buffer.path;
                    self.tab_order.push(path.clone());
                    self.buffers.insert(
                        path.clone(),
                        BufferState {
                            path: path.clone(),
                            saved_text: session_buffer.text.clone(),
                            text: session_buffer.text,
                            revision,
                            disk_revision: Revision::INITIAL,
                            view_mode: session_buffer.view_mode,
                            conflicted: false,
                            undo_stack: Vec::new(),
                            redo_stack: Vec::new(),
                        },
                    );
                    self.active_file = Some(path.clone());
                    events.push(Event::BufferOpened {
                        path: path.clone(),
                        revision,
                    });
                    if session_buffer.view_mode != ViewMode::Text {
                        self.revision = self.revision.next();
                        events.push(Event::BufferViewModeChanged {
                            path,
                            mode: session_buffer.view_mode,
                            revision: self.revision,
                        });
                    }
                }

                let next_active = active_file
                    .filter(|path| self.buffers.contains_key(path))
                    .or_else(|| self.tab_order.last().cloned());
                if let Some(next_active) = next_active {
                    if self.active_file.as_ref() != Some(&next_active) {
                        self.active_file = Some(next_active.clone());
                        self.revision = self.revision.next();
                        events.push(Event::ActiveBufferChanged {
                            path: next_active,
                            revision: self.revision,
                        });
                    }
                }
                Ok(events)
            }
            DocumentStoreCommand::OpenBuffer {
                path,
                text,
                disk_revision,
            } => {
                let revision = self.revision.next();
                self.revision = revision;
                if !self.buffers.contains_key(&path) {
                    self.tab_order.push(path.clone());
                }
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
                        undo_stack: Vec::new(),
                        redo_stack: Vec::new(),
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
                if buffer.text == text {
                    return Ok(Vec::new());
                }
                buffer.record_snapshot();
                buffer.text = text;
                buffer.revision = expected_revision.next();
                let event = Event::BufferUpdated {
                    path: path.clone(),
                    revision: buffer.revision,
                    dirty: buffer.dirty(),
                };
                Ok(vec![
                    event,
                    Event::BufferTextUpdated {
                        path,
                        revision: buffer.revision,
                        text: buffer.text.clone(),
                    },
                ])
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
                self.active_file = Some(path.clone());
                self.revision = self.revision.next();
                Ok(vec![Event::ActiveBufferChanged {
                    path,
                    revision: self.revision,
                }])
            }
            DocumentStoreCommand::SetViewMode { path, mode } => {
                let buffer = self.buffer_mut(&path)?;
                buffer.view_mode = mode;
                self.revision = self.revision.next();
                Ok(vec![Event::BufferViewModeChanged {
                    path,
                    mode,
                    revision: self.revision,
                }])
            }
            DocumentStoreCommand::CloseBuffer { path } => {
                if self.buffers.remove(&path).is_none() {
                    return Err(RuntimeError::new(
                        ServiceName::DocumentStore,
                        RuntimeErrorKind::NotFound,
                        format!("buffer not open: {path}"),
                    ));
                }
                self.tab_order.retain(|candidate| candidate != &path);
                if self.active_file.as_ref() == Some(&path) {
                    self.active_file = self.tab_order.last().cloned();
                }
                self.revision = self.revision.next();
                Ok(vec![Event::BufferClosed {
                    path,
                    active_file: self.active_file.clone(),
                    revision: self.revision,
                }])
            }
            DocumentStoreCommand::UndoBufferText {
                path,
                expected_revision,
            } => self.apply_history_edit(path, expected_revision, HistoryDirection::Undo),
            DocumentStoreCommand::RedoBufferText {
                path,
                expected_revision,
            } => self.apply_history_edit(path, expected_revision, HistoryDirection::Redo),
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

    fn apply_history_edit(
        &mut self,
        path: Utf8PathBuf,
        expected_revision: Revision,
        direction: HistoryDirection,
    ) -> RuntimeResult<Vec<Event>> {
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
        let next_text = match direction {
            HistoryDirection::Undo => {
                let Some(previous) = buffer.undo_stack.pop() else {
                    return Ok(Vec::new());
                };
                buffer.redo_stack.push(buffer.text.clone());
                previous
            }
            HistoryDirection::Redo => {
                let Some(next) = buffer.redo_stack.pop() else {
                    return Ok(Vec::new());
                };
                buffer.undo_stack.push(buffer.text.clone());
                next
            }
        };
        buffer.text = next_text;
        buffer.revision = expected_revision.next();
        Ok(vec![
            Event::BufferUpdated {
                path: path.clone(),
                revision: buffer.revision,
                dirty: buffer.dirty(),
            },
            Event::BufferTextUpdated {
                path,
                revision: buffer.revision,
                text: buffer.text.clone(),
            },
        ])
    }
}

#[derive(Debug, Clone, Copy)]
enum HistoryDirection {
    Undo,
    Redo,
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
