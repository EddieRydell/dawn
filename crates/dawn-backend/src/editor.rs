use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::{
    preferences::{ProjectSessionPreferences, ProjectSessionTabPreference},
    project::{Project, ProjectFileSnapshot},
    types::{EditorViewMode, FileVersion},
    BackendError, BackendErrorKind, BackendResult,
};

const INACTIVE_RESTORE_LOAD_LIMIT_BYTES: u64 = 1024 * 1024;
const UNDO_LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Revision(u64);

impl Revision {
    pub const INITIAL: Self = Self(0);

    fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl Default for Revision {
    fn default() -> Self {
        Self::INITIAL
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EditorTab {
    path: Utf8PathBuf,
    view_mode: EditorViewMode,
    content: EditorTabContent,
}

#[derive(Debug, Clone)]
pub(crate) enum EditorTabContent {
    Loaded(EditorBuffer),
    Unloaded(UnloadedEditorTab),
}

#[derive(Debug, Clone)]
pub(crate) struct EditorBuffer {
    text: String,
    saved_text: String,
    disk_version: FileVersion,
    revision: Revision,
    undo_stack: Vec<String>,
    redo_stack: Vec<String>,
}

impl EditorBuffer {
    fn from_snapshot(snapshot: ProjectFileSnapshot) -> Self {
        Self {
            text: snapshot.text.clone(),
            saved_text: snapshot.text,
            disk_version: snapshot.version,
            revision: Revision::INITIAL,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn is_dirty(&self) -> bool {
        self.text != self.saved_text
    }

    fn update_text(&mut self, text: String) {
        if self.text == text {
            return;
        }
        self.record_undo_snapshot();
        self.text = text;
        self.revision = self.revision.next();
    }

    fn undo(&mut self) {
        let Some(previous) = self.undo_stack.pop() else {
            return;
        };
        self.redo_stack.push(self.text.clone());
        self.text = previous;
        self.revision = self.revision.next();
    }

    fn redo(&mut self) {
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        self.undo_stack.push(self.text.clone());
        self.text = next;
        self.revision = self.revision.next();
    }

    fn record_undo_snapshot(&mut self) {
        if self.undo_stack.last() == Some(&self.text) {
            self.redo_stack.clear();
            return;
        }
        self.undo_stack.push(self.text.clone());
        if self.undo_stack.len() > UNDO_LIMIT {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UnloadedEditorTab {
    byte_len: u64,
}

#[derive(Debug, Clone, Default)]
pub struct EditorView {
    pub tabs: Vec<EditorTabView>,
    pub active_file: Option<Utf8PathBuf>,
    pub active_buffer: Option<LoadedEditorTabView>,
}

#[derive(Debug, Clone)]
pub enum EditorTabView {
    Loaded(LoadedEditorTabView),
    Unloaded(UnloadedEditorTabView),
}

#[derive(Debug, Clone)]
pub struct LoadedEditorTabView {
    pub path: Utf8PathBuf,
    pub view_mode: EditorViewMode,
    pub buffer: EditorBufferView,
}

#[derive(Debug, Clone)]
pub struct UnloadedEditorTabView {
    pub path: Utf8PathBuf,
    pub view_mode: EditorViewMode,
    pub byte_len: u64,
}

#[derive(Debug, Clone)]
pub struct EditorBufferView {
    pub text: String,
    pub saved_text: String,
    pub disk_version: FileVersion,
    pub revision: Revision,
    pub dirty: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Editor {
    tabs: Vec<EditorTab>,
    active_file: Option<Utf8PathBuf>,
}

impl Editor {
    pub(crate) fn restore_for_project(
        &mut self,
        project: &Project,
        preferences: ProjectSessionPreferences,
    ) -> BackendResult<()> {
        self.tabs.clear();
        self.active_file = None;

        for tab_preference in preferences.tabs {
            let Some(metadata) = project.file_metadata(&tab_preference.path)? else {
                continue;
            };
            let should_load = preferences.active_file.as_ref() == Some(&tab_preference.path)
                || metadata.len <= INACTIVE_RESTORE_LOAD_LIMIT_BYTES;
            let content = if should_load {
                EditorTabContent::Loaded(EditorBuffer::from_snapshot(
                    project.read_file_snapshot(&tab_preference.path)?,
                ))
            } else {
                EditorTabContent::Unloaded(UnloadedEditorTab {
                    byte_len: metadata.len,
                })
            };
            self.tabs.push(EditorTab {
                path: tab_preference.path,
                view_mode: tab_preference.view_mode,
                content,
            });
        }

        self.active_file = preferences
            .active_file
            .filter(|path| self.tab_index(path).is_some())
            .or_else(|| self.tabs.last().map(|tab| tab.path.clone()));

        Ok(())
    }

    pub(crate) fn open_file(&mut self, project: &Project, path: Utf8PathBuf) -> BackendResult<()> {
        if let Some(index) = self.tab_index(&path) {
            self.load_tab_at(project, index)?;
            self.active_file = Some(path);
            return Ok(());
        }

        let buffer = EditorBuffer::from_snapshot(project.read_file_snapshot(&path)?);
        self.tabs.push(EditorTab {
            path: path.clone(),
            view_mode: EditorViewMode::Text,
            content: EditorTabContent::Loaded(buffer),
        });
        self.active_file = Some(path);
        Ok(())
    }

    pub(crate) fn set_active_file(
        &mut self,
        project: &Project,
        path: Utf8PathBuf,
    ) -> BackendResult<()> {
        let index = self.tab_index(&path).ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::NotFound,
                format!("file is not open: {path}"),
            )
        })?;
        self.load_tab_at(project, index)?;
        self.active_file = Some(path);
        Ok(())
    }

    pub(crate) fn close_file(&mut self, path: Utf8PathBuf) -> BackendResult<()> {
        let index = self.tab_index(&path).ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::NotFound,
                format!("file is not open: {path}"),
            )
        })?;
        if let EditorTabContent::Loaded(buffer) = &self.tabs[index].content {
            if buffer.is_dirty() {
                return Err(BackendError::new(
                    BackendErrorKind::Conflict,
                    format!("cannot close dirty file: {path}"),
                ));
            }
        }

        self.tabs.remove(index);
        if self.active_file.as_ref() == Some(&path) {
            self.active_file = self.tabs.last().map(|tab| tab.path.clone());
        }
        Ok(())
    }

    pub(crate) fn update_active_text(&mut self, text: String) -> BackendResult<()> {
        self.active_buffer_mut()?.update_text(text);
        Ok(())
    }

    pub(crate) fn set_active_view_mode(&mut self, view_mode: EditorViewMode) -> BackendResult<()> {
        let tab = self.active_loaded_tab_mut()?;
        tab.view_mode = view_mode;
        Ok(())
    }

    pub(crate) fn undo_active_edit(&mut self) -> BackendResult<()> {
        self.active_buffer_mut()?.undo();
        Ok(())
    }

    pub(crate) fn redo_active_edit(&mut self) -> BackendResult<()> {
        self.active_buffer_mut()?.redo();
        Ok(())
    }

    pub(crate) fn session_preferences(&self) -> ProjectSessionPreferences {
        ProjectSessionPreferences {
            tabs: self
                .tabs
                .iter()
                .map(|tab| ProjectSessionTabPreference {
                    path: tab.path.clone(),
                    view_mode: tab.view_mode,
                })
                .collect(),
            active_file: self.active_file.clone(),
        }
    }

    pub(crate) fn snapshot(&self) -> EditorView {
        let active_buffer = self.active_file.as_ref().and_then(|path| {
            self.tabs
                .iter()
                .find(|tab| &tab.path == path)
                .and_then(LoadedEditorTabView::from_tab)
        });
        EditorView {
            tabs: self.tabs.iter().map(EditorTabView::from).collect(),
            active_file: self.active_file.clone(),
            active_buffer,
        }
    }

    fn load_tab_at(&mut self, project: &Project, index: usize) -> BackendResult<()> {
        if matches!(self.tabs[index].content, EditorTabContent::Loaded(_)) {
            return Ok(());
        }
        let snapshot = project.read_file_snapshot(&self.tabs[index].path)?;
        self.tabs[index].content = EditorTabContent::Loaded(EditorBuffer::from_snapshot(snapshot));
        Ok(())
    }

    fn active_loaded_tab_mut(&mut self) -> BackendResult<&mut EditorTab> {
        let active_file = self
            .active_file
            .clone()
            .ok_or_else(no_active_loaded_buffer)?;
        let index = self
            .tab_index(&active_file)
            .ok_or_else(no_active_loaded_buffer)?;
        if matches!(self.tabs[index].content, EditorTabContent::Unloaded(_)) {
            return Err(no_active_loaded_buffer());
        }
        Ok(&mut self.tabs[index])
    }

    fn active_buffer_mut(&mut self) -> BackendResult<&mut EditorBuffer> {
        match &mut self.active_loaded_tab_mut()?.content {
            EditorTabContent::Loaded(buffer) => Ok(buffer),
            EditorTabContent::Unloaded(_) => Err(no_active_loaded_buffer()),
        }
    }

    fn tab_index(&self, path: &Utf8PathBuf) -> Option<usize> {
        self.tabs.iter().position(|tab| &tab.path == path)
    }
}

fn no_active_loaded_buffer() -> BackendError {
    BackendError::new(
        BackendErrorKind::InvalidInput,
        "there is no active loaded editor buffer",
    )
}

impl From<&EditorTab> for EditorTabView {
    fn from(tab: &EditorTab) -> Self {
        match &tab.content {
            EditorTabContent::Loaded(buffer) => Self::Loaded(LoadedEditorTabView {
                path: tab.path.clone(),
                view_mode: tab.view_mode,
                buffer: EditorBufferView {
                    text: buffer.text.clone(),
                    saved_text: buffer.saved_text.clone(),
                    disk_version: buffer.disk_version.clone(),
                    revision: buffer.revision,
                    dirty: buffer.is_dirty(),
                },
            }),
            EditorTabContent::Unloaded(unloaded) => Self::Unloaded(UnloadedEditorTabView {
                path: tab.path.clone(),
                view_mode: tab.view_mode,
                byte_len: unloaded.byte_len,
            }),
        }
    }
}

impl LoadedEditorTabView {
    fn from_tab(tab: &EditorTab) -> Option<Self> {
        let EditorTabContent::Loaded(buffer) = &tab.content else {
            return None;
        };
        Some(Self {
            path: tab.path.clone(),
            view_mode: tab.view_mode,
            buffer: EditorBufferView {
                text: buffer.text.clone(),
                saved_text: buffer.saved_text.clone(),
                disk_version: buffer.disk_version.clone(),
                revision: buffer.revision,
                dirty: buffer.is_dirty(),
            },
        })
    }
}
