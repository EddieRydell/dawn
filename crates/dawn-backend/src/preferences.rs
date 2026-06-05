use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{types::ProjectSessionPreferences, BackendError, BackendErrorKind, BackendResult};

const CONFIG_FILE_NAME: &str = "workbench.json";

#[derive(Debug, Default)]
pub(crate) struct Preferences {
    user: UserPreferences,
    project_sessions: ProjectSessionPreferencesStore,
    loaded: bool,
}

impl Preferences {
    pub(crate) fn session_for_project(
        &mut self,
        project_root: &Path,
    ) -> BackendResult<ProjectSessionPreferences> {
        self.ensure_loaded()?;
        Ok(self.project_sessions.get(project_root))
    }

    pub(crate) fn remember_last_project(&mut self, project_root: &Path) -> BackendResult<()> {
        self.ensure_loaded()?;
        self.user.last_project_root = Some(project_root.to_path_buf());
        self.save()
    }

    pub(crate) fn set_session_for_project(
        &mut self,
        project_root: &Path,
        session: ProjectSessionPreferences,
    ) -> BackendResult<()> {
        self.ensure_loaded()?;
        self.project_sessions
            .sessions
            .insert(project_root.to_path_buf(), session);
        self.save()
    }

    fn ensure_loaded(&mut self) -> BackendResult<()> {
        if self.loaded {
            return Ok(());
        }
        let path = config_path();
        if !path.exists() {
            self.loaded = true;
            return Ok(());
        }
        let content = fs::read_to_string(&path).map_err(|error| {
            BackendError::new(
                BackendErrorKind::Preferences,
                format!("failed to read preferences '{}': {error}", path.display()),
            )
        })?;
        let persisted =
            serde_json::from_str::<PersistedPreferences>(&content).map_err(|error| {
                BackendError::new(
                    BackendErrorKind::Preferences,
                    format!("failed to parse preferences '{}': {error}", path.display()),
                )
            })?;
        self.user = persisted.user;
        self.project_sessions = persisted.project_sessions;
        self.loaded = true;
        Ok(())
    }

    fn save(&self) -> BackendResult<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                BackendError::new(
                    BackendErrorKind::Preferences,
                    format!(
                        "failed to create preferences directory '{}': {error}",
                        parent.display()
                    ),
                )
            })?;
        }
        let content = serde_json::to_string_pretty(&PersistedPreferences {
            user: self.user.clone(),
            project_sessions: self.project_sessions.clone(),
        })
        .map_err(|error| {
            BackendError::new(
                BackendErrorKind::Preferences,
                format!("failed to serialize preferences: {error}"),
            )
        })?;
        fs::write(&path, content).map_err(|error| {
            BackendError::new(
                BackendErrorKind::Preferences,
                format!("failed to write preferences '{}': {error}", path.display()),
            )
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedPreferences {
    user: UserPreferences,
    project_sessions: ProjectSessionPreferencesStore,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserPreferences {
    last_project_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSessionPreferencesStore {
    sessions: HashMap<PathBuf, ProjectSessionPreferences>,
}

impl ProjectSessionPreferencesStore {
    fn get(&self, project_root: &Path) -> ProjectSessionPreferences {
        self.sessions.get(project_root).cloned().unwrap_or_default()
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_default()
        .join("dawn")
        .join(CONFIG_FILE_NAME)
}
