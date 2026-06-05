use std::{
    error::Error,
    fmt::{self, Display},
    path::PathBuf,
};

use camino::Utf8PathBuf;
use dawn_language::{analysis::ProjectOverlay, document::SequenceDocument};

use crate::{
    active_document, analysis, audio, document_editing,
    editor::{self, EditorBufferSaveRequest},
    fixture_edit_planning, layout_edit_planning, output, preferences, preview, project, render,
    sequence_edit_planning,
    tasks::{BackendTask, BackendTaskOutput},
    types::{
        ActiveDocumentView, ActiveGuiDocument, ActiveGuiDocumentBlocked, ActiveGuiDocumentCacheKey,
        ActiveGuiDocumentRequest, AnalysisTaskOutput, EditorViewMode, ExportFseqTaskOutput,
        FixtureGuiEdit, FseqExportOptions, LayoutGuiEdit, RenderEffectPreviewRequestEffect,
        RenderEffectPreviewTaskOutput, RenderFrameTaskOutput, SequenceClipboard,
        SequenceEffectPreviewResultBatch, SequenceGuiEdit, SequenceSelectionEdit,
        SequenceSelectionEditResult,
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
    sequence_clipboard: Option<SequenceClipboard>,
    active_gui_document_cache: Option<ActiveGuiDocumentCacheEntry>,
}

impl AppBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn view(&self) -> AppView {
        let _ = (&self.preview, &self.audio, &self.output);

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
            BackendTaskOutput::RenderFrame(output) => self.accept_render_frame_output(output),
            BackendTaskOutput::RenderEffectPreviews(output) => {
                self.accept_render_effect_previews_output(output)
            }
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
        }
        Ok(self.idle_update())
    }

    pub fn render_sequence_frame(
        &mut self,
        document: SequenceDocument,
        position_seconds: f64,
        generation: u64,
    ) -> BackendResult<AppUpdate> {
        let analysis = render::require_analysis(self.analysis.snapshot())?;
        Ok(AppUpdate {
            view: self.view(),
            tasks: vec![BackendTask::RenderFrame(self.renderer.request_frame(
                analysis,
                document,
                position_seconds,
                generation,
            ))],
        })
    }

    pub fn render_effect_previews(
        &mut self,
        document: SequenceDocument,
        effects: Vec<RenderEffectPreviewRequestEffect>,
    ) -> BackendResult<AppUpdate> {
        let analysis = render::require_analysis(self.analysis.snapshot())?;
        Ok(AppUpdate {
            view: self.view(),
            tasks: vec![BackendTask::RenderEffectPreviews(
                self.renderer.request_effect_previews(
                    Utf8PathBuf::new(),
                    String::new(),
                    0,
                    analysis,
                    document,
                    effects,
                ),
            )],
        })
    }

    pub fn request_sequence_effect_previews(
        &mut self,
        path: Utf8PathBuf,
        object_key: String,
        request_id: u32,
        effects: Vec<RenderEffectPreviewRequestEffect>,
    ) -> BackendResult<AppUpdate> {
        let analysis = render::require_analysis(self.analysis.snapshot())?;
        let active = self.require_active_sequence_document()?;
        active_document::validate_sequence_preview_request(
            &active.path,
            &active.object_key,
            &active.document,
            &path,
            &object_key,
        )?;
        Ok(AppUpdate {
            view: self.view(),
            tasks: vec![BackendTask::RenderEffectPreviews(
                self.renderer.request_effect_previews(
                    path,
                    object_key,
                    request_id,
                    analysis,
                    active.document,
                    effects,
                ),
            )],
        })
    }

    pub fn take_sequence_effect_preview_results(
        &mut self,
        path: Utf8PathBuf,
        object_key: String,
    ) -> BackendResult<SequenceEffectPreviewResultBatch> {
        let active = self.require_active_sequence_document()?;
        active_document::validate_sequence_preview_request(
            &active.path,
            &active.object_key,
            &active.document,
            &path,
            &object_key,
        )?;
        Ok(self
            .renderer
            .take_effect_preview_results(&path, &object_key)
            .unwrap_or(SequenceEffectPreviewResultBatch {
                request_id: 0,
                results: Vec::new(),
            }))
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
            tasks: vec![BackendTask::ExportFseq(self.renderer.request_export_fseq(
                analysis,
                document,
                output_path,
                options,
            ))],
        })
    }

    pub fn accept_render_frame_output(
        &mut self,
        output: RenderFrameTaskOutput,
    ) -> BackendResult<AppUpdate> {
        self.renderer.accept_frame(output);
        Ok(self.idle_update())
    }

    pub fn accept_render_effect_previews_output(
        &mut self,
        output: RenderEffectPreviewTaskOutput,
    ) -> BackendResult<AppUpdate> {
        self.renderer.accept_effect_previews(output);
        Ok(self.idle_update())
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

    pub fn apply_sequence_gui_edit(&mut self, edit: SequenceGuiEdit) -> BackendResult<AppUpdate> {
        let edit = sequence_edit_planning::sequence_document_edit_from_gui(edit);
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
        self.replace_active_text_and_save(serialized_content)
    }

    pub fn apply_sequence_selection_edit(
        &mut self,
        edit: SequenceSelectionEdit,
    ) -> BackendResult<(AppUpdate, SequenceSelectionEditResult)> {
        let active = self.require_active_sequence_document()?;
        let sequence =
            sequence_edit_planning::parse_authored_sequence(&active.text, &active.object_key)
                .map_err(invalid_input_error)?;
        let planned = sequence_edit_planning::plan_sequence_selection_edit(
            edit,
            &mut self.sequence_clipboard,
            &sequence,
            &active.document,
        )
        .map_err(invalid_input_error)?;

        let update = if let Some(edit) = planned.document_edit {
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
            self.replace_active_text_and_save(serialized_content)?
        } else {
            self.idle_update()
        };

        Ok((update, planned.result))
    }

    pub fn apply_layout_gui_edit(&mut self, edit: LayoutGuiEdit) -> BackendResult<AppUpdate> {
        let active = self.require_active_layout_document()?;
        let mut document = active.document;
        layout_edit_planning::apply_layout_gui_edit(&mut document, edit)
            .map_err(invalid_input_error)?;
        let fs = self.project.workspace_fs()?;
        let overlays = self.editor.dirty_overlays();
        let outcome = document_editing::apply_layout_document_edit(
            &fs,
            active.path,
            &active.object_key,
            document,
            active.text,
            overlays,
        )?;
        self.replace_active_text_and_save(outcome.serialized_content)
    }

    pub fn apply_fixture_gui_edit(&mut self, edit: FixtureGuiEdit) -> BackendResult<AppUpdate> {
        let active = self.require_active_fixture_document()?;
        let mut document = active.document;
        fixture_edit_planning::apply_fixture_gui_edit(&mut document, edit)
            .map_err(invalid_input_error)?;
        let fs = self.project.workspace_fs()?;
        let overlays = self.editor.dirty_overlays();
        let outcome = document_editing::apply_fixture_document_edit(
            &fs,
            active.path,
            document,
            active.text,
            overlays,
        )?;
        self.replace_active_text_and_save(outcome.serialized_content)
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
        Ok(BackendTask::AnalyzeProject(self.analysis.request(
            project_root,
            project_file,
            overlays,
            active_gui_document,
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
