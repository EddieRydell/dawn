use std::collections::HashSet;
use std::path::{Path, PathBuf};

use dawn_project::analysis::{analyze_project_with_overlays, ProjectAnalysis, ProjectOverlay};
use dawn_project::document::{
    apply_fixture_document_edit as edit_fixture_document,
    apply_layout_document_edit as edit_layout_document,
    apply_sequence_document_edit as edit_sequence_document,
    get_fixture_document as inspect_fixture_document,
    get_layout_document as inspect_layout_document,
    get_sequence_document as inspect_sequence_document, inspect_document as inspect_dawn_document,
    DocumentDescriptor, DocumentEditOutcome, FixtureDocument, LayoutDocument, SequenceDocument,
    SequenceDocumentEdit,
};
use dawn_project::fs::{WorkspaceEntry, WorkspaceEntryKind, WorkspaceFs};
use dawn_project::path::{serialized_import_path, utf8_path, PathStringExt, Utf8PathBuf};

#[derive(Debug, Default)]
pub struct WorkspaceService {
    root_path: Option<PathBuf>,
    root_display: Option<String>,
    fs: Option<WorkspaceFs>,
    project_file: Option<Utf8PathBuf>,
    active_sequence: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedMove {
    old_path: Utf8PathBuf,
    new_path: Utf8PathBuf,
}

impl WorkspaceService {
    pub fn open_project(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        let (root, project_file) = if path.is_dir() {
            (path.to_path_buf(), Utf8PathBuf::from("project.dawn"))
        } else {
            let file_name = path
                .file_name()
                .ok_or_else(|| "project file has no file name".to_string())?;
            let root = path
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| "project file has no parent".to_string())?;
            (root, utf8_path(PathBuf::from(file_name))?)
        };
        let fs = WorkspaceFs::open(&root).map_err(|error| error.to_string())?;
        self.root_path = Some(root.clone());
        self.root_display = Some(root.to_string_lossy().replace('\\', "/"));
        self.fs = Some(fs);
        self.project_file = Some(project_file);
        self.active_sequence = None;
        Ok(())
    }

    pub fn close_project(&mut self) {
        self.root_display = None;
        self.root_path = None;
        self.fs = None;
        self.project_file = None;
        self.active_sequence = None;
    }

    pub fn project_root_display(&self) -> Option<&str> {
        self.root_display.as_deref()
    }

    pub fn project_entries(&self) -> Result<Vec<WorkspaceEntry>, String> {
        list_project_entries(self.project_fs()?)
    }

    pub fn analyze(&self, overlays: Vec<ProjectOverlay>) -> Result<ProjectAnalysis, String> {
        Ok(analyze_project_with_overlays(
            self.project_fs()?,
            self.current_project_file()?,
            None,
            overlays,
        ))
    }

    pub fn inspect_document(
        &self,
        path: Utf8PathBuf,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<DocumentDescriptor, String> {
        inspect_dawn_document(self.project_fs()?, path, overlays)
    }

    pub fn layout_document(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<LayoutDocument, String> {
        inspect_layout_document(
            self.project_fs()?,
            path,
            object_key,
            self.current_project_file()?,
            overlays,
        )
    }

    pub fn fixture_document(
        &self,
        path: Utf8PathBuf,
        selected_object_key: Option<&str>,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<FixtureDocument, String> {
        inspect_fixture_document(self.project_fs()?, path, selected_object_key, overlays)
    }

    pub fn sequence_document(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<SequenceDocument, String> {
        inspect_sequence_document(
            self.project_fs()?,
            path,
            object_key,
            self.current_project_file()?,
            overlays,
        )
    }

    pub fn inspect_fixture_file(
        &self,
        selected_file: &Path,
    ) -> Result<(Utf8PathBuf, FixtureDocument), String> {
        let path = self.project_path_for_selected_file(selected_file)?;
        let document =
            inspect_fixture_document(self.project_fs()?, path.clone(), None, Vec::new())?;
        Ok((path, document))
    }

    pub fn fixture_import_string(
        &self,
        importing_path: &Utf8PathBuf,
        selected_file: &Path,
        object_key: &str,
    ) -> Result<(String, bool), String> {
        let path = self.project_path_for_selected_file(selected_file)?;
        let is_absolute = path.is_absolute();
        let import_path = if is_absolute {
            path.to_slash_string()
        } else {
            serialized_import_path(importing_path, &path)
        };
        Ok((format!("{import_path}::{object_key}"), is_absolute))
    }

    pub fn apply_layout_edit(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
        document: LayoutDocument,
        base_content: String,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<DocumentEditOutcome<LayoutDocument>, String> {
        edit_layout_document(
            self.project_fs()?,
            path,
            object_key,
            document,
            base_content,
            overlays,
        )
    }

    pub fn apply_fixture_edit(
        &self,
        path: Utf8PathBuf,
        document: FixtureDocument,
        base_content: String,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<DocumentEditOutcome<FixtureDocument>, String> {
        edit_fixture_document(self.project_fs()?, path, document, base_content, overlays)
    }

    pub fn apply_sequence_edit(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
        edit: SequenceDocumentEdit,
        base_content: String,
        overlays: Vec<ProjectOverlay>,
        analysis: &ProjectAnalysis,
    ) -> Result<DocumentEditOutcome<SequenceDocument>, String> {
        edit_sequence_document(
            self.project_fs()?,
            path,
            object_key,
            edit,
            base_content,
            overlays,
            analysis,
        )
    }

    pub fn read_file(&self, path: Utf8PathBuf) -> Result<String, String> {
        self.project_fs()?
            .read_to_string(&path)
            .map_err(|error| error.to_string())
    }

    pub fn write_file(&self, path: Utf8PathBuf, content: impl AsRef<[u8]>) -> Result<(), String> {
        self.project_fs()?
            .write(&path, content)
            .map_err(|error| error.to_string())
    }

    pub fn create_file(&mut self, parent: Utf8PathBuf, name: &str) -> Result<Utf8PathBuf, String> {
        let name = file_name_with_default_extension(name)?;
        validate_file_name(&name)?;
        let fs = self.project_fs()?.clone();
        if !parent.as_str().is_empty() && !fs.is_dir(&parent) {
            return Err("parent path is not a directory".to_string());
        }
        let path = parent.join(&name);
        if fs.exists(&path) {
            return Err("target path already exists".to_string());
        }
        fs.create_file(&path, [])
            .map_err(|error| error.to_string())?;
        Ok(path)
    }

    pub fn create_directory(
        &mut self,
        parent: Utf8PathBuf,
        name: &str,
    ) -> Result<Utf8PathBuf, String> {
        validate_file_name(name)?;
        let fs = self.project_fs()?.clone();
        if !parent.as_str().is_empty() && !fs.is_dir(&parent) {
            return Err("parent path is not a directory".to_string());
        }
        let path = parent.join(name);
        if fs.exists(&path) {
            return Err("target path already exists".to_string());
        }
        fs.create_dir(&path).map_err(|error| error.to_string())?;
        Ok(path)
    }

    pub fn delete_path(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        let fs = self.project_fs()?.clone();
        if path.as_str().is_empty() {
            return Err("cannot delete project root".to_string());
        }
        if !fs.exists(&path) {
            return Err("path does not exist".to_string());
        }
        fs.delete_path(&path).map_err(|error| error.to_string())?;
        if self
            .active_sequence
            .as_ref()
            .is_some_and(|sequence| sequence == &path || sequence.starts_with(&path))
        {
            self.active_sequence = None;
        }
        Ok(())
    }

    pub fn rename_path(
        &mut self,
        path: Utf8PathBuf,
        new_name: &str,
    ) -> Result<Vec<(Utf8PathBuf, Utf8PathBuf)>, String> {
        validate_file_name(new_name)?;
        let fs = self.project_fs()?.clone();
        let new_path = path
            .parent()
            .ok_or_else(|| "path has no parent".to_string())?
            .join(new_name);
        if fs.exists(&new_path) {
            return Err("target path already exists".to_string());
        }
        fs.rename(&path, &new_path)
            .map_err(|error| error.to_string())?;
        update_active_sequence_after_moves(
            &mut self.active_sequence,
            &[PlannedMove {
                old_path: path.clone(),
                new_path: new_path.clone(),
            }],
        );
        Ok(vec![(path, new_path)])
    }

    pub fn move_paths(
        &mut self,
        paths: Vec<Utf8PathBuf>,
        new_parent: Utf8PathBuf,
    ) -> Result<Vec<(Utf8PathBuf, Utf8PathBuf)>, String> {
        let fs = self.project_fs()?.clone();
        let planned_moves = plan_moves(&fs, paths, new_parent)?;
        apply_planned_moves(&fs, &planned_moves)?;
        update_active_sequence_after_moves(&mut self.active_sequence, &planned_moves);

        Ok(project_path_moves_from_plan(&planned_moves))
    }

    pub fn open_sequence(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        if !self.project_fs()?.is_file(&path) {
            return Err(format!(
                "sequence file not found: {}",
                path.to_slash_string()
            ));
        }
        self.active_sequence = Some(path);
        Ok(())
    }

    pub fn active_sequence(&self) -> Option<&Utf8PathBuf> {
        self.active_sequence.as_ref()
    }

    fn project_fs(&self) -> Result<&WorkspaceFs, String> {
        self.fs.as_ref().ok_or_else(no_project)
    }

    fn current_project_file(&self) -> Result<Utf8PathBuf, String> {
        self.project_file.clone().ok_or_else(no_project)
    }

    fn project_path_for_selected_file(&self, selected_file: &Path) -> Result<Utf8PathBuf, String> {
        let root = self.root_path.as_ref().ok_or_else(no_project)?;
        let selected_file = selected_file
            .canonicalize()
            .map_err(|error| format!("failed to inspect selected file: {error}"))?;
        let root = root
            .canonicalize()
            .map_err(|error| format!("failed to inspect project root: {error}"))?;
        if let Ok(relative) = selected_file.strip_prefix(&root) {
            utf8_path(relative)
        } else {
            utf8_path(selected_file)
        }
    }
}

fn no_project() -> String {
    "no project open".to_string()
}

fn list_project_entries(fs: &WorkspaceFs) -> Result<Vec<WorkspaceEntry>, String> {
    let mut entries = fs.list_entries().map_err(|error| error.to_string())?;
    entries.sort_by(|left, right| {
        (left.kind != WorkspaceEntryKind::Directory, &left.path)
            .cmp(&(right.kind != WorkspaceEntryKind::Directory, &right.path))
    });
    Ok(entries)
}

fn validate_file_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name == "." || name == ".." {
        return Err("name cannot be . or ..".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("name cannot contain path separators".to_string());
    }
    Ok(())
}

fn file_name_with_default_extension(name: &str) -> Result<String, String> {
    validate_file_name(name)?;
    let path = Path::new(name);
    if path.extension().is_none() {
        Ok(format!("{name}.dawn"))
    } else {
        Ok(name.to_string())
    }
}

fn plan_moves(
    fs: &WorkspaceFs,
    paths: Vec<Utf8PathBuf>,
    new_parent: Utf8PathBuf,
) -> Result<Vec<PlannedMove>, String> {
    if !fs.is_dir(&new_parent) {
        return Err("drop target is not a directory".to_string());
    }

    let mut selected_paths = Vec::new();
    let mut seen_sources = HashSet::new();
    for old_path in paths {
        if !seen_sources.insert(old_path.clone()) {
            return Err(format!(
                "duplicate source path: {}",
                old_path.to_slash_string()
            ));
        }
        selected_paths.push(old_path);
    }
    reject_nested_selected_paths(&selected_paths)?;

    let mut planned_moves = Vec::new();
    let mut seen_destinations = HashSet::new();
    for old_path in selected_paths {
        let name = old_path
            .file_name()
            .ok_or_else(|| "path has no file name".to_string())?;
        let new_path = new_parent.join(name);
        if old_path == new_path {
            continue;
        }
        if fs.is_dir(&old_path) && new_path.starts_with(&old_path) {
            return Err("cannot move a directory into itself".to_string());
        }
        if !seen_destinations.insert(new_path.clone()) {
            return Err(format!(
                "duplicate destination path: {}",
                new_path.to_slash_string()
            ));
        }
        if fs.exists(&new_path) {
            return Err(format!(
                "target already exists: {}",
                new_path.to_slash_string()
            ));
        }
        planned_moves.push(PlannedMove { old_path, new_path });
    }

    Ok(planned_moves)
}

fn reject_nested_selected_paths(paths: &[Utf8PathBuf]) -> Result<(), String> {
    for (left_index, left) in paths.iter().enumerate() {
        for right in paths.iter().skip(left_index + 1) {
            if left.starts_with(right) || right.starts_with(left) {
                return Err(format!(
                    "cannot move nested selected paths together: {} and {}",
                    left.to_slash_string(),
                    right.to_slash_string()
                ));
            }
        }
    }
    Ok(())
}

fn apply_planned_moves(fs: &WorkspaceFs, planned_moves: &[PlannedMove]) -> Result<(), String> {
    let mut completed = Vec::new();
    for planned_move in planned_moves {
        if let Err(error) = fs.rename(&planned_move.old_path, &planned_move.new_path) {
            let rollback_error = rollback_completed_moves(fs, &completed);
            return Err(match rollback_error {
                Ok(()) => error.to_string(),
                Err(rollback_error) => format!("{}; rollback failed: {}", error, rollback_error),
            });
        }
        completed.push(planned_move.clone());
    }
    Ok(())
}

fn rollback_completed_moves(fs: &WorkspaceFs, completed: &[PlannedMove]) -> Result<(), String> {
    let mut errors = Vec::new();
    for completed_move in completed.iter().rev() {
        if let Err(error) = fs.rename(&completed_move.new_path, &completed_move.old_path) {
            errors.push(format!(
                "{} -> {}: {}",
                completed_move.new_path.to_slash_string(),
                completed_move.old_path.to_slash_string(),
                error
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn project_path_moves_from_plan(planned_moves: &[PlannedMove]) -> Vec<(Utf8PathBuf, Utf8PathBuf)> {
    planned_moves
        .iter()
        .map(|planned_move| (planned_move.old_path.clone(), planned_move.new_path.clone()))
        .collect()
}

fn update_active_sequence_after_moves(
    active_sequence: &mut Option<Utf8PathBuf>,
    planned_moves: &[PlannedMove],
) {
    if let Some(sequence) = active_sequence.as_ref() {
        for planned_move in planned_moves {
            if let Some(new_sequence) =
                moved_path(sequence, &planned_move.old_path, &planned_move.new_path)
            {
                *active_sequence = Some(new_sequence);
                return;
            }
        }
    }
}

fn moved_path(
    path: &Utf8PathBuf,
    old_path: &Utf8PathBuf,
    new_path: &Utf8PathBuf,
) -> Option<Utf8PathBuf> {
    if path == old_path {
        return Some(new_path.clone());
    }
    if !path.starts_with(old_path) {
        return None;
    }
    let relative = path.strip_prefix(old_path).ok()?;
    Some(new_path.join(relative))
}
