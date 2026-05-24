use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use dawn_project::{
    analyze_project_with_overlays, apply_fixture_document_edit as edit_fixture_document,
    apply_layout_document_edit as edit_layout_document,
    get_fixture_document as inspect_fixture_document,
    get_layout_document as inspect_layout_document, inspect_document as inspect_dawn_document,
    DiagnosticCode, DiagnosticSeverity, DocumentDescriptor, DocumentEditResult, FixtureDocument,
    LayoutDocument, ProjectDiagnostic, ProjectFs, ProjectFsEntryKind, ProjectOverlay, ProjectPath,
    TextRange,
};
use serde::{Deserialize, Serialize};
use specta_typescript::Typescript;
use tauri::State;
use tauri_specta::{collect_commands, Builder};

#[derive(Default)]
struct AppState {
    project: Mutex<ProjectSession>,
}

#[derive(Default)]
struct ProjectSession {
    root_display: Option<String>,
    fs: Option<ProjectFs>,
    project_file: Option<ProjectPath>,
    active_sequence: Option<ProjectPath>,
}

#[derive(Serialize, specta::Type)]
struct ProjectState {
    root: String,
    files: Vec<String>,
    entries: Vec<ProjectEntry>,
    diagnostics: Vec<LanguageProblem>,
}

#[derive(Serialize, specta::Type)]
struct ProjectEntry {
    path: String,
    kind: ProjectEntryKind,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
enum ProjectEntryKind {
    Directory,
    File,
}

#[derive(Serialize, specta::Type)]
struct FileOperationState {
    project: ProjectState,
    moved: Vec<FileMove>,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct FileMove {
    old_path: String,
    new_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedMove {
    old_path: ProjectPath,
    new_path: ProjectPath,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct FrameSummary {
    pixels: u32,
    fixture_spans: u32,
    warnings: Option<Vec<String>>,
}

#[derive(Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ProjectOverlayInput {
    path: String,
    content: String,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AnalysisState {
    diagnostics: Vec<LanguageProblem>,
    resolved: bool,
    reachable_file_count: u32,
    object_count: u32,
}

#[derive(Serialize, specta::Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum LayoutDocumentEditResponse {
    Applied {
        serialized_content: String,
        analysis: AnalysisState,
        refreshed_document: LayoutDocument,
    },
    Blocked {
        diagnostics: Vec<LanguageProblem>,
        message: String,
    },
}

#[derive(Serialize, specta::Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum FixtureDocumentEditResponse {
    Applied {
        serialized_content: String,
        analysis: AnalysisState,
        refreshed_document: FixtureDocument,
    },
    Blocked {
        diagnostics: Vec<LanguageProblem>,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct LanguageProblem {
    path: String,
    severity: DiagnosticSeverity,
    message: String,
    code: DiagnosticCode,
    source: String,
    line: u32,
    column: u32,
    end_line: u32,
    end_column: u32,
}

#[tauri::command]
#[specta::specta]
fn open_project(path: String, state: State<'_, AppState>) -> Result<ProjectState, String> {
    let path = PathBuf::from(path);
    let (root, project_file) = if path.is_dir() {
        (path, ProjectPath::new("project.dawn"))
    } else {
        let file_name = path
            .file_name()
            .ok_or_else(|| "project file has no file name".to_string())?;
        let root = path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "project file has no parent".to_string())?;
        (root, ProjectPath::parse(PathBuf::from(file_name))?)
    };
    let fs = ProjectFs::open_ambient(&root).map_err(|error| error.to_string())?;
    let root_display = root.to_string_lossy().replace('\\', "/");
    {
        let mut session = state
            .project
            .lock()
            .map_err(|_| "project state lock failed")?;
        session.root_display = Some(root_display);
        session.fs = Some(fs);
        session.project_file = Some(project_file);
        session.active_sequence = None;
    }
    check_project(state)
}

#[tauri::command]
#[specta::specta]
fn check_project(state: State<'_, AppState>) -> Result<ProjectState, String> {
    let (root, fs) = {
        let session = state
            .project
            .lock()
            .map_err(|_| "project state lock failed")?;
        (
            session
                .root_display
                .clone()
                .ok_or_else(|| "no project open".to_string())?,
            session
                .fs
                .clone()
                .ok_or_else(|| "no project open".to_string())?,
        )
    };
    Ok(ProjectState {
        root,
        files: list_source_files(&fs)?,
        entries: list_project_entries(&fs)?,
        diagnostics: Vec::new(),
    })
}

#[tauri::command]
#[specta::specta]
fn analyze_project(
    overlays: Vec<ProjectOverlayInput>,
    state: State<'_, AppState>,
) -> Result<AnalysisState, String> {
    let fs = project_fs(&state)?;
    let project_file = current_project_file(&state)?;
    let overlays = project_overlays_from_inputs(overlays)?;
    let analysis = analyze_project_with_overlays(&fs, project_file, None, overlays);
    let object_count = analysis
        .files
        .values()
        .filter_map(|file| file.file.as_ref())
        .map(|file| file.len())
        .sum::<usize>() as u32;

    Ok(AnalysisState {
        diagnostics: analysis
            .diagnostics
            .iter()
            .map(problem_from_diagnostic)
            .collect(),
        resolved: analysis.resolved.is_some(),
        reachable_file_count: analysis.files.len() as u32,
        object_count,
    })
}

#[tauri::command]
#[specta::specta]
fn inspect_document(
    path: String,
    overlays: Vec<ProjectOverlayInput>,
    state: State<'_, AppState>,
) -> Result<DocumentDescriptor, String> {
    inspect_dawn_document(
        &project_fs(&state)?,
        ProjectPath::parse(path)?,
        project_overlays_from_inputs(overlays)?,
    )
}

#[tauri::command]
#[specta::specta]
fn get_layout_document(
    path: String,
    object_key: String,
    overlays: Vec<ProjectOverlayInput>,
    state: State<'_, AppState>,
) -> Result<LayoutDocument, String> {
    inspect_layout_document(
        &project_fs(&state)?,
        ProjectPath::parse(path)?,
        &object_key,
        current_project_file(&state)?,
        project_overlays_from_inputs(overlays)?,
    )
}

#[tauri::command]
#[specta::specta]
fn get_fixture_document(
    path: String,
    selected_object_key: Option<String>,
    overlays: Vec<ProjectOverlayInput>,
    state: State<'_, AppState>,
) -> Result<FixtureDocument, String> {
    inspect_fixture_document(
        &project_fs(&state)?,
        ProjectPath::parse(path)?,
        selected_object_key.as_deref(),
        project_overlays_from_inputs(overlays)?,
    )
}

#[tauri::command]
#[specta::specta]
fn apply_layout_document_edit(
    path: String,
    object_key: String,
    document: LayoutDocument,
    base_content: String,
    overlays: Vec<ProjectOverlayInput>,
    allow_breaking_references: bool,
    state: State<'_, AppState>,
) -> Result<LayoutDocumentEditResponse, String> {
    let result = edit_layout_document(
        &project_fs(&state)?,
        ProjectPath::parse(path)?,
        &object_key,
        document,
        base_content,
        project_overlays_from_inputs(overlays)?,
        current_project_file(&state)?,
        allow_breaking_references,
    )?;
    Ok(match result {
        DocumentEditResult::Applied(outcome) => LayoutDocumentEditResponse::Applied {
            serialized_content: outcome.serialized_content,
            analysis: analysis_to_state(outcome.analysis),
            refreshed_document: outcome.refreshed_document,
        },
        DocumentEditResult::Blocked(blocked) => LayoutDocumentEditResponse::Blocked {
            diagnostics: blocked
                .diagnostics
                .iter()
                .map(problem_from_diagnostic)
                .collect(),
            message: blocked.message,
        },
    })
}

#[tauri::command]
#[specta::specta]
fn apply_fixture_document_edit(
    path: String,
    document: FixtureDocument,
    base_content: String,
    overlays: Vec<ProjectOverlayInput>,
    allow_breaking_references: bool,
    state: State<'_, AppState>,
) -> Result<FixtureDocumentEditResponse, String> {
    let result = edit_fixture_document(
        &project_fs(&state)?,
        ProjectPath::parse(path)?,
        document,
        base_content,
        project_overlays_from_inputs(overlays)?,
        current_project_file(&state)?,
        allow_breaking_references,
    )?;
    Ok(match result {
        DocumentEditResult::Applied(outcome) => FixtureDocumentEditResponse::Applied {
            serialized_content: outcome.serialized_content,
            analysis: analysis_to_state(outcome.analysis),
            refreshed_document: outcome.refreshed_document,
        },
        DocumentEditResult::Blocked(blocked) => FixtureDocumentEditResponse::Blocked {
            diagnostics: blocked
                .diagnostics
                .iter()
                .map(problem_from_diagnostic)
                .collect(),
            message: blocked.message,
        },
    })
}

#[tauri::command]
#[specta::specta]
fn read_file(path: String, state: State<'_, AppState>) -> Result<String, String> {
    project_fs(&state)?
        .read_to_string(&ProjectPath::parse(path)?)
        .map_err(|err| err.to_string())
}

#[tauri::command]
#[specta::specta]
fn write_file(path: String, content: String, state: State<'_, AppState>) -> Result<(), String> {
    project_fs(&state)?
        .write(&ProjectPath::parse(path)?, content)
        .map_err(|err| err.to_string())
}

#[tauri::command]
#[specta::specta]
fn rename_path(
    path: String,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<FileOperationState, String> {
    let fs = project_fs(&state)?;
    validate_file_name(&new_name)?;
    let old_path = ProjectPath::parse(path)?;
    let new_path = old_path
        .parent()
        .ok_or_else(|| "path has no parent".to_string())?
        .join(new_name)?;
    if fs.exists(&new_path) {
        return Err("target path already exists".to_string());
    }
    fs.rename(&old_path, &new_path)
        .map_err(|err| err.to_string())?;
    update_active_sequence_after_move(&state, &old_path, &new_path)?;
    Ok(FileOperationState {
        project: check_project(state)?,
        moved: vec![FileMove {
            old_path: old_path.to_slash_string(),
            new_path: new_path.to_slash_string(),
        }],
    })
}

#[tauri::command]
#[specta::specta]
fn move_paths(
    paths: Vec<String>,
    new_parent: String,
    state: State<'_, AppState>,
) -> Result<FileOperationState, String> {
    let fs = project_fs(&state)?;
    let planned_moves = plan_moves(&fs, paths, new_parent)?;
    apply_planned_moves(&fs, &planned_moves)?;
    update_active_sequence_after_moves(&state, &planned_moves)?;

    Ok(FileOperationState {
        project: check_project(state)?,
        moved: file_moves_from_plan(&planned_moves),
    })
}

#[tauri::command]
#[specta::specta]
fn open_sequence(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut session = state
        .project
        .lock()
        .map_err(|_| "project state lock failed")?;
    session.active_sequence = Some(ProjectPath::parse(path)?);
    Ok(())
}

#[tauri::command]
#[specta::specta]
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
#[specta::specta]
fn play() {}

#[tauri::command]
#[specta::specta]
fn pause() {}

#[tauri::command]
#[specta::specta]
fn seek(_time: f64) {}

pub fn export_bindings(path: impl AsRef<Path>) -> Result<(), String> {
    let builder = command_builder();
    builder
        .export(Typescript::default(), path)
        .map_err(|error| error.to_string())
}

pub fn run() {
    let builder = command_builder();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Dawn");
}

fn command_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        open_project,
        check_project,
        analyze_project,
        inspect_document,
        get_layout_document,
        get_fixture_document,
        apply_layout_document_edit,
        apply_fixture_document_edit,
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
}

fn list_source_files(fs: &ProjectFs) -> Result<Vec<String>, String> {
    let mut files = fs
        .list_entries()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|entry| {
            entry.kind == ProjectFsEntryKind::File
                && entry
                    .path
                    .as_path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext == "dawn")
        })
        .map(|entry| entry.path.to_slash_string())
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn list_project_entries(fs: &ProjectFs) -> Result<Vec<ProjectEntry>, String> {
    let mut entries = fs
        .list_entries()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|entry| ProjectEntry {
            path: entry.path.to_slash_string(),
            kind: if entry.kind == ProjectFsEntryKind::Directory {
                ProjectEntryKind::Directory
            } else {
                ProjectEntryKind::File
            },
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn project_fs(state: &State<'_, AppState>) -> Result<ProjectFs, String> {
    let session = state
        .project
        .lock()
        .map_err(|_| "project state lock failed")?;
    session
        .fs
        .clone()
        .ok_or_else(|| "no project open".to_string())
}

fn current_project_file(state: &State<'_, AppState>) -> Result<ProjectPath, String> {
    let session = state
        .project
        .lock()
        .map_err(|_| "project state lock failed")?;
    session
        .project_file
        .clone()
        .ok_or_else(|| "no project open".to_string())
}

fn problem_from_diagnostic(diagnostic: &ProjectDiagnostic) -> LanguageProblem {
    let (line, column, end_line, end_column) = range_to_one_based(diagnostic.range);
    LanguageProblem {
        path: diagnostic.path.to_slash_string(),
        severity: diagnostic.severity,
        message: diagnostic.message.clone(),
        code: diagnostic.code,
        source: "dawn-project".to_string(),
        line,
        column,
        end_line,
        end_column,
    }
}

fn project_overlays_from_inputs(
    overlays: Vec<ProjectOverlayInput>,
) -> Result<Vec<ProjectOverlay>, String> {
    overlays
        .into_iter()
        .map(|overlay| {
            Ok(ProjectOverlay {
                path: ProjectPath::parse(overlay.path)?,
                content: overlay.content,
            })
        })
        .collect()
}

fn analysis_to_state(analysis: dawn_project::ProjectAnalysis) -> AnalysisState {
    let object_count = analysis
        .files
        .values()
        .filter_map(|file| file.file.as_ref())
        .map(|file| file.len())
        .sum::<usize>() as u32;

    AnalysisState {
        diagnostics: analysis
            .diagnostics
            .iter()
            .map(problem_from_diagnostic)
            .collect(),
        resolved: analysis.resolved.is_some(),
        reachable_file_count: analysis.files.len() as u32,
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

fn validate_file_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("name cannot contain path separators".to_string());
    }
    Ok(())
}

fn plan_moves(
    fs: &ProjectFs,
    paths: Vec<String>,
    new_parent: String,
) -> Result<Vec<PlannedMove>, String> {
    let new_parent = ProjectPath::parse(new_parent)?;
    if !fs.is_dir(&new_parent) {
        return Err("drop target is not a directory".to_string());
    }

    let mut selected_paths = Vec::new();
    let mut seen_sources = HashSet::new();
    for path in paths {
        let old_path = ProjectPath::parse(path)?;
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
        let new_path = new_parent.join(PathBuf::from(name))?;
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

fn reject_nested_selected_paths(paths: &[ProjectPath]) -> Result<(), String> {
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

fn apply_planned_moves(fs: &ProjectFs, planned_moves: &[PlannedMove]) -> Result<(), String> {
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

fn rollback_completed_moves(fs: &ProjectFs, completed: &[PlannedMove]) -> Result<(), String> {
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

fn file_moves_from_plan(planned_moves: &[PlannedMove]) -> Vec<FileMove> {
    planned_moves
        .iter()
        .map(|planned_move| FileMove {
            old_path: planned_move.old_path.to_slash_string(),
            new_path: planned_move.new_path.to_slash_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn problem_defaults_missing_ranges_to_one_based_start() {
        let diagnostic = ProjectDiagnostic {
            path: ProjectPath::new("project.dawn"),
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
            path: ProjectPath::new("project.dawn"),
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

    #[test]
    fn runtime_backend_does_not_use_ambient_fs_or_walkdir() {
        let source =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
                .unwrap();

        assert!(!source.contains(&["use std", "fs;"].join("::")));
        assert!(!source.contains(&["walkdir", ""].join("::")));
        assert!(!source.contains(&["Walk", "Dir"].join("")));
        assert!(!source.contains(&["ensure", "inside", "root"].join("_")));
    }

    #[test]
    fn apply_planned_moves_rolls_back_completed_moves_after_later_rename_failure() {
        let root = temp_project_dir("move-rollback");
        std::fs::write(root.join("a.dawn"), "a").unwrap();
        std::fs::write(root.join("b.dawn"), "b").unwrap();
        std::fs::create_dir(root.join("target")).unwrap();
        let fs = ProjectFs::open_ambient(&root).unwrap();
        let planned_moves = plan_moves(
            &fs,
            vec!["a.dawn".to_string(), "b.dawn".to_string()],
            "target".to_string(),
        )
        .unwrap();
        std::fs::remove_file(root.join("b.dawn")).unwrap();

        let result = apply_planned_moves(&fs, &planned_moves);

        assert!(result.is_err());
        assert!(root.join("a.dawn").exists());
        assert!(!root.join("target").join("a.dawn").exists());
    }

    #[test]
    fn plan_moves_rejects_duplicate_destination_paths_before_rename() {
        let root = temp_project_dir("move-duplicate-destination");
        std::fs::create_dir(root.join("one")).unwrap();
        std::fs::create_dir(root.join("two")).unwrap();
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("one").join("same.dawn"), "one").unwrap();
        std::fs::write(root.join("two").join("same.dawn"), "two").unwrap();
        let fs = ProjectFs::open_ambient(&root).unwrap();

        let result = plan_moves(
            &fs,
            vec!["one/same.dawn".to_string(), "two/same.dawn".to_string()],
            "target".to_string(),
        );

        assert!(result
            .unwrap_err()
            .contains("duplicate destination path: target/same.dawn"));
        assert!(root.join("one").join("same.dawn").exists());
        assert!(root.join("two").join("same.dawn").exists());
    }

    #[test]
    fn plan_moves_rejects_duplicate_source_paths_before_rename() {
        let root = temp_project_dir("move-duplicate-source");
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("source.dawn"), "source").unwrap();
        let fs = ProjectFs::open_ambient(&root).unwrap();

        let result = plan_moves(
            &fs,
            vec!["source.dawn".to_string(), "source.dawn".to_string()],
            "target".to_string(),
        );

        assert!(result
            .unwrap_err()
            .contains("duplicate source path: source.dawn"));
        assert!(root.join("source.dawn").exists());
        assert!(!root.join("target").join("source.dawn").exists());
    }

    #[test]
    fn plan_moves_rejects_nested_selected_paths_before_rename() {
        let root = temp_project_dir("move-nested-selection");
        std::fs::create_dir(root.join("folder")).unwrap();
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("folder").join("file.dawn"), "file").unwrap();
        let fs = ProjectFs::open_ambient(&root).unwrap();

        let result = plan_moves(
            &fs,
            vec!["folder".to_string(), "folder/file.dawn".to_string()],
            "target".to_string(),
        );

        assert!(result
            .unwrap_err()
            .contains("cannot move nested selected paths together"));
        assert!(root.join("folder").exists());
        assert!(root.join("folder").join("file.dawn").exists());
    }

    #[test]
    fn plan_moves_rejects_existing_later_target_before_rename() {
        let root = temp_project_dir("move-existing-later-target");
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("a.dawn"), "a").unwrap();
        std::fs::write(root.join("b.dawn"), "b").unwrap();
        std::fs::write(root.join("target").join("b.dawn"), "existing").unwrap();
        let fs = ProjectFs::open_ambient(&root).unwrap();

        let result = plan_moves(
            &fs,
            vec!["a.dawn".to_string(), "b.dawn".to_string()],
            "target".to_string(),
        );

        assert!(result
            .unwrap_err()
            .contains("target already exists: target/b.dawn"));
        assert!(root.join("a.dawn").exists());
        assert!(!root.join("target").join("a.dawn").exists());
    }

    #[test]
    fn successful_batch_move_returns_expected_file_moves() {
        let root = temp_project_dir("move-success");
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("a.dawn"), "a").unwrap();
        std::fs::write(root.join("b.dawn"), "b").unwrap();
        let fs = ProjectFs::open_ambient(&root).unwrap();
        let planned_moves = plan_moves(
            &fs,
            vec!["a.dawn".to_string(), "b.dawn".to_string()],
            "target".to_string(),
        )
        .unwrap();

        apply_planned_moves(&fs, &planned_moves).unwrap();
        let moved = file_moves_from_plan(&planned_moves);

        assert_eq!(moved.len(), 2);
        assert_eq!(moved[0].old_path, "a.dawn");
        assert_eq!(moved[0].new_path, "target/a.dawn");
        assert_eq!(moved[1].old_path, "b.dawn");
        assert_eq!(moved[1].new_path, "target/b.dawn");
        assert!(root.join("target").join("a.dawn").exists());
        assert!(root.join("target").join("b.dawn").exists());
    }

    #[test]
    fn active_sequence_updates_only_after_full_batch_succeeds() {
        let mut session = ProjectSession {
            active_sequence: Some(ProjectPath::new("sequence.dawn")),
            ..ProjectSession::default()
        };
        let planned_moves = vec![PlannedMove {
            old_path: ProjectPath::new("sequence.dawn"),
            new_path: ProjectPath::new("target/sequence.dawn"),
        }];

        assert_eq!(
            session.active_sequence,
            Some(ProjectPath::new("sequence.dawn"))
        );
        update_project_session_active_sequence_after_moves(&mut session, &planned_moves);

        assert_eq!(
            session.active_sequence,
            Some(ProjectPath::new("target/sequence.dawn"))
        );
    }

    fn temp_project_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("dawn-desktop-{label}-{nanos}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}

fn update_active_sequence_after_move(
    state: &State<'_, AppState>,
    old_path: &ProjectPath,
    new_path: &ProjectPath,
) -> Result<(), String> {
    update_active_sequence_after_moves(
        state,
        &[PlannedMove {
            old_path: old_path.clone(),
            new_path: new_path.clone(),
        }],
    )
}

fn update_active_sequence_after_moves(
    state: &State<'_, AppState>,
    planned_moves: &[PlannedMove],
) -> Result<(), String> {
    let mut session = state
        .project
        .lock()
        .map_err(|_| "project state lock failed")?;
    update_project_session_active_sequence_after_moves(&mut session, planned_moves);
    Ok(())
}

fn update_project_session_active_sequence_after_moves(
    session: &mut ProjectSession,
    planned_moves: &[PlannedMove],
) {
    if let Some(active_sequence) = session.active_sequence.as_ref() {
        if let Some(planned_move) = planned_moves
            .iter()
            .find(|planned_move| &planned_move.old_path == active_sequence)
        {
            session.active_sequence = Some(planned_move.new_path.clone());
        }
    }
}
