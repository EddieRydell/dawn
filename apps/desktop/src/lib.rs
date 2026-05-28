#![deny(clippy::disallowed_methods)]

use std::path::PathBuf;
use std::sync::Mutex;

use dawn_app_core::actions::AppAction;
use dawn_app_core::app_model::{AppModel, DispatchOutcome};
use dawn_app_core::dto::AppSnapshotDto;
use dawn_project::path::Utf8PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    model: Mutex<AppModel>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            model: Mutex::new(AppModel::default()),
        }
    }
}

type CommandResult<T> = Result<T, String>;

#[specta::specta]
#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    Ok(lock_model(&state)?.snapshot_dto())
}

#[specta::specta]
#[tauri::command]
fn open_project_dialog(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AppSnapshotDto> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Open Dawn Project")
        .pick_folder()
    else {
        return get_snapshot(state);
    };
    dispatch(&app, &state, AppAction::OpenProject(path))
}

#[specta::specta]
#[tauri::command]
fn open_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::OpenProject(PathBuf::from(path)))
}

#[specta::specta]
#[tauri::command]
fn open_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::OpenFile(project_path(path)))
}

#[specta::specta]
#[tauri::command]
fn close_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::CloseFile(project_path(path)))
}

#[specta::specta]
#[tauri::command]
fn set_active_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::SetActiveFile(project_path(path)))
}

#[specta::specta]
#[tauri::command]
fn update_active_text(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::UpdateActiveText(text))
}

#[specta::specta]
#[tauri::command]
fn flush_autosave(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::FlushAutosave)
}

#[specta::specta]
#[tauri::command]
fn create_file(
    app: AppHandle,
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(
        &app,
        &state,
        AppAction::CreateFile {
            parent: project_path(parent),
            name,
        },
    )
}

#[specta::specta]
#[tauri::command]
fn create_directory(
    app: AppHandle,
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(
        &app,
        &state,
        AppAction::CreateDirectory {
            parent: project_path(parent),
            name,
        },
    )
}

#[specta::specta]
#[tauri::command]
fn rename_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    new_name: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(
        &app,
        &state,
        AppAction::RenamePath {
            path: project_path(path),
            new_name,
        },
    )
}

#[specta::specta]
#[tauri::command]
fn delete_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::DeletePath(project_path(path)))
}

#[specta::specta]
#[tauri::command]
fn reload_project(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::ReloadProject)
}

#[specta::specta]
#[tauri::command]
fn toggle_project_tree(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::ToggleProjectTree)
}

#[specta::specta]
#[tauri::command]
fn preview_play(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    Ok(lock_model(&state)?.snapshot_dto())
}

#[specta::specta]
#[tauri::command]
fn preview_pause(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    Ok(lock_model(&state)?.snapshot_dto())
}

#[specta::specta]
#[tauri::command]
fn preview_stop(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    Ok(lock_model(&state)?.snapshot_dto())
}

pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new().commands(tauri_specta::collect_commands![
        get_snapshot,
        open_project_dialog,
        open_project,
        open_file,
        close_file,
        set_active_file,
        update_active_text,
        flush_autosave,
        create_file,
        create_directory,
        rename_path,
        delete_path,
        reload_project,
        toggle_project_tree,
        preview_play,
        preview_pause,
        preview_stop
    ])
}

pub fn export_bindings() -> Result<(), Box<dyn std::error::Error>> {
    specta_builder().export(
        specta_typescript::Typescript::default(),
        "apps/desktop/frontend/src/bindings.ts",
    )?;
    Ok(())
}

pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Dawn desktop");
}

fn dispatch(
    app: &AppHandle,
    state: &State<'_, AppState>,
    action: AppAction,
) -> CommandResult<AppSnapshotDto> {
    let mut model = lock_model(state)?;
    let outcome = model.dispatch(action)?;
    let snapshot = model.snapshot_dto();
    if outcome == DispatchOutcome::SnapshotChanged {
        app.emit("app_snapshot_changed", &snapshot)
            .map_err(|error| error.to_string())?;
    }
    Ok(snapshot)
}

fn lock_model<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<std::sync::MutexGuard<'a, AppModel>> {
    state
        .model
        .lock()
        .map_err(|_| "application state lock is poisoned".to_string())
}

fn project_path(path: String) -> Utf8PathBuf {
    Utf8PathBuf::from(path)
}
