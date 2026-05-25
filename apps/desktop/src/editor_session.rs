use std::collections::BTreeMap;

use dawn_project::analysis::ProjectOverlay;
use dawn_project::path::Utf8PathBuf;
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

#[derive(Debug, Clone)]
pub struct EditorBuffer {
    pub path: Utf8PathBuf,
    pub text: String,
    pub saved_text: String,
    pub view_mode: EditorViewMode,
}

impl EditorBuffer {
    pub fn is_dirty(&self) -> bool {
        self.text != self.saved_text
    }
}

#[derive(Debug, Default, Clone)]
pub struct EditorSession {
    open_editors: BTreeMap<Utf8PathBuf, EditorBuffer>,
    tab_order: Vec<Utf8PathBuf>,
    active_file: Option<Utf8PathBuf>,
}

impl EditorSession {
    pub fn open_file(&mut self, path: Utf8PathBuf, text: String) {
        if !self.open_editors.contains_key(&path) {
            self.open_editors.insert(
                path.clone(),
                EditorBuffer {
                    path: path.clone(),
                    saved_text: text.clone(),
                    text,
                    view_mode: EditorViewMode::Text,
                },
            );
            self.tab_order.push(path.clone());
        }
        self.active_file = Some(path);
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
            buffer.text = text;
        }
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
        tabs: Vec<(Utf8PathBuf, String, EditorViewMode)>,
        active_file: Option<Utf8PathBuf>,
    ) {
        self.clear();
        for (path, text, view_mode) in tabs {
            self.open_editors.insert(
                path.clone(),
                EditorBuffer {
                    path: path.clone(),
                    saved_text: text.clone(),
                    text,
                    view_mode,
                },
            );
            self.tab_order.push(path);
        }
        self.active_file = active_file
            .filter(|path| self.open_editors.contains_key(path))
            .or_else(|| self.tab_order.last().cloned());
    }

    pub fn mark_saved(&mut self, path: &Utf8PathBuf, saved_text: String) {
        if let Some(buffer) = self.open_editors.get_mut(path) {
            buffer.text = saved_text.clone();
            buffer.saved_text = saved_text;
        }
    }

    pub fn dirty_overlays(&self) -> Vec<ProjectOverlay> {
        self.open_editors
            .values()
            .filter(|buffer| buffer.is_dirty())
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
