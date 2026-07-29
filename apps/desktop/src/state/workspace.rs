use std::fs;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::{
    check_document_text, check_package, check_project_document_text, save_project,
};

use super::{
    DesktopState, FsEntryKind, PendingOperatorRewriteKind, absolute_project_path,
    descriptor_for_path, document_id_for_workspace_path, editor_buffer_for_path, lock_unpoisoned,
    path_matches_or_is_child, project_diagnostic, project_path_is_structural, upsert_tab,
    valid_child_name,
};
use crate::dto::{AppSnapshot, DiagnosticSeverity, EditorViewMode, NewSequenceRequest};
use crate::dto::{
    WorkspaceExplorerState, WorkspacePathChangeImpact, WorkspacePathChangePlan,
    WorkspacePathChangeRequest, WorkspacePathOwnership,
};
use crate::persistence::{
    PersistedEditorViewStateUpdate, PersistedSequenceViewportStateUpdate, ProjectRestoreState,
};
use crate::project_templates::{new_project_files, write_new_project_files};

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
        self.apply_project_open_check(root.as_str(), check_package(root))
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
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let relative_path = Utf8PathBuf::from(path);
        let Some(buffer) = editor_buffer_for_path(&project, &relative_path) else {
            return self.snapshot_with_error(
                "file.open",
                path,
                "File is not part of the loaded project",
            );
        };
        let Some(descriptor) = descriptor_for_path(&project, &relative_path) else {
            return self.snapshot_with_error(
                "file.open",
                path,
                "File is not part of the loaded project",
            );
        };
        self.update_snapshot(|snapshot| {
            upsert_tab(&mut snapshot.tabs, buffer.clone());
            snapshot.active_file = Some(buffer.path.clone());
            snapshot.active_buffer = Some(buffer);
            snapshot.active_document_descriptor = Some(descriptor);
            snapshot
                .workspace_explorer
                .recent_files
                .retain(|item| item != path);
            snapshot
                .workspace_explorer
                .recent_files
                .insert(0, path.to_string());
            snapshot.workspace_explorer.recent_files.truncate(20);
            snapshot.status = format!("Opened {path}");
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
        let description = self.plan_workspace_path_change(request.clone())?;
        if description.structural && self.snapshot().tabs.iter().any(|buffer| buffer.dirty) {
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
        lock_unpoisoned(&self.gui_save).invalidate_pending();
        lock_unpoisoned(&self.render_refresh).invalidate_pending();
        let candidate = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            dawn_project_io::apply_path_change(&project, &plan)?
        };
        let root = candidate.source.project_root().to_string();
        self.persistence
            .remap_project_paths(&root, &request.source, &request.destination)?;
        *lock_unpoisoned(&self.project) = Some(std::sync::Arc::new(candidate.clone()));
        lock_unpoisoned(&self.gui_history).clear();
        *lock_unpoisoned(&self.pending_operator_rewrite) = None;
        let entries = super::workspace_entries(&candidate);
        let package = super::package_status(Utf8Path::new(&root), Some(&candidate));
        let remap = |path: &str| remap_workspace_path(path, &request.source, &request.destination);
        let snapshot = self.update_snapshot(|snapshot| {
            for tab in &mut snapshot.tabs {
                tab.path = remap(&tab.path);
                tab.name = Utf8Path::new(&tab.path)
                    .file_name()
                    .unwrap_or(tab.path.as_str())
                    .to_string();
            }
            snapshot.active_file = snapshot.active_file.as_deref().map(remap);
            snapshot.active_buffer = snapshot
                .active_file
                .as_deref()
                .and_then(|path| super::editor_buffer_for_path(&candidate, Utf8Path::new(path)));
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
            snapshot.pending_operator_rewrite = None;
            snapshot.project_revision = snapshot.project_revision.saturating_add(1);
            snapshot.status = if description.structural {
                "Structural path change applied; GUI undo and redo history were cleared."
                    .to_string()
            } else {
                "Path change applied.".to_string()
            };
        });
        self.schedule_render_refresh(std::sync::Arc::new(candidate));
        Ok(snapshot)
    }

    pub fn set_active_file_path(&self, path: &str) -> AppSnapshot {
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let relative_path = Utf8PathBuf::from(path);
        let buffer = self
            .snapshot()
            .tabs
            .into_iter()
            .find(|tab| tab.path == path)
            .or_else(|| editor_buffer_for_path(&project, &relative_path));
        let Some(buffer) = buffer else {
            return self.snapshot_with_error(
                "file.activate",
                path,
                "File is not part of the loaded project",
            );
        };
        let Some(descriptor) = descriptor_for_path(&project, &relative_path) else {
            return self.snapshot_with_error(
                "file.activate",
                path,
                "File is not part of the loaded project",
            );
        };
        self.update_snapshot(|snapshot| {
            snapshot.active_file = Some(buffer.path.clone());
            snapshot.active_buffer = Some(buffer);
            snapshot.active_document_descriptor = Some(descriptor);
        })
    }

    pub fn close_file_path(&self, path: &str) -> AppSnapshot {
        self.update_snapshot(|snapshot| {
            let Some(index) = snapshot.tabs.iter().position(|tab| tab.path == path) else {
                return;
            };
            snapshot.tabs.remove(index);
            if snapshot.active_file.as_deref() != Some(path) {
                return;
            }
            if let Some(next) = snapshot
                .tabs
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|previous| snapshot.tabs.get(previous))
                })
                .cloned()
            {
                snapshot.active_file = Some(next.path.clone());
                snapshot.active_document_descriptor = self
                    .project_session()
                    .and_then(|project| descriptor_for_path(&project, Utf8Path::new(&next.path)));
                snapshot.active_buffer = Some(next);
            } else {
                snapshot.active_file = None;
                snapshot.active_buffer = None;
                snapshot.active_document_descriptor = None;
            }
        })
    }

    pub fn update_active_text(&self, text: String) -> AppSnapshot {
        if lock_unpoisoned(&self.pending_operator_rewrite).is_some() {
            self.invalidate_operator_rewrite();
        }
        let active_path = self.snapshot().active_file;
        let diagnostics = active_path
            .as_deref()
            .map(|path| {
                if is_operator_document(path) {
                    return check_document_text(Utf8Path::new(path), &text);
                }
                self.project_session().map_or_else(
                    || check_document_text(Utf8Path::new(path), &text),
                    |project| {
                        document_id_for_workspace_path(&project, Utf8Path::new(path)).map_or_else(
                            || check_document_text(Utf8Path::new(path), &text),
                            |document_id| {
                                check_project_document_text(&project, &document_id, &text)
                            },
                        )
                    },
                )
            })
            .unwrap_or_default();
        self.update_snapshot(|snapshot| {
            if let Some(buffer) = snapshot.active_buffer.as_mut() {
                let changed = buffer.text != text;
                buffer.text = text.clone();
                buffer.dirty |= changed;
                if let Some(tab) = snapshot.tabs.iter_mut().find(|tab| tab.path == buffer.path) {
                    tab.text = text.clone();
                    tab.dirty |= changed;
                }
            }
            if let Some(active_path) = active_path.as_deref() {
                snapshot
                    .diagnostics
                    .retain(|diagnostic| diagnostic.path != active_path);
                snapshot
                    .diagnostics
                    .extend(diagnostics.iter().map(project_diagnostic));
            }
        })
    }

    pub fn autosave_active_text(&self, path: &str, text: String) -> Result<AppSnapshot, String> {
        if self.snapshot().active_file.as_deref() != Some(path) {
            return Ok(self.snapshot());
        }
        let pending_matches = lock_unpoisoned(&self.pending_operator_rewrite)
            .as_ref()
            .is_some_and(|pending| {
                matches!(
                    &pending.kind,
                    PendingOperatorRewriteKind::Document {
                        path: pending_path,
                        ..
                    } if pending_path == Utf8Path::new(path)
                )
            });
        if pending_matches
            && self
                .snapshot()
                .active_buffer
                .as_ref()
                .is_some_and(|buffer| buffer.text == text)
        {
            return Ok(self.snapshot());
        }
        self.update_active_text(text);
        let snapshot = self.snapshot();
        let Some(buffer) = snapshot.active_buffer.as_ref() else {
            return Err("No active text buffer to autosave".to_string());
        };
        if is_operator_document(&buffer.path) {
            return Ok(snapshot);
        }
        let Some(project) = self.project_session() else {
            return Err("No project is open".to_string());
        };
        let relative_path = Utf8PathBuf::from(&buffer.path);
        let Some(path) = absolute_project_path(&project, &relative_path) else {
            return Err("File path is outside the loaded project".to_string());
        };
        {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            fs::write(&path, &buffer.text).map_err(|error| error.to_string())?;
        }
        self.after_file_saved(&buffer.path);
        let root = project.source.project_root();
        Ok(self.apply_project_refresh_check(root.as_str(), check_package(root)))
    }

    pub fn set_editor_view_mode(&self, mode: EditorViewMode) -> AppSnapshot {
        let mut settings = self.snapshot().settings;
        settings.editor_view_mode = mode;
        self.update_app_settings(settings)
    }

    pub fn save_active_buffer(&self) -> AppSnapshot {
        let snapshot = self.snapshot();
        let Some(buffer) = snapshot.active_buffer else {
            return snapshot;
        };
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        match self.save_operator_draft(&buffer.path, &buffer.text) {
            Ok(Some(snapshot)) => return snapshot,
            Ok(None) => {}
            Err(error) => {
                return self.snapshot_with_error("file.save", &buffer.path, &error);
            }
        }
        let relative_path = Utf8PathBuf::from(&buffer.path);
        let Some(path) = absolute_project_path(&project, &relative_path) else {
            return self.snapshot_with_error(
                "file.save",
                &buffer.path,
                "File path is outside the loaded project",
            );
        };
        let result = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            fs::write(&path, &buffer.text)
        };
        match result {
            Ok(()) => {
                self.after_file_saved(&buffer.path);
                let root = project.source.project_root();
                self.apply_project_refresh_check(root.as_str(), check_package(root))
            }
            Err(error) => self.snapshot_with_error("file.save", &buffer.path, &error.to_string()),
        }
    }

    pub fn reload_active_buffer_from_disk(&self) -> AppSnapshot {
        let snapshot = self.snapshot();
        let Some(active_file) = snapshot.active_file else {
            return snapshot;
        };
        self.open_file_path(&active_file)
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
        self.apply_project_open_check(root.as_str(), check_package(&root))
    }

    pub fn create_sequence(&self, request: NewSequenceRequest) -> AppSnapshot {
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let sequence_path = Utf8PathBuf::from(&request.file_path);
        let Ok(duration) = Duration::try_from_secs_f64(request.duration_seconds) else {
            return self.snapshot_with_error(
                "sequence.create",
                &request.file_path,
                "Sequence duration is outside the supported range",
            );
        };
        let root = project.source.project_root().to_path_buf();
        let mut edited = (*project).clone();
        let result = dawn_project_io::insert_sequence(
            &mut edited,
            sequence_path.clone(),
            request.object_key.clone(),
            dawn_language::values::DawnDuration(duration),
            request.frame_rate,
        )
        .and_then(|_| {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            save_project(&edited).map(|_| ())
        });
        if let Err(error) = result {
            return self.snapshot_with_error(
                "sequence.create",
                &request.file_path,
                &error.to_string(),
            );
        }
        let refreshed = self.apply_project_refresh_check(root.as_str(), check_package(&root));
        if refreshed
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.severity, DiagnosticSeverity::Error))
        {
            return refreshed;
        }
        self.open_file_path(&request.file_path)
    }

    pub fn delete_path(&self, path: &str) -> AppSnapshot {
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let relative_path = Utf8PathBuf::from(path);
        if project_path_is_structural(&project, &relative_path) {
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

    pub fn reload_project(&self) -> AppSnapshot {
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let root = project.source.project_root();
        self.apply_project_refresh_check(root.as_str(), check_package(root))
    }
}

fn is_operator_document(path: &str) -> bool {
    Utf8Path::new(path)
        .file_name()
        .is_some_and(|name| name.ends_with(".operator.dawn"))
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
