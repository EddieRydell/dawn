use std::path::PathBuf;

// Thin Tauri adapter only: convert DTOs, call backend, emit updates.
use dawn_backend::{
    AppBackend, AppUpdate, BackendResult, EditorViewMode, RenderEffectPreviewRequestEffect,
    SequenceEffectPreviewResult, SequenceEffectPreviewResultBatch,
};
use dawn_language::path::Utf8PathBuf;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::{
    dto::{
        AppCommandDto, AppCommandResponseDto, AppSnapshotDto, EditorViewModeDto, PreviewSceneDto,
        PreviewTransportMode, SequenceEffectPreviewRequestEffectDto,
        SequenceEffectPreviewResultsDto,
    },
    jobs,
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
        AppCommandDto::ApplyActiveDocumentEdit { edit } => {
            let edit = edit.try_into()?;
            run_backend_command(&app, state.inner(), |backend| {
                backend.apply_active_document_edit(edit)
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
        AppCommandDto::ChooseSequenceAudio => {
            choose_sequence_audio(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ClearSequenceAudio => {
            state.audio().clear();
            run_backend_command(&app, state.inner(), AppBackend::clear_active_sequence_audio)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::SetEffectPreviewEnabled { enabled } => {
            state.audio().clear();
            run_backend_command(&app, state.inner(), |backend| {
                backend.set_effect_preview_enabled(enabled)
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::SetEffectPreviewEffects { ids } => {
            run_backend_command(&app, state.inner(), |backend| {
                backend.set_effect_preview_effects(ids)
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::OpenPreviewWindow => {
            open_preview_window(app)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::PreviewPlay => {
            preview_play(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::PreviewPause => {
            let clock = state.audio().pause()?;
            run_backend_command(&app, state.inner(), |backend| {
                if backend.preview_snapshot().audio.is_some() {
                    backend.preview_apply_audio_clock(clock)
                } else {
                    backend.preview_pause()
                }
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::PreviewStop => {
            let home_seconds = state.lock_backend()?.preview_snapshot().home_seconds;
            let clock = state.audio().stop(home_seconds)?;
            run_backend_command(&app, state.inner(), |backend| {
                if backend.preview_snapshot().audio.is_some() {
                    backend.preview_apply_audio_clock(clock)
                } else {
                    backend.preview_stop()
                }
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::PreviewRewindToZero => {
            let clock = state.audio().stop(0.0)?;
            run_backend_command(&app, state.inner(), |backend| {
                if backend.preview_snapshot().audio.is_some() {
                    backend.preview_apply_audio_clock(clock)
                } else {
                    backend.preview_rewind_to_zero()
                }
            })?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::PreviewSeek { position_seconds } => {
            let snapshot = state.lock_backend()?.preview_snapshot();
            if let Some(audio) = snapshot.audio.filter(|audio| audio.exists) {
                let clock = state
                    .audio()
                    .seek(&audio, position_seconds, snapshot.is_playing)?;
                run_backend_command(&app, state.inner(), |backend| {
                    backend.preview_apply_audio_clock(clock)
                })?;
            } else {
                run_backend_command(&app, state.inner(), |backend| {
                    backend.preview_seek(position_seconds)
                })?;
            }
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ChooseNewProjectParentDirectory
        | AppCommandDto::CreateNewProject { .. }
        | AppCommandDto::ExportActiveSequenceFseq { .. }
        | AppCommandDto::ReloadProject
        | AppCommandDto::ToggleProjectTree
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

fn choose_sequence_audio(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let dialog = {
        let backend = state.lock_backend()?;
        backend
            .active_sequence_audio_dialog()
            .map_err(|error| error.to_string())?
    };
    let Some(path) = rfd::FileDialog::new()
        .set_title("Choose Sequence Audio")
        .set_directory(dialog.audio_directory)
        .pick_file()
    else {
        return Ok(());
    };
    run_backend_command(&app, state.inner(), |backend| {
        backend.set_active_sequence_audio(path)
    })
}

fn preview_play(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let (update, snapshot) = {
        let mut backend = state.lock_backend()?;
        backend
            .prepare_preview_play()
            .map_err(|error| error.to_string())?
    };
    jobs::handle_backend_update(&app, state.backend(), update)?;
    if let Some(audio) = snapshot.audio.filter(|audio| audio.exists) {
        let clock = state.audio().play(&audio, snapshot.position_seconds)?;
        run_backend_command(&app, state.inner(), |backend| {
            backend.preview_apply_audio_clock(clock)
        })
    } else {
        state.audio().clear();
        run_backend_command(&app, state.inner(), AppBackend::preview_play_silent)
    }
}

fn open_preview_window(app: AppHandle) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window("preview") {
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    WebviewWindowBuilder::new(
        &app,
        "preview",
        WebviewUrl::App("index.html?view=preview".into()),
    )
    .title("Dawn Preview")
    .inner_size(960.0, 720.0)
    .build()
    .map(|_| ())
    .map_err(|error| error.to_string())
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

fn editor_view_mode(mode: EditorViewModeDto) -> EditorViewMode {
    match mode {
        EditorViewModeDto::Text => EditorViewMode::Text,
        EditorViewModeDto::Gui => EditorViewMode::Gui,
    }
}

#[specta::specta]
#[tauri::command]
pub(crate) fn request_sequence_effect_previews(
    state: State<'_, AppState>,
    path: String,
    object_key: String,
    request_id: u32,
    effects: Vec<SequenceEffectPreviewRequestEffectDto>,
) -> CommandResult<()> {
    let effects = effects
        .into_iter()
        .map(|effect| RenderEffectPreviewRequestEffect {
            effect_id: effect.effect_id,
            signature: effect.signature,
        })
        .collect();
    let request = {
        let backend = state.lock_backend()?;
        backend
            .request_sequence_effect_previews(
                Utf8PathBuf::from(path),
                object_key,
                request_id,
                effects,
            )
            .map_err(|error| error.to_string())?
    };
    state.effect_preview().submit(request)?;
    Ok(())
}

#[specta::specta]
#[tauri::command]
pub(crate) fn take_sequence_effect_preview_results(
    state: State<'_, AppState>,
    path: String,
    object_key: String,
) -> CommandResult<SequenceEffectPreviewResultsDto> {
    let path = Utf8PathBuf::from(path);
    {
        let backend = state.lock_backend()?;
        backend
            .validate_sequence_effect_preview_key(&path, &object_key)
            .map_err(|error| error.to_string())?
    }
    let batch = state.effect_preview().take_results(&path, &object_key);
    Ok(sequence_effect_preview_results_dto(batch))
}

fn sequence_effect_preview_results_dto(
    batch: SequenceEffectPreviewResultBatch,
) -> SequenceEffectPreviewResultsDto {
    SequenceEffectPreviewResultsDto {
        results: batch
            .results
            .into_iter()
            .map(|result| sequence_effect_preview_result_dto(batch.request_id, result))
            .collect(),
    }
}

fn sequence_effect_preview_result_dto(
    request_id: u32,
    result: SequenceEffectPreviewResult,
) -> crate::dto::SequenceEffectPreviewResultDto {
    match crate::dto::SequenceEffectPreviewResultDto::from(result) {
        crate::dto::SequenceEffectPreviewResultDto::Ready(mut result) => {
            result.request_id = request_id;
            crate::dto::SequenceEffectPreviewResultDto::Ready(result)
        }
        crate::dto::SequenceEffectPreviewResultDto::Unavailable(mut result) => {
            result.request_id = request_id;
            crate::dto::SequenceEffectPreviewResultDto::Unavailable(result)
        }
        crate::dto::SequenceEffectPreviewResultDto::Error(mut result) => {
            result.request_id = request_id;
            crate::dto::SequenceEffectPreviewResultDto::Error(result)
        }
    }
}

#[specta::specta]
#[tauri::command]
pub(crate) fn get_preview_scene(state: State<'_, AppState>) -> CommandResult<PreviewSceneDto> {
    let backend = state.lock_backend()?;
    Ok(PreviewSceneDto::from(&backend.preview_snapshot().frame))
}

#[specta::specta]
#[tauri::command]
pub(crate) fn init_preview_transport(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let window = app
        .get_webview_window("preview")
        .ok_or_else(|| "preview window is not open".to_string())?;
    let pixel_count = {
        let backend = state.lock_backend()?;
        preview_pixel_count(&backend.preview_snapshot().frame)
    };
    state
        .lock_preview_transport()?
        .init_window(&window, pixel_count)
}

#[specta::specta]
#[tauri::command]
pub(crate) fn dispose_preview_transport(state: State<'_, AppState>) -> CommandResult<()> {
    state.lock_preview_transport()?.dispose_window("preview");
    Ok(())
}

#[specta::specta]
#[tauri::command]
pub(crate) fn get_preview_transport_mode() -> CommandResult<PreviewTransportMode> {
    Ok(
        match crate::preview_transport::PreviewTransportRuntime::mode() {
            crate::preview_transport::PreviewTransportMode::Webview2Shared => {
                PreviewTransportMode::Webview2Shared
            }
            crate::preview_transport::PreviewTransportMode::Unsupported => {
                PreviewTransportMode::Unsupported
            }
        },
    )
}

fn preview_pixel_count(frame: &dawn_backend::RenderedFrame) -> usize {
    frame
        .fixtures
        .iter()
        .map(|fixture| fixture.pixels.len())
        .sum()
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
