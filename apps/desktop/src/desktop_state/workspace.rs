use std::fs;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};

use super::{
    DesktopState, FsEntryKind, LoadedProject, absolute_project_path, absolute_root_path,
    descriptor_for_path, lock_unpoisoned, path_matches_or_is_child, valid_child_name,
};
use crate::dto::{AppSnapshot, EditorViewMode, NewSequenceRequest};
use crate::dto::{
    WorkspaceExplorerState, WorkspacePathChangeImpact, WorkspacePathChangePlan,
    WorkspacePathChangeRequest, WorkspacePathOwnership,
};
use crate::persistence::{
    PersistedEditorViewStateUpdate, PersistedSequenceViewportStateUpdate, ProjectRestoreState,
};
use crate::project::{new_project_files, write_new_project_files};

impl DesktopState {
    pub fn open_project_path(&self, path: &str) -> AppSnapshot {
        let candidate = Utf8Path::new(path);
        let root = if candidate.is_dir() {
            candidate
        } else if candidate.file_name() == Some(dawn_package::MANIFEST_FILE) {
            candidate.parent().unwrap_or(candidate)
        } else {
            return self.snapshot_with_error(
                "project.open",
                path,
                "Open a project by selecting its dawn-package.json manifest.",
            );
        };
        self.load_working_copy(root)
    }

    pub fn save_editor_view_state(&self, update: PersistedEditorViewStateUpdate) -> AppSnapshot {
        let snapshot = self.snapshot();
        let Some(project_root) = snapshot.project_root.clone() else {
            return snapshot;
        };
        match self.persistence.record_editor_state(&project_root, update) {
            Ok(()) => snapshot,
            Err(error) => {
                self.set_persistence_error(format!("Editor state was not saved: {error}"))
            }
        }
    }

    pub fn save_sequence_viewport_state(
        &self,
        update: PersistedSequenceViewportStateUpdate,
    ) -> AppSnapshot {
        let snapshot = self.snapshot();
        let Some(project_root) = snapshot.project_root.clone() else {
            return snapshot;
        };
        match self
            .persistence
            .record_sequence_viewport(&project_root, update)
        {
            Ok(()) => snapshot,
            Err(error) => {
                self.set_persistence_error(format!("Sequence view state was not saved: {error}"))
            }
        }
    }

    pub fn restored_view_state(&self) -> ProjectRestoreState {
        let snapshot = self.snapshot();
        let Some(project_root) = snapshot.project_root.as_deref() else {
            return ProjectRestoreState {
                editor_states: Default::default(),
                sequence_viewports: Default::default(),
            };
        };
        self.persistence.restore_view_state(project_root)
    }

    pub fn open_file_path(&self, path: &str) -> AppSnapshot {
        let Some(root) = self.project_root_path() else {
            return self.snapshot();
        };
        let relative = Utf8PathBuf::from(path);
        let Some(absolute) = absolute_root_path(&root, &relative) else {
            return self.snapshot_with_error("file.open", path, "Invalid project file path");
        };
        let known = lock_unpoisoned(&self.workspace)
            .documents
            .contains_key(&relative);
        if !known {
            let text = match fs::read_to_string(&absolute) {
                Ok(text) => text,
                Err(error) => {
                    return self.snapshot_with_error("file.open", path, &error.to_string());
                }
            };
            lock_unpoisoned(&self.workspace).documents.insert(
                relative.clone(),
                super::workspace_state::WorkingDocument::new(&relative, text),
            );
        }
        let descriptor = self
            .project_session()
            .and_then(|project| descriptor_for_path(&project, &relative));
        {
            let mut workspace = lock_unpoisoned(&self.workspace);
            if !workspace.tabs.contains(&relative) {
                workspace.tabs.push(relative);
            }
            workspace.view.active_file = Some(path.to_string());
        }
        self.update_snapshot(|snapshot| {
            snapshot.active_document_descriptor = descriptor;
            snapshot
                .workspace_explorer
                .recent_files
                .retain(|item| item != path);
            snapshot
                .workspace_explorer
                .recent_files
                .insert(0, path.into());
            snapshot.workspace_explorer.recent_files.truncate(20);
        })
    }

    pub fn save_workspace_explorer_state(&self, state: WorkspaceExplorerState) -> AppSnapshot {
        self.update_snapshot(|snapshot| {
            snapshot.workspace_explorer = state;
        })
    }

    pub fn plan_workspace_path_change(
        &self,
        request: WorkspacePathChangeRequest,
    ) -> Result<WorkspacePathChangePlan, String> {
        let snapshot = self.snapshot();
        if request.project_revision != snapshot.project_revision {
            return Err("The project changed; plan the path operation again.".to_string());
        }
        let project = self
            .project_session()
            .ok_or_else(|| "No project is open.".to_string())?;
        let plan = dawn_project_io::plan_path_change(
            &project,
            Utf8Path::new(&request.source),
            Utf8Path::new(&request.destination),
        )?;
        let open_files = snapshot
            .tabs
            .iter()
            .filter(|buffer| path_matches_or_is_child(&buffer.path, &request.source))
            .map(|buffer| buffer.path.clone())
            .collect();
        let recent_files = snapshot
            .workspace_explorer
            .recent_files
            .iter()
            .filter(|path| path_matches_or_is_child(path, &request.source))
            .cloned()
            .collect();
        let restore = snapshot
            .project_root
            .as_deref()
            .map(|root| self.persistence.restore_view_state(root));
        let persisted_state = restore
            .into_iter()
            .flat_map(|state| {
                state
                    .editor_states
                    .into_keys()
                    .chain(state.sequence_viewports.into_keys())
            })
            .filter(|path| path_matches_or_is_child(path, &request.source))
            .collect();
        Ok(WorkspacePathChangePlan {
            request,
            structural: plan.structural,
            ownership: match plan.ownership {
                dawn_project_io::PathChangeOwnership::Project => WorkspacePathOwnership::Project,
                dawn_project_io::PathChangeOwnership::PathDependency {
                    module_id,
                    module_root,
                } => WorkspacePathOwnership::PathDependency {
                    module_id: module_id.to_string(),
                    module_root,
                },
            },
            impact: WorkspacePathChangeImpact {
                documents: plan.impact.documents,
                imports: plan.impact.imports,
                manifests: plan.impact.manifests,
                assets: plan.impact.assets,
                modules: plan.impact.modules,
                open_files,
                recent_files,
                persisted_state,
            },
        })
    }

    pub fn apply_workspace_path_change(
        &self,
        request: WorkspacePathChangeRequest,
    ) -> Result<AppSnapshot, String> {
        let _authoring = self.settled_authoring();
        let description = self.plan_workspace_path_change(request.clone())?;
        if description.structural
            && lock_unpoisoned(&self.workspace)
                .documents
                .values()
                .any(|document| document.buffer.dirty)
        {
            return Err(
                "Structural path changes require every open text buffer to be saved.".to_string(),
            );
        }
        let project = self
            .project_session()
            .ok_or_else(|| "No project is open.".to_string())?;
        let plan = dawn_project_io::plan_path_change(
            &project,
            Utf8Path::new(&request.source),
            Utf8Path::new(&request.destination),
        )?;
        self.working_copy.invalidate_pending();
        self.render_refresh.invalidate_pending();
        let candidate = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            dawn_project_io::apply_path_change(&project, &plan)?
        };
        let root = candidate.source.project_root().to_string();
        self.persistence
            .remap_project_paths(&root, &request.source, &request.destination)?;
        let candidate = std::sync::Arc::new(candidate);
        lock_unpoisoned(&self.gui_history).clear();
        let entries = super::workspace_entries(&candidate);
        let package = super::package_status(Utf8Path::new(&root), Some(&candidate));
        let remap = |path: &str| remap_workspace_path(path, &request.source, &request.destination);
        {
            let mut workspace = lock_unpoisoned(&self.workspace);
            workspace.documents = std::mem::take(&mut workspace.documents)
                .into_iter()
                .map(|(path, mut document)| {
                    let path = Utf8PathBuf::from(remap(path.as_str()));
                    document.buffer.path = path.to_string();
                    document.buffer.name = path.file_name().unwrap_or(path.as_str()).to_string();
                    (path, document)
                })
                .collect();
            for (path, document) in &mut workspace.documents {
                if document.buffer.dirty {
                    continue;
                }
                if let Some(bytes) =
                    super::working_copy::read_disk(&Utf8Path::new(&root).join(path))?
                {
                    let text =
                        String::from_utf8(bytes.clone()).map_err(|error| error.to_string())?;
                    document.observed = Some(bytes);
                    document.edit(text);
                }
            }
            workspace.tabs = workspace
                .tabs
                .iter()
                .map(|path| Utf8PathBuf::from(remap(path.as_str())))
                .collect();
            workspace.view.active_file = workspace.view.active_file.as_deref().map(remap);
            workspace.view.project_revision += 1;
            workspace.view.state_revision += 1;
            workspace.typed_revision = Some(workspace.view.project_revision);
            workspace.project = LoadedProject::Ready(std::sync::Arc::clone(&candidate));
        }
        let snapshot = self.update_snapshot(|snapshot| {
            snapshot.active_document_descriptor = snapshot
                .active_file
                .as_deref()
                .and_then(|path| super::descriptor_for_path(&candidate, Utf8Path::new(path)));
            snapshot.workspace_explorer.expanded_paths = snapshot
                .workspace_explorer
                .expanded_paths
                .iter()
                .map(|path| remap(path))
                .collect();
            snapshot.workspace_explorer.recent_files = snapshot
                .workspace_explorer
                .recent_files
                .iter()
                .map(|path| remap(path))
                .collect();
            snapshot.project_entries = entries;
            snapshot.package = package;
            snapshot.status = if description.structural {
                "Structural path change applied; GUI undo and redo history were cleared."
                    .to_string()
            } else {
                "Path change applied.".to_string()
            };
        });
        self.schedule_render_refresh(candidate);
        Ok(snapshot)
    }

    pub fn set_active_file_path(&self, path: &str) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        self.open_file_path(path)
    }

    pub(super) fn close_file_path(&self, path: &str) -> AppSnapshot {
        let next = {
            let mut workspace = lock_unpoisoned(&self.workspace);
            let Some(index) = workspace.tabs.iter().position(|item| item.as_str() == path) else {
                return workspace.snapshot();
            };
            workspace.tabs.remove(index);
            if workspace.view.active_file.as_deref() == Some(path) {
                workspace.view.active_file = workspace
                    .tabs
                    .get(index)
                    .or_else(|| workspace.tabs.last())
                    .map(ToString::to_string);
            }
            workspace.view.active_file.clone()
        };
        if let Some(next) = next {
            self.open_file_path(&next)
        } else {
            self.update_snapshot(|snapshot| snapshot.active_document_descriptor = None)
        }
    }

    pub fn set_editor_view_mode(&self, mode: EditorViewMode) -> AppSnapshot {
        if matches!(mode, EditorViewMode::Gui) {
            if let Err(error) = self.reconcile_external_files() {
                return self.snapshot_with_error("project.reconcile", "", &error);
            }
            let _authoring = self.settled_authoring();
            if self.project_session().is_none() {
                self.focus_first_diagnostic();
                return self.update_snapshot(|snapshot| {
                    snapshot.settings.editor_view_mode = EditorViewMode::Text;
                    snapshot.workspace_layout.active_sidebar_view =
                        crate::dto::SidebarView::Problems;
                    snapshot.workspace_layout.sidebar_collapsed = false;
                    snapshot.status = "Fix the project errors before entering GUI".into();
                });
            }
            let mut settings = self.snapshot().settings;
            settings.editor_view_mode = mode;
            return self.update_app_settings_locked(settings);
        }
        let mut settings = self.snapshot().settings;
        settings.editor_view_mode = mode;
        self.update_app_settings(settings)
    }

    pub fn create_file(&self, parent: &str, name: &str) -> AppSnapshot {
        self.create_fs_entry(parent, name, FsEntryKind::File)
    }

    pub fn create_directory(&self, parent: &str, name: &str) -> AppSnapshot {
        self.create_fs_entry(parent, name, FsEntryKind::Directory)
    }

    pub fn create_new_project(&self, parent_path: &str, directory_name: &str) -> AppSnapshot {
        if !valid_child_name(directory_name) {
            return self.snapshot_with_error(
                "project.create",
                directory_name,
                "Project folder name must be a single path segment",
            );
        }
        let parent = Utf8PathBuf::from(parent_path);
        if !parent.is_dir() {
            return self.snapshot_with_error(
                "project.create",
                parent_path,
                "Parent location is not a directory",
            );
        }
        let root = parent.join(directory_name);
        if root.exists() {
            return self.snapshot_with_error(
                "project.create",
                root.as_str(),
                "Project folder already exists",
            );
        }
        let files = match new_project_files(directory_name) {
            Ok(files) => files,
            Err(error) => {
                return self.snapshot_with_error("project.create", root.as_str(), &error);
            }
        };
        let result = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            write_new_project_files(&root, &files)
        };
        if let Err(error) = result {
            return self.snapshot_with_error("project.create", root.as_str(), &error);
        }
        self.load_working_copy(&root)
    }

    pub fn create_sequence(&self, request: NewSequenceRequest) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let sequence_path = Utf8PathBuf::from(&request.file_path);
        let Ok(duration) = Duration::try_from_secs_f32(request.duration_seconds) else {
            return self.snapshot_with_error(
                "sequence.create",
                &request.file_path,
                "Sequence duration is outside the supported range",
            );
        };
        let mut edited = (*project).clone();
        let result = dawn_project_io::insert_sequence(
            &mut edited,
            sequence_path.clone(),
            request.object_key.clone(),
            dawn_language::values::DawnDuration(duration),
            request.frame_rate,
        );
        if let Err(error) = result {
            return self.snapshot_with_error(
                "sequence.create",
                &request.file_path,
                &error.to_string(),
            );
        }
        let mut paths = std::collections::BTreeSet::from([request.file_path.clone()]);
        if let Some(entrypoint) = &edited.source.entrypoint {
            paths.insert(entrypoint.path().to_string());
        }
        let texts = match super::generated_source_texts(&edited, &paths) {
            Ok(texts) => texts,
            Err(error) => {
                return self.snapshot_with_error("sequence.create", &request.file_path, &error);
            }
        };
        if let Err(error) =
            self.accept_gui_sources(std::sync::Arc::new(edited), texts, "Sequence created")
        {
            return self.snapshot_with_error("sequence.create", &request.file_path, &error);
        }
        lock_unpoisoned(&self.gui_history).clear();
        self.open_file_path(&request.file_path)
    }

    pub fn delete_path(&self, path: &str) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        if lock_unpoisoned(&self.workspace)
            .documents
            .values()
            .any(|document| {
                document.buffer.dirty && path_matches_or_is_child(&document.buffer.path, path)
            })
        {
            return self.snapshot_with_error(
                "file.delete",
                path,
                "Save or discard edits before deleting this path",
            );
        }
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let relative_path = Utf8PathBuf::from(path);
        if project.source.is_structural_workspace_path(&relative_path) {
            return self.snapshot_with_error(
                "file.delete",
                path,
                "Imported documents and the project entrypoint cannot be deleted from the workspace.",
            );
        }
        let Some(absolute_path) = absolute_project_path(&project, &relative_path) else {
            return self.snapshot_with_error(
                "file.delete",
                path,
                "Path is outside the loaded project",
            );
        };
        let result = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            if absolute_path.is_dir() {
                fs::remove_dir_all(&absolute_path)
            } else {
                fs::remove_file(&absolute_path)
            }
        };
        match result {
            Ok(()) => self.after_workspace_changed(Some(path), None),
            Err(error) => self.snapshot_with_error("file.delete", path, &error.to_string()),
        }
    }
}

fn remap_workspace_path(path: &str, source: &str, destination: &str) -> String {
    if path == source {
        return destination.to_string();
    }
    path.strip_prefix(source)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .map(|suffix| format!("{destination}/{suffix}"))
        .unwrap_or_else(|| path.to_string())
}
