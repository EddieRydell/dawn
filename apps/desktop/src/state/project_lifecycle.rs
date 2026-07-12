use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use camino::Utf8Path;
use dawn_project_io::{ProjectCheckReport, ProjectSession, SourceObjectKind};

use super::{
    DesktopState, descriptor_for_path, editor_buffer, lock_unpoisoned, project_diagnostics,
    refresh_clean_buffers, restored_active_buffers, workspace_entries,
};
use crate::dto::{
    AppSnapshot, DocumentViewId, GuiDocument, GuiDocumentRequest, ProjectDiagnostic,
    ProjectTreeMode, SequenceAudio, WorkspaceEntryKind,
};

impl DesktopState {
    pub(super) fn apply_project_open_check(
        &self,
        entrypoint: &str,
        report: ProjectCheckReport,
    ) -> AppSnapshot {
        lock_unpoisoned(&self.gui_history).clear();
        let diagnostics = project_diagnostics(&report);
        match report.session {
            Some(session) => self.replace_project(session, diagnostics),
            None => self.update_snapshot(|snapshot| {
                snapshot.diagnostics = diagnostics;
                snapshot.status = format!("Failed to open project {entrypoint}");
            }),
        }
    }

    pub(super) fn apply_project_refresh_check(
        &self,
        entrypoint: &str,
        report: ProjectCheckReport,
    ) -> AppSnapshot {
        lock_unpoisoned(&self.gui_history).clear();
        let diagnostics = project_diagnostics(&report);
        match report.session {
            Some(session) => self.refresh_project(session, diagnostics),
            None => self.update_snapshot(|snapshot| {
                snapshot.diagnostics = diagnostics;
                snapshot.status = format!("Project check failed for {entrypoint}");
            }),
        }
    }

    pub(super) fn replace_project(
        &self,
        session: ProjectSession,
        diagnostics: Vec<ProjectDiagnostic>,
    ) -> AppSnapshot {
        let entries = workspace_entries(&session);
        let root = session.source.source_root.to_string();
        let entrypoint = session.source.entrypoint.clone();
        let valid_paths = entries
            .iter()
            .filter(|entry| matches!(entry.kind, WorkspaceEntryKind::File))
            .map(|entry| entry.path.clone())
            .collect();
        let restore = self.persistence.restore_for_project(&root, &valid_paths);
        let active = restored_active_buffers(&session, restore.as_ref().map(|item| &item.session))
            .or_else(|| {
                session
                    .source
                    .documents
                    .get(&entrypoint)
                    .and_then(|_document| {
                        let buffer = editor_buffer(&session, &entrypoint)?;
                        Some((vec![buffer.clone()], buffer.path))
                    })
            });
        let active_descriptor = active
            .as_ref()
            .and_then(|(_, active_path)| descriptor_for_path(&session, Utf8Path::new(active_path)));
        *lock_unpoisoned(&self.project) = Some(Arc::new(session));
        self.unload_render_session();
        self.update_snapshot(|snapshot| {
            snapshot.project_root = Some(root);
            snapshot.project_tree_visible = restore
                .as_ref()
                .map(|restore| restore.session.project_tree_visible)
                .unwrap_or(true);
            match snapshot.settings.project_tree_mode {
                ProjectTreeMode::Remember => {}
                ProjectTreeMode::Show => snapshot.project_tree_visible = true,
                ProjectTreeMode::Hide => snapshot.project_tree_visible = false,
            }
            snapshot.project_entries = entries;
            snapshot.tabs = active
                .as_ref()
                .map(|(buffers, _)| buffers.clone())
                .unwrap_or_default();
            snapshot.active_file = active.as_ref().map(|(_, path)| path.clone());
            snapshot.active_buffer = active.as_ref().and_then(|(buffers, active_path)| {
                buffers
                    .iter()
                    .find(|buffer| buffer.path == *active_path)
                    .cloned()
            });
            snapshot.active_document_descriptor = active_descriptor;
            snapshot.diagnostics = diagnostics;
            snapshot.status = if snapshot.diagnostics.is_empty() {
                format!("Opened project {entrypoint}")
            } else {
                format!(
                    "Opened project {entrypoint} with {} diagnostics",
                    snapshot.diagnostics.len()
                )
            };
            if let Some(restore) = restore
                .as_ref()
                .filter(|restore| !restore.stale_tabs.is_empty())
            {
                snapshot.status = format!(
                    "{}. Skipped stale tabs: {}",
                    snapshot.status,
                    restore.stale_tabs.join(", ")
                );
            }
            snapshot.audio_transport = self.audio_snapshot();
            if let Some(restore) = restore.as_ref() {
                snapshot.audio_transport.position_seconds = restore.session.audio_position_seconds;
                snapshot.audio_transport.home_seconds = restore.session.audio_home_seconds;
                snapshot.live_output.enabled = restore.session.live_output_enabled;
                snapshot.live_output.status = if restore.session.live_output_enabled {
                    "Enabled".to_string()
                } else {
                    "Disabled".to_string()
                };
            }
            snapshot.project_revision = snapshot.project_revision.saturating_add(1);
        })
    }

    pub(super) fn record_persistent_snapshot(&self, snapshot: &AppSnapshot) {
        if let Err(error) = self.persistence.record_snapshot(snapshot) {
            lock_unpoisoned(&self.snapshot).status =
                format!("Desktop state was not saved: {error}");
        }
    }

    pub(super) fn refresh_project(
        &self,
        session: ProjectSession,
        diagnostics: Vec<ProjectDiagnostic>,
    ) -> AppSnapshot {
        // A refresh applies the latest semantic project model in place. It must not
        // reload editor text or reset user-owned editor state; only full project
        // opens are allowed to replace tabs, buffers, GUI state, or transport.
        let entries = workspace_entries(&session);
        let root = session.source.source_root.to_string();
        let render_error = self.refresh_render_session(&session.project);
        let active_descriptor = self
            .snapshot()
            .active_file
            .as_deref()
            .and_then(|path| descriptor_for_path(&session, Utf8Path::new(path)));
        *lock_unpoisoned(&self.project) = Some(Arc::new(session));
        self.update_snapshot(|snapshot| {
            snapshot.project_root = Some(root);
            snapshot.project_entries = entries;
            if active_descriptor.is_some() {
                snapshot.active_document_descriptor = active_descriptor;
            }
            snapshot.diagnostics = diagnostics;
            snapshot.status = if snapshot.diagnostics.is_empty() {
                "Project checked".to_string()
            } else {
                format!(
                    "Project checked with {} diagnostics",
                    snapshot.diagnostics.len()
                )
            };
            snapshot.render_error =
                render_error.map(|error| format!("Render refresh failed: {error:?}"));
            snapshot.project_revision = snapshot.project_revision.saturating_add(1);
        })
    }

    pub(super) fn apply_gui_project_update(
        &self,
        session: Arc<ProjectSession>,
        status: &str,
        generated_text: BTreeMap<String, String>,
    ) -> AppSnapshot {
        let entries = workspace_entries(&session);
        let root = session.source.source_root.to_string();
        let active_descriptor = self
            .snapshot()
            .active_file
            .as_deref()
            .and_then(|path| descriptor_for_path(&session, Utf8Path::new(path)));
        *lock_unpoisoned(&self.project) = Some(Arc::clone(&session));
        self.schedule_render_refresh(Arc::clone(&session));
        self.update_snapshot(|snapshot| {
            snapshot.project_root = Some(root);
            snapshot.project_entries = entries;
            if active_descriptor.is_some() {
                snapshot.active_document_descriptor = active_descriptor;
            }
            refresh_clean_buffers(snapshot, &generated_text);
            snapshot.status = status.to_string();
            snapshot.project_revision = snapshot.project_revision.saturating_add(1);
        })
    }

    pub(super) fn project_session(&self) -> Option<Arc<ProjectSession>> {
        lock_unpoisoned(&self.project).clone()
    }

    pub(super) fn dirty_affected_path(&self, affected_paths: &BTreeSet<String>) -> Option<String> {
        self.snapshot()
            .tabs
            .into_iter()
            .find(|tab| tab.dirty && affected_paths.contains(&tab.path))
            .map(|tab| tab.path)
    }

    pub(super) fn project_revision(&self) -> u64 {
        lock_unpoisoned(&self.snapshot).project_revision.into()
    }

    pub(super) fn active_sequence_gui_request(&self) -> Option<GuiDocumentRequest> {
        let snapshot = self.snapshot();
        let active_path = snapshot.active_file?;
        let descriptor = snapshot.active_document_descriptor?;
        let object_key = descriptor
            .default_object_keys
            .iter()
            .find(|item| matches!(item.view, DocumentViewId::Sequence))?
            .object_key
            .clone();
        Some(GuiDocumentRequest {
            path: active_path,
            view: DocumentViewId::Sequence,
            object_key: Some(object_key),
        })
    }

    pub(super) fn resolve_sequence_audio(
        &self,
        request: &GuiDocumentRequest,
    ) -> Option<SequenceAudio> {
        let project = self.project_session();
        match crate::gui::project_gui_document(project.as_deref(), request) {
            GuiDocument::Sequence { document } => document.audio,
            _ => None,
        }
    }

    pub(super) fn resolve_sequence_id(
        &self,
        request: &GuiDocumentRequest,
    ) -> Option<dawn_language::sequence::SequenceId> {
        let project = self.project_session()?;
        let path = Utf8Path::new(&request.path);
        project
            .source
            .documents
            .get(path)?
            .objects()
            .iter()
            .find(|object| {
                object.kind() == &SourceObjectKind::Sequence
                    && request
                        .object_key
                        .as_deref()
                        .is_none_or(|key| object.id() == key)
            })
            .map(|object| {
                dawn_language::sequence::SequenceId(dawn_language::identity::SourceIdentity::new(
                    path.to_path_buf(),
                    object.id().to_string(),
                ))
            })
    }
}
