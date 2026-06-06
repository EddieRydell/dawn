use std::{
    error::Error,
    fmt::{self, Display},
    path::PathBuf,
};

// Transaction-script facade: orchestrate backend modules, do not absorb document algorithms.
use camino::Utf8PathBuf;
use dawn_language::{
    analysis::ProjectOverlay,
    document::{DocumentEdit, SequenceDocument, SequenceDocumentEdit},
};

use crate::{
    active_document, analysis, audio, document_editing,
    editor::{self, EditorBufferSaveRequest},
    output, preferences, preview, preview_render, project, render,
    tasks::{BackendTask, BackendTaskOutput},
    types::{
        ActiveDocumentView, ActiveGuiDocument, ActiveGuiDocumentBlocked, ActiveGuiDocumentCacheKey,
        ActiveGuiDocumentRequest, AnalysisTaskOutput, EditorViewMode, EffectPreviewRequest,
        ExportFseqTaskOutput, FseqExportOptions, PreviewAudioClock, PreviewHostState,
        PreviewTickOutput, RenderEffectPreviewRequestEffect, SequenceAudioDialog,
    },
    view::AppView,
};

pub type BackendResult<T> = Result<T, BackendError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError {
    kind: BackendErrorKind,
    message: String,
}

impl BackendError {
    pub(crate) fn new(kind: BackendErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> BackendErrorKind {
        self.kind
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendErrorKind {
    NoProject,
    NotFound,
    Conflict,
    InvalidInput,
    Io,
    Preferences,
}

impl Display for BackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for BackendError {}

#[derive(Debug, Clone)]
pub struct AppUpdate {
    pub view: AppView,
    pub tasks: Vec<BackendTask>,
}

#[derive(Debug, Default)]
pub struct AppBackend {
    project: project::Project,
    editor: editor::Editor,
    analysis: analysis::Analysis,
    preview: preview::Preview,
    renderer: render::Renderer,
    audio: audio::Audio,
    output: output::Output,
    preferences: preferences::Preferences,
    active_gui_document_cache: Option<ActiveGuiDocumentCacheEntry>,
}

impl AppBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn view(&self) -> AppView {
        let _ = (&self.audio, &self.output);

        AppView {
            project_root: self
                .project
                .root()
                .ok()
                .map(|root| root.to_string_lossy().replace('\\', "/")),
            project_file: self
                .project
                .project_file()
                .ok()
                .map(|path| path.as_str().replace('\\', "/")),
            project_entries: self
                .project
                .project_entries()
                .map(|entries| entries.to_vec())
                .unwrap_or_default(),
            analysis: self.analysis.snapshot(),
            editor: self.editor.snapshot(),
            active_document: self.active_document_view(),
            render: self.renderer.snapshot(),
            preview: Some(self.preview.snapshot()),
            effect_preview_enabled: self.preview.snapshot().effect_preview_active,
        }
    }

    pub fn open_project(&mut self, path: PathBuf) -> BackendResult<AppUpdate> {
        self.project.open(path)?;
        self.analysis = analysis::Analysis::default();

        let project_session_preferences =
            self.preferences.session_for_project(self.project.root()?)?;
        self.editor.restore_for_project(
            project_session_preferences,
            |path| self.project.file_metadata(path),
            |path| self.project.read_file_snapshot(path),
        )?;
        self.preferences
            .set_session_for_project(self.project.root()?, self.editor.session_preferences())?;
        self.preferences
            .remember_last_project(self.project.root()?)?;

        Ok(AppUpdate {
            view: self.view(),
            tasks: vec![self.analyze_project_task()?],
        })
    }

    pub fn restore_last_project(&mut self) -> BackendResult<AppUpdate> {
        let Some(project_root) = self.preferences.last_project_root()? else {
            return Ok(self.idle_update());
        };
        self.open_project(project_root)
    }

    pub fn complete_task(&mut self, output: BackendTaskOutput) -> BackendResult<AppUpdate> {
        match output {
            BackendTaskOutput::AnalyzeProject(output) => self.accept_analysis_output(*output),
            BackendTaskOutput::ExportFseq(output) => self.accept_export_fseq_output(output),
        }
    }

    pub fn accept_analysis_output(
        &mut self,
        output: AnalysisTaskOutput,
    ) -> BackendResult<AppUpdate> {
        if self.analysis.accept(&output) {
            if let Some(active_gui_document) = output.active_gui_document {
                if self.current_active_gui_document_cache_key()?
                    == Some(active_gui_document.cache_key.clone())
                {
                    self.active_gui_document_cache = Some(ActiveGuiDocumentCacheEntry {
                        cache_key: active_gui_document.cache_key,
                        document: *active_gui_document.document,
                    });
                }
            }
            self.sync_preview_source_if_idle(preview::PreviewFrameDemand::Needed);
        }
        Ok(self.idle_update())
    }

    pub fn request_sequence_effect_previews(
        &self,
        path: Utf8PathBuf,
        object_key: String,
        request_id: u32,
        effects: Vec<RenderEffectPreviewRequestEffect>,
    ) -> BackendResult<EffectPreviewRequest> {
        let analysis = render::require_analysis(self.analysis.snapshot())?;
        let active = self.require_active_sequence_document()?;
        active_document::validate_sequence_preview_request(
            &active.path,
            &active.object_key,
            &active.document,
            &path,
            &object_key,
        )?;
        Ok(EffectPreviewRequest {
            path,
            object_key,
            request_id,
            analysis,
            document: active.document,
            effects,
        })
    }

    pub fn validate_sequence_effect_preview_key(
        &self,
        path: &Utf8PathBuf,
        object_key: &str,
    ) -> BackendResult<()> {
        let active = self.require_active_sequence_document()?;
        active_document::validate_sequence_preview_request(
            &active.path,
            &active.object_key,
            &active.document,
            path,
            object_key,
        )
    }

    pub fn export_fseq(
        &mut self,
        document: SequenceDocument,
        output_path: Utf8PathBuf,
        options: FseqExportOptions,
    ) -> BackendResult<AppUpdate> {
        let analysis = render::require_analysis(self.analysis.snapshot())?;
        Ok(AppUpdate {
            view: self.view(),
            tasks: vec![BackendTask::ExportFseq(Box::new(
                self.renderer
                    .request_export_fseq(analysis, document, output_path, options),
            ))],
        })
    }

    pub fn accept_export_fseq_output(
        &mut self,
        output: ExportFseqTaskOutput,
    ) -> BackendResult<AppUpdate> {
        self.renderer.accept_export(output);
        Ok(self.idle_update())
    }

    pub fn open_file(&mut self, path: Utf8PathBuf) -> BackendResult<AppUpdate> {
        self.editor
            .open_file(path, |path| self.project.read_file_snapshot(path))?;
        self.persist_active_session()?;
        if self
            .editor
            .active_loaded_buffer()
            .is_ok_and(|active| active.view_mode == EditorViewMode::Gui)
        {
            return self.analysis_update();
        }
        self.sync_preview_source_if_idle(preview::PreviewFrameDemand::Needed);
        Ok(self.idle_update())
    }

    pub fn close_file(&mut self, path: Utf8PathBuf) -> BackendResult<AppUpdate> {
        self.editor.close_file(path)?;
        self.persist_active_session()?;
        Ok(self.idle_update())
    }

    pub fn set_active_file(&mut self, path: Utf8PathBuf) -> BackendResult<AppUpdate> {
        self.editor
            .set_active_file(path, |path| self.project.read_file_snapshot(path))?;
        self.persist_active_session()?;
        if self
            .editor
            .active_loaded_buffer()
            .is_ok_and(|active| active.view_mode == EditorViewMode::Gui)
        {
            return self.analysis_update();
        }
        self.sync_preview_source_if_idle(preview::PreviewFrameDemand::Needed);
        Ok(self.idle_update())
    }

    pub fn update_active_text(&mut self, text: String) -> BackendResult<AppUpdate> {
        self.editor.update_active_text(text)?;
        Ok(self.idle_update())
    }

    pub fn save_active_file(&mut self) -> BackendResult<AppUpdate> {
        let request = self.editor.active_save_request()?;
        if !request.dirty {
            return Ok(self.idle_update());
        }
        self.ensure_save_request_current(&request)?;
        let version = self
            .project
            .write_text_file_with_version(&request.path, &request.text)?;
        self.editor.mark_active_buffer_saved(version)?;
        self.analysis_update()
    }

    pub fn reload_active_file_from_disk(&mut self) -> BackendResult<AppUpdate> {
        let request = self.editor.active_save_request()?;
        match self.project.read_file_snapshot(&request.path) {
            Ok(snapshot) => {
                self.editor.replace_active_buffer_from_snapshot(snapshot)?;
                Ok(self.idle_update())
            }
            Err(error) if error.kind() == BackendErrorKind::NotFound => {
                self.editor.close_active_buffer_force()?;
                self.persist_active_session()?;
                Ok(self.idle_update())
            }
            Err(error) => Err(error),
        }
    }

    pub fn keep_active_file(&mut self) -> BackendResult<AppUpdate> {
        let request = self.editor.active_save_request()?;
        let version = self
            .project
            .write_text_file_with_version(&request.path, &request.text)?;
        self.editor.mark_active_buffer_saved(version)?;
        self.analysis_update()
    }

    pub fn create_file(&mut self, parent: Utf8PathBuf, name: String) -> BackendResult<AppUpdate> {
        let path = self.project.create_file(&parent, &name)?;
        self.editor
            .open_file(path, |path| self.project.read_file_snapshot(path))?;
        self.persist_active_session()?;
        self.analysis_update()
    }

    pub fn create_directory(
        &mut self,
        parent: Utf8PathBuf,
        name: String,
    ) -> BackendResult<AppUpdate> {
        self.project.create_directory(&parent, &name)?;
        self.analysis_update()
    }

    pub fn rename_path(&mut self, path: Utf8PathBuf, new_name: String) -> BackendResult<AppUpdate> {
        self.save_affected_dirty_buffers(std::slice::from_ref(&path))?;
        let path_move = self.project.rename_path(&path, &new_name)?;
        self.editor
            .reconcile_moved_paths(std::slice::from_ref(&path_move));
        self.persist_active_session()?;
        self.analysis_update()
    }

    pub fn move_paths(
        &mut self,
        paths: Vec<Utf8PathBuf>,
        new_parent: Utf8PathBuf,
    ) -> BackendResult<AppUpdate> {
        self.save_affected_dirty_buffers(&paths)?;
        let path_moves = self.project.move_paths(&paths, &new_parent)?;
        self.editor.reconcile_moved_paths(&path_moves);
        self.persist_active_session()?;
        self.analysis_update()
    }

    pub fn delete_path(&mut self, path: Utf8PathBuf) -> BackendResult<AppUpdate> {
        self.project.delete_path(&path)?;
        self.editor.reconcile_deleted_path(&path);
        self.persist_active_session()?;
        self.analysis_update()
    }

    pub fn set_active_view_mode(&mut self, view_mode: EditorViewMode) -> BackendResult<AppUpdate> {
        self.editor.set_active_view_mode(view_mode)?;
        self.persist_active_session()?;
        if view_mode == EditorViewMode::Gui {
            return self.analysis_update();
        }
        Ok(self.idle_update())
    }

    pub fn apply_active_document_edit(&mut self, edit: DocumentEdit) -> BackendResult<AppUpdate> {
        match edit {
            DocumentEdit::Sequence(edit) => self.apply_active_sequence_document_edit(edit),
            DocumentEdit::Layout(edit) => {
                let active = self.require_active_layout_document()?;
                let fs = self.project.workspace_fs()?;
                let overlays = self.editor.dirty_overlays();
                let outcome = document_editing::apply_layout_document_edit(
                    &fs,
                    active.path,
                    &active.object_key,
                    active.document,
                    edit,
                    active.text,
                    overlays,
                )?;
                self.replace_active_text_and_save(outcome.serialized_content)
            }
            DocumentEdit::Fixture(edit) => {
                let active = self.require_active_fixture_document()?;
                let fs = self.project.workspace_fs()?;
                let overlays = self.editor.dirty_overlays();
                let outcome = document_editing::apply_fixture_document_edit(
                    &fs,
                    active.path,
                    active.document,
                    edit,
                    active.text,
                    overlays,
                )?;
                self.replace_active_text_and_save(outcome.serialized_content)
            }
        }
    }

    fn apply_active_sequence_document_edit(
        &mut self,
        edit: SequenceDocumentEdit,
    ) -> BackendResult<AppUpdate> {
        if matches!(&edit, SequenceDocumentEdit::SetAudio { .. }) {
            self.preview.stop();
        }
        let active = self.require_active_sequence_document()?;
        let fs = self.project.workspace_fs()?;
        let overlays = self.editor.dirty_overlays();
        let analysis = render::require_analysis(self.analysis.snapshot())?;
        let serialized_content = document_editing::apply_sequence_document_text_edit(
            &fs,
            active.path,
            &active.object_key,
            edit,
            active.text,
            overlays,
            &analysis,
        )?;
        let update = self.replace_active_text_and_save(serialized_content)?;
        self.sync_preview_source_if_idle(preview::PreviewFrameDemand::Needed);
        Ok(update)
    }

    pub fn prepare_preview_play(&mut self) -> BackendResult<(AppUpdate, preview::PreviewSnapshot)> {
        self.sync_preview_source(preview::PreviewFrameDemand::Needed);
        self.preview.clear_effect_preview();
        let snapshot = self.preview.snapshot();
        if snapshot.audio.as_ref().is_some_and(|audio| !audio.exists) {
            self.preview
                .set_timing_status("silent", preview::AudioPlaybackStatus::Missing);
            return Err(invalid_input_error("configured sequence audio is missing"));
        }
        Ok((self.idle_update(), self.preview.snapshot()))
    }

    pub fn preview_play_silent(&mut self) -> BackendResult<AppUpdate> {
        self.sync_preview_source(preview::PreviewFrameDemand::Needed);
        self.preview.play();
        self.preview
            .set_timing_status("silent", preview::AudioPlaybackStatus::None);
        Ok(self.idle_update())
    }

    pub fn preview_pause(&mut self) -> BackendResult<AppUpdate> {
        self.preview.pause();
        Ok(self.idle_update())
    }

    pub fn preview_stop(&mut self) -> BackendResult<AppUpdate> {
        self.preview.stop();
        Ok(self.idle_update())
    }

    pub fn preview_rewind_to_zero(&mut self) -> BackendResult<AppUpdate> {
        self.preview.go_to_sequence_beginning();
        Ok(self.idle_update())
    }

    pub fn preview_seek(&mut self, position_seconds: f64) -> BackendResult<AppUpdate> {
        validate_position_seconds(position_seconds)?;
        self.preview.seek(position_seconds);
        Ok(self.idle_update())
    }

    pub fn preview_apply_audio_clock(
        &mut self,
        clock: PreviewAudioClock,
    ) -> BackendResult<AppUpdate> {
        if clock.error.is_some() {
            self.preview.pause_at(clock.position_seconds);
            self.preview
                .set_timing_status("nativeAudio", preview::AudioPlaybackStatus::Error);
            return Ok(self.idle_update());
        }
        if clock.status == preview::AudioPlaybackStatus::Playing || clock.ended {
            self.preview
                .render_at_native_audio_clock(clock.position_seconds, clock.ended);
        } else if clock.status.is_loading() {
            self.preview.set_timing_status("nativeAudio", clock.status);
        } else {
            self.preview
                .seek_native_audio(clock.position_seconds, false);
            self.preview.set_timing_status("nativeAudio", clock.status);
        }
        Ok(self.idle_update())
    }

    pub fn set_effect_preview_enabled(&mut self, enabled: bool) -> BackendResult<AppUpdate> {
        if enabled {
            self.preview.pause();
        } else {
            self.preview.clear_effect_preview();
        }
        Ok(self.idle_update())
    }

    pub fn set_effect_preview_effects(&mut self, ids: Vec<u32>) -> BackendResult<AppUpdate> {
        self.preview.set_effect_preview_ids(ids);
        Ok(self.idle_update())
    }

    pub fn update_preview_clock(
        &mut self,
        audio_clock: Option<PreviewAudioClock>,
    ) -> BackendResult<PreviewTickOutput> {
        if let Some(clock) = audio_clock {
            let _ = self.preview_apply_audio_clock(clock)?;
        } else {
            self.preview.tick();
        }
        let snapshot = self.preview.snapshot();
        Ok(PreviewTickOutput {
            target_fps: self.preview.target_fps(),
            render_timing: self.preview.last_render_timing(),
            snapshot,
        })
    }

    pub fn begin_preview_frame_render(&mut self) -> Option<preview_render::PreviewFrameRenderTask> {
        self.preview.begin_frame_render(self.analysis.snapshot())
    }

    pub fn complete_preview_frame_render(
        &mut self,
        output: preview_render::PreviewFrameRenderOutput,
    ) -> bool {
        self.preview.complete_frame_render(output)
    }

    pub fn preview_snapshot(&self) -> preview::PreviewSnapshot {
        self.preview.snapshot()
    }

    pub fn preview_host_state(&self) -> PreviewHostState {
        let snapshot = self.preview.snapshot_ref();
        PreviewHostState {
            target_fps: self.preview.target_fps(),
            frame_generation: snapshot.frame.generation,
            is_playing: snapshot.is_playing,
            preview_updating: snapshot.preview_updating,
            effect_preview_active: snapshot.effect_preview_active,
            audio_playback_status: snapshot.audio_playback_status,
            has_valid_audio: snapshot.audio.as_ref().is_some_and(|audio| audio.exists),
        }
    }

    pub fn preview_frame(&self) -> &crate::types::RenderedFrame {
        &self.preview.snapshot_ref().frame
    }

    pub fn active_sequence_audio_dialog(&self) -> BackendResult<SequenceAudioDialog> {
        let active = self.require_active_sequence_document()?;
        let project_root = self.project.root()?.to_path_buf();
        let sequence_path = if active.path.is_absolute() {
            active.path
        } else {
            Utf8PathBuf::from_path_buf(project_root.join(active.path.as_std_path())).map_err(
                |path| {
                    BackendError::new(
                        BackendErrorKind::InvalidInput,
                        format!("sequence path '{}' is not valid UTF-8", path.display()),
                    )
                },
            )?
        };
        let audio_directory = project_root.join("audio");
        Ok(SequenceAudioDialog {
            project_root,
            sequence_path,
            audio_directory,
        })
    }

    pub fn set_active_sequence_audio(
        &mut self,
        selected_audio_path: PathBuf,
    ) -> BackendResult<AppUpdate> {
        let dialog = self.active_sequence_audio_dialog()?;
        let selected = dawn_language::path::utf8_path(selected_audio_path)
            .map_err(|error| BackendError::new(BackendErrorKind::InvalidInput, error))?;
        let import = dawn_language::path::serialized_import_path(&dialog.sequence_path, &selected);
        self.apply_active_document_edit(DocumentEdit::Sequence(SequenceDocumentEdit::SetAudio {
            import: Some(import),
        }))
    }

    pub fn clear_active_sequence_audio(&mut self) -> BackendResult<AppUpdate> {
        self.apply_active_document_edit(DocumentEdit::Sequence(SequenceDocumentEdit::SetAudio {
            import: None,
        }))
    }

    fn idle_update(&self) -> AppUpdate {
        AppUpdate {
            view: self.view(),
            tasks: Vec::new(),
        }
    }

    fn active_document_view(&self) -> ActiveDocumentView {
        let Ok(active) = self.editor.active_loaded_buffer() else {
            return ActiveDocumentView::default();
        };
        let Ok(fs) = self.project.workspace_fs() else {
            return ActiveDocumentView::default();
        };
        let overlays = self.editor.dirty_overlays();
        let descriptor = match active_document::inspect_active_document(
            &fs,
            active.path.clone(),
            overlays.clone(),
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                return ActiveDocumentView {
                    descriptor: None,
                    gui_document: (active.view_mode == EditorViewMode::Gui).then(|| {
                        ActiveGuiDocument::Blocked(active_document::blocked_gui_document(
                            &active.path,
                            error.to_string(),
                        ))
                    }),
                };
            }
        };

        let gui_document = if active.view_mode == EditorViewMode::Gui {
            match self.active_gui_document_cache_key(&active.path, &descriptor) {
                Ok(cache_key) => self
                    .active_gui_document_cache
                    .as_ref()
                    .filter(|cache| cache.cache_key == cache_key)
                    .map(|cache| cache.document.clone()),
                Err(blocked) => Some(ActiveGuiDocument::Blocked(blocked)),
            }
        } else {
            None
        };

        ActiveDocumentView {
            descriptor: Some(descriptor),
            gui_document,
        }
    }

    fn active_sequence_preview_source(
        &self,
    ) -> Option<(
        preview::SequenceKey,
        dawn_language::document::SequenceDocument,
    )> {
        let active = self.editor.active_loaded_buffer().ok()?;
        if active.view_mode != EditorViewMode::Gui {
            return None;
        }
        let ActiveGuiDocument::Sequence(document) =
            self.active_document_view().gui_document.or_else(|| {
                self.active_gui_document_cache
                    .as_ref()
                    .map(|cache| cache.document.clone())
            })?
        else {
            return None;
        };
        Some((
            preview::SequenceKey {
                path: active.path,
                object_key: document.object_key.clone(),
            },
            document,
        ))
    }

    fn sync_preview_source(&mut self, demand: preview::PreviewFrameDemand) {
        let source = self.active_sequence_preview_source();
        self.preview.sync_source(source, demand);
    }

    fn sync_preview_source_if_idle(&mut self, demand: preview::PreviewFrameDemand) {
        if self.preview.is_playing() {
            return;
        }
        self.sync_preview_source(demand);
    }

    fn require_active_sequence_document(&self) -> BackendResult<ActiveSequenceDocument> {
        let active = self.require_active_gui_document()?;
        match active.gui_document {
            ActiveGuiDocument::Sequence(document) => Ok(ActiveSequenceDocument {
                path: active.path,
                text: active.text,
                object_key: document.object_key.clone(),
                document,
            }),
            ActiveGuiDocument::Blocked(blocked) => Err(invalid_input_error(blocked.reason)),
            _ => Err(invalid_input_error("active GUI document is not a sequence")),
        }
    }

    fn require_active_layout_document(&self) -> BackendResult<ActiveLayoutDocument> {
        let active = self.require_active_gui_document()?;
        match active.gui_document {
            ActiveGuiDocument::Layout(document) => Ok(ActiveLayoutDocument {
                path: active.path,
                text: active.text,
                object_key: document.object_key.clone(),
                document,
            }),
            ActiveGuiDocument::Blocked(blocked) => Err(invalid_input_error(blocked.reason)),
            _ => Err(invalid_input_error("active GUI document is not a layout")),
        }
    }

    fn require_active_fixture_document(&self) -> BackendResult<ActiveFixtureDocument> {
        let active = self.require_active_gui_document()?;
        match active.gui_document {
            ActiveGuiDocument::Fixture(document) => Ok(ActiveFixtureDocument {
                path: active.path,
                text: active.text,
                document,
            }),
            ActiveGuiDocument::Blocked(blocked) => Err(invalid_input_error(blocked.reason)),
            _ => Err(invalid_input_error("active GUI document is not a fixture")),
        }
    }

    fn require_active_gui_document(&self) -> BackendResult<ActiveGuiDocumentContext> {
        let active = self.editor.active_loaded_buffer()?;
        if active.view_mode != EditorViewMode::Gui {
            return Err(invalid_input_error(
                "active editor buffer is not in GUI mode",
            ));
        }
        let view = self.active_document_view();
        let gui_document = view
            .gui_document
            .ok_or_else(|| invalid_input_error("active GUI document is unavailable"))?;
        Ok(ActiveGuiDocumentContext {
            path: active.path,
            text: active.text,
            gui_document,
        })
    }

    fn replace_active_text_and_save(&mut self, text: String) -> BackendResult<AppUpdate> {
        self.editor.replace_active_text(text)?;
        self.save_active_file()
    }

    fn analysis_update(&mut self) -> BackendResult<AppUpdate> {
        Ok(AppUpdate {
            view: self.view(),
            tasks: vec![self.analyze_project_task()?],
        })
    }

    fn persist_active_session(&mut self) -> BackendResult<()> {
        self.preferences
            .set_session_for_project(self.project.root()?, self.editor.session_preferences())
    }

    fn analyze_project_task(&mut self) -> BackendResult<BackendTask> {
        let project_root =
            Utf8PathBuf::from_path_buf(self.project.root()?.to_path_buf()).map_err(|path| {
                BackendError::new(
                    BackendErrorKind::InvalidInput,
                    format!("project root '{}' is not valid UTF-8", path.display()),
                )
            })?;
        let project_file = self.project.project_file()?.to_path_buf();
        let overlays = self.editor.dirty_overlays();
        let active_gui_document =
            self.active_gui_document_request(&project_root, overlays.clone())?;
        Ok(BackendTask::AnalyzeProject(Box::new(
            self.analysis
                .request(project_root, project_file, overlays, active_gui_document),
        )))
    }

    fn active_gui_document_request(
        &self,
        project_root: &Utf8PathBuf,
        overlays: Vec<ProjectOverlay>,
    ) -> BackendResult<Option<ActiveGuiDocumentRequest>> {
        let Ok(active) = self.editor.active_loaded_buffer() else {
            return Ok(None);
        };
        if active.view_mode != EditorViewMode::Gui {
            return Ok(None);
        }
        let fs = self.project.workspace_fs()?;
        let descriptor =
            match active_document::inspect_active_document(&fs, active.path.clone(), overlays) {
                Ok(descriptor) => descriptor,
                Err(_) => return Ok(None),
            };
        let cache_key = match active_document::active_gui_document_cache_key(
            project_root,
            &active.path,
            &descriptor,
        ) {
            Ok(cache_key) => cache_key,
            Err(_) => return Ok(None),
        };
        Ok(Some(ActiveGuiDocumentRequest {
            cache_key,
            descriptor,
        }))
    }

    fn active_gui_document_cache_key(
        &self,
        path: &Utf8PathBuf,
        descriptor: &dawn_language::document::DocumentDescriptor,
    ) -> Result<ActiveGuiDocumentCacheKey, ActiveGuiDocumentBlocked> {
        let project_root = self.project.root().map_err(|error| {
            active_document::blocked_gui_document(
                path,
                format!("could not load project root: {error}"),
            )
        })?;
        let project_root =
            Utf8PathBuf::from_path_buf(project_root.to_path_buf()).map_err(|invalid_path| {
                active_document::blocked_gui_document(
                    path,
                    format!(
                        "project root '{}' is not valid UTF-8",
                        invalid_path.display()
                    ),
                )
            })?;
        active_document::active_gui_document_cache_key(&project_root, path, descriptor)
    }

    fn current_active_gui_document_cache_key(
        &self,
    ) -> BackendResult<Option<ActiveGuiDocumentCacheKey>> {
        let Ok(active) = self.editor.active_loaded_buffer() else {
            return Ok(None);
        };
        if active.view_mode != EditorViewMode::Gui {
            return Ok(None);
        }
        let fs = self.project.workspace_fs()?;
        let descriptor = match active_document::inspect_active_document(
            &fs,
            active.path.clone(),
            self.editor.dirty_overlays(),
        ) {
            Ok(descriptor) => descriptor,
            Err(_) => return Ok(None),
        };
        Ok(self
            .active_gui_document_cache_key(&active.path, &descriptor)
            .ok())
    }

    fn save_affected_dirty_buffers(&mut self, paths: &[Utf8PathBuf]) -> BackendResult<()> {
        let requests = self.editor.affected_dirty_save_requests(paths);
        for request in &requests {
            self.ensure_save_request_current(request)?;
        }
        for request in requests {
            let version = self
                .project
                .write_text_file_with_version(&request.path, &request.text)?;
            self.editor.mark_buffer_saved(&request.path, version)?;
        }
        Ok(())
    }

    fn ensure_save_request_current(&self, request: &EditorBufferSaveRequest) -> BackendResult<()> {
        let snapshot = self
            .project
            .read_file_snapshot(&request.path)
            .map_err(|error| {
                if error.kind() == BackendErrorKind::NotFound {
                    BackendError::new(
                        BackendErrorKind::Conflict,
                        format!("file no longer exists on disk: {}", request.path),
                    )
                } else {
                    error
                }
            })?;
        if snapshot.version != request.saved_disk_version {
            return Err(BackendError::new(
                BackendErrorKind::Conflict,
                format!("file changed on disk: {}", request.path),
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ActiveGuiDocumentContext {
    path: Utf8PathBuf,
    text: String,
    gui_document: ActiveGuiDocument,
}

#[derive(Debug, Clone)]
struct ActiveGuiDocumentCacheEntry {
    cache_key: ActiveGuiDocumentCacheKey,
    document: ActiveGuiDocument,
}

#[derive(Debug)]
struct ActiveSequenceDocument {
    path: Utf8PathBuf,
    text: String,
    object_key: String,
    document: SequenceDocument,
}

#[derive(Debug)]
struct ActiveLayoutDocument {
    path: Utf8PathBuf,
    text: String,
    object_key: String,
    document: dawn_language::document::LayoutDocument,
}

#[derive(Debug)]
struct ActiveFixtureDocument {
    path: Utf8PathBuf,
    text: String,
    document: dawn_language::document::FixtureDocument,
}

fn invalid_input_error(message: impl Into<String>) -> BackendError {
    BackendError::new(BackendErrorKind::InvalidInput, message)
}

fn validate_position_seconds(position_seconds: f64) -> BackendResult<()> {
    if position_seconds.is_finite() && position_seconds >= 0.0 {
        Ok(())
    } else {
        Err(invalid_input_error(
            "preview seek seconds must be finite and non-negative",
        ))
    }
}
