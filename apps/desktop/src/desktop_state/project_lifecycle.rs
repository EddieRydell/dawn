use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use camino::Utf8Path;
use dawn_project_io::{ProjectCheckReport, ProjectSession, SourceObjectKind};

use super::{
    DesktopState, LoadedProject, descriptor_for_path, editor_buffer, lock_unpoisoned,
    package_status, project_diagnostics, recovery_descriptor_for_path, recovery_editor_buffer,
    recovery_workspace_entries, refresh_clean_buffers, restored_active_buffers, workspace_entries,
};
use crate::dto::{
    AppSnapshot, DocumentViewId, GuiDocument, GuiDocumentRequest, ProjectDiagnostic, ProjectHealth,
    SequenceAudio, WorkspaceEntryKind,
};

impl DesktopState {
    pub(super) fn apply_project_open_check(
        &self,
        _entrypoint: &str,
        report: ProjectCheckReport,
    ) -> AppSnapshot {
        lock_unpoisoned(&self.project_analysis).invalidate_pending();
        *lock_unpoisoned(&self.pending_operator_rewrite) = None;
        lock_unpoisoned(&self.gui_history).clear();
        let diagnostics = project_diagnostics(&report);
        match report.session {
            Some(session) => self.replace_project(session, diagnostics),
            None => self.replace_with_recovery(report.recovery, diagnostics, true),
        }
    }

    pub(super) fn apply_project_refresh_check(
        &self,
        entrypoint: &str,
        report: ProjectCheckReport,
    ) -> AppSnapshot {
        lock_unpoisoned(&self.project_analysis).invalidate_pending();
        *lock_unpoisoned(&self.pending_operator_rewrite) = None;
        lock_unpoisoned(&self.gui_history).clear();
        let diagnostics = project_diagnostics(&report);
        match report.session {
            Some(session) => self.refresh_project(session, diagnostics),
            None => {
                let _ = entrypoint;
                self.replace_with_recovery(report.recovery, diagnostics, false)
            }
        }
    }

    fn replace_with_recovery(
        &self,
        recovery: dawn_project_io::ProjectRecovery,
        diagnostics: Vec<ProjectDiagnostic>,
        opening: bool,
    ) -> AppSnapshot {
        self.suspend_live_output();
        lock_unpoisoned(&self.gui_save).invalidate_pending();
        lock_unpoisoned(&self.render_refresh).invalidate_pending();
        self.unload_render_session();
        let audio_transport = lock_unpoisoned(&self.audio).unload();
        lock_unpoisoned(&self.gui_history).clear();
        *lock_unpoisoned(&self.pending_operator_rewrite) = None;
        *lock_unpoisoned(&self.sequence_clipboard) = None;
        *lock_unpoisoned(&self.sequence_clip_raster) =
            crate::sequence_clip_raster::SequenceClipRasterService::new();

        let entries = recovery_workspace_entries(&recovery);
        let root = recovery.root.to_string();
        let package = package_status(Utf8Path::new(&root), None);
        let recovery = Arc::new(recovery);
        let current = self.snapshot();
        let root_changed = current.project_root.as_deref() != Some(root.as_str());
        let valid_paths = entries
            .iter()
            .filter(|entry| matches!(entry.kind, WorkspaceEntryKind::File))
            .map(|entry| entry.path.clone())
            .collect();
        let restore = (opening || root_changed)
            .then(|| self.persistence.restore_for_project(&root, &valid_paths))
            .flatten();
        let restored = restore
            .as_ref()
            .and_then(|restore| restored_recovery_buffers(&recovery, &restore.session));
        let fallback_path = recovery
            .manifest
            .as_ref()
            .and_then(|manifest| manifest.project.as_ref())
            .map(|project| project.entrypoint.clone())
            .filter(|path| recovery.root.join(path).is_file())
            .or_else(|| {
                recovery
                    .documents
                    .keys()
                    .find(|path| recovery.root.join(path).is_file())
                    .map(ToString::to_string)
            })
            .or_else(|| {
                recovery
                    .root
                    .join(dawn_package::MANIFEST_FILE)
                    .is_file()
                    .then(|| dawn_package::MANIFEST_FILE.to_string())
            });
        let fallback = fallback_path.as_deref().and_then(|path| {
            recovery_editor_buffer(&recovery, Utf8Path::new(path))
                .map(|buffer| (vec![buffer.clone()], buffer.path))
        });
        let active = if opening || root_changed {
            restored.or(fallback)
        } else {
            None
        };
        *lock_unpoisoned(&self.project) = LoadedProject::Recovery(Arc::clone(&recovery));
        self.update_snapshot(|snapshot| {
            snapshot.project_root = Some(root);
            snapshot.project_health = ProjectHealth::Recovery;
            snapshot.package = package;
            snapshot.project_entries = entries;
            if let Some((buffers, active_path)) = &active {
                snapshot.tabs = buffers.clone();
                snapshot.active_file = Some(active_path.clone());
                snapshot.active_buffer = buffers
                    .iter()
                    .find(|buffer| buffer.path == *active_path)
                    .cloned();
            } else if root_changed {
                snapshot.tabs.clear();
                snapshot.active_file = None;
                snapshot.active_buffer = None;
            }
            snapshot.active_document_descriptor = snapshot
                .active_file
                .as_deref()
                .and_then(|path| recovery_descriptor_for_path(&recovery, Utf8Path::new(path)));
            snapshot.diagnostics = diagnostics;
            snapshot.status = format!(
                "Project recovery mode: {} actionable diagnostics",
                snapshot.diagnostics.len()
            );
            snapshot.render_error = None;
            snapshot.preview_error = None;
            snapshot.pending_operator_rewrite = None;
            snapshot.audio_transport = audio_transport;
            snapshot.project_revision = snapshot.project_revision.saturating_add(1);
        })
    }

    pub(super) fn replace_project(
        &self,
        session: ProjectSession,
        diagnostics: Vec<ProjectDiagnostic>,
    ) -> AppSnapshot {
        let Some(entrypoint) = session.source.entrypoint.clone() else {
            return self.snapshot_with_error(
                "project.open",
                session.source.project_root().as_str(),
                "Active project manifest has no project entrypoint",
            );
        };
        self.suspend_live_output();
        let entries = workspace_entries(&session);
        let root = session.source.project_root().to_string();
        let package = package_status(Utf8Path::new(&root), Some(&session));
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
                        let buffer = editor_buffer(&session, entrypoint.path())?;
                        Some((vec![buffer.clone()], buffer.path))
                    })
            });
        let active_descriptor = active
            .as_ref()
            .and_then(|(_, active_path)| descriptor_for_path(&session, Utf8Path::new(active_path)));
        *lock_unpoisoned(&self.project) = LoadedProject::Ready(Arc::new(session));
        self.unload_render_session();
        self.update_snapshot(|snapshot| {
            snapshot.pending_operator_rewrite = None;
            snapshot.project_root = Some(root);
            snapshot.project_health = ProjectHealth::Ready;
            snapshot.package = package;
            snapshot.workspace_explorer = restore
                .as_ref()
                .map(|restore| restore.session.workspace_explorer.clone())
                .unwrap_or_default();
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
                format!("Opened project {}", entrypoint.path())
            } else {
                format!(
                    "Opened project {} with {} diagnostics",
                    entrypoint.path(),
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
        self.suspend_live_output();
        // A refresh applies the latest semantic project model in place. It must not
        // reload editor text or reset user-owned editor state; only full project
        // opens are allowed to replace tabs, buffers, GUI state, or transport.
        let entries = workspace_entries(&session);
        let root = session.source.project_root().to_string();
        let package = package_status(Utf8Path::new(&root), Some(&session));
        let render_error = self.refresh_render_session(&session.project);
        let render_ready = render_error.is_none();
        let active_descriptor = self
            .snapshot()
            .active_file
            .as_deref()
            .and_then(|path| descriptor_for_path(&session, Utf8Path::new(path)));
        *lock_unpoisoned(&self.project) = LoadedProject::Ready(Arc::new(session));
        let snapshot = self.update_snapshot(|snapshot| {
            snapshot.pending_operator_rewrite = None;
            snapshot.project_root = Some(root);
            snapshot.project_health = ProjectHealth::Ready;
            snapshot.package = package;
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
        });
        if render_ready {
            self.resume_live_output_after_prepare();
            self.snapshot()
        } else {
            snapshot
        }
    }

    pub(super) fn apply_gui_project_update(
        &self,
        session: Arc<ProjectSession>,
        status: &str,
        generated_text: BTreeMap<String, String>,
    ) -> AppSnapshot {
        *lock_unpoisoned(&self.pending_operator_rewrite) = None;
        self.suspend_live_output();
        let entries = workspace_entries(&session);
        let root = session.source.project_root().to_string();
        let active_descriptor = self
            .snapshot()
            .active_file
            .as_deref()
            .and_then(|path| descriptor_for_path(&session, Utf8Path::new(path)));
        *lock_unpoisoned(&self.project) = LoadedProject::Ready(Arc::clone(&session));
        self.schedule_render_refresh(Arc::clone(&session));
        self.update_snapshot(|snapshot| {
            snapshot.pending_operator_rewrite = None;
            snapshot.project_root = Some(root);
            snapshot.project_health = ProjectHealth::Ready;
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
        match &*lock_unpoisoned(&self.project) {
            LoadedProject::Ready(session) => Some(Arc::clone(session)),
            LoadedProject::Closed | LoadedProject::Recovery(_) => None,
        }
    }

    pub(super) fn project_recovery(&self) -> Option<Arc<dawn_project_io::ProjectRecovery>> {
        match &*lock_unpoisoned(&self.project) {
            LoadedProject::Recovery(recovery) => Some(Arc::clone(recovery)),
            LoadedProject::Closed | LoadedProject::Ready(_) => None,
        }
    }

    pub(super) fn project_root_path(&self) -> Option<camino::Utf8PathBuf> {
        match &*lock_unpoisoned(&self.project) {
            LoadedProject::Ready(session) => Some(session.source.project_root().to_path_buf()),
            LoadedProject::Recovery(recovery) => Some(recovery.root.clone()),
            LoadedProject::Closed => None,
        }
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

    pub(super) fn schedule_project_analysis(&self, root: camino::Utf8PathBuf) -> bool {
        let payload = crate::state_tasks::ProjectAnalysisPayload {
            root,
            project_revision: self.snapshot().project_revision,
            filesystem: Arc::clone(&self.filesystem),
        };
        lock_unpoisoned(&self.project_analysis).schedule(payload)
    }

    pub(super) fn drain_project_analysis_results(&self) {
        let results = lock_unpoisoned(&self.project_analysis).drain_current_results();
        for result in results {
            let snapshot = lock_unpoisoned(&self.snapshot).clone();
            if snapshot.project_revision != result.project_revision
                || snapshot.project_root.as_deref() != Some(result.root.as_str())
            {
                continue;
            }
            self.apply_project_refresh_check(result.root.as_str(), result.report);
        }
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
            .get(&project.source.project_document(path.to_path_buf()))?
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
                dawn_language::sequence::SequenceId(
                    dawn_language::identity::SourceIdentity::from_document(
                        project.source.project_document(path.to_path_buf()),
                        object.id().to_string(),
                    ),
                )
            })
    }
}

fn restored_recovery_buffers(
    recovery: &dawn_project_io::ProjectRecovery,
    restore: &crate::persistence::PersistedProjectSession,
) -> Option<(Vec<crate::dto::EditorBuffer>, String)> {
    let mut buffers = Vec::new();
    for path in &restore.tabs {
        if let Some(buffer) = recovery_editor_buffer(recovery, Utf8Path::new(path)) {
            buffers.push(buffer);
        }
    }
    if buffers.is_empty() {
        return None;
    }
    let active_file = restore
        .active_file
        .as_ref()
        .filter(|path| buffers.iter().any(|buffer| &buffer.path == *path))
        .cloned()
        .unwrap_or_else(|| buffers[0].path.clone());
    Some((buffers, active_file))
}
