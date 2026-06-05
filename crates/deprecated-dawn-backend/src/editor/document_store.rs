use std::collections::BTreeMap;

use dawn_language::analysis::ProjectOverlay;
use dawn_language::path::{moved_path, path_affects, path_matches_any, Utf8PathBuf};

use crate::editor::{BufferExternalState, BufferTab, EditorViewMode, FileVersion};
use crate::runtime::contracts::{
    Event, Revision, RuntimeError, RuntimeErrorKind, RuntimeResult, ServiceName,
};

#[derive(Debug, Clone)]
pub struct BufferState {
    pub path: Utf8PathBuf,
    pub text: String,
    pub saved_text: String,
    pub revision: Revision,
    pub disk_version: Option<FileVersion>,
    pub external_state: BufferExternalState,
    pub view_mode: EditorViewMode,
    undo_stack: Vec<String>,
    redo_stack: Vec<String>,
}

impl BufferState {
    pub fn dirty(&self) -> bool {
        self.text != self.saved_text
    }

    pub fn is_conflicted(&self) -> bool {
        self.external_state != BufferExternalState::Current
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
    pub disk_version: Option<FileVersion>,
    pub view_mode: EditorViewMode,
}

#[derive(Debug, Clone)]
pub(crate) enum ExternalDiskSnapshot {
    Present {
        text: String,
        disk_version: FileVersion,
    },
    Missing,
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
        disk_version: Option<FileVersion>,
    },
    UpdateBufferText {
        path: Utf8PathBuf,
        expected_revision: Revision,
        text: String,
    },
    MarkSaved {
        path: Utf8PathBuf,
        expected_revision: Revision,
        disk_version: FileVersion,
    },
    ExternalDiskChanged {
        path: Utf8PathBuf,
        disk_version: FileVersion,
        text: String,
    },
    ExternalDiskDeleted {
        path: Utf8PathBuf,
    },
    ReloadBufferFromDisk {
        path: Utf8PathBuf,
        text: String,
        disk_version: FileVersion,
    },
    ReconcileMovedPath {
        old_path: Utf8PathBuf,
        new_path: Utf8PathBuf,
    },
    ReconcileDeletedPath {
        path: Utf8PathBuf,
    },
    SetActiveBuffer {
        path: Utf8PathBuf,
    },
    SetViewMode {
        path: Utf8PathBuf,
        mode: EditorViewMode,
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

    pub fn active_file(&self) -> Option<&Utf8PathBuf> {
        self.active_file.as_ref()
    }

    pub fn active_buffer(&self) -> Option<&BufferState> {
        self.active_file
            .as_ref()
            .and_then(|path| self.buffers.get(path))
    }

    pub fn active_path(&self) -> Result<Utf8PathBuf, String> {
        self.active_file()
            .cloned()
            .ok_or_else(|| "no active document".to_string())
    }

    pub fn active_text(&self) -> Result<String, String> {
        self.active_buffer()
            .map(|buffer| buffer.text.clone())
            .ok_or_else(|| "no active document".to_string())
    }

    pub fn ensure_active_buffer_not_conflicted(&self) -> Result<(), String> {
        let Some(buffer) = self.active_buffer() else {
            return Ok(());
        };
        if buffer.is_conflicted() {
            return Err("active document has external disk changes".to_string());
        }
        Ok(())
    }

    pub fn tabs(&self) -> Vec<BufferTab> {
        self.tab_order
            .iter()
            .filter_map(|path| self.buffers.get(path).map(BufferTab::from))
            .collect()
    }

    pub fn active_tab(&self) -> Option<BufferTab> {
        self.active_buffer().map(BufferTab::from)
    }

    pub fn buffer_tabs(&self) -> Vec<BufferTab> {
        self.buffers.values().map(BufferTab::from).collect()
    }

    pub(crate) fn buffer_paths_for_external_changes(
        &self,
        changed_paths: Vec<Utf8PathBuf>,
    ) -> Vec<Utf8PathBuf> {
        let watched_paths = if changed_paths.is_empty() {
            self.buffers.keys().cloned().collect()
        } else {
            changed_paths
        };
        self.buffers
            .keys()
            .filter(|path| path_matches_any(path, &watched_paths))
            .cloned()
            .collect()
    }

    pub(crate) fn external_disk_command_for(
        &self,
        path: Utf8PathBuf,
        disk: ExternalDiskSnapshot,
    ) -> Option<DocumentStoreCommand> {
        let buffer = self.buffer(&path)?;
        match disk {
            ExternalDiskSnapshot::Present { text, disk_version } => {
                if buffer.disk_version.as_ref() == Some(&disk_version) {
                    None
                } else {
                    Some(DocumentStoreCommand::ExternalDiskChanged {
                        path,
                        disk_version,
                        text,
                    })
                }
            }
            ExternalDiskSnapshot::Missing => {
                Some(DocumentStoreCommand::ExternalDiskDeleted { path })
            }
        }
    }

    pub(crate) fn active_reload_from_disk_command(
        &self,
        disk: ExternalDiskSnapshot,
    ) -> Option<DocumentStoreCommand> {
        let path = self.active_file.clone()?;
        match disk {
            ExternalDiskSnapshot::Present { text, disk_version } => {
                Some(DocumentStoreCommand::ReloadBufferFromDisk {
                    path,
                    text,
                    disk_version,
                })
            }
            ExternalDiskSnapshot::Missing => Some(DocumentStoreCommand::CloseBuffer { path }),
        }
    }

    pub(crate) fn active_mark_saved_command(
        &self,
        disk_version: FileVersion,
    ) -> Option<DocumentStoreCommand> {
        let buffer = self.active_buffer()?;
        Some(DocumentStoreCommand::MarkSaved {
            path: buffer.path.clone(),
            expected_revision: buffer.revision,
            disk_version,
        })
    }

    pub fn dirty_overlays(&self) -> Vec<ProjectOverlay> {
        self.buffers
            .values()
            .filter(|buffer| buffer.dirty())
            .map(|buffer| ProjectOverlay {
                path: buffer.path.clone(),
                content: buffer.text.clone(),
            })
            .collect()
    }

    pub fn dirty_autosave_buffers(&self) -> Vec<BufferTab> {
        self.buffers
            .values()
            .filter(|buffer| buffer.dirty() && !buffer.is_conflicted())
            .map(BufferTab::from)
            .collect()
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
                            disk_version: session_buffer.disk_version,
                            external_state: BufferExternalState::Current,
                            view_mode: session_buffer.view_mode,
                            undo_stack: Vec::new(),
                            redo_stack: Vec::new(),
                        },
                    );
                    self.active_file = Some(path.clone());
                    events.push(Event::BufferOpened {
                        path: path.clone(),
                        revision,
                        text: self.buffers[&path].text.clone(),
                        disk_version: self.buffers[&path].disk_version.clone(),
                        external_state: self.buffers[&path].external_state,
                        view_mode: self.buffers[&path].view_mode,
                    });
                    if session_buffer.view_mode != EditorViewMode::Text {
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
                disk_version,
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
                        disk_version,
                        external_state: BufferExternalState::Current,
                        view_mode: EditorViewMode::Text,
                        undo_stack: Vec::new(),
                        redo_stack: Vec::new(),
                    },
                );
                self.active_file = Some(path.clone());
                let buffer = &self.buffers[&path];
                Ok(vec![Event::BufferOpened {
                    path: path.clone(),
                    revision,
                    text: buffer.text.clone(),
                    disk_version: buffer.disk_version.clone(),
                    external_state: buffer.external_state,
                    view_mode: buffer.view_mode,
                }])
            }
            DocumentStoreCommand::UpdateBufferText {
                path,
                expected_revision,
                text,
            } => {
                let buffer = self.buffer_mut(&path)?;
                if buffer.external_state != BufferExternalState::Current {
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
                    disk_version: buffer.disk_version.clone(),
                    external_state: buffer.external_state,
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
                disk_version,
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
                buffer.disk_version = Some(disk_version);
                buffer.external_state = BufferExternalState::Current;
                Ok(vec![Event::BufferUpdated {
                    path,
                    revision: buffer.revision,
                    dirty: false,
                    disk_version: buffer.disk_version.clone(),
                    external_state: buffer.external_state,
                }])
            }
            DocumentStoreCommand::ExternalDiskChanged {
                path,
                disk_version,
                text,
            } => {
                let buffer = self.buffer_mut(&path)?;
                if buffer.dirty() {
                    let clean_revision = buffer.revision;
                    buffer.external_state = BufferExternalState::ChangedOnDisk;
                    buffer.disk_version = Some(disk_version);
                    return Ok(vec![Event::BufferConflict {
                        path,
                        clean_revision,
                        disk_version: buffer.disk_version.clone(),
                        external_state: buffer.external_state,
                    }]);
                }
                buffer.text = text.clone();
                buffer.saved_text = text;
                buffer.disk_version = Some(disk_version);
                buffer.external_state = BufferExternalState::Current;
                buffer.revision = buffer.revision.next();
                Ok(vec![
                    Event::BufferUpdated {
                        path: path.clone(),
                        revision: buffer.revision,
                        dirty: false,
                        disk_version: buffer.disk_version.clone(),
                        external_state: buffer.external_state,
                    },
                    Event::BufferTextUpdated {
                        path,
                        revision: buffer.revision,
                        text: buffer.text.clone(),
                    },
                ])
            }
            DocumentStoreCommand::ExternalDiskDeleted { path } => {
                let buffer = self.buffer_mut(&path)?;
                if buffer.dirty() {
                    let clean_revision = buffer.revision;
                    buffer.external_state = BufferExternalState::DeletedOnDisk;
                    buffer.disk_version = None;
                    return Ok(vec![Event::BufferConflict {
                        path,
                        clean_revision,
                        disk_version: None,
                        external_state: buffer.external_state,
                    }]);
                }
                self.close_buffer_events(path)
            }
            DocumentStoreCommand::ReloadBufferFromDisk {
                path,
                text,
                disk_version,
            } => {
                let buffer = self.buffer_mut(&path)?;
                buffer.record_snapshot();
                buffer.text = text.clone();
                buffer.saved_text = text;
                buffer.disk_version = Some(disk_version);
                buffer.external_state = BufferExternalState::Current;
                buffer.revision = buffer.revision.next();
                Ok(vec![
                    Event::BufferUpdated {
                        path: path.clone(),
                        revision: buffer.revision,
                        dirty: false,
                        disk_version: buffer.disk_version.clone(),
                        external_state: buffer.external_state,
                    },
                    Event::BufferTextUpdated {
                        path,
                        revision: buffer.revision,
                        text: buffer.text.clone(),
                    },
                ])
            }
            DocumentStoreCommand::ReconcileMovedPath { old_path, new_path } => {
                Ok(self.reconcile_moved_path_events(old_path, new_path))
            }
            DocumentStoreCommand::ReconcileDeletedPath { path } => {
                Ok(self.reconcile_deleted_path_events(path))
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
                if !self.buffers.contains_key(&path) {
                    return Err(RuntimeError::new(
                        ServiceName::DocumentStore,
                        RuntimeErrorKind::NotFound,
                        format!("buffer not open: {path}"),
                    ));
                }
                self.close_buffer_events(path)
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

    fn close_buffer_events(&mut self, path: Utf8PathBuf) -> RuntimeResult<Vec<Event>> {
        self.buffers.remove(&path);
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

    fn reconcile_moved_path_events(
        &mut self,
        old_path: Utf8PathBuf,
        new_path: Utf8PathBuf,
    ) -> Vec<Event> {
        let changed_paths = self
            .buffers
            .keys()
            .filter_map(|path| {
                moved_path(path, &old_path, &new_path).map(|next| (path.clone(), next))
            })
            .collect::<Vec<_>>();

        let mut events = Vec::new();
        for (old_buffer_path, new_buffer_path) in changed_paths {
            if let Some(mut buffer) = self.buffers.remove(&old_buffer_path) {
                buffer.path = new_buffer_path.clone();
                self.buffers.insert(new_buffer_path.clone(), buffer);
                self.revision = self.revision.next();
                events.push(Event::BufferPathReconciled {
                    old_path: old_buffer_path,
                    new_path: new_buffer_path,
                    revision: self.revision,
                });
            }
        }
        for tab in &mut self.tab_order {
            if let Some(next_tab) = moved_path(tab, &old_path, &new_path) {
                *tab = next_tab;
            }
        }
        if let Some(active_file) = self.active_file.as_ref() {
            if let Some(next_active_file) = moved_path(active_file, &old_path, &new_path) {
                self.active_file = Some(next_active_file);
            }
        }
        events
    }

    fn reconcile_deleted_path_events(&mut self, path: Utf8PathBuf) -> Vec<Event> {
        let closed_paths = self
            .buffers
            .keys()
            .filter(|candidate| path_affects(candidate, &path))
            .cloned()
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for closed_path in closed_paths {
            self.buffers.remove(&closed_path);
            self.tab_order.retain(|candidate| candidate != &closed_path);
            if self.active_file.as_ref() == Some(&closed_path) {
                self.active_file = self.tab_order.last().cloned();
            }
            self.revision = self.revision.next();
            events.push(Event::BufferClosed {
                path: closed_path,
                active_file: self.active_file.clone(),
                revision: self.revision,
            });
        }
        events
    }

    fn apply_history_edit(
        &mut self,
        path: Utf8PathBuf,
        expected_revision: Revision,
        direction: HistoryDirection,
    ) -> RuntimeResult<Vec<Event>> {
        let buffer = self.buffer_mut(&path)?;
        if buffer.external_state != BufferExternalState::Current {
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
                disk_version: buffer.disk_version.clone(),
                external_state: buffer.external_state,
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

impl From<&BufferState> for BufferTab {
    fn from(buffer: &BufferState) -> Self {
        Self {
            path: buffer.path.clone(),
            text: buffer.text.clone(),
            saved_text: buffer.saved_text.clone(),
            disk_version: buffer.disk_version.clone(),
            external_state: buffer.external_state,
            view_mode: buffer.view_mode,
            revision: buffer.revision,
        }
    }
}
