use std::{
    collections::hash_map::DefaultHasher,
    collections::HashSet,
    fs,
    hash::{Hash, Hasher},
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::fs::{WorkspaceEntry as LanguageWorkspaceEntry, WorkspaceFs};

use crate::{
    types::{
        FileVersion, ProjectFileMetadata, ProjectFileSnapshot, ProjectPathMove, WorkspaceEntry,
        WorkspaceEntryKind,
    },
    BackendError, BackendErrorKind, BackendResult,
};

const DEFAULT_PROJECT_FILE: &str = "project.dawn";

#[derive(Debug, Default)]
pub(crate) struct Project {
    active: Option<OpenProject>,
}

#[derive(Debug)]
pub(crate) struct OpenProject {
    root: PathBuf,
    project_file: Utf8PathBuf,
    project_entries: Vec<WorkspaceEntry>,
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
            let project_file = Utf8PathBuf::from(DEFAULT_PROJECT_FILE);
            let project_file_path = root.join(project_file.as_std_path());

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
            let project_file_path_buf = project_file_path
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
            let project_file =
                Utf8PathBuf::from_path_buf(project_file_path_buf).map_err(|path| {
                    BackendError::new(
                        BackendErrorKind::InvalidInput,
                        format!("project file '{}' is not valid UTF-8", path.display()),
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

        let project_entries = list_project_entries(&root)?;
        self.active = Some(OpenProject {
            root,
            project_file,
            project_entries,
        });

        Ok(())
    }

    pub(crate) fn root(&self) -> BackendResult<&Path> {
        Ok(&self.require_open()?.root)
    }

    pub(crate) fn project_file(&self) -> BackendResult<&Utf8Path> {
        Ok(&self.require_open()?.project_file)
    }

    pub(crate) fn project_entries(&self) -> BackendResult<&[WorkspaceEntry]> {
        Ok(&self.require_open()?.project_entries)
    }

    pub(crate) fn workspace_fs(&self) -> BackendResult<WorkspaceFs> {
        WorkspaceFs::open(self.root()?).map_err(|error| {
            BackendError::new(
                BackendErrorKind::Io,
                format!(
                    "failed to open project root '{}' as workspace fs: {error}",
                    self.root()
                        .map(|root| root.display().to_string())
                        .unwrap_or_else(|_| "<closed>".to_string())
                ),
            )
        })
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
        let resolved = self.resolve_project_path(path)?;
        let text = fs::read_to_string(&resolved).map_err(|error| {
            let kind = if error.kind() == std::io::ErrorKind::NotFound {
                BackendErrorKind::NotFound
            } else {
                BackendErrorKind::Io
            };
            BackendError::new(kind, format!("failed to read file '{}': {error}", path))
        })?;
        let metadata = self.metadata_for_resolved_path(path, &resolved)?;
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

    pub(crate) fn write_text_file_with_version(
        &self,
        path: &Utf8PathBuf,
        content: &str,
    ) -> BackendResult<FileVersion> {
        let resolved = self.resolve_project_path(path)?;
        fs::write(&resolved, content).map_err(|error| {
            BackendError::new(
                BackendErrorKind::Io,
                format!("failed to write file '{}': {error}", path),
            )
        })?;
        self.file_version_for_content(path, content)?
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorKind::NotFound,
                    format!("written file not found: {path}"),
                )
            })
    }

    pub(crate) fn create_file(
        &mut self,
        parent: &Utf8PathBuf,
        name: &str,
    ) -> BackendResult<Utf8PathBuf> {
        let name = file_name_with_default_extension(name)?;
        validate_file_name(&name)?;
        self.require_directory_parent(parent)?;
        let path = parent.join(&name);
        let resolved = self.resolve_project_path(&path)?;
        if resolved.exists() {
            return Err(BackendError::new(
                BackendErrorKind::Conflict,
                format!("target path already exists: {path}"),
            ));
        }
        fs::File::create(&resolved).map_err(|error| {
            BackendError::new(
                BackendErrorKind::Io,
                format!("failed to create file '{}': {error}", path),
            )
        })?;
        self.refresh_project_entries()?;
        Ok(path)
    }

    pub(crate) fn create_directory(
        &mut self,
        parent: &Utf8PathBuf,
        name: &str,
    ) -> BackendResult<()> {
        validate_file_name(name)?;
        self.require_directory_parent(parent)?;
        let path = parent.join(name);
        let resolved = self.resolve_project_path(&path)?;
        if resolved.exists() {
            return Err(BackendError::new(
                BackendErrorKind::Conflict,
                format!("target path already exists: {path}"),
            ));
        }
        fs::create_dir(&resolved).map_err(|error| {
            BackendError::new(
                BackendErrorKind::Io,
                format!("failed to create directory '{}': {error}", path),
            )
        })?;
        self.refresh_project_entries()?;
        Ok(())
    }

    pub(crate) fn rename_path(
        &mut self,
        path: &Utf8PathBuf,
        new_name: &str,
    ) -> BackendResult<ProjectPathMove> {
        self.reject_project_root_path(path, "rename")?;
        validate_file_name(new_name)?;
        let resolved = self.resolve_project_path(path)?;
        if !resolved.exists() {
            return Err(BackendError::new(
                BackendErrorKind::NotFound,
                format!("path not found: {path}"),
            ));
        }
        let new_path = path
            .parent()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorKind::InvalidInput,
                    format!("path has no parent: {path}"),
                )
            })?
            .join(new_name);
        let new_resolved = self.resolve_project_path(&new_path)?;
        if new_resolved.exists() {
            return Err(BackendError::new(
                BackendErrorKind::Conflict,
                format!("target path already exists: {new_path}"),
            ));
        }
        fs::rename(&resolved, &new_resolved).map_err(|error| {
            BackendError::new(
                BackendErrorKind::Io,
                format!("failed to rename '{}' to '{}': {error}", path, new_path),
            )
        })?;
        self.refresh_project_entries()?;
        Ok(ProjectPathMove {
            old_path: path.clone(),
            new_path,
        })
    }

    pub(crate) fn move_paths(
        &mut self,
        paths: &[Utf8PathBuf],
        new_parent: &Utf8PathBuf,
    ) -> BackendResult<Vec<ProjectPathMove>> {
        let new_parent_resolved = self.resolve_project_path(new_parent)?;
        if !new_parent_resolved.is_dir() {
            return Err(BackendError::new(
                BackendErrorKind::InvalidInput,
                format!("move target is not a directory: {new_parent}"),
            ));
        }

        let mut selected_paths = Vec::new();
        let mut seen_sources = HashSet::new();
        for path in paths {
            self.reject_project_root_path(path, "move")?;
            if !seen_sources.insert(path.clone()) {
                return Err(BackendError::new(
                    BackendErrorKind::InvalidInput,
                    format!("duplicate source path: {path}"),
                ));
            }
            let resolved = self.resolve_project_path(path)?;
            if !resolved.exists() {
                return Err(BackendError::new(
                    BackendErrorKind::NotFound,
                    format!("source path not found: {path}"),
                ));
            }
            selected_paths.push((path.clone(), resolved));
        }
        reject_nested_selected_paths(&selected_paths)?;

        let mut planned_moves = Vec::new();
        let mut seen_destinations = HashSet::new();
        for (old_path, resolved) in selected_paths {
            let name = old_path.file_name().ok_or_else(|| {
                BackendError::new(
                    BackendErrorKind::InvalidInput,
                    format!("path has no file name: {old_path}"),
                )
            })?;
            let new_path = new_parent.join(name);
            if old_path == new_path {
                continue;
            }
            if resolved.is_dir() && new_path.starts_with(&old_path) {
                return Err(BackendError::new(
                    BackendErrorKind::InvalidInput,
                    format!("cannot move a directory into itself: {old_path}"),
                ));
            }
            if !seen_destinations.insert(new_path.clone()) {
                return Err(BackendError::new(
                    BackendErrorKind::InvalidInput,
                    format!("duplicate destination path: {new_path}"),
                ));
            }
            let new_resolved = self.resolve_project_path(&new_path)?;
            if new_resolved.exists() {
                return Err(BackendError::new(
                    BackendErrorKind::Conflict,
                    format!("target path already exists: {new_path}"),
                ));
            }
            planned_moves.push(ProjectPathMove { old_path, new_path });
        }

        let mut completed = Vec::new();
        for planned_move in &planned_moves {
            let old_resolved = self.resolve_project_path(&planned_move.old_path)?;
            let new_resolved = self.resolve_project_path(&planned_move.new_path)?;
            if let Err(error) = fs::rename(&old_resolved, &new_resolved) {
                let rollback_error = self.rollback_completed_moves(&completed);
                return Err(BackendError::new(
                    BackendErrorKind::Io,
                    match rollback_error {
                        Ok(()) => format!(
                            "failed to move '{}' to '{}': {error}",
                            planned_move.old_path, planned_move.new_path
                        ),
                        Err(rollback_error) => format!(
                            "failed to move '{}' to '{}': {error}; rollback failed: {rollback_error}",
                            planned_move.old_path, planned_move.new_path
                        ),
                    },
                ));
            }
            completed.push(planned_move.clone());
        }

        self.refresh_project_entries()?;
        Ok(planned_moves)
    }

    pub(crate) fn delete_path(&mut self, path: &Utf8PathBuf) -> BackendResult<()> {
        self.reject_project_root_path(path, "delete")?;
        let resolved = self.resolve_project_path(path)?;
        let metadata = self.metadata_for_resolved_path(path, &resolved)?;
        if metadata.is_dir() {
            fs::remove_dir_all(&resolved).map_err(|error| {
                BackendError::new(
                    BackendErrorKind::Io,
                    format!("failed to delete directory '{}': {error}", path),
                )
            })?;
        } else if metadata.is_file() {
            fs::remove_file(&resolved).map_err(|error| {
                BackendError::new(
                    BackendErrorKind::Io,
                    format!("failed to delete file '{}': {error}", path),
                )
            })?;
        } else {
            return Err(BackendError::new(
                BackendErrorKind::InvalidInput,
                format!("path is not a file or directory: {path}"),
            ));
        }
        self.refresh_project_entries()?;
        Ok(())
    }

    fn require_open(&self) -> BackendResult<&OpenProject> {
        self.active
            .as_ref()
            .ok_or_else(|| BackendError::new(BackendErrorKind::NoProject, "project is not open"))
    }

    fn require_open_mut(&mut self) -> BackendResult<&mut OpenProject> {
        self.active
            .as_mut()
            .ok_or_else(|| BackendError::new(BackendErrorKind::NoProject, "project is not open"))
    }

    fn refresh_project_entries(&mut self) -> BackendResult<()> {
        let project = self.require_open_mut()?;
        project.project_entries = list_project_entries(&project.root)?;
        Ok(())
    }

    fn resolve_project_file(&self, path: &Utf8PathBuf) -> BackendResult<PathBuf> {
        self.resolve_project_path(path)
    }

    fn resolve_project_path(&self, path: &Utf8PathBuf) -> BackendResult<PathBuf> {
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

    fn metadata_for_resolved_path(
        &self,
        path: &Utf8PathBuf,
        resolved: &Path,
    ) -> BackendResult<fs::Metadata> {
        fs::metadata(resolved).map_err(|error| {
            let kind = if error.kind() == std::io::ErrorKind::NotFound {
                BackendErrorKind::NotFound
            } else {
                BackendErrorKind::Io
            };
            BackendError::new(kind, format!("failed to inspect path '{}': {error}", path))
        })
    }

    fn file_version_for_content(
        &self,
        path: &Utf8PathBuf,
        content: &str,
    ) -> BackendResult<Option<FileVersion>> {
        let resolved = self.resolve_project_path(path)?;
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
        Ok(Some(file_version_from_metadata(&metadata, content)))
    }

    fn require_directory_parent(&self, parent: &Utf8PathBuf) -> BackendResult<()> {
        let resolved = self.resolve_project_path(parent)?;
        if !resolved.is_dir() {
            return Err(BackendError::new(
                BackendErrorKind::InvalidInput,
                format!("parent path is not a directory: {parent}"),
            ));
        }
        Ok(())
    }

    fn reject_project_root_path(&self, path: &Utf8PathBuf, operation: &str) -> BackendResult<()> {
        self.resolve_project_path(path)?;
        if path.as_str().is_empty() {
            return Err(BackendError::new(
                BackendErrorKind::InvalidInput,
                format!("cannot {operation} project root"),
            ));
        }
        Ok(())
    }

    fn rollback_completed_moves(&self, completed: &[ProjectPathMove]) -> BackendResult<()> {
        let mut errors = Vec::new();
        for completed_move in completed.iter().rev() {
            let old_resolved = self.resolve_project_path(&completed_move.old_path)?;
            let new_resolved = self.resolve_project_path(&completed_move.new_path)?;
            if let Err(error) = fs::rename(&new_resolved, &old_resolved) {
                errors.push(format!(
                    "{} -> {}: {}",
                    completed_move.new_path, completed_move.old_path, error
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(BackendError::new(BackendErrorKind::Io, errors.join("; ")))
        }
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

fn content_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn file_version_from_metadata(metadata: &fs::Metadata, content: &str) -> FileVersion {
    let modified_millis = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    FileVersion {
        len: metadata.len(),
        modified_millis,
        content_hash: content_hash(content),
    }
}

fn validate_file_name(name: &str) -> BackendResult<()> {
    if name.trim().is_empty() {
        return Err(BackendError::new(
            BackendErrorKind::InvalidInput,
            "name cannot be empty",
        ));
    }
    if name == "." || name == ".." {
        return Err(BackendError::new(
            BackendErrorKind::InvalidInput,
            "name cannot be . or ..",
        ));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(BackendError::new(
            BackendErrorKind::InvalidInput,
            "name cannot contain path separators",
        ));
    }
    Ok(())
}

fn file_name_with_default_extension(name: &str) -> BackendResult<String> {
    validate_file_name(name)?;
    let path = Path::new(name);
    if path.extension().is_none() {
        Ok(format!("{name}.dawn"))
    } else {
        Ok(name.to_string())
    }
}

fn reject_nested_selected_paths(paths: &[(Utf8PathBuf, PathBuf)]) -> BackendResult<()> {
    for (left_index, (left, _)) in paths.iter().enumerate() {
        for (right, _) in paths.iter().skip(left_index + 1) {
            if left.starts_with(right) || right.starts_with(left) {
                return Err(BackendError::new(
                    BackendErrorKind::InvalidInput,
                    format!("cannot move nested selected paths together: {left} and {right}"),
                ));
            }
        }
    }
    Ok(())
}

fn list_project_entries(root: &Path) -> BackendResult<Vec<WorkspaceEntry>> {
    let fs = WorkspaceFs::open(root).map_err(|error| {
        BackendError::new(
            BackendErrorKind::InvalidInput,
            format!(
                "failed to open project root '{}' for entry listing: {error}",
                root.display()
            ),
        )
    })?;
    let entries = fs
        .list_entries()
        .map_err(|error| {
            BackendError::new(
                BackendErrorKind::Io,
                format!(
                    "failed to list project entries for '{}': {error}",
                    root.display()
                ),
            )
        })?
        .into_iter()
        .map(WorkspaceEntry::from)
        .collect();
    Ok(entries)
}

impl From<LanguageWorkspaceEntry> for WorkspaceEntry {
    fn from(entry: LanguageWorkspaceEntry) -> Self {
        Self {
            path: entry.path,
            kind: match entry.kind {
                dawn_language::fs::WorkspaceEntryKind::Directory => WorkspaceEntryKind::Directory,
                dawn_language::fs::WorkspaceEntryKind::File => WorkspaceEntryKind::File,
            },
        }
    }
}
