use std::collections::BTreeSet;
use std::fs;
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::{
    IoDiagnostic, IoDiagnosticSeverity, ProjectCheckReport, ProjectSession, SourceDocument,
    SourceDocumentKind, SourceObjectKind, check_document_text, check_project, save_project,
    source_document_text as serialized_source_document_text,
};
use indexmap::IndexSet;

use crate::dto::{
    AppSettings, AppSnapshot, AudioTransportSnapshot, AudioTransportState, BufferExternalState,
    DefaultDawnViewMode, DiagnosticSeverity, DocumentDefaultObjectKey, DocumentDescriptor,
    DocumentObjectDescriptor, DocumentViewId, EditorBuffer, EditorViewMode, GuiDocument,
    GuiDocumentRequest, GuiEditCommand, GuiEditResult, LiveOutputSnapshot, ObjectKind,
    ProjectDiagnostic, ProjectTreeMode, SequenceAudio, SequenceSelectionEdit,
    SequenceSelectionEditResult, WorkspaceEntry, WorkspaceEntryKind,
};
use crate::persistence::{
    PersistedEditorViewStateUpdate, PersistedProjectSession, PersistedSequenceViewportStateUpdate,
    PersistenceService, ProjectRestoreState,
};

pub struct DesktopState {
    snapshot: Mutex<AppSnapshot>,
    project: Mutex<Option<ProjectSession>>,
    gui_save: Mutex<GuiSaveScheduler>,
    render_refresh: Mutex<RenderRefreshScheduler>,
    audio: Mutex<crate::audio::AudioEngine>,
    show_render: Mutex<crate::show_render::ShowRenderService>,
    sequence_clip_raster: Mutex<crate::sequence_clip_raster::SequenceClipRasterService>,
    sequence_clipboard: Mutex<Option<crate::gui::SequenceClipboard>>,
    persistence: PersistenceService,
}

impl DesktopState {
    pub fn new() -> Self {
        Self {
            snapshot: Mutex::new(empty_snapshot()),
            project: Mutex::new(None),
            gui_save: Mutex::new(GuiSaveScheduler::new()),
            render_refresh: Mutex::new(RenderRefreshScheduler::new()),
            audio: Mutex::new(crate::audio::AudioEngine::new()),
            show_render: Mutex::new(crate::show_render::ShowRenderService::new()),
            sequence_clip_raster: Mutex::new(
                crate::sequence_clip_raster::SequenceClipRasterService::new(),
            ),
            sequence_clipboard: Mutex::new(None),
            persistence: PersistenceService::new(),
        }
    }

    pub fn persistence(&self) -> &PersistenceService {
        &self.persistence
    }

    pub fn apply_persisted_settings(&self) -> AppSnapshot {
        let settings = sanitize_app_settings(self.persistence.settings());
        self.update_snapshot(|snapshot| {
            snapshot.settings = settings;
        })
    }

    pub fn snapshot(&self) -> AppSnapshot {
        self.drain_gui_save_results();
        self.drain_render_refresh_results();
        let mut snapshot = match self.snapshot.lock() {
            Ok(snapshot) => snapshot.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        snapshot.audio_transport = self.merged_audio_snapshot(&snapshot.audio_transport);
        snapshot
    }

    pub fn audio_snapshot(&self) -> AudioTransportSnapshot {
        match self.audio.lock() {
            Ok(mut audio) => audio.snapshot(),
            Err(poisoned) => poisoned.into_inner().snapshot(),
        }
    }

    fn merged_audio_snapshot(&self, previous: &AudioTransportSnapshot) -> AudioTransportSnapshot {
        let mut audio_transport = self.audio_snapshot();
        if matches!(audio_transport.state, AudioTransportState::Unloaded) {
            audio_transport.position_seconds = previous.position_seconds;
            audio_transport.home_seconds = previous.home_seconds;
        }
        audio_transport
    }

    pub fn update_snapshot(&self, update: impl FnOnce(&mut AppSnapshot)) -> AppSnapshot {
        let snapshot = match self.snapshot.lock() {
            Ok(mut snapshot) => {
                update(&mut snapshot);
                snapshot.audio_transport = self.merged_audio_snapshot(&snapshot.audio_transport);
                snapshot.clone()
            }
            Err(poisoned) => {
                let mut snapshot = poisoned.into_inner();
                update(&mut snapshot);
                snapshot.audio_transport = self.merged_audio_snapshot(&snapshot.audio_transport);
                snapshot.clone()
            }
        };
        self.record_persistent_snapshot(&snapshot);
        snapshot
    }

    pub fn set_persistence_error(&self, message: String) -> AppSnapshot {
        self.update_snapshot(|snapshot| {
            snapshot.status = message;
        })
    }

    pub fn update_app_settings(&self, settings: AppSettings) -> AppSnapshot {
        let settings = sanitize_app_settings(settings);
        if let Err(error) = self.persistence.record_settings(settings.clone()) {
            return self.set_persistence_error(format!("Settings were not saved: {error}"));
        }
        self.update_snapshot(|snapshot| {
            let tree_mode = settings.project_tree_mode.clone();
            snapshot.settings = settings;
            match tree_mode {
                ProjectTreeMode::Show => snapshot.project_tree_visible = true,
                ProjectTreeMode::Hide => snapshot.project_tree_visible = false,
                ProjectTreeMode::Remember => {}
            }
        })
    }

    pub fn set_render_error_if_changed(&self, message: String) {
        let current = match self.snapshot.lock() {
            Ok(snapshot) => snapshot.render_error.clone(),
            Err(poisoned) => poisoned.into_inner().render_error.clone(),
        };
        if current.as_deref() != Some(message.as_str()) {
            self.update_snapshot(|snapshot| {
                snapshot.render_error = Some(message);
            });
        }
    }

    pub fn clear_render_error_if_set(&self) {
        let current = match self.snapshot.lock() {
            Ok(snapshot) => snapshot.render_error.is_some(),
            Err(poisoned) => poisoned.into_inner().render_error.is_some(),
        };
        if current {
            self.update_snapshot(|snapshot| {
                snapshot.render_error = None;
            });
        }
    }

    pub fn load_sequence_audio(&self, request: GuiDocumentRequest) -> AppSnapshot {
        let audio = self.resolve_sequence_audio(&request);
        let sequence_id = self.resolve_sequence_id(&request);
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.load(audio),
            Err(poisoned) => poisoned.into_inner().load(audio),
        };
        let render_error = match (self.project_session(), sequence_id) {
            (Some(project), Some(sequence_id)) => {
                self.prepare_sequence_render(&project, &sequence_id)
            }
            _ => {
                self.unload_render_session();
                None
            }
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
            if let Some(error) = render_error {
                snapshot.render_error = Some(format!("Render prepare failed: {error:?}"));
            } else {
                snapshot.render_error = None;
            }
        })
    }

    pub fn unload_audio(&self) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.unload(),
            Err(poisoned) => poisoned.into_inner().unload(),
        };
        self.unload_render_session();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
            snapshot.render_error = None;
        })
    }

    pub fn audio_play(&self) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.play(),
            Err(poisoned) => poisoned.into_inner().play(),
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_pause(&self) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.pause(),
            Err(poisoned) => poisoned.into_inner().pause(),
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_stop(&self) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.stop(),
            Err(poisoned) => poisoned.into_inner().stop(),
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_rewind_to_zero(&self) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.rewind_to_zero(),
            Err(poisoned) => poisoned.into_inner().rewind_to_zero(),
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_seek(&self, position_seconds: f64) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.seek(position_seconds),
            Err(poisoned) => poisoned.into_inner().seek(position_seconds),
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn render_current_sequence_frame(
        &self,
    ) -> Result<crate::show_render::AudioClockRenderedFrame, crate::show_render::ShowRenderError>
    {
        self.drain_render_refresh_results();
        let audio_transport = self.audio_snapshot();
        match self.show_render.lock() {
            Ok(show_render) => show_render.render_current_sequence_frame(&audio_transport),
            Err(poisoned) => poisoned
                .into_inner()
                .render_current_sequence_frame(&audio_transport),
        }
    }

    pub fn preview_scene(&self) -> Option<crate::preview::PreviewScene> {
        match self.project.lock() {
            Ok(project) => {
                let session = project.as_ref()?;
                Some(crate::preview::PreviewScene::from_project(
                    self.project_revision(),
                    &session.project,
                ))
            }
            Err(poisoned) => {
                let project = poisoned.into_inner();
                let session = project.as_ref()?;
                Some(crate::preview::PreviewScene::from_project(
                    self.project_revision(),
                    &session.project,
                ))
            }
        }
    }

    pub fn preview_scene_revision(&self) -> Option<u64> {
        match self.project.lock() {
            Ok(project) => project.as_ref().map(|_| self.project_revision()),
            Err(poisoned) => poisoned
                .into_inner()
                .as_ref()
                .map(|_| self.project_revision()),
        }
    }

    pub fn open_project_path(&self, path: &str) -> AppSnapshot {
        let entrypoint = normalize_project_entrypoint(path);
        self.apply_project_open_check(entrypoint.as_str(), check_project(&entrypoint))
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
        let Some(mut buffer) = editor_buffer_for_path(&project, &relative_path) else {
            return self.snapshot_with_error(
                "file.open",
                path,
                "File is not part of the loaded project",
            );
        };
        let descriptor = project
            .source
            .documents
            .get(&relative_path)
            .map(document_descriptor)
            .unwrap_or_else(|| empty_document_descriptor(&relative_path));
        let existing_view_mode = self
            .snapshot()
            .tabs
            .into_iter()
            .find(|tab| tab.path == path)
            .map(|tab| tab.view_mode);
        buffer.view_mode = existing_view_mode.unwrap_or_else(|| {
            default_view_mode_for_descriptor(&self.snapshot().settings, &descriptor)
        });
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
            .get(&relative_path)
            .map(document_descriptor)
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
                        .get(Utf8Path::new(&next.path))
                        .map(document_descriptor)
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
        let active_path = self.snapshot().active_file;
        let diagnostics = active_path
            .as_deref()
            .map(|path| check_document_text(Utf8Path::new(path), &text))
            .unwrap_or_default();
        self.update_snapshot(|snapshot| {
            if let Some(buffer) = snapshot.active_buffer.as_mut() {
                buffer.text = text.clone();
                buffer.dirty = true;
                if let Some(tab) = snapshot.tabs.iter_mut().find(|tab| tab.path == buffer.path) {
                    tab.text = text;
                    tab.dirty = true;
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

    pub fn set_active_view_mode(&self, mode: EditorViewMode) -> AppSnapshot {
        let active_path = self.snapshot().active_file;
        let text = if matches!(mode, EditorViewMode::Text) {
            match active_path
                .as_deref()
                .map(|path| self.text_for_active_view_mode_switch(path))
            {
                Some(Ok(text)) => text,
                Some(Err(error)) => {
                    return self.snapshot_with_error(
                        "file.viewMode",
                        active_path.as_deref().unwrap_or_default(),
                        &error,
                    );
                }
                None => None,
            }
        } else {
            None
        };
        self.update_snapshot(|snapshot| {
            if let Some(buffer) = snapshot.active_buffer.as_mut() {
                buffer.view_mode = mode.clone();
                if let Some(text) = text.as_ref() {
                    buffer.text = text.clone();
                    buffer.dirty = false;
                    buffer.external_state = BufferExternalState::Current;
                }
            }
            if let Some(path) = snapshot.active_file.as_deref()
                && let Some(tab) = snapshot.tabs.iter_mut().find(|tab| tab.path == path)
            {
                tab.view_mode = mode;
                if let Some(text) = text {
                    tab.text = text;
                    tab.dirty = false;
                    tab.external_state = BufferExternalState::Current;
                }
            }
        })
    }

    pub fn save_active_buffer(&self) -> AppSnapshot {
        let snapshot = self.snapshot();
        let Some(buffer) = snapshot.active_buffer else {
            return snapshot;
        };
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
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
                let entrypoint = project.source.source_root.join(&project.source.entrypoint);
                self.apply_project_refresh_check(entrypoint.as_str(), check_project(&entrypoint))
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
        let entrypoint = project.source.source_root.join(&project.source.entrypoint);
        self.apply_project_refresh_check(entrypoint.as_str(), check_project(&entrypoint))
    }

    pub fn get_gui_document(&self, request: GuiDocumentRequest) -> GuiDocument {
        let project = self.project_session();
        crate::gui::project_gui_document(project.as_ref(), &request)
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
        let project_model = project.map(|project| project.project);
        match self.sequence_clip_raster.lock() {
            Ok(mut raster) => raster.request(
                project_revision,
                raster_settings,
                project_model,
                setup_id,
                sequence_id,
                request,
            ),
            Err(poisoned) => poisoned.into_inner().request(
                project_revision,
                raster_settings,
                project_model,
                setup_id,
                sequence_id,
                request,
            ),
        }
    }

    pub fn take_sequence_clip_raster_results(
        &self,
        request: GuiDocumentRequest,
        request_id: u32,
    ) -> crate::dto::SequenceClipRasterResultBatch {
        let project_revision = self.snapshot().project_revision;
        match self.sequence_clip_raster.lock() {
            Ok(mut raster) => raster.take_results(project_revision, request, request_id),
            Err(poisoned) => {
                poisoned
                    .into_inner()
                    .take_results(project_revision, request, request_id)
            }
        }
    }

    pub fn sequence_clip_raster_pixels(&self, token: &str) -> Option<Vec<u8>> {
        match self.sequence_clip_raster.lock() {
            Ok(mut raster) => raster.pixels_rgba_for_token(token),
            Err(poisoned) => poisoned.into_inner().pixels_rgba_for_token(token),
        }
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
        let affected_paths = match crate::gui::affected_paths(&project, &request, &edit) {
            Ok(paths) => paths,
            Err(error) => {
                let snapshot = self.snapshot_with_error("gui.edit", &request.path, error.message());
                return GuiEditResult {
                    snapshot,
                    document: crate::gui::blocked(error.message(), Vec::new()),
                };
            }
        };
        let dirty_path = self
            .snapshot()
            .tabs
            .into_iter()
            .find(|tab| tab.dirty && affected_paths.contains(&tab.path))
            .map(|tab| tab.path);
        if let Some(path) = dirty_path {
            let message = format!("Save or reload {path} before using GUI edits.");
            let snapshot = self.snapshot_with_error("gui.dirty", &path, &message);
            return GuiEditResult {
                snapshot,
                document: crate::gui::blocked(message, Vec::new()),
            };
        }

        let mut edited = project;
        if let Err(error) = crate::gui::apply_edit(&mut edited, &request, edit) {
            let snapshot = self.snapshot_with_error("gui.edit", &request.path, error.message());
            return GuiEditResult {
                snapshot,
                document: crate::gui::blocked(error.message(), Vec::new()),
            };
        }
        let document = crate::gui::project_gui_document(Some(&edited), &request);
        self.schedule_gui_save(&edited, affected_paths, request.path.clone());
        let snapshot = self.apply_gui_project_update(edited, "GUI edit applied");
        GuiEditResult { snapshot, document }
    }

    pub fn apply_active_sequence_gui_edit(&self, edit: crate::dto::SequenceGuiEdit) -> AppSnapshot {
        let Some(request) = self.active_sequence_gui_request() else {
            return self.snapshot_with_error(
                "gui.sequence",
                "",
                "No active sequence GUI document is available.",
            );
        };
        self.apply_gui_edit(request, GuiEditCommand::Sequence { edit })
            .snapshot
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
        let affected_paths = match crate::gui::affected_paths(
            &project,
            &request,
            &GuiEditCommand::Sequence {
                edit: crate::dto::SequenceGuiEdit::DeleteEffect { id: 0 },
            },
        ) {
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
        let dirty_path = self
            .snapshot()
            .tabs
            .into_iter()
            .find(|tab| tab.dirty && affected_paths.contains(&tab.path))
            .map(|tab| tab.path);
        if let Some(path) = dirty_path {
            let message = format!("Save or reload {path} before using GUI edits.");
            return SequenceSelectionEditResult {
                snapshot: self.snapshot_with_error("gui.dirty", &path, &message),
                document: crate::gui::blocked(message, Vec::new()),
                selection: None,
                copied_count: 0,
                skipped_count: 0,
            };
        }

        let mut edited = project;
        let previous_project = edited.project.clone();
        let mutation = match self.sequence_clipboard.lock() {
            Ok(mut clipboard) => crate::gui::apply_sequence_selection_edit(
                &mut edited,
                &request,
                edit,
                &mut clipboard,
            ),
            Err(poisoned) => {
                let mut clipboard = poisoned.into_inner();
                crate::gui::apply_sequence_selection_edit(
                    &mut edited,
                    &request,
                    edit,
                    &mut clipboard,
                )
            }
        };
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
        let snapshot = if edited.project == previous_project {
            self.snapshot()
        } else {
            self.schedule_gui_save(&edited, affected_paths, request.path.clone());
            self.apply_gui_project_update(edited, "GUI selection edit applied")
        };
        SequenceSelectionEditResult {
            snapshot,
            document,
            selection: mutation.selection,
            copied_count: mutation.copied_count,
            skipped_count: mutation.skipped_count,
        }
    }

    fn apply_project_open_check(
        &self,
        entrypoint: &str,
        report: ProjectCheckReport,
    ) -> AppSnapshot {
        let diagnostics = project_diagnostics(&report);
        match report.session {
            Some(session) => self.replace_project(session, diagnostics),
            None => self.update_snapshot(|snapshot| {
                snapshot.diagnostics = diagnostics;
                snapshot.status = format!("Failed to open project {entrypoint}");
            }),
        }
    }

    fn apply_project_refresh_check(
        &self,
        entrypoint: &str,
        report: ProjectCheckReport,
    ) -> AppSnapshot {
        let diagnostics = project_diagnostics(&report);
        match report.session {
            Some(session) => self.refresh_project(session, diagnostics),
            None => self.update_snapshot(|snapshot| {
                snapshot.diagnostics = diagnostics;
                snapshot.status = format!("Project check failed for {entrypoint}");
            }),
        }
    }

    fn replace_project(
        &self,
        session: ProjectSession,
        diagnostics: Vec<ProjectDiagnostic>,
    ) -> AppSnapshot {
        let entries = workspace_entries(&session);
        let root = session.source.source_root.to_string();
        let entrypoint = session.source.entrypoint.clone();
        let valid_paths = valid_project_paths(&session);
        let restore = self.persistence.restore_for_project(&root, &valid_paths);
        let active = restored_active_buffers(&session, restore.as_ref().map(|item| &item.session))
            .or_else(|| {
                session.source.documents.get(&entrypoint).map(|document| {
                    let buffer = editor_buffer(&session, document);
                    (vec![buffer.clone()], buffer.path)
                })
            });
        let active_descriptor = active
            .as_ref()
            .and_then(|(_, active_path)| descriptor_for_path(&session, Utf8Path::new(active_path)));
        match self.project.lock() {
            Ok(mut project) => *project = Some(session),
            Err(poisoned) => *poisoned.into_inner() = Some(session),
        }
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

    fn record_persistent_snapshot(&self, snapshot: &AppSnapshot) {
        if let Err(error) = self.persistence.record_snapshot(snapshot) {
            match self.snapshot.lock() {
                Ok(mut current) => {
                    current.status = format!("Desktop state was not saved: {error}");
                }
                Err(poisoned) => {
                    poisoned.into_inner().status = format!("Desktop state was not saved: {error}");
                }
            }
        }
    }

    fn refresh_project(
        &self,
        session: ProjectSession,
        diagnostics: Vec<ProjectDiagnostic>,
    ) -> AppSnapshot {
        // A refresh applies the latest semantic project model in place. It must not
        // reload editor text or reset user-owned editor state; only full project
        // opens are allowed to replace tabs, buffers, GUI state, or transport.
        let entries = workspace_entries(&session);
        let root = session.source.source_root.to_string();
        let project_model = session.project.clone();
        let active_descriptor = self.snapshot().active_file.as_deref().and_then(|path| {
            let relative_path = Utf8Path::new(path);
            session
                .source
                .documents
                .get(relative_path)
                .map(document_descriptor)
                .or_else(|| {
                    absolute_project_path(&session, relative_path)
                        .is_some_and(|path| path.is_file())
                        .then(|| empty_document_descriptor(relative_path))
                })
        });
        match self.project.lock() {
            Ok(mut project) => *project = Some(session),
            Err(poisoned) => *poisoned.into_inner() = Some(session),
        }
        let render_error = self.refresh_render_session(&project_model);
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

    fn apply_gui_project_update(&self, session: ProjectSession, status: &str) -> AppSnapshot {
        let entries = workspace_entries(&session);
        let root = session.source.source_root.to_string();
        let project_model = session.project.clone();
        let active_descriptor = self.snapshot().active_file.as_deref().and_then(|path| {
            let relative_path = Utf8Path::new(path);
            session
                .source
                .documents
                .get(relative_path)
                .map(document_descriptor)
                .or_else(|| {
                    absolute_project_path(&session, relative_path)
                        .is_some_and(|path| path.is_file())
                        .then(|| empty_document_descriptor(relative_path))
                })
        });
        match self.project.lock() {
            Ok(mut project) => *project = Some(session),
            Err(poisoned) => *poisoned.into_inner() = Some(session),
        }
        self.schedule_render_refresh(project_model);
        self.update_snapshot(|snapshot| {
            snapshot.project_root = Some(root);
            snapshot.project_entries = entries;
            if active_descriptor.is_some() {
                snapshot.active_document_descriptor = active_descriptor;
            }
            snapshot.status = status.to_string();
            snapshot.project_revision = snapshot.project_revision.saturating_add(1);
        })
    }

    fn project_session(&self) -> Option<ProjectSession> {
        match self.project.lock() {
            Ok(project) => project.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn text_for_active_view_mode_switch(&self, path: &str) -> Result<Option<String>, String> {
        let Some(project) = self.project_session() else {
            return Ok(None);
        };
        let relative_path = Utf8Path::new(path);
        match serialized_source_document_text(&project, relative_path) {
            Ok(Some(text)) => Ok(Some(text)),
            Ok(None) => {
                let Some(absolute_path) = absolute_project_path(&project, relative_path) else {
                    return Ok(None);
                };
                fs::read_to_string(&absolute_path)
                    .map(Some)
                    .map_err(|error| error.to_string())
            }
            Err(error) => Err(error.to_string()),
        }
    }

    fn project_revision(&self) -> u64 {
        match self.snapshot.lock() {
            Ok(snapshot) => snapshot.project_revision.into(),
            Err(poisoned) => poisoned.into_inner().project_revision.into(),
        }
    }

    fn active_sequence_gui_request(&self) -> Option<GuiDocumentRequest> {
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

    fn resolve_sequence_audio(&self, request: &GuiDocumentRequest) -> Option<SequenceAudio> {
        let project = self.project_session();
        match crate::gui::project_gui_document(project.as_ref(), request) {
            GuiDocument::Sequence { document } => document.audio,
            _ => None,
        }
    }

    fn resolve_sequence_id(
        &self,
        request: &GuiDocumentRequest,
    ) -> Option<dawn_language::sequence::SequenceId> {
        let project = self.project_session()?;
        let path = Utf8Path::new(&request.path);
        project
            .source
            .source_map
            .objects
            .iter()
            .find(|(id, location)| {
                id.kind == SourceObjectKind::Sequence
                    && location.document == path
                    && request
                        .object_key
                        .as_deref()
                        .is_none_or(|key| location.object_key == key)
            })
            .map(|(id, _)| dawn_language::sequence::SequenceId(id.id.clone()))
    }

    fn prepare_sequence_render(
        &self,
        session: &ProjectSession,
        sequence_id: &dawn_language::sequence::SequenceId,
    ) -> Option<dawn_runtime::RenderError> {
        let setup_id = session.project.root.setup.clone();
        match self.show_render.lock() {
            Ok(mut show_render) => {
                let result = show_render.prepare(&session.project, &setup_id, sequence_id);
                if result.is_err() {
                    show_render.unload();
                }
                result.err()
            }
            Err(poisoned) => {
                let mut show_render = poisoned.into_inner();
                let result = show_render.prepare(&session.project, &setup_id, sequence_id);
                if result.is_err() {
                    show_render.unload();
                }
                result.err()
            }
        }
    }

    fn refresh_render_session(
        &self,
        project: &dawn_language::model::DawnProject,
    ) -> Option<dawn_runtime::RenderError> {
        match self.show_render.lock() {
            Ok(mut show_render) => {
                let result = show_render.refresh_project(project);
                if result.is_err() {
                    show_render.unload();
                }
                result.err()
            }
            Err(poisoned) => {
                let mut show_render = poisoned.into_inner();
                let result = show_render.refresh_project(project);
                if result.is_err() {
                    show_render.unload();
                }
                result.err()
            }
        }
    }

    fn schedule_render_refresh(&self, project: dawn_language::model::DawnProject) {
        let target = match self.show_render.lock() {
            Ok(show_render) => show_render.active_target(),
            Err(poisoned) => poisoned.into_inner().active_target(),
        };
        let Some((setup_id, sequence_id)) = target else {
            return;
        };
        if !project.sequences.contains_key(&sequence_id) {
            self.unload_render_session();
            return;
        }
        let request = RenderRefreshPayload {
            project,
            setup_id,
            sequence_id,
        };
        match self.render_refresh.lock() {
            Ok(mut scheduler) => {
                let _ = scheduler.schedule(request);
            }
            Err(poisoned) => {
                let mut scheduler = poisoned.into_inner();
                let _ = scheduler.schedule(request);
            }
        }
    }

    fn drain_render_refresh_results(&self) {
        let results = match self.render_refresh.lock() {
            Ok(scheduler) => scheduler.drain_current_results(),
            Err(poisoned) => poisoned.into_inner().drain_current_results(),
        };
        for result in results {
            match result {
                RenderRefreshResult::Refreshed {
                    sequence: _,
                    session,
                } => {
                    match self.show_render.lock() {
                        Ok(mut show_render) => show_render.apply_prepared(session),
                        Err(poisoned) => poisoned.into_inner().apply_prepared(session),
                    }
                    self.clear_render_error_if_set();
                }
                RenderRefreshResult::Failed {
                    sequence: _,
                    message,
                } => {
                    self.set_render_error_if_changed(format!("Render refresh failed: {message}"));
                }
            }
        }
    }

    fn unload_render_session(&self) {
        match self.show_render.lock() {
            Ok(mut show_render) => show_render.unload(),
            Err(poisoned) => poisoned.into_inner().unload(),
        }
    }

    fn create_fs_entry(&self, parent: &str, name: &str, kind: FsEntryKind) -> AppSnapshot {
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

    fn after_file_saved(&self, path: &str) -> AppSnapshot {
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

    fn after_workspace_changed(
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

    fn refresh_project_session(&self) -> bool {
        let Some(project) = self.project_session() else {
            return false;
        };
        let entrypoint = project.source.source_root.join(&project.source.entrypoint);
        let report = check_project(&entrypoint);
        let refreshed = report.session.is_some();
        self.apply_project_refresh_check(entrypoint.as_str(), report);
        refreshed
    }

    fn snapshot_with_error(&self, code: &str, path: &str, message: &str) -> AppSnapshot {
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

    fn schedule_gui_save(
        &self,
        session: &ProjectSession,
        affected_paths: std::collections::BTreeSet<String>,
        status_path: String,
    ) {
        let path_for_error = status_path.clone();
        let request = GuiSavePayload {
            session: session.clone(),
            affected_paths,
            status_path,
        };
        match self.gui_save.lock() {
            Ok(mut scheduler) => {
                if scheduler.schedule(request).is_err() {
                    self.snapshot_with_error(
                        "gui.save",
                        path_for_error.as_str(),
                        "GUI save worker is unavailable.",
                    );
                }
            }
            Err(poisoned) => {
                let mut scheduler = poisoned.into_inner();
                if scheduler.schedule(request).is_err() {
                    self.snapshot_with_error(
                        "gui.save",
                        path_for_error.as_str(),
                        "GUI save worker is unavailable.",
                    );
                }
            }
        }
    }

    fn drain_gui_save_results(&self) {
        let results = match self.gui_save.lock() {
            Ok(scheduler) => scheduler.drain_current_results(),
            Err(poisoned) => poisoned.into_inner().drain_current_results(),
        };
        for result in results {
            match result {
                GuiSaveResult::Saved {
                    sequence: _,
                    session,
                    affected_paths,
                } => {
                    if self.project_session().as_ref().is_some_and(|project| {
                        project.source.source_root == session.source.source_root
                    }) {
                        self.refresh_saved_tabs(&session, &affected_paths);
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

    fn refresh_saved_tabs(
        &self,
        _session: &ProjectSession,
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

struct GuiSaveScheduler {
    sender: mpsc::Sender<SequencedGuiSaveRequest>,
    receiver: mpsc::Receiver<GuiSaveResult>,
    latest_sequence: u64,
}

impl GuiSaveScheduler {
    fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        thread::spawn(move || gui_save_worker(request_receiver, result_sender));
        Self {
            sender: request_sender,
            receiver: result_receiver,
            latest_sequence: 0,
        }
    }

    fn schedule(&mut self, payload: GuiSavePayload) -> Result<(), GuiSaveScheduleError> {
        self.latest_sequence = self.latest_sequence.saturating_add(1);
        self.sender
            .send(SequencedGuiSaveRequest {
                sequence: self.latest_sequence,
                payload,
            })
            .map_err(|_| GuiSaveScheduleError)
    }

    fn drain_current_results(&self) -> Vec<GuiSaveResult> {
        self.receiver
            .try_iter()
            .filter(|result| result.sequence() == self.latest_sequence)
            .collect()
    }
}

struct SequencedGuiSaveRequest {
    sequence: u64,
    payload: GuiSavePayload,
}

struct GuiSavePayload {
    session: ProjectSession,
    affected_paths: std::collections::BTreeSet<String>,
    status_path: String,
}

struct GuiSaveScheduleError;

impl GuiSaveResult {
    fn sequence(&self) -> u64 {
        match self {
            GuiSaveResult::Saved { sequence, .. } | GuiSaveResult::Failed { sequence, .. } => {
                *sequence
            }
        }
    }
}

enum GuiSaveResult {
    Saved {
        sequence: u64,
        session: Box<ProjectSession>,
        affected_paths: std::collections::BTreeSet<String>,
    },
    Failed {
        sequence: u64,
        status_path: String,
        message: String,
    },
}

fn gui_save_worker(
    receiver: mpsc::Receiver<SequencedGuiSaveRequest>,
    sender: mpsc::Sender<GuiSaveResult>,
) {
    let debounce = Duration::from_millis(250);
    while let Ok(mut pending) = receiver.recv() {
        while let Ok(next) = receiver.recv_timeout(debounce) {
            pending = next;
        }
        let result = match save_project(&pending.payload.session) {
            Ok(_) => GuiSaveResult::Saved {
                sequence: pending.sequence,
                session: Box::new(pending.payload.session),
                affected_paths: pending.payload.affected_paths,
            },
            Err(error) => GuiSaveResult::Failed {
                sequence: pending.sequence,
                status_path: pending.payload.status_path,
                message: error.to_string(),
            },
        };
        if sender.send(result).is_err() {
            break;
        }
    }
}

struct RenderRefreshScheduler {
    sender: mpsc::Sender<SequencedRenderRefreshRequest>,
    receiver: mpsc::Receiver<RenderRefreshResult>,
    latest_sequence: u64,
}

impl RenderRefreshScheduler {
    fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        thread::spawn(move || render_refresh_worker(request_receiver, result_sender));
        Self {
            sender: request_sender,
            receiver: result_receiver,
            latest_sequence: 0,
        }
    }

    fn schedule(
        &mut self,
        payload: RenderRefreshPayload,
    ) -> Result<(), RenderRefreshScheduleError> {
        self.latest_sequence = self.latest_sequence.saturating_add(1);
        self.sender
            .send(SequencedRenderRefreshRequest {
                sequence: self.latest_sequence,
                payload,
            })
            .map_err(|_| RenderRefreshScheduleError)
    }

    fn drain_current_results(&self) -> Vec<RenderRefreshResult> {
        self.receiver
            .try_iter()
            .filter(|result| result.sequence() == self.latest_sequence)
            .collect()
    }
}

struct SequencedRenderRefreshRequest {
    sequence: u64,
    payload: RenderRefreshPayload,
}

struct RenderRefreshPayload {
    project: dawn_language::model::DawnProject,
    setup_id: dawn_language::setup::SetupId,
    sequence_id: dawn_language::sequence::SequenceId,
}

struct RenderRefreshScheduleError;

impl RenderRefreshResult {
    fn sequence(&self) -> u64 {
        match self {
            RenderRefreshResult::Refreshed { sequence, .. }
            | RenderRefreshResult::Failed { sequence, .. } => *sequence,
        }
    }
}

enum RenderRefreshResult {
    Refreshed {
        sequence: u64,
        session: crate::show_render::PreparedRenderSession,
    },
    Failed {
        sequence: u64,
        message: String,
    },
}

fn render_refresh_worker(
    receiver: mpsc::Receiver<SequencedRenderRefreshRequest>,
    sender: mpsc::Sender<RenderRefreshResult>,
) {
    while let Ok(pending) = receiver.recv() {
        let result = match crate::show_render::prepare_render_session(
            &pending.payload.project,
            &pending.payload.setup_id,
            &pending.payload.sequence_id,
        ) {
            Ok(session) => RenderRefreshResult::Refreshed {
                sequence: pending.sequence,
                session,
            },
            Err(error) => RenderRefreshResult::Failed {
                sequence: pending.sequence,
                message: format!("{error:?}"),
            },
        };
        if sender.send(result).is_err() {
            break;
        }
    }
}

impl Default for DesktopState {
    fn default() -> Self {
        Self::new()
    }
}

fn empty_snapshot() -> AppSnapshot {
    AppSnapshot {
        settings: AppSettings::default(),
        project_root: None,
        project_revision: 0,
        project_tree_visible: true,
        project_entries: Vec::new(),
        tabs: Vec::new(),
        active_file: None,
        active_buffer: None,
        active_document_descriptor: None,
        diagnostics: Vec::new(),
        status: "Ready".to_string(),
        render_error: None,
        preview_error: None,
        audio_transport: crate::audio::AudioEngine::empty_snapshot(),
        live_output: LiveOutputSnapshot {
            enabled: false,
            status: "Disabled".to_string(),
            active_universe_count: 0,
            last_error: None,
        },
    }
}

fn normalize_project_entrypoint(path: &str) -> Utf8PathBuf {
    let path = Utf8PathBuf::from(path);
    if path.is_dir() {
        path.join("project.dawn")
    } else {
        path
    }
}

fn project_diagnostic(diagnostic: &IoDiagnostic) -> ProjectDiagnostic {
    ProjectDiagnostic {
        path: diagnostic.path.to_string(),
        range: diagnostic
            .range
            .as_ref()
            .map(|range| crate::dto::TextRange {
                start: crate::dto::TextPosition {
                    line: range.start.line,
                    character: range.start.character,
                },
                end: crate::dto::TextPosition {
                    line: range.end.line,
                    character: range.end.character,
                },
            }),
        severity: match diagnostic.severity {
            IoDiagnosticSeverity::Error => DiagnosticSeverity::Error,
            IoDiagnosticSeverity::Warning => DiagnosticSeverity::Warning,
        },
        code: diagnostic.code.as_str().to_string(),
        message: diagnostic.message.clone(),
    }
}

fn project_diagnostics(report: &ProjectCheckReport) -> Vec<ProjectDiagnostic> {
    report
        .diagnostics
        .iter()
        .map(project_diagnostic)
        .collect::<Vec<_>>()
}

fn workspace_entries(session: &ProjectSession) -> Vec<WorkspaceEntry> {
    let mut paths = IndexSet::new();
    collect_workspace_paths(&session.source.source_root, Utf8Path::new(""), &mut paths);
    for path in session.source.documents.keys() {
        insert_path_with_parents(&mut paths, path);
    }
    paths.sort();
    paths.into_iter().map(workspace_entry).collect()
}

fn collect_workspace_paths(
    root: &Utf8Path,
    relative: &Utf8Path,
    paths: &mut IndexSet<Utf8PathBuf>,
) {
    let absolute = root.join(relative);
    let Ok(entries) = fs::read_dir(absolute) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let path = if relative.as_str().is_empty() {
            Utf8PathBuf::from(name)
        } else {
            relative.join(name)
        };
        paths.insert(path.clone());
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            collect_workspace_paths(root, &path, paths);
        }
    }
}

fn insert_path_with_parents(paths: &mut IndexSet<Utf8PathBuf>, path: &Utf8Path) {
    let mut current = Utf8PathBuf::new();
    for component in path.components() {
        let camino::Utf8Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        paths.insert(current.clone());
    }
}

fn workspace_entry(path: Utf8PathBuf) -> WorkspaceEntry {
    let name = path
        .file_name()
        .map(ToString::to_string)
        .unwrap_or_else(|| path.to_string());
    let parent = path.parent().map(Utf8Path::to_string).unwrap_or_default();
    let kind = if path.extension().is_some() {
        WorkspaceEntryKind::File
    } else {
        WorkspaceEntryKind::Directory
    };
    WorkspaceEntry {
        path: path.to_string(),
        kind,
        name,
        parent,
    }
}

fn editor_buffer(session: &ProjectSession, document: &SourceDocument) -> EditorBuffer {
    let disk_path = session.source.source_root.join(&document.relative_path);
    let text = fs::read_to_string(&disk_path).unwrap_or_else(|_| source_document_text(document));
    EditorBuffer {
        path: document.relative_path.to_string(),
        name: document
            .relative_path
            .file_name()
            .map(ToString::to_string)
            .unwrap_or_else(|| document.relative_path.to_string()),
        text,
        dirty: false,
        external_state: BufferExternalState::Current,
        view_mode: EditorViewMode::Text,
    }
}

fn editor_buffer_for_path(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<EditorBuffer> {
    if let Some(document) = session.source.documents.get(relative_path) {
        return Some(editor_buffer(session, document));
    }
    let path = absolute_project_path(session, relative_path)?;
    if !path.is_file() {
        return None;
    }
    let text = fs::read_to_string(&path).unwrap_or_default();
    Some(EditorBuffer {
        path: relative_path.to_string(),
        name: relative_path
            .file_name()
            .map(ToString::to_string)
            .unwrap_or_else(|| relative_path.to_string()),
        text,
        dirty: false,
        external_state: BufferExternalState::Current,
        view_mode: EditorViewMode::Text,
    })
}

fn restored_active_buffers(
    session: &ProjectSession,
    restore: Option<&PersistedProjectSession>,
) -> Option<(Vec<EditorBuffer>, String)> {
    let restore = restore?;
    let mut buffers = Vec::new();
    for tab in &restore.tabs {
        let relative_path = Utf8Path::new(&tab.path);
        if let Some(mut buffer) = editor_buffer_for_path(session, relative_path) {
            buffer.view_mode = tab.view_mode.clone();
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

fn descriptor_for_path(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<DocumentDescriptor> {
    session
        .source
        .documents
        .get(relative_path)
        .map(document_descriptor)
        .or_else(|| {
            absolute_project_path(session, relative_path)
                .is_some_and(|path| path.is_file())
                .then(|| empty_document_descriptor(relative_path))
        })
}

fn valid_project_paths(session: &ProjectSession) -> BTreeSet<String> {
    workspace_entries(session)
        .into_iter()
        .filter(|entry| matches!(entry.kind, WorkspaceEntryKind::File))
        .map(|entry| entry.path)
        .collect()
}

fn source_document_text(document: &SourceDocument) -> String {
    match &document.kind {
        SourceDocumentKind::Dawn { value, .. } => {
            yaml_serde::to_string(value).unwrap_or_else(|_| String::new())
        }
        SourceDocumentKind::Effect { source } => source.clone(),
    }
}

fn document_descriptor(document: &SourceDocument) -> DocumentDescriptor {
    let objects = document
        .exported_objects
        .iter()
        .zip(document_object_kinds(document))
        .map(|(key, kind)| DocumentObjectDescriptor {
            key: key.clone(),
            kind,
        })
        .collect::<Vec<_>>();
    let available_views = available_views(&objects);
    let default_object_keys = default_object_keys(&objects);
    DocumentDescriptor {
        path: document.relative_path.to_string(),
        objects,
        available_views,
        default_object_keys,
    }
}

fn empty_document_descriptor(path: &Utf8Path) -> DocumentDescriptor {
    DocumentDescriptor {
        path: path.to_string(),
        objects: Vec::new(),
        available_views: vec![DocumentViewId::Text],
        default_object_keys: Vec::new(),
    }
}

fn document_object_kinds(document: &SourceDocument) -> Vec<ObjectKind> {
    match &document.kind {
        SourceDocumentKind::Dawn { object_types, .. } => {
            object_types.iter().map(object_kind).collect()
        }
        SourceDocumentKind::Effect { .. } => document
            .exported_objects
            .iter()
            .map(|_| ObjectKind::Effect)
            .collect(),
    }
}

fn object_kind(kind: &SourceObjectKind) -> ObjectKind {
    match kind {
        SourceObjectKind::Project => ObjectKind::Project,
        SourceObjectKind::Setup => ObjectKind::Setup,
        SourceObjectKind::Controller => ObjectKind::Controller,
        SourceObjectKind::Layout => ObjectKind::Layout,
        SourceObjectKind::Patch => ObjectKind::Patch,
        SourceObjectKind::FixtureDefinition => ObjectKind::Fixture,
        SourceObjectKind::Curve => ObjectKind::Curve,
        SourceObjectKind::Sequence => ObjectKind::Sequence,
        SourceObjectKind::EffectDefinition | SourceObjectKind::EffectInstance => ObjectKind::Effect,
    }
}

fn available_views(objects: &[DocumentObjectDescriptor]) -> Vec<DocumentViewId> {
    let mut views = vec![DocumentViewId::Text];
    for object in objects {
        let view = match object.kind {
            ObjectKind::Layout => Some(DocumentViewId::Layout),
            ObjectKind::Fixture => Some(DocumentViewId::Fixture),
            ObjectKind::Sequence => Some(DocumentViewId::Sequence),
            _ => None,
        };
        if let Some(view) = view
            && !views.iter().any(|existing| same_view(existing, &view))
        {
            views.push(view);
        }
    }
    views
}

fn default_object_keys(objects: &[DocumentObjectDescriptor]) -> Vec<DocumentDefaultObjectKey> {
    objects
        .iter()
        .filter_map(|object| {
            let view = match object.kind {
                ObjectKind::Layout => DocumentViewId::Layout,
                ObjectKind::Fixture => DocumentViewId::Fixture,
                ObjectKind::Sequence => DocumentViewId::Sequence,
                _ => return None,
            };
            Some(DocumentDefaultObjectKey {
                view,
                object_key: object.key.clone(),
            })
        })
        .collect()
}

fn same_view(left: &DocumentViewId, right: &DocumentViewId) -> bool {
    matches!(
        (left, right),
        (DocumentViewId::Text, DocumentViewId::Text)
            | (DocumentViewId::Layout, DocumentViewId::Layout)
            | (DocumentViewId::Fixture, DocumentViewId::Fixture)
            | (DocumentViewId::Sequence, DocumentViewId::Sequence)
    )
}

fn default_view_mode_for_descriptor(
    settings: &AppSettings,
    descriptor: &DocumentDescriptor,
) -> EditorViewMode {
    match settings.default_dawn_view_mode {
        DefaultDawnViewMode::Remember | DefaultDawnViewMode::Text => EditorViewMode::Text,
        DefaultDawnViewMode::Gui => {
            if descriptor
                .available_views
                .iter()
                .any(|view| !matches!(view, DocumentViewId::Text))
            {
                EditorViewMode::Gui
            } else {
                EditorViewMode::Text
            }
        }
    }
}

fn sanitize_app_settings(mut settings: AppSettings) -> AppSettings {
    if !settings.sequence_initial_px_per_second.is_finite() {
        settings.sequence_initial_px_per_second = 80.0;
    }
    if !settings.sequence_initial_lane_height_px.is_finite() {
        settings.sequence_initial_lane_height_px = 42.0;
    }
    if !settings.effect_raster.render_scale.is_finite() {
        settings.effect_raster.render_scale = 1.0;
    }
    settings.sequence_initial_px_per_second =
        settings.sequence_initial_px_per_second.clamp(20.0, 12000.0);
    settings.sequence_initial_lane_height_px =
        settings.sequence_initial_lane_height_px.clamp(24.0, 120.0);
    settings.effect_raster.render_scale = settings.effect_raster.render_scale.clamp(0.25, 2.0);
    settings.effect_raster.max_columns = settings.effect_raster.max_columns.clamp(16, 1024);
    settings.effect_raster.max_rows = settings.effect_raster.max_rows.clamp(1, 200);
    settings.effect_raster.min_frame_stride = settings.effect_raster.min_frame_stride.clamp(1, 16);
    settings
}

fn upsert_tab(tabs: &mut Vec<EditorBuffer>, buffer: EditorBuffer) {
    if let Some(tab) = tabs.iter_mut().find(|tab| tab.path == buffer.path) {
        *tab = buffer;
    } else {
        tabs.push(buffer);
    }
}

#[derive(Clone, Copy)]
enum FsEntryKind {
    File,
    Directory,
}

fn absolute_project_path(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<Utf8PathBuf> {
    if relative_path.is_absolute() {
        return None;
    }
    let mut normalized = Utf8PathBuf::new();
    for component in relative_path.components() {
        match component {
            camino::Utf8Component::Normal(part) => normalized.push(part),
            camino::Utf8Component::CurDir => {}
            camino::Utf8Component::ParentDir
            | camino::Utf8Component::RootDir
            | camino::Utf8Component::Prefix(_) => return None,
        }
    }
    Some(session.source.source_root.join(normalized))
}

fn valid_child_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
}

fn path_matches_or_is_child(candidate: &str, parent: &str) -> bool {
    candidate == parent
        || candidate
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with('\\'))
}
