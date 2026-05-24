use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use dawn_project::{
    analyze_project_with_overlays, get_layout_document as inspect_layout_document,
    inspect_document as inspect_dawn_document, save_layout_document_content, DiagnosticCode,
    DiagnosticSeverity, DocumentDescriptor, LayoutDocument, ProjectDiagnostic, ProjectOverlay,
    TextRange,
};
use serde::{Deserialize, Serialize};
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
    diagnostics: Vec<ProblemDto>,
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
struct FrameSummary {
    pixels: usize,
    fixture_spans: usize,
    warnings: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverlayDto {
    path: String,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisStateDto {
    diagnostics: Vec<ProblemDto>,
    resolved: bool,
    reachable_file_count: usize,
    object_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LayoutSaveResultDto {
    serialized_content: String,
    project: ProjectState,
    analysis: AnalysisStateDto,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ProblemDto {
    path: String,
    severity: String,
    message: String,
    code: String,
    source: String,
    line: u32,
    column: u32,
    end_line: u32,
    end_column: u32,
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
fn analyze_project(
    overlays: Vec<OverlayDto>,
    state: State<'_, AppState>,
) -> Result<AnalysisStateDto, String> {
    let project_file = current_project_file(&state)?;
    let overlays = overlays
        .into_iter()
        .map(|overlay| ProjectOverlay {
            path: PathBuf::from(overlay.path),
            content: overlay.content,
        })
        .collect::<Vec<_>>();
    let analysis = analyze_project_with_overlays(&project_file, None, overlays);
    let object_count = analysis
        .files
        .values()
        .filter_map(|file| file.file.as_ref())
        .map(|file| file.len())
        .sum();

    Ok(AnalysisStateDto {
        diagnostics: analysis
            .diagnostics
            .iter()
            .map(problem_from_diagnostic)
            .collect(),
        resolved: analysis.resolved.is_some(),
        reachable_file_count: analysis.files.len(),
        object_count,
    })
}

#[tauri::command]
fn inspect_document(
    path: String,
    overlays: Vec<OverlayDto>,
) -> Result<DocumentDescriptor, String> {
    inspect_dawn_document(path, project_overlays_from_dto(overlays))
}

#[tauri::command]
fn get_layout_document(
    path: String,
    object_key: String,
    overlays: Vec<OverlayDto>,
    state: State<'_, AppState>,
) -> Result<LayoutDocument, String> {
    inspect_layout_document(
        path,
        &object_key,
        current_project_file(&state)?,
        project_overlays_from_dto(overlays),
    )
}

#[tauri::command]
fn save_layout_document(
    path: String,
    object_key: String,
    document: LayoutDocument,
    base_content: Option<String>,
    overlays: Vec<OverlayDto>,
    state: State<'_, AppState>,
) -> Result<LayoutSaveResultDto, String> {
    let (serialized_content, analysis) = save_layout_document_content(
        &path,
        &object_key,
        document,
        base_content,
        project_overlays_from_dto(overlays),
    )?;
    fs::write(&path, &serialized_content).map_err(|error| error.to_string())?;
    let analysis_dto = analysis_to_dto(analysis);
    Ok(LayoutSaveResultDto {
        serialized_content,
        project: check_project(state)?,
        analysis: analysis_dto,
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
            analyze_project,
            inspect_document,
            get_layout_document,
            save_layout_document,
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

fn current_project_file(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let session = state
        .project
        .lock()
        .map_err(|_| "project state lock failed")?;
    session
        .project_file
        .clone()
        .ok_or_else(|| "no project open".to_string())
}

fn problem_from_diagnostic(diagnostic: &ProjectDiagnostic) -> ProblemDto {
    let (line, column, end_line, end_column) = range_to_one_based(diagnostic.range);
    ProblemDto {
        path: diagnostic.path.display().to_string(),
        severity: severity_to_string(diagnostic.severity),
        message: diagnostic.message.clone(),
        code: code_to_string(diagnostic.code),
        source: "dawn-project".to_string(),
        line,
        column,
        end_line,
        end_column,
    }
}

fn project_overlays_from_dto(overlays: Vec<OverlayDto>) -> Vec<ProjectOverlay> {
    overlays
        .into_iter()
        .map(|overlay| ProjectOverlay {
            path: PathBuf::from(overlay.path),
            content: overlay.content,
        })
        .collect()
}

fn analysis_to_dto(analysis: dawn_project::ProjectAnalysis) -> AnalysisStateDto {
    let object_count = analysis
        .files
        .values()
        .filter_map(|file| file.file.as_ref())
        .map(|file| file.len())
        .sum();

    AnalysisStateDto {
        diagnostics: analysis
            .diagnostics
            .iter()
            .map(problem_from_diagnostic)
            .collect(),
        resolved: analysis.resolved.is_some(),
        reachable_file_count: analysis.files.len(),
        object_count,
    }
}

fn range_to_one_based(range: Option<TextRange>) -> (u32, u32, u32, u32) {
    let Some(range) = range else {
        return (1, 1, 1, 1);
    };
    (
        range.start.line.saturating_add(1),
        range.start.character.saturating_add(1),
        range.end.line.saturating_add(1),
        range.end.character.saturating_add(1).max(1),
    )
}

fn severity_to_string(severity: DiagnosticSeverity) -> String {
    match severity {
        DiagnosticSeverity::Error => "Error",
        DiagnosticSeverity::Warning => "Warning",
    }
    .to_string()
}

fn code_to_string(code: DiagnosticCode) -> String {
    match code {
        DiagnosticCode::Io => "io",
        DiagnosticCode::Yaml => "yaml",
        DiagnosticCode::Import => "import",
        DiagnosticCode::Lower => "lower",
        DiagnosticCode::ProjectKey => "project_key",
    }
    .to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problem_defaults_missing_ranges_to_one_based_start() {
        let diagnostic = ProjectDiagnostic {
            path: PathBuf::from("project.dawn"),
            range: None,
            severity: DiagnosticSeverity::Error,
            code: DiagnosticCode::ProjectKey,
            message: "missing project".to_string(),
        };

        let problem = problem_from_diagnostic(&diagnostic);

        assert_eq!(problem.line, 1);
        assert_eq!(problem.column, 1);
        assert_eq!(problem.end_line, 1);
        assert_eq!(problem.end_column, 1);
    }

    #[test]
    fn problem_converts_zero_based_ranges_to_one_based_ranges() {
        let diagnostic = ProjectDiagnostic {
            path: PathBuf::from("project.dawn"),
            range: Some(TextRange {
                start: dawn_project::TextPosition {
                    line: 2,
                    character: 4,
                },
                end: dawn_project::TextPosition {
                    line: 2,
                    character: 11,
                },
            }),
            severity: DiagnosticSeverity::Warning,
            code: DiagnosticCode::Yaml,
            message: "bad yaml".to_string(),
        };

        let problem = problem_from_diagnostic(&diagnostic);

        assert_eq!(problem.line, 3);
        assert_eq!(problem.column, 5);
        assert_eq!(problem.end_line, 3);
        assert_eq!(problem.end_column, 12);
    }
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
