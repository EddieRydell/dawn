use std::fs;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::{ProjectSession, check_source_graph};

use super::{
    DesktopState, FsEntryKind, absolute_project_path, lock_unpoisoned, path_matches_or_is_child,
    valid_child_name, workspace_entries,
};
use crate::dto::{AppSnapshot, BufferExternalState, DiagnosticSeverity, ProjectDiagnostic};
use crate::state_tasks::{GuiSavePayload, GuiSaveResult};

impl DesktopState {
    pub(super) fn create_fs_entry(
        &self,
        parent: &str,
        name: &str,
        kind: FsEntryKind,
    ) -> AppSnapshot {
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        if !valid_child_name(name) {
            return self.snapshot_with_error(
                "file.create",
                parent,
                "Name must be a single path segment",
            );
        }
        let parent_path = Utf8PathBuf::from(parent);
        let relative_path = if parent.is_empty() {
            Utf8PathBuf::from(name)
        } else {
            parent_path.join(name)
        };
        let Some(path) = absolute_project_path(&project, &relative_path) else {
            return self.snapshot_with_error(
                "file.create",
                relative_path.as_str(),
                "Path is outside the loaded project",
            );
        };
        let result = match kind {
            FsEntryKind::File => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).and_then(|()| {
                        fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&path)
                            .map(|_| ())
                    })
                } else {
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .map(|_| ())
                }
            }
            FsEntryKind::Directory => fs::create_dir_all(&path),
        };
        match result {
            Ok(()) => self.after_workspace_changed(None, Some(relative_path.as_str())),
            Err(error) => {
                self.snapshot_with_error("file.create", relative_path.as_str(), &error.to_string())
            }
        }
    }

    pub(super) fn after_file_saved(&self, path: &str) -> AppSnapshot {
        self.update_snapshot(|snapshot| {
            if let Some(buffer) = snapshot
                .active_buffer
                .as_mut()
                .filter(|buffer| buffer.path == path)
            {
                buffer.dirty = false;
                buffer.external_state = BufferExternalState::Current;
            }
            if let Some(tab) = snapshot.tabs.iter_mut().find(|tab| tab.path == path) {
                tab.dirty = false;
                tab.external_state = BufferExternalState::Current;
            }
            snapshot.status = format!("Saved {path}");
        })
    }

    pub(super) fn after_workspace_changed(
        &self,
        old_path: Option<&str>,
        new_active_path: Option<&str>,
    ) -> AppSnapshot {
        let refreshed = self.refresh_project_session();
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        let entries = workspace_entries(&project);
        self.update_snapshot(|snapshot| {
            snapshot.project_entries = entries;
            if let Some(old_path) = old_path {
                snapshot
                    .tabs
                    .retain(|tab| !path_matches_or_is_child(&tab.path, old_path));
                if snapshot
                    .active_file
                    .as_deref()
                    .is_some_and(|active| path_matches_or_is_child(active, old_path))
                {
                    snapshot.active_file = None;
                    snapshot.active_buffer = None;
                    snapshot.active_document_descriptor = None;
                }
            }
            if refreshed {
                snapshot.status = "Project files updated".to_string();
            }
        });
        if let Some(path) = new_active_path
            && absolute_project_path(&project, Utf8Path::new(path))
                .is_some_and(|path| path.is_file())
        {
            return self.open_file_path(path);
        }
        self.snapshot()
    }

    pub(super) fn refresh_project_session(&self) -> bool {
        let Some(project) = self.project_session() else {
            return false;
        };
        let Some(entrypoint) = project.source.entrypoint.as_ref() else {
            return false;
        };
        let entrypoint = entrypoint.path().to_string();
        let report = check_source_graph(project.source.source_graph.clone());
        let refreshed = report.session.is_some();
        self.apply_project_refresh_check(&entrypoint, report);
        refreshed
    }

    pub(super) fn snapshot_with_error(&self, code: &str, path: &str, message: &str) -> AppSnapshot {
        self.update_snapshot(|snapshot| {
            snapshot.status = message.to_string();
            snapshot.diagnostics = vec![ProjectDiagnostic {
                path: path.to_string(),
                range: None,
                severity: DiagnosticSeverity::Error,
                code: code.to_string(),
                message: message.to_string(),
            }];
        })
    }

    pub(super) fn schedule_gui_save(
        &self,
        session: Arc<ProjectSession>,
        affected_paths: std::collections::BTreeSet<String>,
        status_path: String,
    ) {
        let path_for_error = status_path.clone();
        let request = GuiSavePayload {
            session,
            affected_paths,
            status_path,
        };
        if !lock_unpoisoned(&self.gui_save).schedule(request) {
            self.snapshot_with_error(
                "gui.save",
                path_for_error.as_str(),
                "GUI save worker is unavailable.",
            );
        }
    }

    pub(super) fn drain_gui_save_results(&self) {
        let results = lock_unpoisoned(&self.gui_save).drain_current_results();
        for result in results {
            match result {
                GuiSaveResult::Saved {
                    sequence: _,
                    session,
                    affected_paths,
                } => {
                    if self.project_session().as_ref().is_some_and(|project| {
                        project.source.project_root() == session.source.project_root()
                    }) {
                        self.refresh_saved_tabs(&affected_paths);
                    }
                }
                GuiSaveResult::Failed {
                    sequence: _,
                    status_path,
                    message,
                } => {
                    self.snapshot_with_error("gui.save", &status_path, &message);
                }
            }
        }
    }

    pub(super) fn refresh_saved_tabs(
        &self,
        paths: &std::collections::BTreeSet<String>,
    ) -> AppSnapshot {
        self.update_snapshot(|snapshot| {
            for path in paths {
                if let Some(tab) = snapshot.tabs.iter_mut().find(|tab| tab.path == *path) {
                    if tab.dirty {
                        tab.external_state = BufferExternalState::ChangedOnDisk;
                    } else {
                        tab.external_state = BufferExternalState::Current;
                    }
                }
                if let Some(buffer) = snapshot
                    .active_buffer
                    .as_mut()
                    .filter(|buffer| buffer.path == *path)
                {
                    if buffer.dirty {
                        buffer.external_state = BufferExternalState::ChangedOnDisk;
                    } else {
                        buffer.external_state = BufferExternalState::Current;
                    }
                }
            }
        })
    }
}
