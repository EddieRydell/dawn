use std::collections::HashMap;
use std::path::PathBuf;

use crate::actions::AppAction;
use crate::editor_session::{EditorBuffer, EditorSession};
use crate::layout_persistence::{load_workbench_layout, save_workbench_layout, WorkbenchLayout};
use crate::ui::theme;
use crate::workspace::WorkspaceService;
use dawn_project::analysis::{ProjectAnalysis, ProjectDiagnostic};
use dawn_project::document::{
    DocumentDescriptor, DocumentEditResult, DocumentViewId, FixtureDefinitionDocument,
    FixtureDocument, LayoutDocument, LayoutFixturePlacement, LayoutFixtureRef,
    ResolvedLayoutFixture,
};
use dawn_project::fs::WorkspaceEntry;
use dawn_project::model::{ColorModel, FixtureId, Geometry, Point3, Rotation3, Scale3, Transform};
use dawn_project::path::Utf8PathBuf;
use dawn_project::render::{GeometryRenderBounds, GeometryRenderPlan, GeometryRenderPoint};

#[derive(Debug, Clone)]
pub struct PlaybackState {
    pub is_playing: bool,
    pub time: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewRigKind {
    Strand,
    VerticalStrand,
    Circle,
    Grid,
}

impl PreviewRigKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Strand => "Strand",
            Self::VerticalStrand => "Vertical",
            Self::Circle => "Circle",
            Self::Grid => "Grid",
        }
    }
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            is_playing: false,
            time: 0.0,
        }
    }
}

#[derive(Debug)]
pub struct AppModel {
    pub workspace: WorkspaceService,
    pub editors: EditorSession,
    pub workbench_layout: WorkbenchLayout,
    pub playback: PlaybackState,
    pub project_root: Option<String>,
    pub project_entries: Vec<WorkspaceEntry>,
    pub analysis: Option<ProjectAnalysis>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub pending_layout_fixture_import: Option<PendingLayoutFixtureImport>,
    pub pending_layout_fixture_name: Option<PendingLayoutFixtureName>,
    pub selected_fixture_definitions: HashMap<Utf8PathBuf, String>,
    pub selected_preview_fixture: Option<FixtureId>,
    pub preview_rig: PreviewRigKind,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct AppSnapshot {
    pub project_root: Option<String>,
    pub project_entries: Vec<WorkspaceEntry>,
    pub analysis: Option<ProjectAnalysis>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub workbench_layout: WorkbenchLayout,
    pub playback: PlaybackState,
    pub tabs: Vec<EditorBuffer>,
    pub active_file: Option<Utf8PathBuf>,
    pub active_buffer: Option<EditorBuffer>,
    pub active_descriptor: Option<DocumentDescriptor>,
    pub active_layout_document: Option<LayoutDocument>,
    pub active_fixture_document: Option<FixtureDocument>,
    pub pending_layout_fixture_import: Option<PendingLayoutFixtureImport>,
    pub pending_layout_fixture_name: Option<PendingLayoutFixtureName>,
    pub selected_preview_fixture: Option<FixtureId>,
    pub preview_rig: PreviewRigKind,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct PendingLayoutFixtureImport {
    pub layout_path: Utf8PathBuf,
    pub layout_object_key: String,
    pub selected_file: PathBuf,
    pub x: f64,
    pub y: f64,
    pub fixtures: Vec<FixtureDefinitionDocument>,
}

#[derive(Debug, Clone)]
pub struct PendingLayoutFixtureName {
    pub suggested_name: String,
    pub context: String,
    pub request: PendingLayoutFixtureNameRequest,
}

#[derive(Debug, Clone)]
pub enum PendingLayoutFixtureNameRequest {
    Inline {
        x: f64,
        y: f64,
    },
    Import {
        selected_file: PathBuf,
        object_key: String,
        x: f64,
        y: f64,
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
            playback: PlaybackState::default(),
            project_root: None,
            project_entries: Vec::new(),
            analysis: None,
            diagnostics: Vec::new(),
            pending_layout_fixture_import: None,
            pending_layout_fixture_name: None,
            selected_fixture_definitions: HashMap::new(),
            selected_preview_fixture: None,
            preview_rig: PreviewRigKind::Strand,
            status: "No project open".to_string(),
        };
        if let Some(path) = last_project_root {
            match model.open_project(path, false, true) {
                Ok(()) => {
                    model.status = "Project restored".to_string();
                }
                Err(error) => {
                    model.status = format!("Could not restore last project: {error}");
                }
            }
        }
        model
    }
}

impl AppModel {
    pub fn snapshot(&self) -> AppSnapshot {
        let active_file = self.editors.active_file().cloned();
        let active_buffer = self.editors.active_buffer().cloned();
        let active_dawn_file = active_file.as_ref().filter(|path| is_dawn_path(path));
        let active_descriptor = active_dawn_file.and_then(|path| {
            self.workspace
                .inspect_document(path.clone(), self.editors.dirty_overlays())
                .ok()
        });
        let active_layout_document = active_file.as_ref().and_then(|path| {
            let descriptor = active_descriptor.as_ref()?;
            let object_key = descriptor
                .default_object_keys
                .get(&DocumentViewId::Layout)?;
            self.workspace
                .layout_document(path.clone(), object_key, self.editors.dirty_overlays())
                .ok()
        });
        let active_fixture_document = active_file.as_ref().and_then(|path| {
            let descriptor = active_descriptor.as_ref()?;
            if !descriptor
                .available_views
                .contains(&DocumentViewId::Fixture)
            {
                return None;
            }
            let object_key = self
                .selected_fixture_definitions
                .get(path)
                .map(String::as_str)
                .or_else(|| {
                    descriptor
                        .default_object_keys
                        .get(&DocumentViewId::Fixture)
                        .map(String::as_str)
                });
            self.workspace
                .fixture_document(path.clone(), object_key, self.editors.dirty_overlays())
                .ok()
        });
        AppSnapshot {
            project_root: self.project_root.clone(),
            project_entries: self.project_entries.clone(),
            analysis: self.analysis.clone(),
            diagnostics: self.diagnostics.clone(),
            workbench_layout: self.workbench_layout.clone(),
            playback: self.playback.clone(),
            tabs: self.editors.tabs(),
            active_file,
            active_buffer,
            active_descriptor,
            active_layout_document,
            active_fixture_document,
            pending_layout_fixture_import: self.pending_layout_fixture_import.clone(),
            pending_layout_fixture_name: self.pending_layout_fixture_name.clone(),
            selected_preview_fixture: self.selected_preview_fixture,
            preview_rig: self.preview_rig,
            status: self.status.clone(),
        }
    }

    pub fn dispatch(&mut self, action: AppAction) -> Result<(), String> {
        match action {
            AppAction::OpenProject(path) => {
                self.flush_autosave()?;
                self.open_project(path, true, false)?;
                self.status = "Project opened".to_string();
            }
            AppAction::NewProject => {
                self.status = "New project is not implemented yet".to_string();
            }
            AppAction::CloseProject => {
                self.flush_autosave()?;
                self.workspace.close_project();
                self.workbench_layout.last_project_root = None;
                self.project_root = None;
                self.project_entries.clear();
                self.analysis = None;
                self.diagnostics.clear();
                self.editors.clear();
                self.selected_fixture_definitions.clear();
                self.selected_preview_fixture = None;
                self.preview_rig = PreviewRigKind::Strand;
                self.pending_layout_fixture_import = None;
                self.pending_layout_fixture_name = None;
                self.workbench_layout.editor_session = self.editors.state();
                save_workbench_layout(&self.workbench_layout)?;
                self.status = "No project open".to_string();
            }
            AppAction::Quit => {
                self.flush_autosave()?;
            }
            AppAction::OpenSettings => {
                self.status = "Settings are not implemented yet".to_string();
            }
            AppAction::Reload | AppAction::Check => {
                self.refresh_project_entries()?;
                self.refresh_analysis()?;
                self.status = "Project checked".to_string();
            }
            AppAction::OpenFile(path) => {
                self.pending_layout_fixture_import = None;
                self.pending_layout_fixture_name = None;
                let text = self.workspace.read_file(path.clone())?;
                self.editors.open_file(path, text);
                self.refresh_analysis()?;
                self.persist_workbench_layout()?;
            }
            AppAction::CloseFile(path) => {
                self.pending_layout_fixture_import = None;
                self.pending_layout_fixture_name = None;
                self.editors.close_file(&path);
                self.refresh_analysis()?;
                self.persist_workbench_layout()?;
            }
            AppAction::SetActiveFile(path) => {
                self.pending_layout_fixture_import = None;
                self.pending_layout_fixture_name = None;
                let active_changed = self.editors.active_file() != Some(&path);
                self.editors.set_active_file(path);
                if active_changed {
                    self.persist_workbench_layout()?;
                }
            }
            AppAction::UpdateActiveText(text) => {
                self.editors.update_active_text(text);
                self.save_active_file()?;
            }
            AppAction::SaveActiveFile => self.save_active_file()?,
            AppAction::SetEditorViewMode { path, mode } => {
                self.editors.set_view_mode(&path, mode);
                self.persist_workbench_layout()?;
            }
            AppAction::CycleTabs { reverse } => {
                self.editors.cycle_tabs(reverse);
                self.persist_workbench_layout()?;
            }
            AppAction::RenamePath { path, new_name } => {
                self.flush_autosave()?;
                let moves = self.workspace.rename_path(path.clone(), &new_name)?;
                self.refresh_project_entries()?;
                self.editors.reconcile_moved_paths(&moves);
                self.reconcile_selected_fixture_paths(&moves);
                self.refresh_analysis()?;
                self.persist_workbench_layout()?;
            }
            AppAction::CreateFile { parent, name } => {
                self.flush_autosave()?;
                let path = self.workspace.create_file(parent, &name)?;
                self.refresh_project_entries()?;
                let text = self.workspace.read_file(path.clone())?;
                self.editors.open_file(path, text);
                self.refresh_analysis()?;
                self.persist_workbench_layout()?;
            }
            AppAction::CreateDirectory { parent, name } => {
                self.flush_autosave()?;
                self.workspace.create_directory(parent, &name)?;
                self.refresh_project_entries()?;
                self.refresh_analysis()?;
            }
            AppAction::DeletePath(path) => {
                self.flush_autosave()?;
                self.workspace.delete_path(path.clone())?;
                self.refresh_project_entries()?;
                self.editors.reconcile_deleted_path(&path);
                self.selected_fixture_definitions
                    .retain(|selected_path, _| {
                        selected_path != &path && !selected_path.starts_with(&path)
                    });
                self.refresh_analysis()?;
                self.persist_workbench_layout()?;
            }
            AppAction::MovePaths { paths, new_parent } => {
                self.flush_autosave()?;
                let moves = self.workspace.move_paths(paths, new_parent)?;
                self.refresh_project_entries()?;
                self.editors.reconcile_moved_paths(&moves);
                self.reconcile_selected_fixture_paths(&moves);
                self.refresh_analysis()?;
                self.persist_workbench_layout()?;
            }
            AppAction::NudgeLayoutFixtures {
                fixture_ids,
                dx,
                dy,
            } => {
                self.edit_active_layout(|document| {
                    for fixture in &mut document.fixtures {
                        if fixture_ids.contains(&fixture.id) {
                            fixture.transform.position.x += dx;
                            fixture.transform.position.y += dy;
                        }
                    }
                })?;
            }
            AppAction::DuplicateLayoutFixture { fixture_id } => {
                let Some(document) = self.snapshot().active_layout_document else {
                    self.status = "active editor is not a layout document".to_string();
                    return Ok(());
                };
                if next_fixture_id(&document).is_none() {
                    self.status = "No numeric fixture IDs are available".to_string();
                    return Ok(());
                }
                self.edit_active_layout(|document| {
                    if let Some(fixture) = document
                        .fixtures
                        .iter()
                        .find(|fixture| fixture.id == fixture_id)
                        .cloned()
                    {
                        let mut duplicate = fixture;
                        let Some(id) = next_fixture_id(document) else {
                            return;
                        };
                        duplicate.id = id;
                        duplicate.name = unique_display_name(
                            &format!("{} Copy", duplicate.name),
                            document
                                .fixtures
                                .iter()
                                .map(|fixture| fixture.name.as_str()),
                        );
                        duplicate.transform.position.x += theme::LAYOUT_DUPLICATE_OFFSET;
                        duplicate.transform.position.y += theme::LAYOUT_DUPLICATE_OFFSET;
                        document.fixtures.push(duplicate);
                    }
                })?;
            }
            AppAction::DeleteLayoutFixture { fixture_id } => {
                self.edit_active_layout(|document| {
                    document.fixtures.retain(|fixture| fixture.id != fixture_id);
                    for group in &mut document.groups {
                        group.members.retain(|member| *member != fixture_id);
                    }
                })?;
            }
            AppAction::CreateInlineLayoutFixture { x, y } => {
                self.pending_layout_fixture_import = None;
                let Some(document) = self.snapshot().active_layout_document else {
                    self.status = "active editor is not a layout document".to_string();
                    return Ok(());
                };
                self.pending_layout_fixture_name = Some(PendingLayoutFixtureName {
                    suggested_name: unique_display_name(
                        "Fixture",
                        document
                            .fixtures
                            .iter()
                            .map(|fixture| fixture.name.as_str()),
                    ),
                    context: "Inline fixture".to_string(),
                    request: PendingLayoutFixtureNameRequest::Inline { x, y },
                });
                self.status = "Name the new fixture".to_string();
            }
            AppAction::ConfirmLayoutFixtureName { name } => {
                let Some(pending) = self.pending_layout_fixture_name.take() else {
                    return Ok(());
                };
                let name = name.trim().to_string();
                if name.is_empty() {
                    self.status = "Fixture name cannot be empty".to_string();
                    self.pending_layout_fixture_name = Some(pending);
                    return Ok(());
                }
                let Some(document) = self.snapshot().active_layout_document else {
                    self.status =
                        "Fixture creation canceled because the active layout changed".to_string();
                    return Ok(());
                };
                if document
                    .fixtures
                    .iter()
                    .any(|fixture| fixture.name.trim() == name)
                {
                    self.status = format!("Fixture name `{name}` already exists");
                    self.pending_layout_fixture_name = Some(pending);
                    return Ok(());
                }
                if next_fixture_id(&document).is_none() {
                    self.status = "No numeric fixture IDs are available".to_string();
                    return Ok(());
                }
                match pending.request {
                    PendingLayoutFixtureNameRequest::Inline { x, y } => {
                        self.edit_active_layout(|document| {
                            if let Some(id) = next_fixture_id(document) {
                                document
                                    .fixtures
                                    .push(inline_layout_fixture(id, name, x, y));
                            }
                        })?;
                    }
                    PendingLayoutFixtureNameRequest::Import {
                        selected_file,
                        object_key,
                        x,
                        y,
                    } => {
                        self.import_layout_fixture(selected_file, object_key, name, x, y)?;
                    }
                }
            }
            AppAction::StartImportLayoutFixture {
                selected_file,
                x,
                y,
            } => {
                self.pending_layout_fixture_import = None;
                self.pending_layout_fixture_name = None;
                let snapshot = self.snapshot();
                let Some(layout) = snapshot.active_layout_document else {
                    self.status = "active editor is not a layout document".to_string();
                    return Ok(());
                };
                let fixture_document = match self.workspace.inspect_fixture_file(&selected_file) {
                    Ok((_path, document)) => document,
                    Err(error) => {
                        self.status = format!("Selected file could not be imported: {error}");
                        return Ok(());
                    }
                };
                match fixture_document.fixtures.as_slice() {
                    [] => {
                        self.status = "Selected file contains no fixture objects".to_string();
                    }
                    [fixture] => {
                        let object_key = fixture.object_key.clone();
                        self.pending_layout_fixture_name = Some(PendingLayoutFixtureName {
                            suggested_name: unique_display_name(
                                "Fixture",
                                layout.fixtures.iter().map(|fixture| fixture.name.as_str()),
                            ),
                            context: format!("{}  {}", fixture.name, fixture.geometry_summary),
                            request: PendingLayoutFixtureNameRequest::Import {
                                selected_file,
                                object_key,
                                x,
                                y,
                            },
                        });
                        self.status = "Name the imported fixture".to_string();
                    }
                    fixtures => {
                        self.pending_layout_fixture_import = Some(PendingLayoutFixtureImport {
                            layout_path: snapshot
                                .active_file
                                .expect("active layout documents come from active files"),
                            layout_object_key: layout.object_key,
                            selected_file,
                            x,
                            y,
                            fixtures: fixtures.to_vec(),
                        });
                        self.status = "Choose a fixture to import".to_string();
                    }
                }
            }
            AppAction::ConfirmImportLayoutFixture { object_key } => {
                let Some(pending) = self.pending_layout_fixture_import.take() else {
                    return Ok(());
                };
                let snapshot = self.snapshot();
                let Some(active_layout) = snapshot.active_layout_document else {
                    self.status =
                        "Fixture import canceled because the active layout changed".to_string();
                    return Ok(());
                };
                if snapshot.active_file.as_ref() != Some(&pending.layout_path)
                    || active_layout.object_key != pending.layout_object_key
                {
                    self.status =
                        "Fixture import canceled because the active layout changed".to_string();
                    return Ok(());
                }
                let Some(fixture) = pending
                    .fixtures
                    .iter()
                    .find(|fixture| fixture.object_key == object_key)
                else {
                    self.status =
                        "Fixture import canceled because the fixture was not found".to_string();
                    return Ok(());
                };
                self.pending_layout_fixture_name = Some(PendingLayoutFixtureName {
                    suggested_name: unique_display_name(
                        "Fixture",
                        active_layout
                            .fixtures
                            .iter()
                            .map(|fixture| fixture.name.as_str()),
                    ),
                    context: format!("{}  {}", fixture.name, fixture.geometry_summary),
                    request: PendingLayoutFixtureNameRequest::Import {
                        selected_file: pending.selected_file,
                        object_key,
                        x: pending.x,
                        y: pending.y,
                    },
                });
                self.status = "Name the imported fixture".to_string();
            }
            AppAction::CancelImportLayoutFixture => {
                self.pending_layout_fixture_import = None;
                self.status = "Fixture import canceled".to_string();
            }
            AppAction::CancelLayoutFixtureName => {
                self.pending_layout_fixture_name = None;
                self.status = "Fixture creation canceled".to_string();
            }
            AppAction::AdjustFixtureBulb { object_key, delta } => {
                self.edit_active_fixture(|document| {
                    if let Some(fixture) = document
                        .fixtures
                        .iter_mut()
                        .find(|fixture| fixture.object_key == object_key)
                    {
                        fixture.bulb_size =
                            (fixture.bulb_size + delta).max(theme::FIXTURE_MIN_BULB_SIZE);
                    }
                })?;
            }
            AppAction::SelectFixtureDefinition { object_key } => {
                if let Some(path) = self.editors.active_file().cloned() {
                    self.selected_fixture_definitions
                        .insert(path, object_key.clone());
                    self.status = format!("Selected fixture `{object_key}`");
                }
            }
            AppAction::NudgeFixtureGeometryHandles {
                object_key,
                handles,
                dx,
                dy,
            } => {
                self.edit_active_fixture(|document| {
                    if let Some(fixture) = document
                        .fixtures
                        .iter_mut()
                        .find(|fixture| fixture.object_key == object_key)
                    {
                        nudge_fixture_geometry_handles(&mut fixture.geometry, &handles, dx, dy);
                    }
                })?;
            }
            AppAction::DuplicateFixtureDefinition { object_key } => {
                self.edit_active_fixture(|document| {
                    if let Some(fixture) = document
                        .fixtures
                        .iter()
                        .find(|fixture| fixture.object_key == object_key)
                        .cloned()
                    {
                        let mut duplicate = fixture;
                        duplicate.object_key = unique_name(
                            &duplicate.object_key,
                            document
                                .fixtures
                                .iter()
                                .map(|fixture| fixture.object_key.as_str()),
                        );
                        duplicate.name = format!("{} Copy", duplicate.name);
                        document.fixtures.push(duplicate);
                    }
                })?;
            }
            AppAction::DeleteFixtureDefinition { object_key } => {
                self.edit_active_fixture(|document| {
                    document
                        .fixtures
                        .retain(|fixture| fixture.object_key != object_key);
                })?;
            }
            AppAction::OpenSequence(path) => self.workspace.open_sequence(path)?,
            AppAction::SelectPreviewFixture(fixture_id) => {
                self.selected_preview_fixture = fixture_id;
                self.status = match fixture_id {
                    Some(id) => format!("Preview target fixture `{id}`"),
                    None => "Preview target all fixtures".to_string(),
                };
            }
            AppAction::SelectPreviewRig(rig) => {
                self.preview_rig = rig;
                self.selected_preview_fixture = None;
                self.status = format!("Preview rig `{}`", rig.label());
            }
            AppAction::Play => self.playback.is_playing = true,
            AppAction::Pause => self.playback.is_playing = false,
            AppAction::Stop => {
                self.playback.is_playing = false;
                self.playback.time = 0.0;
            }
            AppAction::About => {
                self.status = "Dawn desktop IDE".to_string();
            }
            AppAction::Seek(time) => {
                self.playback.time = time.clamp(0.0, theme::PREVIEW_DURATION_SECONDS);
            }
            AppAction::ToggleProjectTree => {
                self.workbench_layout.project_tree_visible =
                    !self.workbench_layout.project_tree_visible;
                save_workbench_layout(&self.workbench_layout)?;
            }
            AppAction::ToggleInspector => {
                self.workbench_layout.inspector_visible = !self.workbench_layout.inspector_visible;
                save_workbench_layout(&self.workbench_layout)?;
            }
            AppAction::SetInspectorTab(tab) => {
                self.workbench_layout.active_inspector_tab = tab;
                save_workbench_layout(&self.workbench_layout)?;
            }
            AppAction::ResetLayout => {
                self.workbench_layout.reset();
                save_workbench_layout(&self.workbench_layout)?;
            }
        }
        Ok(())
    }

    fn reconcile_selected_fixture_paths(&mut self, moves: &[(Utf8PathBuf, Utf8PathBuf)]) {
        if self.selected_fixture_definitions.is_empty() {
            return;
        }
        let selected = std::mem::take(&mut self.selected_fixture_definitions);
        self.selected_fixture_definitions = selected
            .into_iter()
            .map(|(path, object_key)| {
                let moved = moves
                    .iter()
                    .find_map(|(old_path, new_path)| {
                        moved_workspace_path(&path, old_path, new_path)
                    })
                    .unwrap_or(path);
                (moved, object_key)
            })
            .collect();
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
        self.selected_fixture_definitions.clear();
        self.selected_preview_fixture = None;
        self.preview_rig = PreviewRigKind::Strand;
        self.pending_layout_fixture_import = None;
        self.pending_layout_fixture_name = None;
        if restore_editor_session {
            self.restore_editor_session();
        }
        self.refresh_analysis()?;
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

    fn edit_active_layout(&mut self, edit: impl FnOnce(&mut LayoutDocument)) -> Result<(), String> {
        let snapshot = self.snapshot();
        let Some(buffer) = snapshot.active_buffer else {
            return Ok(());
        };
        let Some(mut document) = snapshot.active_layout_document else {
            return Err("active editor is not a layout document".to_string());
        };
        let object_key = document.object_key.clone();
        edit(&mut document);
        match self.workspace.apply_layout_edit(
            buffer.path,
            &object_key,
            document,
            buffer.text,
            self.editors.dirty_overlays(),
            false,
        )? {
            DocumentEditResult::Applied(outcome) => {
                self.editors.update_active_text(outcome.serialized_content);
                self.save_active_file()?;
                let analysis = outcome.analysis;
                self.diagnostics = analysis.diagnostics.clone();
                self.analysis = Some(analysis);
                self.status = "Layout edit applied".to_string();
            }
            DocumentEditResult::Blocked(blocked) => {
                self.diagnostics = blocked.diagnostics;
                self.status = blocked.message;
            }
        }
        Ok(())
    }

    fn import_layout_fixture(
        &mut self,
        selected_file: PathBuf,
        object_key: String,
        name: String,
        x: f64,
        y: f64,
    ) -> Result<(), String> {
        let snapshot = self.snapshot();
        let Some(layout_path) = snapshot.active_file else {
            return Ok(());
        };
        let (import, is_absolute) =
            self.workspace
                .fixture_import_string(&layout_path, &selected_file, &object_key)?;
        self.edit_active_layout(|document| {
            if let Some(id) = next_fixture_id(document) {
                document
                    .fixtures
                    .push(imported_layout_fixture(id, name, import, x, y));
            }
        })?;
        if is_absolute {
            self.status = "Fixture imported with an absolute file import".to_string();
        } else {
            self.status = "Fixture imported".to_string();
        }
        Ok(())
    }

    fn edit_active_fixture(
        &mut self,
        edit: impl FnOnce(&mut FixtureDocument),
    ) -> Result<(), String> {
        let snapshot = self.snapshot();
        let Some(buffer) = snapshot.active_buffer else {
            return Ok(());
        };
        let Some(mut document) = snapshot.active_fixture_document else {
            return Err("active editor is not a fixture document".to_string());
        };
        edit(&mut document);
        match self.workspace.apply_fixture_edit(
            buffer.path,
            document,
            buffer.text,
            self.editors.dirty_overlays(),
            false,
        )? {
            DocumentEditResult::Applied(outcome) => {
                self.editors.update_active_text(outcome.serialized_content);
                self.save_active_file()?;
                let analysis = outcome.analysis;
                self.diagnostics = analysis.diagnostics.clone();
                self.analysis = Some(analysis);
                self.status = "Fixture edit applied".to_string();
            }
            DocumentEditResult::Blocked(blocked) => {
                self.diagnostics = blocked.diagnostics;
                self.status = blocked.message;
            }
        }
        Ok(())
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

    pub fn save_active_file(&mut self) -> Result<(), String> {
        let Some(buffer) = self.editors.active_buffer().cloned() else {
            return Ok(());
        };
        self.workspace
            .write_file(buffer.path.clone(), buffer.text.as_bytes())?;
        self.editors.mark_saved(&buffer.path, buffer.text);
        self.refresh_analysis()
    }

    pub fn flush_autosave(&mut self) -> Result<(), String> {
        for buffer in self.editors.dirty_buffers() {
            self.workspace
                .write_file(buffer.path.clone(), buffer.text.as_bytes())?;
            self.editors.mark_saved(&buffer.path, buffer.text);
        }
        Ok(())
    }
}

fn unique_name<'a>(base: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let existing = existing.collect::<std::collections::BTreeSet<_>>();
    for index in 1.. {
        let candidate = format!("{base}_copy_{index}");
        if !existing.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("unbounded iterator should find a unique name")
}

fn unique_display_name<'a>(base: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let existing = existing.collect::<std::collections::BTreeSet<_>>();
    if !existing.contains(base) {
        return base.to_string();
    }
    for index in 2.. {
        let candidate = format!("{base} {index}");
        if !existing.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("unbounded iterator should find a unique name")
}

fn nudge_fixture_geometry_handles(geometry: &mut Geometry, handles: &[usize], dx: f64, dy: f64) {
    match geometry {
        Geometry::Points { points } | Geometry::Lines { points, .. } => {
            for index in handles {
                if let Some(point) = points.get_mut(*index) {
                    point.x += dx;
                    point.y += dy;
                }
            }
        }
        Geometry::Arc { center, .. } => {
            if handles.contains(&0) {
                center.x += dx;
                center.y += dy;
            }
        }
    }
}

fn moved_workspace_path(
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

fn next_fixture_id(document: &LayoutDocument) -> Option<FixtureId> {
    let existing = document
        .fixtures
        .iter()
        .map(|fixture| fixture.id.0)
        .collect::<std::collections::BTreeSet<_>>();
    (1..=u32::MAX)
        .find(|id| !existing.contains(id))
        .map(FixtureId)
}

fn inline_layout_fixture(id: FixtureId, name: String, x: f64, y: f64) -> LayoutFixturePlacement {
    let geometry = Geometry::Points {
        points: vec![Point3::default()],
    };
    LayoutFixturePlacement {
        id,
        name: name.clone(),
        fixture: LayoutFixtureRef::Inline {
            name: "Fixture".to_string(),
            color_model: ColorModel::Rgb,
            bulb_size: 1.0,
            geometry: geometry.clone(),
        },
        resolved_fixture: resolved_layout_fixture(
            "Fixture".to_string(),
            ColorModel::Rgb,
            1.0,
            geometry,
            None,
        ),
        transform: placement_transform(x, y),
    }
}

fn imported_layout_fixture(
    id: FixtureId,
    name: String,
    import: String,
    x: f64,
    y: f64,
) -> LayoutFixturePlacement {
    LayoutFixturePlacement {
        id,
        name,
        fixture: LayoutFixtureRef::Import {
            import,
            object_key: None,
            source_path: None,
        },
        resolved_fixture: resolved_layout_fixture(
            "Imported Fixture".to_string(),
            ColorModel::Rgb,
            1.0,
            Geometry::Points {
                points: vec![Point3::default()],
            },
            None,
        ),
        transform: placement_transform(x, y),
    }
}

fn resolved_layout_fixture(
    name: String,
    color_model: ColorModel,
    bulb_size: f64,
    geometry: Geometry,
    object_key: Option<String>,
) -> ResolvedLayoutFixture {
    ResolvedLayoutFixture {
        name,
        color_model,
        bulb_size,
        geometry: geometry.clone(),
        geometry_summary: String::new(),
        render_plan: placeholder_render_plan(bulb_size),
        source_path: String::new(),
        object_key,
    }
}

fn placeholder_render_plan(bulb_size: f64) -> GeometryRenderPlan {
    GeometryRenderPlan {
        emitters: vec![GeometryRenderPoint {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }],
        guides: Vec::new(),
        bounds: GeometryRenderBounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
        },
        bulb_radius: bulb_size * 0.035,
    }
}

fn placement_transform(x: f64, y: f64) -> Transform {
    Transform {
        position: Point3 { x, y, z: 0.0 },
        rotation: Rotation3::default(),
        scale: Scale3::default(),
    }
}

fn is_dawn_path(path: &Utf8PathBuf) -> bool {
    path.as_std_path()
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == "dawn")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use dawn_project::path::Utf8PathBuf;

    use super::*;

    #[test]
    fn close_project_flushes_dirty_editor_buffers() {
        let root = temp_project_dir("app-model-autosave");
        std::fs::write(root.join("project.dawn"), "{}").unwrap();
        std::fs::write(root.join("notes.dawn"), "old").unwrap();
        let mut model = AppModel::default();
        model
            .dispatch(crate::actions::AppAction::OpenProject(root.clone()))
            .unwrap();
        model
            .dispatch(crate::actions::AppAction::OpenFile(Utf8PathBuf::from(
                "notes.dawn",
            )))
            .unwrap();
        model
            .dispatch(crate::actions::AppAction::UpdateActiveText(
                "new".to_string(),
            ))
            .unwrap();

        model
            .dispatch(crate::actions::AppAction::CloseProject)
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(root.join("notes.dawn")).unwrap(),
            "new"
        );
    }

    fn temp_project_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("dawn-desktop-{label}-{nanos}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
