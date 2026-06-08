use std::collections::BTreeMap;

use dawn_project::analysis::ProjectOverlay;
use dawn_project::Utf8PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorViewMode {
    Text,
    Gui,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSessionState {
    #[serde(default)]
    pub tabs: Vec<EditorTabState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_file: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorTabState {
    pub path: Utf8PathBuf,
    pub view_mode: EditorViewMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiskVersion {
    pub len: u64,
    pub modified_millis: Option<u128>,
    pub content_hash: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferExternalState {
    Current,
    ChangedOnDisk,
    DeletedOnDisk,
}

#[derive(Debug, Clone)]
pub struct EditorBuffer {
    pub path: Utf8PathBuf,
    pub text: String,
    pub saved_text: String,
    pub disk_version: Option<FileDiskVersion>,
    pub external_state: BufferExternalState,
    pub view_mode: EditorViewMode,
    gui_dirty_revision: u64,
    gui_saved_revision: u64,
    undo_stack: Vec<String>,
    redo_stack: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFileOutcome {
    Opened,
    Activated,
    ReloadedFromDisk,
    MarkedChangedOnDisk,
}

impl EditorBuffer {
    pub fn is_dirty(&self) -> bool {
        self.text != self.saved_text || self.gui_dirty_revision != self.gui_saved_revision
    }

    pub fn is_conflicted(&self) -> bool {
        self.external_state != BufferExternalState::Current
    }
}

#[derive(Debug, Default, Clone)]
pub struct EditorSession {
    open_editors: BTreeMap<Utf8PathBuf, EditorBuffer>,
    tab_order: Vec<Utf8PathBuf>,
    active_file: Option<Utf8PathBuf>,
}

impl EditorSession {
    pub fn open_file(&mut self, path: Utf8PathBuf, text: String, disk_version: FileDiskVersion) {
        if !self.open_editors.contains_key(&path) {
            self.open_editors.insert(
                path.clone(),
                EditorBuffer {
                    path: path.clone(),
                    saved_text: text.clone(),
                    text,
                    disk_version: Some(disk_version),
                    external_state: BufferExternalState::Current,
                    view_mode: EditorViewMode::Text,
                    gui_dirty_revision: 0,
                    gui_saved_revision: 0,
                    undo_stack: Vec::new(),
                    redo_stack: Vec::new(),
                },
            );
            self.tab_order.push(path.clone());
        }
        self.active_file = Some(path);
    }

    pub fn open_or_reconcile_file(
        &mut self,
        path: Utf8PathBuf,
        text: String,
        disk_version: FileDiskVersion,
    ) -> OpenFileOutcome {
        let Some(buffer) = self.open_editors.get_mut(&path) else {
            self.open_file(path, text, disk_version);
            return OpenFileOutcome::Opened;
        };

        self.active_file = Some(path);
        if buffer.disk_version.as_ref() == Some(&disk_version) {
            return OpenFileOutcome::Activated;
        }
        if buffer.is_dirty() {
            buffer.external_state = BufferExternalState::ChangedOnDisk;
            return OpenFileOutcome::MarkedChangedOnDisk;
        }

        buffer.text = text.clone();
        buffer.saved_text = text;
        buffer.disk_version = Some(disk_version);
        buffer.external_state = BufferExternalState::Current;
        buffer.gui_dirty_revision = 0;
        buffer.gui_saved_revision = 0;
        OpenFileOutcome::ReloadedFromDisk
    }

    pub fn close_file(&mut self, path: &Utf8PathBuf) {
        self.open_editors.remove(path);
        self.tab_order.retain(|candidate| candidate != path);
        if self.active_file.as_ref() == Some(path) {
            self.active_file = self.tab_order.last().cloned();
        }
    }

    pub fn set_active_file(&mut self, path: Utf8PathBuf) {
        if self.open_editors.contains_key(&path) {
            self.active_file = Some(path);
        }
    }

    pub fn active_file(&self) -> Option<&Utf8PathBuf> {
        self.active_file.as_ref()
    }

    pub fn active_buffer(&self) -> Option<&EditorBuffer> {
        self.active_file
            .as_ref()
            .and_then(|path| self.open_editors.get(path))
    }

    pub fn active_buffer_mut(&mut self) -> Option<&mut EditorBuffer> {
        let path = self.active_file.clone()?;
        self.open_editors.get_mut(&path)
    }

    pub fn update_active_text(&mut self, text: String) {
        if let Some(buffer) = self.active_buffer_mut() {
            if buffer.is_conflicted() {
                return;
            }
            if buffer.text == text {
                return;
            }
            buffer.record_snapshot();
            buffer.text = text;
        }
    }

    pub fn replace_active_text_from_edit(&mut self, text: String) {
        if let Some(buffer) = self.active_buffer_mut() {
            if buffer.is_conflicted() {
                return;
            }
            if buffer.text == text {
                return;
            }
            buffer.record_snapshot();
            buffer.text = text;
        }
    }

    pub fn undo_active_text_edit(&mut self) -> bool {
        let Some(buffer) = self.active_buffer_mut() else {
            return false;
        };
        let Some(previous) = buffer.undo_stack.pop() else {
            return false;
        };
        buffer.redo_stack.push(buffer.text.clone());
        buffer.text = previous;
        true
    }

    pub fn redo_active_text_edit(&mut self) -> bool {
        let Some(buffer) = self.active_buffer_mut() else {
            return false;
        };
        let Some(next) = buffer.redo_stack.pop() else {
            return false;
        };
        buffer.undo_stack.push(buffer.text.clone());
        buffer.text = next;
        true
    }

    pub fn set_view_mode(&mut self, path: &Utf8PathBuf, mode: EditorViewMode) {
        if let Some(buffer) = self.open_editors.get_mut(path) {
            buffer.view_mode = mode;
        }
    }

    pub fn cycle_tabs(&mut self, reverse: bool) {
        if self.tab_order.is_empty() {
            self.active_file = None;
            return;
        }
        let current = self
            .active_file
            .as_ref()
            .and_then(|path| {
                self.tab_order
                    .iter()
                    .position(|candidate| candidate == path)
            })
            .unwrap_or(0);
        let next = if reverse {
            current
                .checked_sub(1)
                .unwrap_or_else(|| self.tab_order.len().saturating_sub(1))
        } else {
            (current + 1) % self.tab_order.len()
        };
        self.active_file = self.tab_order.get(next).cloned();
    }

    pub fn tabs(&self) -> Vec<EditorBuffer> {
        self.tab_order
            .iter()
            .filter_map(|path| self.open_editors.get(path).cloned())
            .collect()
    }

    pub fn state(&self) -> EditorSessionState {
        EditorSessionState {
            tabs: self
                .tab_order
                .iter()
                .filter_map(|path| {
                    self.open_editors.get(path).map(|buffer| EditorTabState {
                        path: path.clone(),
                        view_mode: buffer.view_mode,
                    })
                })
                .collect(),
            active_file: self.active_file.clone(),
        }
    }

    pub fn restore(
        &mut self,
        tabs: Vec<(Utf8PathBuf, String, FileDiskVersion, EditorViewMode)>,
        active_file: Option<Utf8PathBuf>,
    ) {
        self.clear();
        for (path, text, disk_version, view_mode) in tabs {
            self.open_editors.insert(
                path.clone(),
                EditorBuffer {
                    path: path.clone(),
                    saved_text: text.clone(),
                    text,
                    disk_version: Some(disk_version),
                    external_state: BufferExternalState::Current,
                    view_mode,
                    gui_dirty_revision: 0,
                    gui_saved_revision: 0,
                    undo_stack: Vec::new(),
                    redo_stack: Vec::new(),
                },
            );
            self.tab_order.push(path);
        }
        self.active_file = active_file
            .filter(|path| self.open_editors.contains_key(path))
            .or_else(|| self.tab_order.last().cloned());
    }

    pub fn mark_saved(
        &mut self,
        path: &Utf8PathBuf,
        saved_text: String,
        disk_version: FileDiskVersion,
    ) {
        if let Some(buffer) = self.open_editors.get_mut(path) {
            buffer.text = saved_text.clone();
            buffer.saved_text = saved_text;
            buffer.disk_version = Some(disk_version);
            buffer.external_state = BufferExternalState::Current;
            buffer.gui_dirty_revision = 0;
            buffer.gui_saved_revision = 0;
        }
    }

    pub fn record_saved_version(&mut self, path: &Utf8PathBuf, disk_version: FileDiskVersion) {
        if let Some(buffer) = self.open_editors.get_mut(path) {
            buffer.saved_text = buffer.text.clone();
            buffer.disk_version = Some(disk_version);
            buffer.external_state = BufferExternalState::Current;
            buffer.gui_saved_revision = buffer.gui_dirty_revision;
        }
    }

    pub fn mark_gui_edit_dirty(&mut self, path: &Utf8PathBuf, revision: u64) {
        if let Some(buffer) = self.open_editors.get_mut(path) {
            buffer.gui_dirty_revision = revision;
        }
    }

    pub fn complete_gui_edit_save(
        &mut self,
        path: &Utf8PathBuf,
        revision: u64,
        text: String,
        disk_version: FileDiskVersion,
    ) -> bool {
        let Some(buffer) = self.open_editors.get_mut(path) else {
            return false;
        };
        if buffer.gui_dirty_revision != revision {
            return false;
        }
        buffer.text = text.clone();
        buffer.saved_text = text;
        buffer.disk_version = Some(disk_version);
        buffer.external_state = BufferExternalState::Current;
        buffer.gui_saved_revision = revision;
        true
    }

    pub fn replace_from_disk(
        &mut self,
        path: &Utf8PathBuf,
        text: String,
        disk_version: FileDiskVersion,
        preserve_undo: bool,
    ) {
        if let Some(buffer) = self.open_editors.get_mut(path) {
            if preserve_undo {
                buffer.record_snapshot();
            }
            buffer.text = text.clone();
            buffer.saved_text = text;
            buffer.disk_version = Some(disk_version);
            buffer.external_state = BufferExternalState::Current;
            buffer.gui_dirty_revision = 0;
            buffer.gui_saved_revision = 0;
        }
    }

    pub fn mark_external_state(&mut self, path: &Utf8PathBuf, state: BufferExternalState) {
        if let Some(buffer) = self.open_editors.get_mut(path) {
            buffer.external_state = state;
        }
    }

    pub fn dirty_overlays(&self) -> Vec<ProjectOverlay> {
        self.open_editors
            .values()
            .filter(|buffer| buffer.text != buffer.saved_text)
            .map(|buffer| ProjectOverlay {
                path: buffer.path.clone(),
                content: buffer.text.clone(),
            })
            .collect()
    }

    pub fn dirty_buffers(&self) -> Vec<EditorBuffer> {
        self.open_editors
            .values()
            .filter(|buffer| buffer.is_dirty())
            .cloned()
            .collect()
    }

    pub fn dirty_autosave_buffers(&self) -> Vec<EditorBuffer> {
        self.open_editors
            .values()
            .filter(|buffer| buffer.is_dirty() && !buffer.is_conflicted())
            .cloned()
            .collect()
    }

    pub fn buffers(&self) -> Vec<EditorBuffer> {
        self.open_editors.values().cloned().collect()
    }

    pub fn reconcile_moved_paths(&mut self, moves: &[(Utf8PathBuf, Utf8PathBuf)]) {
        for (old_path, new_path) in moves {
            let changed_paths = self
                .open_editors
                .keys()
                .filter_map(|path| {
                    moved_path(path, old_path, new_path).map(|next| (path.clone(), next))
                })
                .collect::<Vec<_>>();

            for (old_buffer_path, new_buffer_path) in changed_paths {
                if let Some(mut buffer) = self.open_editors.remove(&old_buffer_path) {
                    buffer.path = new_buffer_path.clone();
                    self.open_editors.insert(new_buffer_path, buffer);
                }
            }
            for tab in &mut self.tab_order {
                if let Some(new_tab) = moved_path(tab, old_path, new_path) {
                    *tab = new_tab;
                }
            }
            if let Some(active_file) = self.active_file.as_ref() {
                if let Some(new_active_file) = moved_path(active_file, old_path, new_path) {
                    self.active_file = Some(new_active_file);
                }
            }
        }
    }

    pub fn reconcile_deleted_path(&mut self, deleted_path: &Utf8PathBuf) {
        let closed_paths = self
            .open_editors
            .keys()
            .filter(|path| *path == deleted_path || path.starts_with(deleted_path))
            .cloned()
            .collect::<Vec<_>>();
        for path in closed_paths {
            self.open_editors.remove(&path);
            self.tab_order.retain(|candidate| candidate != &path);
        }
        if self
            .active_file
            .as_ref()
            .is_some_and(|path| path == deleted_path || path.starts_with(deleted_path))
        {
            self.active_file = self.tab_order.last().cloned();
        }
    }

    pub fn clear(&mut self) {
        self.open_editors.clear();
        self.tab_order.clear();
        self.active_file = None;
    }
}

impl EditorBuffer {
    fn record_snapshot(&mut self) {
        if self.undo_stack.last() == Some(&self.text) {
            return;
        }
        self.undo_stack.push(self.text.clone());
        self.redo_stack.clear();
    }
}

fn moved_path(
    path: &Utf8PathBuf,
    old_path: &Utf8PathBuf,
    new_path: &Utf8PathBuf,
) -> Option<Utf8PathBuf> {
    if path == old_path {
        return Some(new_path.clone());
    }
    if !path.starts_with(old_path) {
        return None;
    }
    let relative = path.strip_prefix(old_path).ok()?;
    Some(new_path.join(relative))
}
