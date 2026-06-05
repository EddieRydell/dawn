use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{BackendError, BackendResult};

const DEFAULT_PROJECT_FILE: &str = "project.dawn";

#[derive(Debug, Default)]
pub(crate) struct Project {
    active: Option<OpenProject>,
}

#[derive(Debug)]
pub(crate) struct OpenProject {
    root: PathBuf,
    project_file: PathBuf,
}

impl Project {
    pub(crate) fn open(&mut self, path: PathBuf) -> BackendResult<()> {
        if path.as_os_str().is_empty() {
            return Err(BackendError::new("project path cannot be empty"));
        }

        let metadata = fs::metadata(&path).map_err(|error| {
            BackendError::new(format!(
                "failed to inspect project path '{}': {error}",
                path.display()
            ))
        })?;

        let (root, project_file) = if metadata.is_dir() {
            let root = fs::canonicalize(&path).map_err(|error| {
                BackendError::new(format!(
                    "failed to canonicalize project root '{}': {error}",
                    path.display()
                ))
            })?;
            let project_file = PathBuf::from(DEFAULT_PROJECT_FILE);
            let project_file_path = root.join(&project_file);

            if !project_file_path.is_file() {
                return Err(BackendError::new(format!(
                    "project root '{}' does not contain {DEFAULT_PROJECT_FILE}",
                    root.display()
                )));
            }

            (root, project_file)
        } else if metadata.is_file() {
            validate_project_file_path(&path)?;

            let project_file_path = fs::canonicalize(&path).map_err(|error| {
                BackendError::new(format!(
                    "failed to canonicalize project file '{}': {error}",
                    path.display()
                ))
            })?;
            let root = project_file_path
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| {
                    BackendError::new(format!(
                        "project file '{}' has no parent directory",
                        project_file_path.display()
                    ))
                })?;
            let project_file = project_file_path
                .file_name()
                .map(PathBuf::from)
                .ok_or_else(|| {
                    BackendError::new(format!(
                        "project file '{}' has no file name",
                        project_file_path.display()
                    ))
                })?;

            (root, project_file)
        } else {
            return Err(BackendError::new(format!(
                "project path '{}' is not a file or directory",
                path.display()
            )));
        };

        self.active = Some(OpenProject { root, project_file });

        Ok(())
    }

    pub(crate) fn root(&self) -> BackendResult<&Path> {
        Ok(&self.require_open()?.root)
    }

    pub(crate) fn project_file(&self) -> BackendResult<&Path> {
        Ok(&self.require_open()?.project_file)
    }

    fn require_open(&self) -> BackendResult<&OpenProject> {
        self.active
            .as_ref()
            .ok_or_else(|| BackendError::new("project is not open"))
    }
}

fn validate_project_file_path(path: &Path) -> BackendResult<()> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("dawn") {
        return Err(BackendError::new(format!(
            "project file '{}' must use the .dawn extension",
            path.display()
        )));
    }

    Ok(())
}
