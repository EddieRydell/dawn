use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::{
    dto::{
        AppCommandDto, AppCommandResponseDto, AppSnapshotDto, PreviewSceneDto,
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
        AppCommandDto::ChooseNewProjectParentDirectory
        | AppCommandDto::CreateNewProject { .. }
        | AppCommandDto::OpenFile { .. }
        | AppCommandDto::CloseFile { .. }
        | AppCommandDto::SetActiveFile { .. }
        | AppCommandDto::UpdateActiveText { .. }
        | AppCommandDto::SetActiveViewMode { .. }
        | AppCommandDto::UndoActiveEdit
        | AppCommandDto::RedoActiveEdit
        | AppCommandDto::ApplySequenceGuiEdit { .. }
        | AppCommandDto::ApplySequenceSelectionEdit { .. }
        | AppCommandDto::ChooseSequenceAudio
        | AppCommandDto::ClearSequenceAudio
        | AppCommandDto::ExportActiveSequenceFseq { .. }
        | AppCommandDto::ApplyLayoutGuiEdit { .. }
        | AppCommandDto::ApplyFixtureGuiEdit { .. }
        | AppCommandDto::FlushAutosave
        | AppCommandDto::ReloadActiveBufferFromDisk
        | AppCommandDto::KeepActiveBuffer
        | AppCommandDto::CreateFile { .. }
        | AppCommandDto::CreateDirectory { .. }
        | AppCommandDto::RenamePath { .. }
        | AppCommandDto::DeletePath { .. }
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
    let backend = state.backend();
    let update = {
        let mut backend = state.lock_backend()?;
        backend
            .open_project(path)
            .map_err(|error| error.to_string())?
    };
    jobs::handle_backend_update(&app, backend, update)
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
