use std::fs;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::{
    check_document_text, check_package, check_project_document_text, save_project,
};

use super::{
    DesktopState, FsEntryKind, PendingOperatorRewriteKind, absolute_project_path,
    document_descriptor, editor_buffer_for_path, empty_document_descriptor, lock_unpoisoned,
    project_diagnostic, project_path_is_structural, upsert_tab, valid_child_name,
};
use crate::dto::{AppSnapshot, DiagnosticSeverity, EditorViewMode, NewSequenceRequest};
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
        let descriptor = project
            .source
            .documents
            .get(&project.source.project_document(relative_path.clone()))
            .map(|document| document_descriptor(&relative_path, document))
            .unwrap_or_else(|| empty_document_descriptor(&relative_path));
        self.update_snapshot(|snapshot| {
            upsert_tab(&mut snapshot.tabs, buffer.clone());
            snapshot.active_file = Some(buffer.path.clone());
            snapshot.active_buffer = Some(buffer);
            snapshot.active_document_descriptor = Some(descriptor);
            snapshot.status = format!("Opened {path}");
        })
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
        let descriptor = project
            .source
            .documents
            .get(&project.source.project_document(relative_path.clone()))
            .map(|document| document_descriptor(&relative_path, document))
            .unwrap_or_else(|| empty_document_descriptor(&relative_path));
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
                snapshot.active_document_descriptor = self.project_session().and_then(|project| {
                    project
                        .source
                        .documents
                        .get(
                            &project
                                .source
                                .project_document(Utf8PathBuf::from(&next.path)),
                        )
                        .map(|document| document_descriptor(Utf8Path::new(&next.path), document))
                });
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
                        check_project_document_text(
                            &project,
                            &project.source.project_document(Utf8PathBuf::from(path)),
                            &text,
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
        fs::write(&path, &buffer.text).map_err(|error| error.to_string())?;
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
        match fs::write(&path, &buffer.text) {
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
        if let Err(error) = write_new_project_files(&root, &files) {
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
        if let Err(error) = dawn_project_io::insert_sequence(
            &mut edited,
            sequence_path.clone(),
            request.object_key.clone(),
            dawn_language::values::DawnDuration(duration),
            request.frame_rate,
        )
        .and_then(|_| save_project(&edited).map(|_| ()))
        {
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

    pub fn rename_path(&self, path: &str, new_name: &str) -> AppSnapshot {
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        if !valid_child_name(new_name) {
            return self.snapshot_with_error(
                "file.rename",
                path,
                "New name must be a single path segment",
            );
        }
        let relative_path = Utf8PathBuf::from(path);
        if project_path_is_structural(&project, &relative_path) {
            return self.snapshot_with_error(
                "file.rename",
                path,
                "Imported documents and the project entrypoint cannot be renamed from the workspace.",
            );
        }
        let Some(from) = absolute_project_path(&project, &relative_path) else {
            return self.snapshot_with_error(
                "file.rename",
                path,
                "Path is outside the loaded project",
            );
        };
        let to_relative = relative_path
            .parent()
            .map(|parent| parent.join(new_name))
            .unwrap_or_else(|| Utf8PathBuf::from(new_name));
        let Some(to) = absolute_project_path(&project, &to_relative) else {
            return self.snapshot_with_error(
                "file.rename",
                path,
                "New path is outside the loaded project",
            );
        };
        match fs::rename(&from, &to) {
            Ok(()) => self.after_workspace_changed(Some(path), Some(to_relative.as_str())),
            Err(error) => self.snapshot_with_error("file.rename", path, &error.to_string()),
        }
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
        let result = if absolute_path.is_dir() {
            fs::remove_dir_all(&absolute_path)
        } else {
            fs::remove_file(&absolute_path)
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
