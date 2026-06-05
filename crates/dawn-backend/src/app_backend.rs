use std::{
    error::Error,
    fmt::{self, Display},
    path::PathBuf,
};

use camino::Utf8PathBuf;

use crate::{
    analysis, audio, editor, jobs::BackendJob, output, preferences, preview, project,
    render, types::EditorViewMode, view::AppView,
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
pub struct BackendUpdate {
    pub view: AppView,
    pub jobs: Vec<BackendJob>,
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
        let _ = (
            &self.analysis,
            &self.preview,
            &self.renderer,
            &self.audio,
            &self.output,
        );

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
                .map(|path| path.to_string_lossy().replace('\\', "/")),
            editor: self.editor.snapshot(),
        }
    }

    pub fn open_project(&mut self, path: PathBuf) -> BackendResult<BackendUpdate> {
        self.project.open(path)?;

        let project_session_preferences =
            self.preferences.session_for_project(self.project.root()?)?;
        self.editor
            .restore_for_project(&self.project, project_session_preferences)?;
        self.preferences
            .set_session_for_project(self.project.root()?, self.editor.session_preferences())?;
        self.preferences
            .remember_last_project(self.project.root()?)?;

        Ok(BackendUpdate {
            view: self.view(),
            jobs: vec![BackendJob::AnalyzeProject {
                project_root: self.project.root()?.to_path_buf(),
                project_file: self.project.project_file()?.to_path_buf(),
            }],
        })
    }

    pub fn open_file(&mut self, path: Utf8PathBuf) -> BackendResult<BackendUpdate> {
        self.editor.open_file(&self.project, path)?;
        self.persist_active_session()?;
        Ok(self.idle_update())
    }

    pub fn close_file(&mut self, path: Utf8PathBuf) -> BackendResult<BackendUpdate> {
        self.editor.close_file(path)?;
        self.persist_active_session()?;
        Ok(self.idle_update())
    }

    pub fn set_active_file(&mut self, path: Utf8PathBuf) -> BackendResult<BackendUpdate> {
        self.editor.set_active_file(&self.project, path)?;
        self.persist_active_session()?;
        Ok(self.idle_update())
    }

    pub fn update_active_text(&mut self, text: String) -> BackendResult<BackendUpdate> {
        self.editor.update_active_text(text)?;
        Ok(self.idle_update())
    }

    pub fn set_active_view_mode(
        &mut self,
        view_mode: EditorViewMode,
    ) -> BackendResult<BackendUpdate> {
        self.editor.set_active_view_mode(view_mode)?;
        self.persist_active_session()?;
        Ok(self.idle_update())
    }

    pub fn undo_active_edit(&mut self) -> BackendResult<BackendUpdate> {
        self.editor.undo_active_edit()?;
        Ok(self.idle_update())
    }

    pub fn redo_active_edit(&mut self) -> BackendResult<BackendUpdate> {
        self.editor.redo_active_edit()?;
        Ok(self.idle_update())
    }

    fn idle_update(&self) -> BackendUpdate {
        BackendUpdate {
            view: self.view(),
            jobs: Vec::new(),
        }
    }

    fn persist_active_session(&mut self) -> BackendResult<()> {
        self.preferences
            .set_session_for_project(self.project.root()?, self.editor.session_preferences())
    }
}
