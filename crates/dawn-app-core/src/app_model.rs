use std::path::PathBuf;

use dawn_project::analysis::{ProjectAnalysis, ProjectDiagnostic};
use dawn_project::document::{
    DocumentDescriptor, DocumentViewId, FixtureDocument, LayoutDocument, SequenceDocument,
    SequenceDocumentEdit,
};
use dawn_project::fs::WorkspaceEntry;
use dawn_project::model::Geometry;
use dawn_project::path::Utf8PathBuf;

use crate::actions::AppAction;
use crate::dto::AppSnapshotDto;
use crate::dto::{FixtureGuiEditDto, LayoutGuiEditDto, SequenceGuiEditDto};
use crate::editor_session::{EditorBuffer, EditorSession, EditorViewMode};
use crate::layout_persistence::{load_workbench_layout, save_workbench_layout, WorkbenchLayout};
use crate::preview_session::{PreviewSession, PreviewSnapshot, SequenceKey};
use crate::workspace::WorkspaceService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    SnapshotChanged,
    NoSnapshotChange,
}

impl DispatchOutcome {
    pub fn snapshot_changed(self) -> bool {
        matches!(self, Self::SnapshotChanged)
    }
}

#[derive(Debug)]
pub struct AppModel {
    pub workspace: WorkspaceService,
    pub editors: EditorSession,
    pub workbench_layout: WorkbenchLayout,
    pub preview: PreviewSession,
    pub project_root: Option<String>,
    pub project_entries: Vec<WorkspaceEntry>,
    pub analysis: Option<ProjectAnalysis>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct AppSnapshot {
    pub project_root: Option<String>,
    pub project_entries: Vec<WorkspaceEntry>,
    pub analysis: Option<ProjectAnalysis>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub workbench_layout: WorkbenchLayout,
    pub preview: PreviewSnapshot,
    pub tabs: Vec<EditorBuffer>,
    pub active_file: Option<Utf8PathBuf>,
    pub active_buffer: Option<EditorBuffer>,
    pub active_document_descriptor: Option<DocumentDescriptor>,
    pub active_gui_document: Option<ActiveGuiDocument>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub enum ActiveGuiDocument {
    Sequence(SequenceDocument),
    Layout(LayoutDocument),
    Fixture(FixtureDocument),
    Blocked {
        reason: String,
        diagnostics: Vec<ProjectDiagnostic>,
    },
}

impl Default for AppModel {
    fn default() -> Self {
        let workbench_layout = load_workbench_layout();
        let last_project_root = workbench_layout.last_project_root.clone();
        let mut model = Self {
            workspace: WorkspaceService::default(),
            editors: EditorSession::default(),
            workbench_layout,
            preview: PreviewSession::default(),
            project_root: None,
            project_entries: Vec::new(),
            analysis: None,
            diagnostics: Vec::new(),
            status: "No project open".to_string(),
        };
        if let Some(path) = last_project_root {
            match model.open_project(path, false, true) {
                Ok(()) => model.status = "Project restored".to_string(),
                Err(error) => model.status = format!("Could not restore last project: {error}"),
            }
        }
        model
    }
}

impl AppModel {
    pub fn snapshot(&self) -> AppSnapshot {
        let active_document_descriptor = self.active_document_descriptor();
        let active_gui_document = self.active_gui_document(active_document_descriptor.as_ref());
        AppSnapshot {
            project_root: self.project_root.clone(),
            project_entries: self.project_entries.clone(),
            analysis: self.analysis.clone(),
            diagnostics: self.diagnostics.clone(),
            workbench_layout: self.workbench_layout.clone(),
            preview: self.preview.snapshot(),
            tabs: self.editors.tabs(),
            active_file: self.editors.active_file().cloned(),
            active_buffer: self.editors.active_buffer().cloned(),
            active_document_descriptor,
            active_gui_document,
            status: self.status.clone(),
        }
    }

    pub fn snapshot_dto(&self) -> AppSnapshotDto {
        self.snapshot().into()
    }

    pub fn dispatch(&mut self, action: AppAction) -> Result<DispatchOutcome, String> {
        match action {
            AppAction::OpenProject(path) => {
                self.flush_autosave()?;
                self.open_project(path, true, false)?;
                self.status = "Project opened".to_string();
            }
            AppAction::ReloadProject => {
                self.flush_autosave()?;
                self.refresh_project_entries()?;
                self.refresh_analysis()?;
                self.sync_preview_source();
                self.status = "Project checked".to_string();
            }
            AppAction::OpenFile(path) => {
                let text = self.workspace.read_file(path.clone())?;
                self.editors.open_file(path, text);
                self.refresh_analysis()?;
                self.sync_preview_source();
                self.persist_workbench_layout()?;
            }
            AppAction::CloseFile(path) => {
                self.editors.close_file(&path);
                self.refresh_analysis()?;
                self.sync_preview_source();
                self.persist_workbench_layout()?;
            }
            AppAction::SetActiveFile(path) => {
                let active_changed = self.editors.active_file() != Some(&path);
                self.editors.set_active_file(path);
                if active_changed {
                    self.preview.pause(self.analysis.as_ref());
                    self.sync_preview_source();
                    self.persist_workbench_layout()?;
                }
            }
            AppAction::SetActiveViewMode(mode) => {
                let Some(path) = self.editors.active_file().cloned() else {
                    return Ok(DispatchOutcome::NoSnapshotChange);
                };
                self.editors.set_view_mode(&path, mode.into());
                self.persist_workbench_layout()?;
            }
            AppAction::UpdateActiveText(text) => {
                self.editors.update_active_text(text);
                self.save_active_file_after_text_edit()?;
                self.status = "Autosaved".to_string();
            }
            AppAction::UndoActiveEdit => {
                if self.editors.undo_active_text_edit() {
                    self.save_active_file_after_text_edit()?;
                    self.status = "Undo".to_string();
                } else {
                    return Ok(DispatchOutcome::NoSnapshotChange);
                }
            }
            AppAction::RedoActiveEdit => {
                if self.editors.redo_active_text_edit() {
                    self.save_active_file_after_text_edit()?;
                    self.status = "Redo".to_string();
                } else {
                    return Ok(DispatchOutcome::NoSnapshotChange);
                }
            }
            AppAction::ApplySequenceGuiEdit(edit) => {
                self.apply_sequence_gui_edit(edit)?;
                self.status = "Autosaved".to_string();
            }
            AppAction::ApplyLayoutGuiEdit(edit) => {
                self.apply_layout_gui_edit(edit)?;
                self.status = "Autosaved".to_string();
            }
            AppAction::ApplyFixtureGuiEdit(edit) => {
                self.apply_fixture_gui_edit(edit)?;
                self.status = "Autosaved".to_string();
            }
            AppAction::FlushAutosave => {
                self.flush_autosave()?;
                self.status = "Saved".to_string();
            }
            AppAction::CreateFile { parent, name } => {
                self.flush_autosave()?;
                let path = self.workspace.create_file(parent, &name)?;
                self.refresh_project_entries()?;
                let text = self.workspace.read_file(path.clone())?;
                self.editors.open_file(path, text);
                self.refresh_analysis()?;
                self.sync_preview_source();
                self.persist_workbench_layout()?;
            }
            AppAction::CreateDirectory { parent, name } => {
                self.flush_autosave()?;
                self.workspace.create_directory(parent, &name)?;
                self.refresh_project_entries()?;
                self.refresh_analysis()?;
                self.sync_preview_source();
            }
            AppAction::RenamePath { path, new_name } => {
                self.flush_autosave()?;
                let moves = self.workspace.rename_path(path.clone(), &new_name)?;
                self.refresh_project_entries()?;
                self.editors.reconcile_moved_paths(&moves);
                self.refresh_analysis()?;
                self.sync_preview_source();
                self.persist_workbench_layout()?;
            }
            AppAction::DeletePath(path) => {
                self.flush_autosave()?;
                self.workspace.delete_path(path.clone())?;
                self.refresh_project_entries()?;
                self.editors.reconcile_deleted_path(&path);
                self.refresh_analysis()?;
                self.sync_preview_source();
                self.persist_workbench_layout()?;
            }
            AppAction::ToggleProjectTree => {
                self.workbench_layout.project_tree_visible =
                    !self.workbench_layout.project_tree_visible;
                save_workbench_layout(&self.workbench_layout)?;
            }
        }
        Ok(DispatchOutcome::SnapshotChanged)
    }

    fn open_project(
        &mut self,
        path: PathBuf,
        remember: bool,
        restore_editor_session: bool,
    ) -> Result<(), String> {
        self.workspace.open_project(&path)?;
        self.refresh_project_entries()?;
        self.editors.clear();
        self.preview.reset();
        if restore_editor_session {
            self.restore_editor_session();
        }
        self.refresh_analysis()?;
        self.sync_preview_source();
        if remember {
            self.workbench_layout.last_project_root = Some(path);
            self.persist_workbench_layout()?;
        }
        Ok(())
    }

    fn restore_editor_session(&mut self) {
        let tabs = self
            .workbench_layout
            .editor_session
            .tabs
            .clone()
            .into_iter()
            .filter_map(|tab| {
                self.workspace
                    .read_file(tab.path.clone())
                    .ok()
                    .map(|text| (tab.path, text, tab.view_mode))
            })
            .collect();
        self.editors.restore(
            tabs,
            self.workbench_layout.editor_session.active_file.clone(),
        );
    }

    fn persist_workbench_layout(&mut self) -> Result<(), String> {
        self.workbench_layout.editor_session = self.editors.state();
        save_workbench_layout(&self.workbench_layout)
    }

    pub fn refresh_project_entries(&mut self) -> Result<(), String> {
        self.project_root = self
            .workspace
            .project_root_display()
            .map(ToString::to_string);
        self.project_entries = self.workspace.project_entries()?;
        Ok(())
    }

    pub fn refresh_analysis(&mut self) -> Result<(), String> {
        let analysis = self.workspace.analyze(self.editors.dirty_overlays())?;
        self.diagnostics = analysis.diagnostics.clone();
        self.analysis = Some(analysis);
        Ok(())
    }

    fn save_active_file_after_text_edit(&mut self) -> Result<(), String> {
        let Some(buffer) = self.editors.active_buffer().cloned() else {
            return Ok(());
        };
        self.workspace
            .write_file(buffer.path.clone(), buffer.text.as_bytes())?;
        self.editors.mark_saved(&buffer.path, buffer.text);
        match self.refresh_analysis() {
            Ok(()) => {
                self.sync_preview_source();
                Ok(())
            }
            Err(error) => {
                self.status = error;
                self.sync_preview_source();
                Ok(())
            }
        }
    }

    pub fn flush_autosave(&mut self) -> Result<(), String> {
        let had_dirty_buffers = !self.editors.dirty_buffers().is_empty();
        for buffer in self.editors.dirty_buffers() {
            self.workspace
                .write_file(buffer.path.clone(), buffer.text.as_bytes())?;
            self.editors.mark_saved(&buffer.path, buffer.text);
        }
        if had_dirty_buffers {
            self.refresh_analysis()?;
            self.sync_preview_source();
        }
        Ok(())
    }

    fn sync_preview_source(&mut self) {
        let source = self.active_sequence_source();
        self.preview.sync_source(source, self.analysis.as_ref());
    }

    fn active_document_descriptor(&self) -> Option<DocumentDescriptor> {
        let path = self.editors.active_file()?.clone();
        self.workspace
            .inspect_document(path, self.editors.dirty_overlays())
            .ok()
    }

    fn active_gui_document(
        &self,
        descriptor: Option<&DocumentDescriptor>,
    ) -> Option<ActiveGuiDocument> {
        let buffer = self.editors.active_buffer()?;
        if buffer.view_mode != EditorViewMode::Gui {
            return None;
        }
        let diagnostics = self
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.path == buffer.path)
            .cloned()
            .collect::<Vec<_>>();
        let Some(descriptor) = descriptor else {
            return Some(ActiveGuiDocument::Blocked {
                reason: "Text could not be parsed as a Dawn document.".to_string(),
                diagnostics,
            });
        };
        let overlays = self.editors.dirty_overlays();
        if let Some(object_key) = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
        {
            return Some(
                match self
                    .workspace
                    .sequence_document(buffer.path.clone(), object_key, overlays)
                {
                    Ok(document) => ActiveGuiDocument::Sequence(document),
                    Err(error) => ActiveGuiDocument::Blocked {
                        reason: error,
                        diagnostics,
                    },
                },
            );
        }
        if let Some(object_key) = descriptor.default_object_keys.get(&DocumentViewId::Layout) {
            return Some(
                match self
                    .workspace
                    .layout_document(buffer.path.clone(), object_key, overlays)
                {
                    Ok(document) => ActiveGuiDocument::Layout(document),
                    Err(error) => ActiveGuiDocument::Blocked {
                        reason: error,
                        diagnostics,
                    },
                },
            );
        }
        if descriptor
            .default_object_keys
            .contains_key(&DocumentViewId::Fixture)
        {
            return Some(
                match self
                    .workspace
                    .fixture_document(buffer.path.clone(), None, overlays)
                {
                    Ok(document) => ActiveGuiDocument::Fixture(document),
                    Err(error) => ActiveGuiDocument::Blocked {
                        reason: error,
                        diagnostics,
                    },
                },
            );
        }
        Some(ActiveGuiDocument::Blocked {
            reason: "This document has no GUI editor view.".to_string(),
            diagnostics,
        })
    }

    fn apply_sequence_gui_edit(&mut self, edit: SequenceGuiEditDto) -> Result<(), String> {
        let path = self.active_path_for_gui_edit()?;
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), self.editors.dirty_overlays())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .ok_or_else(|| "active document is not a sequence".to_string())?
            .clone();
        let edit = match edit {
            SequenceGuiEditDto::AddEffect {
                script_path,
                target,
                start_ms,
            } => SequenceDocumentEdit::AddEffect {
                script_path,
                target: target.into(),
                start_ms: start_ms.into(),
            },
            SequenceGuiEditDto::MoveEffect {
                id,
                start_ms,
                target,
            } => SequenceDocumentEdit::MoveEffect {
                id,
                start_ms: start_ms.into(),
                target: target.map(Into::into),
            },
            SequenceGuiEditDto::ResizeEffect {
                id,
                start_ms,
                duration_ms,
            } => SequenceDocumentEdit::ResizeEffect {
                id,
                start_ms: start_ms.into(),
                duration_ms: duration_ms.into(),
            },
            SequenceGuiEditDto::DeleteEffect { id } => SequenceDocumentEdit::DeleteEffect { id },
            SequenceGuiEditDto::RetargetEffect { id, target } => {
                SequenceDocumentEdit::RetargetEffect {
                    id,
                    target: target.into(),
                }
            }
        };
        let analysis = self
            .analysis
            .as_ref()
            .ok_or_else(|| "project analysis is not available".to_string())?;
        let base_content = self.active_buffer_text()?;
        let outcome = self.workspace.apply_sequence_edit(
            path,
            &object_key,
            edit,
            base_content,
            self.editors.dirty_overlays(),
            analysis,
        )?;
        self.commit_active_gui_text(outcome.serialized_content)
    }

    fn apply_layout_gui_edit(&mut self, edit: LayoutGuiEditDto) -> Result<(), String> {
        let path = self.active_path_for_gui_edit()?;
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), self.editors.dirty_overlays())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Layout)
            .ok_or_else(|| "active document is not a layout".to_string())?
            .clone();
        let mut document = self.workspace.layout_document(
            path.clone(),
            &object_key,
            self.editors.dirty_overlays(),
        )?;
        match edit {
            LayoutGuiEditDto::UpdatePlacementTransform { id, transform } => {
                let id = dawn_project::model::FixtureId(id);
                let placement = document
                    .fixtures
                    .iter_mut()
                    .find(|fixture| fixture.id == id)
                    .ok_or_else(|| format!("fixture placement `{id}` was not found"))?;
                placement.transform = transform.into();
            }
        }
        let outcome = self.workspace.apply_layout_edit(
            path,
            &object_key,
            document,
            self.active_buffer_text()?,
            self.editors.dirty_overlays(),
        )?;
        self.commit_active_gui_text(outcome.serialized_content)
    }

    fn apply_fixture_gui_edit(&mut self, edit: FixtureGuiEditDto) -> Result<(), String> {
        let path = self.active_path_for_gui_edit()?;
        let mut document =
            self.workspace
                .fixture_document(path.clone(), None, self.editors.dirty_overlays())?;
        match edit {
            FixtureGuiEditDto::UpdateBulbSize {
                object_key,
                bulb_size,
            } => {
                let fixture = document
                    .fixtures
                    .iter_mut()
                    .find(|fixture| fixture.object_key == object_key)
                    .ok_or_else(|| format!("fixture `{object_key}` was not found"))?;
                fixture.bulb_size = bulb_size.max(0.05);
            }
            FixtureGuiEditDto::MovePoint {
                object_key,
                point_index,
                point,
            } => {
                let fixture = document
                    .fixtures
                    .iter_mut()
                    .find(|fixture| fixture.object_key == object_key)
                    .ok_or_else(|| format!("fixture `{object_key}` was not found"))?;
                let Geometry::Points { points } = &mut fixture.geometry else {
                    return Err("only point geometry can be edited in this milestone".to_string());
                };
                let target = points
                    .get_mut(point_index as usize)
                    .ok_or_else(|| format!("point `{point_index}` was not found"))?;
                *target = point.into();
            }
        }
        let outcome = self.workspace.apply_fixture_edit(
            path,
            document,
            self.active_buffer_text()?,
            self.editors.dirty_overlays(),
        )?;
        self.commit_active_gui_text(outcome.serialized_content)
    }

    fn active_path_for_gui_edit(&self) -> Result<Utf8PathBuf, String> {
        self.editors
            .active_file()
            .cloned()
            .ok_or_else(|| "no active document".to_string())
    }

    fn active_buffer_text(&self) -> Result<String, String> {
        self.editors
            .active_buffer()
            .map(|buffer| buffer.text.clone())
            .ok_or_else(|| "no active document".to_string())
    }

    fn commit_active_gui_text(&mut self, text: String) -> Result<(), String> {
        self.editors.replace_active_text_from_edit(text);
        self.save_active_file_after_text_edit()
    }

    fn active_sequence_source(
        &self,
    ) -> Option<(SequenceKey, dawn_project::document::SequenceDocument)> {
        let path = self.editors.active_file()?.clone();
        let overlays = self.editors.dirty_overlays();
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), overlays.clone())
            .ok()?;
        let object_key = descriptor
            .default_object_keys
            .get(&dawn_project::document::DocumentViewId::Sequence)?;
        let document = self
            .workspace
            .sequence_document(path.clone(), object_key, overlays)
            .ok()?;
        Some((
            SequenceKey {
                path,
                object_key: document.object_key.clone(),
            },
            document,
        ))
    }
}
