use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use camino::Utf8PathBuf;

use crate::{types::FileVersion, BackendError, BackendErrorKind, BackendResult};

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
            return Err(BackendError::new(
                BackendErrorKind::InvalidInput,
                "project path cannot be empty",
            ));
        }

        let metadata = fs::metadata(&path).map_err(|error| {
            BackendError::new(
                BackendErrorKind::Io,
                format!(
                    "failed to inspect project path '{}': {error}",
                    path.display()
                ),
            )
        })?;

        let (root, project_file) = if metadata.is_dir() {
            let root = fs::canonicalize(&path).map_err(|error| {
                BackendError::new(
                    BackendErrorKind::Io,
                    format!(
                        "failed to canonicalize project root '{}': {error}",
                        path.display()
                    ),
                )
            })?;
            let project_file = PathBuf::from(DEFAULT_PROJECT_FILE);
            let project_file_path = root.join(&project_file);

            if !project_file_path.is_file() {
                return Err(BackendError::new(
                    BackendErrorKind::NotFound,
                    format!(
                        "project root '{}' does not contain {DEFAULT_PROJECT_FILE}",
                        root.display()
                    ),
                ));
            }

            (root, project_file)
        } else if metadata.is_file() {
            validate_project_file_path(&path)?;

            let project_file_path = fs::canonicalize(&path).map_err(|error| {
                BackendError::new(
                    BackendErrorKind::Io,
                    format!(
                        "failed to canonicalize project file '{}': {error}",
                        path.display()
                    ),
                )
            })?;
            let root = project_file_path
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| {
                    BackendError::new(
                        BackendErrorKind::InvalidInput,
                        format!(
                            "project file '{}' has no parent directory",
                            project_file_path.display()
                        ),
                    )
                })?;
            let project_file = project_file_path
                .file_name()
                .map(PathBuf::from)
                .ok_or_else(|| {
                    BackendError::new(
                        BackendErrorKind::InvalidInput,
                        format!(
                            "project file '{}' has no file name",
                            project_file_path.display()
                        ),
                    )
                })?;

            (root, project_file)
        } else {
            return Err(BackendError::new(
                BackendErrorKind::InvalidInput,
                format!(
                    "project path '{}' is not a file or directory",
                    path.display()
                ),
            ));
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

    pub(crate) fn file_metadata(
        &self,
        path: &Utf8PathBuf,
    ) -> BackendResult<Option<ProjectFileMetadata>> {
        let resolved = self.resolve_project_file(path)?;
        let metadata = match fs::metadata(&resolved) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(BackendError::new(
                    BackendErrorKind::Io,
                    format!("failed to inspect file '{}': {error}", path),
                ))
            }
        };
        if !metadata.is_file() {
            return Ok(None);
        }
        Ok(Some(ProjectFileMetadata {
            len: metadata.len(),
        }))
    }

    pub(crate) fn read_file_snapshot(
        &self,
        path: &Utf8PathBuf,
    ) -> BackendResult<ProjectFileSnapshot> {
        let resolved = self.resolve_project_file(path)?;
        let text = fs::read_to_string(&resolved).map_err(|error| {
            BackendError::new(
                BackendErrorKind::Io,
                format!("failed to read file '{}': {error}", path),
            )
        })?;
        let metadata = fs::metadata(&resolved).map_err(|error| {
            BackendError::new(
                BackendErrorKind::Io,
                format!("failed to inspect file '{}': {error}", path),
            )
        })?;
        if !metadata.is_file() {
            return Err(BackendError::new(
                BackendErrorKind::NotFound,
                format!("file not found: {path}"),
            ));
        }
        let modified_millis = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis());
        Ok(ProjectFileSnapshot {
            version: FileVersion {
                len: metadata.len(),
                modified_millis,
                content_hash: content_hash(&text),
            },
            text,
        })
    }

    fn require_open(&self) -> BackendResult<&OpenProject> {
        self.active
            .as_ref()
            .ok_or_else(|| BackendError::new(BackendErrorKind::NoProject, "project is not open"))
    }

    fn resolve_project_file(&self, path: &Utf8PathBuf) -> BackendResult<PathBuf> {
        if path.is_absolute() {
            return Err(BackendError::new(
                BackendErrorKind::InvalidInput,
                format!("project file path must be relative: {path}"),
            ));
        }
        if path
            .as_std_path()
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
        {
            return Err(BackendError::new(
                BackendErrorKind::InvalidInput,
                format!("project file path escapes the project root: {path}"),
            ));
        }
        let root = &self.require_open()?.root;
        let resolved = root.join(path.as_std_path());
        if !resolved.starts_with(root) {
            return Err(BackendError::new(
                BackendErrorKind::InvalidInput,
                format!("project file path escapes the project root: {path}"),
            ));
        }
        Ok(resolved)
    }
}

fn validate_project_file_path(path: &Path) -> BackendResult<()> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("dawn") {
        return Err(BackendError::new(
            BackendErrorKind::InvalidInput,
            format!(
                "project file '{}' must use the .dawn extension",
                path.display()
            ),
        ));
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectFileMetadata {
    pub(crate) len: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectFileSnapshot {
    pub(crate) text: String,
    pub(crate) version: FileVersion,
}

fn content_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
