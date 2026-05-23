use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::State;
use walkdir::WalkDir;

#[derive(Default)]
struct AppState {
    project: Mutex<ProjectSession>,
}

#[derive(Default)]
struct ProjectSession {
    root: Option<PathBuf>,
    project_file: Option<PathBuf>,
    active_sequence: Option<PathBuf>,
}

#[derive(Serialize)]
struct ProjectState {
    root: String,
    files: Vec<String>,
    entries: Vec<ProjectEntryDto>,
    diagnostics: Vec<DiagnosticDto>,
}

#[derive(Serialize)]
struct ProjectEntryDto {
    path: String,
    kind: String,
}

#[derive(Serialize)]
struct FileOperationState {
    project: ProjectState,
    moved: Vec<FileMoveDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileMoveDto {
    old_path: String,
    new_path: String,
}

#[derive(Serialize)]
struct DiagnosticDto {
    severity: String,
    path: String,
    message: String,
}

#[derive(Serialize)]
struct FrameSummary {
    pixels: usize,
    fixture_spans: usize,
    warnings: Option<Vec<String>>,
}

#[tauri::command]
fn open_project(path: String, state: State<'_, AppState>) -> Result<ProjectState, String> {
    let path = PathBuf::from(path);
    let project_file = if path.is_dir() {
        path.join("project.dawn")
    } else {
        path
    };
    let root = project_file
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "project file has no parent".to_string())?;
    {
        let mut session = state
            .project
            .lock()
            .map_err(|_| "project state lock failed")?;
        session.root = Some(root.clone());
        session.project_file = Some(project_file);
    }
    check_project(state)
}

#[tauri::command]
fn check_project(state: State<'_, AppState>) -> Result<ProjectState, String> {
    let root = {
        let session = state
            .project
            .lock()
            .map_err(|_| "project state lock failed")?;
        session
            .root
            .clone()
            .ok_or_else(|| "no project open".to_string())?
    };
    Ok(ProjectState {
        root: root.display().to_string(),
        files: list_source_files(&root),
        entries: list_project_entries(&root),
        diagnostics: Vec::new(),
    })
}

#[tauri::command]
fn read_file(path: String) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| err.to_string())
}

#[tauri::command]
fn write_file(path: String, content: String) -> Result<(), String> {
    fs::write(path, content).map_err(|err| err.to_string())
}

#[tauri::command]
fn rename_path(
    path: String,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<FileOperationState, String> {
    let root = project_root(&state)?;
    validate_file_name(&new_name)?;
    let old_path = ensure_inside_root(&root, PathBuf::from(path))?;
    let new_path = old_path
        .parent()
        .ok_or_else(|| "path has no parent".to_string())?
        .join(new_name);
    let new_path = ensure_inside_root(&root, new_path)?;
    if new_path.exists() {
        return Err("target path already exists".to_string());
    }
    fs::rename(&old_path, &new_path).map_err(|err| err.to_string())?;
    update_active_sequence_after_move(&state, &old_path, &new_path)?;
    Ok(FileOperationState {
        project: check_project(state)?,
        moved: vec![FileMoveDto {
            old_path: old_path.display().to_string(),
            new_path: new_path.display().to_string(),
        }],
    })
}

#[tauri::command]
fn move_paths(
    paths: Vec<String>,
    new_parent: String,
    state: State<'_, AppState>,
) -> Result<FileOperationState, String> {
    let root = project_root(&state)?;
    let new_parent = ensure_inside_root(&root, PathBuf::from(new_parent))?;
    if !new_parent.is_dir() {
        return Err("drop target is not a directory".to_string());
    }

    let mut moved = Vec::new();
    for path in paths {
        let old_path = ensure_inside_root(&root, PathBuf::from(path))?;
        let name = old_path
            .file_name()
            .ok_or_else(|| "path has no file name".to_string())?;
        let new_path = ensure_inside_root(&root, new_parent.join(name))?;
        if old_path == new_path {
            continue;
        }
        if old_path.is_dir() && new_path.starts_with(&old_path) {
            return Err("cannot move a directory into itself".to_string());
        }
        if new_path.exists() {
            return Err(format!("target already exists: {}", new_path.display()));
        }
        fs::rename(&old_path, &new_path).map_err(|err| err.to_string())?;
        update_active_sequence_after_move(&state, &old_path, &new_path)?;
        moved.push(FileMoveDto {
            old_path: old_path.display().to_string(),
            new_path: new_path.display().to_string(),
        });
    }

    Ok(FileOperationState {
        project: check_project(state)?,
        moved,
    })
}

#[tauri::command]
fn open_sequence(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut session = state
        .project
        .lock()
        .map_err(|_| "project state lock failed")?;
    session.active_sequence = Some(PathBuf::from(path));
    Ok(())
}

#[tauri::command]
fn render_frame(_time: f64, state: State<'_, AppState>) -> Result<FrameSummary, String> {
    {
        let session = state
            .project
            .lock()
            .map_err(|_| "project state lock failed")?;
        session
            .active_sequence
            .clone()
            .ok_or_else(|| "no sequence open".to_string())?
    };
    Ok(FrameSummary {
        pixels: 0,
        fixture_spans: 0,
        warnings: Some(Vec::new()),
    })
}

#[tauri::command]
fn play() {}

#[tauri::command]
fn pause() {}

#[tauri::command]
fn seek(_time: f64) {}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            open_project,
            check_project,
            read_file,
            write_file,
            rename_path,
            move_paths,
            open_sequence,
            render_frame,
            play,
            pause,
            seek
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Dawn");
}

fn list_source_files(root: &Path) -> Vec<String> {
    let mut files = WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "dawn")
        })
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn list_project_entries(root: &Path) -> Vec<ProjectEntryDto> {
    let mut entries = WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| ProjectEntryDto {
            path: entry.path().display().to_string(),
            kind: if entry.file_type().is_dir() {
                "directory"
            } else {
                "file"
            }
            .to_string(),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries
}

fn project_root(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let session = state
        .project
        .lock()
        .map_err(|_| "project state lock failed")?;
    session
        .root
        .clone()
        .ok_or_else(|| "no project open".to_string())
}

fn ensure_inside_root(root: &Path, path: PathBuf) -> Result<PathBuf, String> {
    let canonical_root = root.canonicalize().map_err(|err| err.to_string())?;
    let canonical_path = if path.exists() {
        path.canonicalize().map_err(|err| err.to_string())?
    } else {
        let parent = path
            .parent()
            .ok_or_else(|| "path has no parent".to_string())?;
        let canonical_parent = parent.canonicalize().map_err(|err| err.to_string())?;
        canonical_parent.join(
            path.file_name()
                .ok_or_else(|| "path has no file name".to_string())?,
        )
    };
    if canonical_path.starts_with(&canonical_root) {
        Ok(canonical_path)
    } else {
        Err("path is outside the open project".to_string())
    }
}

fn validate_file_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("name cannot contain path separators".to_string());
    }
    Ok(())
}

fn update_active_sequence_after_move(
    state: &State<'_, AppState>,
    old_path: &Path,
    new_path: &Path,
) -> Result<(), String> {
    let mut session = state
        .project
        .lock()
        .map_err(|_| "project state lock failed")?;
    if session.active_sequence.as_deref() == Some(old_path) {
        session.active_sequence = Some(new_path.to_path_buf());
    }
    Ok(())
}
