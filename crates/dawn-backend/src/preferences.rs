use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use camino::Utf8PathBuf;

use crate::BackendResult;

#[derive(Debug, Default)]
pub(crate) struct Preferences {
    user: UserPreferences,
    project_sessions: ProjectSessionPreferencesStore,
}

impl Preferences {
    pub(crate) fn session_for_project(
        &self,
        project_root: &Path,
    ) -> BackendResult<ProjectSessionPreferences> {
        Ok(self.project_sessions.get(project_root))
    }

    pub(crate) fn remember_last_project(&mut self, project_root: &Path) -> BackendResult<()> {
        self.user.last_project_root = Some(project_root.to_path_buf());
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct UserPreferences {
    last_project_root: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub(crate) struct ProjectSessionPreferencesStore {
    sessions: HashMap<PathBuf, ProjectSessionPreferences>,
}

impl ProjectSessionPreferencesStore {
    fn get(&self, project_root: &Path) -> ProjectSessionPreferences {
        self.sessions.get(project_root).cloned().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProjectSessionPreferences {
    pub(crate) open_files: Vec<Utf8PathBuf>,
    pub(crate) active_file: Option<Utf8PathBuf>,
}
