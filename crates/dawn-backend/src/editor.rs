use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::{
    types::{
        EditorViewMode, FileVersion, ProjectFileMetadata, ProjectFileSnapshot, ProjectPathMove,
        ProjectSessionPreferences, ProjectSessionTabPreference,
    },
    BackendError, BackendErrorKind, BackendResult,
};
use dawn_language::analysis::ProjectOverlay;

const INACTIVE_RESTORE_LOAD_LIMIT_BYTES: u64 = 1024 * 1024;

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
}

impl EditorBuffer {
    fn from_snapshot(snapshot: ProjectFileSnapshot) -> Self {
        Self {
            text: snapshot.text.clone(),
            saved_text: snapshot.text,
            disk_version: snapshot.version,
            revision: Revision::INITIAL,
        }
    }

    fn is_dirty(&self) -> bool {
        self.text != self.saved_text
    }

    fn update_text(&mut self, text: String) {
        if self.text == text {
            return;
        }
        self.text = text;
        self.revision = self.revision.next();
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

#[derive(Debug, Clone)]
pub(crate) struct EditorBufferSaveRequest {
    pub(crate) path: Utf8PathBuf,
    pub(crate) text: String,
    pub(crate) saved_disk_version: FileVersion,
    pub(crate) dirty: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveEditorBuffer {
    pub(crate) path: Utf8PathBuf,
    pub(crate) view_mode: EditorViewMode,
    pub(crate) text: String,
}

#[derive(Debug, Default)]
pub(crate) struct Editor {
    tabs: Vec<EditorTab>,
    active_file: Option<Utf8PathBuf>,
}

impl Editor {
    pub(crate) fn restore_for_project(
        &mut self,
        preferences: ProjectSessionPreferences,
        mut file_metadata: impl FnMut(&Utf8PathBuf) -> BackendResult<Option<ProjectFileMetadata>>,
        mut read_file_snapshot: impl FnMut(&Utf8PathBuf) -> BackendResult<ProjectFileSnapshot>,
    ) -> BackendResult<()> {
        self.tabs.clear();
        self.active_file = None;

        for tab_preference in preferences.tabs {
            let Some(metadata) = file_metadata(&tab_preference.path)? else {
                continue;
            };
            let should_load = preferences.active_file.as_ref() == Some(&tab_preference.path)
                || metadata.len <= INACTIVE_RESTORE_LOAD_LIMIT_BYTES;
            let content = if should_load {
                EditorTabContent::Loaded(EditorBuffer::from_snapshot(read_file_snapshot(
                    &tab_preference.path,
                )?))
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

    pub(crate) fn open_file(
        &mut self,
        path: Utf8PathBuf,
        mut read_file_snapshot: impl FnMut(&Utf8PathBuf) -> BackendResult<ProjectFileSnapshot>,
    ) -> BackendResult<()> {
        if let Some(index) = self.tab_index(&path) {
            self.load_tab_at(index, read_file_snapshot)?;
            self.active_file = Some(path);
            return Ok(());
        }

        let buffer = EditorBuffer::from_snapshot(read_file_snapshot(&path)?);
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
        path: Utf8PathBuf,
        read_file_snapshot: impl FnMut(&Utf8PathBuf) -> BackendResult<ProjectFileSnapshot>,
    ) -> BackendResult<()> {
        let index = self.tab_index(&path).ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::NotFound,
                format!("file is not open: {path}"),
            )
        })?;
        self.load_tab_at(index, read_file_snapshot)?;
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

    pub(crate) fn replace_active_text(&mut self, text: String) -> BackendResult<()> {
        self.active_buffer_mut()?.update_text(text);
        Ok(())
    }

    pub(crate) fn active_loaded_buffer(&self) -> BackendResult<ActiveEditorBuffer> {
        let active_file = self
            .active_file
            .as_ref()
            .ok_or_else(no_active_loaded_buffer)?;
        let tab = self
            .tabs
            .iter()
            .find(|tab| &tab.path == active_file)
            .ok_or_else(no_active_loaded_buffer)?;
        let EditorTabContent::Loaded(buffer) = &tab.content else {
            return Err(no_active_loaded_buffer());
        };
        Ok(ActiveEditorBuffer {
            path: tab.path.clone(),
            view_mode: tab.view_mode,
            text: buffer.text.clone(),
        })
    }

    pub(crate) fn dirty_overlays(&self) -> Vec<ProjectOverlay> {
        self.tabs
            .iter()
            .filter_map(|tab| {
                let EditorTabContent::Loaded(buffer) = &tab.content else {
                    return None;
                };
                buffer.is_dirty().then(|| ProjectOverlay {
                    path: tab.path.clone(),
                    content: buffer.text.clone(),
                })
            })
            .collect()
    }

    pub(crate) fn set_active_view_mode(&mut self, view_mode: EditorViewMode) -> BackendResult<()> {
        let tab = self.active_loaded_tab_mut()?;
        tab.view_mode = view_mode;
        Ok(())
    }

    pub(crate) fn active_save_request(&self) -> BackendResult<EditorBufferSaveRequest> {
        let active_file = self
            .active_file
            .as_ref()
            .ok_or_else(no_active_loaded_buffer)?;
        let tab = self
            .tabs
            .iter()
            .find(|tab| &tab.path == active_file)
            .ok_or_else(no_active_loaded_buffer)?;
        let EditorTabContent::Loaded(buffer) = &tab.content else {
            return Err(no_active_loaded_buffer());
        };
        Ok(EditorBufferSaveRequest {
            path: tab.path.clone(),
            text: buffer.text.clone(),
            saved_disk_version: buffer.disk_version.clone(),
            dirty: buffer.is_dirty(),
        })
    }

    pub(crate) fn mark_active_buffer_saved(
        &mut self,
        disk_version: FileVersion,
    ) -> BackendResult<()> {
        let buffer = self.active_buffer_mut()?;
        buffer.saved_text = buffer.text.clone();
        buffer.disk_version = disk_version;
        Ok(())
    }

    pub(crate) fn mark_buffer_saved(
        &mut self,
        path: &Utf8PathBuf,
        disk_version: FileVersion,
    ) -> BackendResult<()> {
        let buffer = self.loaded_buffer_mut(path)?;
        buffer.saved_text = buffer.text.clone();
        buffer.disk_version = disk_version;
        Ok(())
    }

    pub(crate) fn replace_active_buffer_from_snapshot(
        &mut self,
        snapshot: ProjectFileSnapshot,
    ) -> BackendResult<()> {
        let buffer = self.active_buffer_mut()?;
        buffer.text = snapshot.text.clone();
        buffer.saved_text = snapshot.text;
        buffer.disk_version = snapshot.version;
        buffer.revision = buffer.revision.next();
        Ok(())
    }

    pub(crate) fn close_active_buffer_force(&mut self) -> BackendResult<()> {
        let active_file = self
            .active_file
            .clone()
            .ok_or_else(no_active_loaded_buffer)?;
        self.close_file_force(&active_file)
    }

    pub(crate) fn affected_dirty_save_requests(
        &self,
        changed_paths: &[Utf8PathBuf],
    ) -> Vec<EditorBufferSaveRequest> {
        self.tabs
            .iter()
            .filter(|tab| path_matches_any(&tab.path, changed_paths))
            .filter_map(|tab| {
                let EditorTabContent::Loaded(buffer) = &tab.content else {
                    return None;
                };
                buffer.is_dirty().then(|| EditorBufferSaveRequest {
                    path: tab.path.clone(),
                    text: buffer.text.clone(),
                    saved_disk_version: buffer.disk_version.clone(),
                    dirty: true,
                })
            })
            .collect()
    }

    pub(crate) fn reconcile_moved_paths(&mut self, moves: &[ProjectPathMove]) {
        for path_move in moves {
            self.reconcile_moved_path(path_move);
        }
    }

    pub(crate) fn reconcile_deleted_path(&mut self, path: &Utf8PathBuf) {
        let closed_paths = self
            .tabs
            .iter()
            .filter(|tab| path_affects(&tab.path, path))
            .map(|tab| tab.path.clone())
            .collect::<Vec<_>>();
        for closed_path in closed_paths {
            let _ = self.close_file_force(&closed_path);
        }
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

    fn load_tab_at(
        &mut self,
        index: usize,
        mut read_file_snapshot: impl FnMut(&Utf8PathBuf) -> BackendResult<ProjectFileSnapshot>,
    ) -> BackendResult<()> {
        if matches!(self.tabs[index].content, EditorTabContent::Loaded(_)) {
            return Ok(());
        }
        let snapshot = read_file_snapshot(&self.tabs[index].path)?;
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

    fn loaded_buffer_mut(&mut self, path: &Utf8PathBuf) -> BackendResult<&mut EditorBuffer> {
        let index = self.tab_index(path).ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::NotFound,
                format!("file is not open: {path}"),
            )
        })?;
        match &mut self.tabs[index].content {
            EditorTabContent::Loaded(buffer) => Ok(buffer),
            EditorTabContent::Unloaded(_) => Err(no_active_loaded_buffer()),
        }
    }

    fn close_file_force(&mut self, path: &Utf8PathBuf) -> BackendResult<()> {
        let index = self.tab_index(path).ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::NotFound,
                format!("file is not open: {path}"),
            )
        })?;
        self.tabs.remove(index);
        if self.active_file.as_ref() == Some(path) {
            self.active_file = self.tabs.last().map(|tab| tab.path.clone());
        }
        Ok(())
    }

    fn reconcile_moved_path(&mut self, path_move: &ProjectPathMove) {
        for tab in &mut self.tabs {
            if let Some(new_path) = moved_path(&tab.path, &path_move.old_path, &path_move.new_path)
            {
                tab.path = new_path;
            }
        }
        if let Some(active_file) = self.active_file.as_ref() {
            if let Some(new_active_file) =
                moved_path(active_file, &path_move.old_path, &path_move.new_path)
            {
                self.active_file = Some(new_active_file);
            }
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

fn path_affects(candidate: &Utf8PathBuf, changed_path: &Utf8PathBuf) -> bool {
    candidate == changed_path || candidate.starts_with(changed_path)
}

fn path_matches_any(candidate: &Utf8PathBuf, changed_paths: &[Utf8PathBuf]) -> bool {
    changed_paths
        .iter()
        .any(|changed_path| path_affects(candidate, changed_path))
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
