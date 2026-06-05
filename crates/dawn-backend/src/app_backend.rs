use std::{
    error::Error,
    fmt::{self, Display},
    path::PathBuf,
};

use crate::{
    analysis, audio, editor, filesystem, jobs::BackendJob, output, preferences, preview, project,
    render, view::AppView,
};

pub type BackendResult<T> = Result<T, BackendError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError {
    message: String,
}

impl BackendError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
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
    filesystem: filesystem::Filesystem,
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
            &self.project,
            &self.editor,
            &self.analysis,
            &self.filesystem,
            &self.preview,
            &self.renderer,
            &self.audio,
            &self.output,
            &self.preferences,
        );

        AppView::default()
    }

    pub fn open_project(&mut self, path: PathBuf) -> BackendResult<BackendUpdate> {
        self.project.open(path)?;

        let project_session_preferences = self.preferences.project_session(self.project.root()?)?;
        self.editor.restore_for_project(&self.project, project_session_preferences)?;
        self.preferences.remember_last_project(self.project.root()?)?;

        Ok(BackendUpdate {
            view: self.view(),
            jobs: vec![BackendJob::AnalyzeProject {
                project_root: self.project.root()?.to_path_buf(),
                project_file: self.project.project_file()?.to_path_buf(),
            }],
        })
    }
}
