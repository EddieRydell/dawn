use crate::actions::AppAction;
use crate::editor_session::{EditorBuffer, EditorSession};
use crate::layout_persistence::{load_panel_layout, save_panel_layout, PanelLayout};
use crate::workspace::{
    AnalysisState, FixtureDocumentEditResponse, LanguageProblem, LayoutDocumentEditResponse,
    ProjectState, WorkspaceService,
};
use dawn_project::document::{DocumentDescriptor, DocumentViewId, FixtureDocument, LayoutDocument};
use dawn_project::path::ProjectPath;

#[derive(Debug, Clone)]
pub struct PlaybackState {
    pub is_playing: bool,
    pub time: f64,
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
    pub panel_layout: PanelLayout,
    pub playback: PlaybackState,
    pub project: Option<ProjectState>,
    pub analysis: Option<AnalysisState>,
    pub diagnostics: Vec<LanguageProblem>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct AppSnapshot {
    pub project: Option<ProjectState>,
    pub analysis: Option<AnalysisState>,
    pub diagnostics: Vec<LanguageProblem>,
    pub panel_layout: PanelLayout,
    pub playback: PlaybackState,
    pub tabs: Vec<EditorBuffer>,
    pub active_file: Option<ProjectPath>,
    pub active_buffer: Option<EditorBuffer>,
    pub active_descriptor: Option<DocumentDescriptor>,
    pub active_layout_document: Option<LayoutDocument>,
    pub active_fixture_document: Option<FixtureDocument>,
    pub preview_frame: Option<crate::workspace::FrameSummary>,
    pub status: String,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            workspace: WorkspaceService::default(),
            editors: EditorSession::default(),
            panel_layout: load_panel_layout(),
            playback: PlaybackState::default(),
            project: None,
            analysis: None,
            diagnostics: Vec::new(),
            status: "No project open".to_string(),
        }
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
            let object_key = descriptor
                .default_object_keys
                .get(&DocumentViewId::Fixture)
                .map(String::as_str);
            self.workspace
                .fixture_document(path.clone(), object_key, self.editors.dirty_overlays())
                .ok()
        });
        let preview_frame = self.workspace.render_frame(self.playback.time).ok();
        AppSnapshot {
            project: self.project.clone(),
            analysis: self.analysis.clone(),
            diagnostics: self.diagnostics.clone(),
            panel_layout: self.panel_layout.clone(),
            playback: self.playback.clone(),
            tabs: self.editors.tabs(),
            active_file,
            active_buffer,
            active_descriptor,
            active_layout_document,
            active_fixture_document,
            preview_frame,
            status: self.status.clone(),
        }
    }

    pub fn dispatch(&mut self, action: AppAction) -> Result<(), String> {
        match action {
            AppAction::OpenProject(path) => {
                self.flush_autosave()?;
                let project = self.workspace.open_project(path)?;
                self.project = Some(project);
                self.editors.clear();
                self.refresh_analysis()?;
                self.status = "Project opened".to_string();
            }
            AppAction::NewProject => {
                self.status = "New project is not implemented yet".to_string();
            }
            AppAction::CloseProject => {
                self.flush_autosave()?;
                self.workspace.close_project();
                self.project = None;
                self.analysis = None;
                self.diagnostics.clear();
                self.editors.clear();
                self.status = "No project open".to_string();
            }
            AppAction::OpenSettings => {
                self.status = "Settings are not implemented yet".to_string();
            }
            AppAction::Reload | AppAction::Check => {
                self.project = Some(self.workspace.snapshot()?);
                self.refresh_analysis()?;
                self.status = "Project checked".to_string();
            }
            AppAction::OpenFile(path) => {
                let text = self.workspace.read_file(path.clone())?;
                self.editors.open_file(path, text);
                self.refresh_analysis()?;
            }
            AppAction::CloseFile(path) => {
                self.editors.close_file(&path);
                self.refresh_analysis()?;
            }
            AppAction::SetActiveFile(path) => self.editors.set_active_file(path),
            AppAction::UpdateActiveText(text) => {
                self.editors.update_active_text(text);
                self.refresh_analysis()?;
            }
            AppAction::SaveActiveFile => self.save_active_file()?,
            AppAction::SetEditorViewMode { path, mode } => self.editors.set_view_mode(&path, mode),
            AppAction::CycleTabs { reverse } => self.editors.cycle_tabs(reverse),
            AppAction::RenamePath { path, new_name } => {
                self.flush_autosave()?;
                let result = self.workspace.rename_path(path.clone(), &new_name)?;
                self.project = Some(result.project);
                let moves = result
                    .moved
                    .iter()
                    .map(|moved| {
                        Ok((
                            ProjectPath::parse(&moved.old_path)?,
                            ProjectPath::parse(&moved.new_path)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                self.editors.reconcile_moved_paths(&moves);
                self.refresh_analysis()?;
            }
            AppAction::CreateFile { parent, name } => {
                self.flush_autosave()?;
                let (project, path) = self.workspace.create_file(parent, &name)?;
                self.project = Some(project);
                let text = self.workspace.read_file(path.clone())?;
                self.editors.open_file(path, text);
                self.refresh_analysis()?;
            }
            AppAction::CreateDirectory { parent, name } => {
                self.flush_autosave()?;
                let (project, _) = self.workspace.create_directory(parent, &name)?;
                self.project = Some(project);
                self.refresh_analysis()?;
            }
            AppAction::DeletePath(path) => {
                self.flush_autosave()?;
                self.project = Some(self.workspace.delete_path(path.clone())?);
                self.editors.reconcile_deleted_path(&path);
                self.refresh_analysis()?;
            }
            AppAction::MovePaths { paths, new_parent } => {
                self.flush_autosave()?;
                let result = self.workspace.move_paths(paths, new_parent)?;
                self.project = Some(result.project);
                let moves = result
                    .moved
                    .iter()
                    .map(|moved| {
                        Ok((
                            ProjectPath::parse(&moved.old_path)?,
                            ProjectPath::parse(&moved.new_path)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                self.editors.reconcile_moved_paths(&moves);
                self.refresh_analysis()?;
            }
            AppAction::NudgeLayoutFixture { id, dx, dy } => {
                self.edit_active_layout(|document| {
                    if let Some(fixture) = document
                        .fixtures
                        .iter_mut()
                        .find(|fixture| fixture.id == id)
                    {
                        fixture.transform.position.x += dx;
                        fixture.transform.position.y += dy;
                    }
                })?;
            }
            AppAction::DuplicateLayoutFixture { id } => {
                self.edit_active_layout(|document| {
                    if let Some(fixture) = document
                        .fixtures
                        .iter()
                        .find(|fixture| fixture.id == id)
                        .cloned()
                    {
                        let mut duplicate = fixture;
                        duplicate.id = unique_name(
                            &duplicate.id,
                            document.fixtures.iter().map(|fixture| fixture.id.as_str()),
                        );
                        duplicate.transform.position.x += 1.0;
                        duplicate.transform.position.y += 1.0;
                        document.fixtures.push(duplicate);
                    }
                })?;
            }
            AppAction::DeleteLayoutFixture { id } => {
                self.edit_active_layout(|document| {
                    document.fixtures.retain(|fixture| fixture.id != id);
                    for group in &mut document.groups {
                        group.members.retain(|member| member != &id);
                    }
                })?;
            }
            AppAction::AdjustFixtureBulb { object_key, delta } => {
                self.edit_active_fixture(|document| {
                    if let Some(fixture) = document
                        .fixtures
                        .iter_mut()
                        .find(|fixture| fixture.object_key == object_key)
                    {
                        fixture.bulb_size = (fixture.bulb_size + delta).max(0.05);
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
                self.playback.time = time.clamp(0.0, 30.0);
                let _ = self.workspace.render_frame(self.playback.time)?;
            }
            AppAction::ToggleLeftPane => {
                self.panel_layout.left_visible = !self.panel_layout.left_visible;
                save_panel_layout(&self.panel_layout)?;
            }
            AppAction::ToggleRightPane => {
                self.panel_layout.right_visible = !self.panel_layout.right_visible;
                save_panel_layout(&self.panel_layout)?;
            }
            AppAction::SetRightPaneTab(tab) => {
                self.panel_layout.active_right_tab = tab;
                save_panel_layout(&self.panel_layout)?;
            }
            AppAction::ResetLayout => {
                self.panel_layout.reset();
                save_panel_layout(&self.panel_layout)?;
            }
        }
        Ok(())
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
            LayoutDocumentEditResponse::Applied {
                serialized_content,
                analysis,
                ..
            } => {
                self.editors.update_active_text(serialized_content);
                self.diagnostics = analysis.diagnostics.clone();
                self.analysis = Some(analysis);
                self.status = "Layout edit applied".to_string();
            }
            LayoutDocumentEditResponse::Blocked {
                diagnostics,
                message,
            } => {
                self.diagnostics = diagnostics;
                self.status = message;
            }
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
            FixtureDocumentEditResponse::Applied {
                serialized_content,
                analysis,
                ..
            } => {
                self.editors.update_active_text(serialized_content);
                self.diagnostics = analysis.diagnostics.clone();
                self.analysis = Some(analysis);
                self.status = "Fixture edit applied".to_string();
            }
            FixtureDocumentEditResponse::Blocked {
                diagnostics,
                message,
            } => {
                self.diagnostics = diagnostics;
                self.status = message;
            }
        }
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

fn is_dawn_path(path: &ProjectPath) -> bool {
    path.as_path()
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == "dawn")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use dawn_project::path::ProjectPath;

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
            .dispatch(crate::actions::AppAction::OpenFile(ProjectPath::new(
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
