use std::path::PathBuf;

use dawn_backend::{AppBackend, AppUpdate, BackendResult, EditorViewMode};
use dawn_language::path::Utf8PathBuf;
use tauri::{AppHandle, State};

use crate::{
    dto::{
        AppCommandDto, AppCommandResponseDto, AppSnapshotDto, EditorViewModeDto, PreviewSceneDto,
        PreviewTransportMode, SequenceEffectPreviewRequestEffectDto,
        SequenceEffectPreviewResultsDto,
    },
    events, jobs,
    state::{AppState, CommandResult},
};

#[specta::specta]
#[tauri::command]
pub(crate) fn get_app_snapshot(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    let backend = state.lock_backend()?;
    Ok(AppSnapshotDto::from(backend.view()))
}

#[specta::specta]
#[tauri::command]
pub(crate) async fn dispatch_app_command(
    app: AppHandle,
    state: State<'_, AppState>,
    command: AppCommandDto,
) -> CommandResult<AppCommandResponseDto> {
    match command {
        AppCommandDto::OpenProjectDialog => {
            open_project_dialog(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::OpenProject { path } => {
            open_project(app, state, PathBuf::from(path))?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::OpenFile { path } => {
            run_backend_command(&app, state.inner(), |backend| {
                backend.open_file(Utf8PathBuf::from(path))
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::CloseFile { path } => {
            run_backend_command(&app, state.inner(), |backend| {
                backend.close_file(Utf8PathBuf::from(path))
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::SetActiveFile { path } => {
            run_backend_command(&app, state.inner(), |backend| {
                backend.set_active_file(Utf8PathBuf::from(path))
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::UpdateActiveText { text } => {
            run_backend_command(&app, state.inner(), |backend| {
                backend.update_active_text(text)
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::SetActiveViewMode { mode } => {
            run_backend_command(&app, state.inner(), |backend| {
                backend.set_active_view_mode(editor_view_mode(mode))
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ApplySequenceGuiEdit { edit } => {
            let edit = edit.try_into()?;
            run_backend_command(&app, state.inner(), |backend| {
                backend.apply_sequence_gui_edit(edit)
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ApplySequenceSelectionEdit { edit } => {
            let edit = edit.into();
            let result = run_backend_command_with_response(&app, state.inner(), |backend| {
                backend.apply_sequence_selection_edit(edit)
            })?;
            Ok(AppCommandResponseDto::SequenceSelectionEditResult {
                result: result.into(),
            })
        }
        AppCommandDto::ApplyLayoutGuiEdit { edit } => {
            let edit = edit.try_into()?;
            run_backend_command(&app, state.inner(), |backend| {
                backend.apply_layout_gui_edit(edit)
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ApplyFixtureGuiEdit { edit } => {
            let edit = edit.try_into()?;
            run_backend_command(&app, state.inner(), |backend| {
                backend.apply_fixture_gui_edit(edit)
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::FlushAutosave => {
            run_backend_command(&app, state.inner(), AppBackend::save_active_file)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ReloadActiveBufferFromDisk => {
            run_backend_command(
                &app,
                state.inner(),
                AppBackend::reload_active_file_from_disk,
            )?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::KeepActiveBuffer => {
            run_backend_command(&app, state.inner(), AppBackend::keep_active_file)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::CreateFile { parent, name } => {
            run_backend_command(&app, state.inner(), |backend| {
                backend.create_file(Utf8PathBuf::from(parent), name)
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::CreateDirectory { parent, name } => {
            run_backend_command(&app, state.inner(), |backend| {
                backend.create_directory(Utf8PathBuf::from(parent), name)
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::RenamePath { path, new_name } => {
            run_backend_command(&app, state.inner(), |backend| {
                backend.rename_path(Utf8PathBuf::from(path), new_name)
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::DeletePath { path } => {
            run_backend_command(&app, state.inner(), |backend| {
                backend.delete_path(Utf8PathBuf::from(path))
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ChooseNewProjectParentDirectory
        | AppCommandDto::CreateNewProject { .. }
        | AppCommandDto::ChooseSequenceAudio
        | AppCommandDto::ClearSequenceAudio
        | AppCommandDto::ExportActiveSequenceFseq { .. }
        | AppCommandDto::ReloadProject
        | AppCommandDto::ToggleProjectTree
        | AppCommandDto::SetEffectPreviewEnabled { .. }
        | AppCommandDto::SetEffectPreviewEffects { .. }
        | AppCommandDto::OpenPreviewWindow
        | AppCommandDto::PreviewPlay
        | AppCommandDto::PreviewPause
        | AppCommandDto::PreviewStop
        | AppCommandDto::PreviewRewindToZero
        | AppCommandDto::PreviewSeek { .. }
        | AppCommandDto::SetLiveOutputEnabled { .. } => {
            Err("this desktop command has not been rebuilt yet".to_string())
        }
    }
}

fn open_project_dialog(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Open Dawn Project")
        .pick_folder()
    else {
        return Ok(());
    };
    open_project(app, state, path)
}

fn open_project(app: AppHandle, state: State<'_, AppState>, path: PathBuf) -> CommandResult<()> {
    run_backend_command(&app, state.inner(), |backend| backend.open_project(path))
}

fn run_backend_command(
    app: &AppHandle,
    state: &AppState,
    command: impl FnOnce(&mut AppBackend) -> BackendResult<AppUpdate>,
) -> CommandResult<()> {
    let backend = state.backend();
    let update = {
        let mut backend = state.lock_backend()?;
        command(&mut backend).map_err(|error| error.to_string())?
    };
    jobs::handle_backend_update(app, backend, update)
}

fn run_backend_command_with_response<T>(
    app: &AppHandle,
    state: &AppState,
    command: impl FnOnce(&mut AppBackend) -> BackendResult<(AppUpdate, T)>,
) -> CommandResult<T> {
    let backend = state.backend();
    let (update, response) = {
        let mut backend = state.lock_backend()?;
        command(&mut backend).map_err(|error| error.to_string())?
    };
    jobs::handle_backend_update(app, backend, update)?;
    Ok(response)
}

fn editor_view_mode(mode: EditorViewModeDto) -> EditorViewMode {
    match mode {
        EditorViewModeDto::Text => EditorViewMode::Text,
        EditorViewModeDto::Gui => EditorViewMode::Gui,
    }
}

#[specta::specta]
#[tauri::command]
pub(crate) fn request_sequence_effect_previews(
    _path: String,
    _object_key: String,
    _request_id: u32,
    _effects: Vec<SequenceEffectPreviewRequestEffectDto>,
) -> CommandResult<()> {
    Err("sequence effect previews have not been rebuilt yet".to_string())
}

#[specta::specta]
#[tauri::command]
pub(crate) fn take_sequence_effect_preview_results(
    _path: String,
    _object_key: String,
) -> CommandResult<SequenceEffectPreviewResultsDto> {
    Ok(SequenceEffectPreviewResultsDto {
        results: Vec::new(),
    })
}

#[specta::specta]
#[tauri::command]
pub(crate) fn get_preview_scene() -> CommandResult<PreviewSceneDto> {
    Ok(PreviewSceneDto::default())
}

#[specta::specta]
#[tauri::command]
pub(crate) fn init_preview_transport(app: AppHandle) -> CommandResult<()> {
    events::emit_backend_error(
        &app,
        "preview transport has not been rebuilt yet".to_string(),
    );
    Ok(())
}

#[specta::specta]
#[tauri::command]
pub(crate) fn dispose_preview_transport() -> CommandResult<()> {
    Ok(())
}

#[specta::specta]
#[tauri::command]
pub(crate) fn get_preview_transport_mode() -> CommandResult<PreviewTransportMode> {
    Ok(PreviewTransportMode::Unsupported)
}

pub(crate) fn register_commands(
    builder: tauri_specta::Builder<tauri::Wry>,
) -> tauri_specta::Builder<tauri::Wry> {
    builder.commands(tauri_specta::collect_commands![
        get_app_snapshot,
        dispatch_app_command,
        request_sequence_effect_previews,
        take_sequence_effect_preview_results,
        get_preview_scene,
        init_preview_transport,
        dispose_preview_transport,
        get_preview_transport_mode
    ])
}
