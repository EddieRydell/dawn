use std::sync::Arc;

use super::{DesktopState, generated_source_texts, lock_unpoisoned};
use crate::dto::{
    AppSnapshot, GuiDocument, GuiDocumentRequest, GuiEditCommand, GuiEditResult,
    SequenceSelectionEdit, SequenceSelectionEditResult,
};
use crate::state_tasks::GuiHistoryEntry;

impl DesktopState {
    pub fn get_gui_document(&self, request: GuiDocumentRequest) -> GuiDocument {
        let project = self.project_session();
        crate::gui::project_gui_document(project.as_deref(), &request)
    }

    pub fn request_sequence_clip_rasters(
        &self,
        request: crate::dto::SequenceClipRasterRequest,
    ) -> crate::dto::SequenceClipRasterResponse {
        let snapshot = self.snapshot();
        let project_revision = snapshot.project_revision;
        let raster_settings = snapshot.settings.effect_raster;
        let project = self.project_session();
        let setup_id = project
            .as_ref()
            .map(|project| project.project.root.setup.clone());
        let sequence_id = self.resolve_sequence_id(&request.document);
        lock_unpoisoned(&self.sequence_clip_raster).request(
            project_revision,
            raster_settings,
            project,
            setup_id,
            sequence_id,
            request,
        )
    }

    pub fn take_sequence_clip_raster_results(
        &self,
        request: GuiDocumentRequest,
        request_id: u32,
    ) -> crate::dto::SequenceClipRasterResultBatch {
        let project_revision = self.snapshot().project_revision;
        lock_unpoisoned(&self.sequence_clip_raster).take_results(
            project_revision,
            request,
            request_id,
        )
    }

    pub fn sequence_clip_raster_pixels(&self, token: &str) -> Option<Vec<u8>> {
        lock_unpoisoned(&self.sequence_clip_raster).pixels_rgba_for_token(token)
    }

    pub fn apply_gui_edit(
        &self,
        request: GuiDocumentRequest,
        edit: GuiEditCommand,
    ) -> GuiEditResult {
        let Some(project) = self.project_session() else {
            let snapshot = self.snapshot();
            return GuiEditResult {
                snapshot,
                document: crate::gui::blocked("No project is loaded.", Vec::new()),
            };
        };
        let affected_paths = match crate::gui::affected_paths(&project, &request) {
            Ok(paths) => paths,
            Err(error) => {
                let snapshot = self.snapshot_with_error("gui.edit", &request.path, error.message());
                return GuiEditResult {
                    snapshot,
                    document: crate::gui::blocked(error.message(), Vec::new()),
                };
            }
        };
        if let Some(path) = self.dirty_affected_path(&affected_paths) {
            let message = format!("Save or reload {path} before using GUI edits.");
            let snapshot = self.snapshot_with_error("gui.dirty", &path, &message);
            return GuiEditResult {
                snapshot,
                document: crate::gui::blocked(message, Vec::new()),
            };
        }

        let before = Arc::clone(&project);
        let mut edited = (*project).clone();
        if let Err(error) = crate::gui::apply_edit(&mut edited, &request, edit) {
            let snapshot = self.snapshot_with_error("gui.edit", &request.path, error.message());
            return GuiEditResult {
                snapshot,
                document: crate::gui::blocked(error.message(), Vec::new()),
            };
        }
        let generated_text = match generated_source_texts(&edited, &affected_paths) {
            Ok(text) => text,
            Err(message) => {
                let snapshot = self.snapshot_with_error("gui.edit", &request.path, &message);
                return GuiEditResult {
                    snapshot,
                    document: crate::gui::blocked(message, Vec::new()),
                };
            }
        };
        let document = crate::gui::project_gui_document(Some(&edited), &request);
        let edited = Arc::new(edited);
        lock_unpoisoned(&self.gui_history).push_undo(GuiHistoryEntry {
            before,
            after: Arc::clone(&edited),
            affected_paths: affected_paths.clone(),
            status_path: request.path.clone(),
        });
        self.schedule_gui_save(Arc::clone(&edited), affected_paths, request.path.clone());
        let snapshot = self.apply_gui_project_update(edited, "GUI edit applied", generated_text);
        GuiEditResult { snapshot, document }
    }

    pub fn finish_composition_graph_editing(&self) -> AppSnapshot {
        lock_unpoisoned(&self.render_refresh).invalidate_pending();
        let render_error = self
            .project_session()
            .and_then(|session| self.refresh_render_session(&session.project));
        self.update_snapshot(|snapshot| {
            snapshot.render_error =
                render_error.map(|error| format!("Render refresh failed: {error:?}"));
        })
    }

    pub fn apply_sequence_selection_edit(
        &self,
        edit: SequenceSelectionEdit,
    ) -> SequenceSelectionEditResult {
        let Some(request) = self.active_sequence_gui_request() else {
            return SequenceSelectionEditResult {
                snapshot: self.snapshot_with_error(
                    "gui.sequence.selection",
                    "",
                    "No active sequence GUI document is available.",
                ),
                document: crate::gui::blocked(
                    "No active sequence GUI document is available.",
                    Vec::new(),
                ),
                selection: None,
                copied_count: 0,
                skipped_count: 0,
            };
        };
        let Some(project) = self.project_session() else {
            return SequenceSelectionEditResult {
                snapshot: self.snapshot(),
                document: crate::gui::blocked("No project is loaded.", Vec::new()),
                selection: None,
                copied_count: 0,
                skipped_count: 0,
            };
        };
        let affected_paths = match crate::gui::affected_paths(&project, &request) {
            Ok(paths) => paths,
            Err(error) => {
                return SequenceSelectionEditResult {
                    snapshot: self.snapshot_with_error(
                        "gui.sequence.selection",
                        &request.path,
                        error.message(),
                    ),
                    document: crate::gui::blocked(error.message(), Vec::new()),
                    selection: None,
                    copied_count: 0,
                    skipped_count: 0,
                };
            }
        };
        if let Some(path) = self.dirty_affected_path(&affected_paths) {
            let message = format!("Save or reload {path} before using GUI edits.");
            return SequenceSelectionEditResult {
                snapshot: self.snapshot_with_error("gui.dirty", &path, &message),
                document: crate::gui::blocked(message, Vec::new()),
                selection: None,
                copied_count: 0,
                skipped_count: 0,
            };
        }

        let before =
            (!matches!(&edit, SequenceSelectionEdit::Copy { .. })).then(|| Arc::clone(&project));
        let mut edited = (*project).clone();
        let mutation = crate::gui::apply_sequence_selection_edit(
            &mut edited,
            &request,
            edit,
            &mut lock_unpoisoned(&self.sequence_clipboard),
        );
        let mutation = match mutation {
            Ok(mutation) => mutation,
            Err(error) => {
                return SequenceSelectionEditResult {
                    snapshot: self.snapshot_with_error(
                        "gui.sequence.selection",
                        &request.path,
                        error.message(),
                    ),
                    document: crate::gui::blocked(error.message(), Vec::new()),
                    selection: None,
                    copied_count: 0,
                    skipped_count: 0,
                };
            }
        };
        let document = crate::gui::project_gui_document(Some(&edited), &request);
        let snapshot = if let Some(before) = before {
            let generated_text = match generated_source_texts(&edited, &affected_paths) {
                Ok(text) => text,
                Err(message) => {
                    return SequenceSelectionEditResult {
                        snapshot: self.snapshot_with_error(
                            "gui.sequence.selection",
                            &request.path,
                            &message,
                        ),
                        document: crate::gui::blocked(message, Vec::new()),
                        selection: None,
                        copied_count: 0,
                        skipped_count: 0,
                    };
                }
            };
            let edited = Arc::new(edited);
            lock_unpoisoned(&self.gui_history).push_undo(GuiHistoryEntry {
                before,
                after: Arc::clone(&edited),
                affected_paths: affected_paths.clone(),
                status_path: request.path.clone(),
            });
            self.schedule_gui_save(Arc::clone(&edited), affected_paths, request.path.clone());
            self.apply_gui_project_update(edited, "GUI selection edit applied", generated_text)
        } else {
            self.snapshot()
        };
        SequenceSelectionEditResult {
            snapshot,
            document,
            selection: mutation.selection,
            copied_count: mutation.copied_count,
            skipped_count: mutation.skipped_count,
        }
    }

    pub fn undo_active_edit(&self) -> AppSnapshot {
        let Some(entry) = lock_unpoisoned(&self.gui_history).peek_undo() else {
            return self.update_snapshot(|snapshot| {
                snapshot.status = "No GUI edit to undo".to_string();
            });
        };
        if let Some(path) = self.dirty_affected_path(&entry.affected_paths) {
            let message = format!("Save or reload {path} before undoing GUI edits.");
            return self.snapshot_with_error("gui.undo.dirty", &path, &message);
        }
        let generated_text = match generated_source_texts(&entry.before, &entry.affected_paths) {
            Ok(text) => text,
            Err(message) => {
                return self.snapshot_with_error("gui.undo", &entry.status_path, &message);
            }
        };
        let entry = {
            let mut history = lock_unpoisoned(&self.gui_history);
            let Some(entry) = history.pop_undo() else {
                return self.snapshot();
            };
            history.push_redo(entry.clone());
            entry
        };
        self.schedule_gui_save(
            Arc::clone(&entry.before),
            entry.affected_paths.clone(),
            entry.status_path.clone(),
        );
        self.apply_gui_project_update(entry.before, "GUI edit undone", generated_text)
    }

    pub fn redo_active_edit(&self) -> AppSnapshot {
        let Some(entry) = lock_unpoisoned(&self.gui_history).peek_redo() else {
            return self.update_snapshot(|snapshot| {
                snapshot.status = "No GUI edit to redo".to_string();
            });
        };
        if let Some(path) = self.dirty_affected_path(&entry.affected_paths) {
            let message = format!("Save or reload {path} before redoing GUI edits.");
            return self.snapshot_with_error("gui.redo.dirty", &path, &message);
        }
        let generated_text = match generated_source_texts(&entry.after, &entry.affected_paths) {
            Ok(text) => text,
            Err(message) => {
                return self.snapshot_with_error("gui.redo", &entry.status_path, &message);
            }
        };
        let entry = {
            let mut history = lock_unpoisoned(&self.gui_history);
            let Some(entry) = history.pop_redo() else {
                return self.snapshot();
            };
            history.push_undo_from_redo(entry.clone());
            entry
        };
        self.schedule_gui_save(
            Arc::clone(&entry.after),
            entry.affected_paths.clone(),
            entry.status_path.clone(),
        );
        self.apply_gui_project_update(entry.after, "GUI edit redone", generated_text)
    }
}
