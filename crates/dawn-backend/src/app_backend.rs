use std::{
    error::Error,
    fmt::{self, Display},
    path::PathBuf,
};

use camino::Utf8PathBuf;
use dawn_language::document::SequenceDocument;

use crate::{
    analysis, audio,
    editor::{self, EditorBufferSaveRequest},
    output, preferences, preview, project, render,
    tasks::{BackendTask, BackendTaskOutput},
    types::{
        AnalysisTaskOutput, EditorViewMode, ExportFseqTaskOutput, FseqExportOptions,
        RenderEffectPreviewRequestEffect, RenderEffectPreviewTaskOutput, RenderFrameTaskOutput,
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
            analysis: self.analysis.snapshot(),
            editor: self.editor.snapshot(),
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

    pub fn complete_task(&mut self, output: BackendTaskOutput) -> BackendResult<AppUpdate> {
        match output {
            BackendTaskOutput::AnalyzeProject(output) => self.accept_analysis_output(output),
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
        self.analysis.accept(output);
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
                self.renderer
                    .request_effect_previews(analysis, document, effects),
            )],
        })
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
        Ok(self.idle_update())
    }

    pub fn undo_active_edit(&mut self) -> BackendResult<AppUpdate> {
        self.editor.undo_active_edit()?;
        Ok(self.idle_update())
    }

    pub fn redo_active_edit(&mut self) -> BackendResult<AppUpdate> {
        self.editor.redo_active_edit()?;
        Ok(self.idle_update())
    }

    fn idle_update(&self) -> AppUpdate {
        AppUpdate {
            view: self.view(),
            tasks: Vec::new(),
        }
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
        Ok(BackendTask::AnalyzeProject(
            self.analysis.request(project_root, project_file),
        ))
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
