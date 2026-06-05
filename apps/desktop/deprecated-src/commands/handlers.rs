use std::path::PathBuf;

use deprecated_dawn_backend::{AppView, SequenceEffectPreviewKey, SequenceEffectPreviewRequest};
use tauri::{AppHandle, Manager, State};

use crate::app::backend::emit_app_snapshot;
use crate::app::state::{
    lock_backend, lock_preview_transport, project_path, AppState, CommandResult,
};
use crate::dto::{
    AppCommandDto, AppCommandResponseDto, AppSnapshotDto, EditorViewModeDto, FixtureGuiEditDto,
    LayoutGuiEditDto, SequenceGuiEditDto, SequenceSelectionEditDto, SequenceSelectionEditResultDto,
};
use crate::preview::effect_previews::{
    sequence_effect_preview_result_dto, SequenceEffectPreviewRequestEffectDto,
    SequenceEffectPreviewResultsDto,
};
use crate::preview::transport::{PreviewTransportMode, PreviewTransportRuntime};
use crate::preview::{
    open_or_focus_preview_window, preview_pixel_count, preview_scene_from_frame, PreviewSceneDto,
};

#[specta::specta]
#[tauri::command]
fn build_app_snapshot(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    hydrate_startup_session(&state)?;
    let backend = lock_backend(&state)?;
    Ok(AppSnapshotDto::from(backend.view()))
}

#[specta::specta]
#[tauri::command]
fn get_app_snapshot(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    build_app_snapshot(state)
}

#[specta::specta]
#[tauri::command]
async fn dispatch_app_command(
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
            open_project(app, state, path)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ChooseNewProjectParentDirectory => {
            Ok(AppCommandResponseDto::OptionalString {
                value: choose_new_project_parent_directory()?,
            })
        }
        AppCommandDto::CreateNewProject {
            parent_path,
            directory_name,
        } => {
            create_new_project(app, state, parent_path, directory_name)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::OpenFile { path } => {
            open_file(app, state, path)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::CloseFile { path } => {
            close_file(app, state, path)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::SetActiveFile { path } => {
            set_active_file(app, state, path)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::UpdateActiveText { text } => {
            update_active_text(app, state, text)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::SetActiveViewMode { mode } => {
            set_active_view_mode(app, state, mode)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::UndoActiveEdit => {
            undo_active_edit(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::RedoActiveEdit => {
            redo_active_edit(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ApplySequenceGuiEdit { edit } => {
            apply_sequence_gui_edit(app, state, edit)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ApplySequenceSelectionEdit { edit } => {
            Ok(AppCommandResponseDto::SequenceSelectionEditResult {
                result: apply_sequence_selection_edit(app, state, edit)?,
            })
        }
        AppCommandDto::ChooseSequenceAudio => {
            choose_sequence_audio(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ClearSequenceAudio => {
            clear_sequence_audio(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ExportActiveSequenceFseq { step_ms } => {
            export_active_sequence_fseq(app, state, step_ms)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ApplyLayoutGuiEdit { edit } => {
            apply_layout_gui_edit(app, state, edit)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ApplyFixtureGuiEdit { edit } => {
            apply_fixture_gui_edit(app, state, edit)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::FlushAutosave => {
            flush_autosave(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ReloadActiveBufferFromDisk => {
            reload_active_buffer_from_disk(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::KeepActiveBuffer => {
            keep_active_buffer(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::CreateFile { parent, name } => {
            create_file(app, state, parent, name)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::CreateDirectory { parent, name } => {
            create_directory(app, state, parent, name)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::RenamePath { path, new_name } => {
            rename_path(app, state, path, new_name)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::DeletePath { path } => {
            delete_path(app, state, path)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ReloadProject => {
            reload_project(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::ToggleProjectTree => {
            toggle_project_tree(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::SetEffectPreviewEnabled { enabled } => {
            set_effect_preview_enabled(app, state, enabled)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::SetEffectPreviewEffects { ids } => {
            set_effect_preview_effects(app, state, ids)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::OpenPreviewWindow => {
            open_preview_window(app, state).await?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::PreviewPlay => {
            preview_play(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::PreviewPause => {
            preview_pause(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::PreviewStop => {
            preview_stop(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::PreviewRewindToZero => {
            preview_rewind_to_zero(app, state)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::PreviewSeek { position_seconds } => {
            preview_seek(app, state, position_seconds)?;
            Ok(AppCommandResponseDto::None)
        }
        AppCommandDto::SetLiveOutputEnabled { enabled } => {
            set_live_output_enabled(app, state, enabled)?;
            Ok(AppCommandResponseDto::None)
        }
    }
}

fn emit_backend_update(app: &AppHandle, view: AppView) -> CommandResult<()> {
    emit_app_snapshot(app, AppSnapshotDto::from(view))?;
    Ok(())
}

fn hydrate_startup_session(state: &State<'_, AppState>) -> CommandResult<()> {
    if state.mark_startup_hydrated() {
        return Ok(());
    }
    lock_backend(state)?
        .restore_last_project()
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[specta::specta]
#[tauri::command]
fn open_project_dialog(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Open Dawn Project")
        .pick_folder()
    else {
        return Ok(());
    };
    open_project_backend_then_emit(&app, &state, path)
}

#[specta::specta]
#[tauri::command]
fn open_project(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    open_project_backend_then_emit(&app, &state, PathBuf::from(path))
}

#[specta::specta]
#[tauri::command]
fn choose_new_project_parent_directory() -> CommandResult<Option<String>> {
    Ok(rfd::FileDialog::new()
        .set_title("Choose New Project Location")
        .pick_folder()
        .map(|path| path.to_string_lossy().replace('\\', "/")))
}

#[specta::specta]
#[tauri::command]
fn create_new_project(
    app: AppHandle,
    state: State<'_, AppState>,
    parent_path: String,
    directory_name: String,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .create_new_project(PathBuf::from(parent_path), directory_name)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

fn open_project_backend_then_emit(
    app: &AppHandle,
    state: &State<'_, AppState>,
    path: PathBuf,
) -> CommandResult<()> {
    let view = lock_backend(state)?
        .open_project(path)
        .map_err(|error| error.to_string())?;
    emit_backend_update(app, view)
}

#[specta::specta]
#[tauri::command]
fn open_file(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .open_file(project_path(path))
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn close_file(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .close_file(project_path(path))
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn set_active_file(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .set_active_file(project_path(path))
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn update_active_text(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .update_active_text(text)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn set_active_view_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: EditorViewModeDto,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .set_active_view_mode(mode.into())
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn undo_active_edit(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .undo_active_edit()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn redo_active_edit(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .redo_active_edit()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn apply_sequence_gui_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: SequenceGuiEditDto,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .apply_sequence_gui_edit(edit.into())
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn apply_sequence_selection_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: SequenceSelectionEditDto,
) -> CommandResult<SequenceSelectionEditResultDto> {
    let output = lock_backend(&state)?
        .apply_sequence_selection_edit(edit.into())
        .map_err(|error| error.to_string())?;
    emit_app_snapshot(&app, AppSnapshotDto::from(output.view))?;
    Ok(output.value.into())
}

#[specta::specta]
#[tauri::command]
fn choose_sequence_audio(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let dialog = lock_backend(&state)?
        .active_sequence_audio_dialog()
        .map_err(|error| error.to_string())?;
    let mut picker = rfd::FileDialog::new()
        .set_title("Choose Sequence Audio")
        .add_filter("Audio", &["mp3", "wav", "flac", "m4a", "aac", "ogg"]);
    if dialog.audio_directory.is_dir() {
        picker = picker.set_directory(&dialog.audio_directory);
    }
    let Some(path) = picker.pick_file() else {
        return Ok(());
    };
    let view = lock_backend(&state)?
        .set_active_sequence_audio(path)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn clear_sequence_audio(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .clear_active_sequence_audio()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn export_active_sequence_fseq(
    app: AppHandle,
    state: State<'_, AppState>,
    step_ms: u8,
) -> CommandResult<()> {
    let default_name = lock_backend(&state)?
        .active_sequence_fseq_default_name()
        .map_err(|error| error.to_string())?;
    let Some(output_path) = rfd::FileDialog::new()
        .set_title("Export FSEQ")
        .set_file_name(&default_name)
        .add_filter("FSEQ", &["fseq"])
        .save_file()
    else {
        return Ok(());
    };
    let view = lock_backend(&state)?
        .export_active_sequence_fseq(output_path, step_ms)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn request_sequence_effect_previews(
    state: State<'_, AppState>,
    path: String,
    object_key: String,
    request_id: u32,
    effects: Vec<SequenceEffectPreviewRequestEffectDto>,
) -> CommandResult<()> {
    let effects = effects.into_iter().map(Into::into).collect::<Vec<_>>();
    lock_backend(&state)?
        .request_sequence_effect_previews(SequenceEffectPreviewRequest {
            path: project_path(path),
            object_key,
            request_id,
            effects,
        })
        .map_err(|error| error.to_string())
}

#[specta::specta]
#[tauri::command]
fn take_sequence_effect_preview_results(
    state: State<'_, AppState>,
    path: String,
    object_key: String,
) -> CommandResult<SequenceEffectPreviewResultsDto> {
    let results = lock_backend(&state)?
        .take_sequence_effect_preview_results(SequenceEffectPreviewKey { path, object_key })
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(sequence_effect_preview_result_dto)
        .collect();
    Ok(SequenceEffectPreviewResultsDto { results })
}

#[specta::specta]
#[tauri::command]
fn apply_layout_gui_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: LayoutGuiEditDto,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .apply_layout_gui_edit(edit.try_into().map_err(str::to_string)?)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn apply_fixture_gui_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: FixtureGuiEditDto,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .apply_fixture_gui_edit(edit.try_into().map_err(str::to_string)?)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn flush_autosave(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .flush_autosave()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn reload_active_buffer_from_disk(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .reload_active_buffer_from_disk()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn keep_active_buffer(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .keep_active_buffer()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn create_file(
    app: AppHandle,
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .create_file(project_path(parent), name)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn create_directory(
    app: AppHandle,
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .create_directory(project_path(parent), name)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn rename_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    new_name: String,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .rename_path(project_path(path), new_name)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn delete_path(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .delete_path(project_path(path))
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn reload_project(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .reload_project()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn toggle_project_tree(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .toggle_project_tree()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn set_effect_preview_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .set_effect_preview_enabled(enabled)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn set_effect_preview_effects(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<u32>,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .set_effect_preview_effects(ids)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
async fn open_preview_window(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    open_or_focus_preview_window(app, state)
}

#[specta::specta]
#[tauri::command]
fn preview_play(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .preview_play()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn preview_pause(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .preview_pause()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn preview_stop(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .preview_stop()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn preview_rewind_to_zero(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .preview_rewind_to_zero()
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn preview_seek(
    app: AppHandle,
    state: State<'_, AppState>,
    position_seconds: f64,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .preview_seek(position_seconds)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn set_live_output_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<()> {
    let view = lock_backend(&state)?
        .set_live_output_enabled(enabled)
        .map_err(|error| error.to_string())?;
    emit_backend_update(&app, view)
}

#[specta::specta]
#[tauri::command]
fn get_preview_scene(state: State<'_, AppState>) -> CommandResult<PreviewSceneDto> {
    let snapshot = lock_backend(&state)?.view().preview;
    Ok(preview_scene_from_frame(
        &snapshot.frame,
        snapshot.source_label,
    ))
}

#[specta::specta]
#[tauri::command]
fn get_preview_transport_mode() -> CommandResult<PreviewTransportMode> {
    Ok(PreviewTransportRuntime::mode())
}

#[specta::specta]
#[tauri::command]
fn init_preview_transport(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let Some(window) = app.get_webview_window("preview") else {
        return Err("preview window is not open".to_string());
    };
    let pixel_count = preview_pixel_count(&lock_backend(&state)?.view().preview.frame);
    lock_preview_transport(&state)?.init_window(&window, pixel_count)
}

#[specta::specta]
#[tauri::command]
fn dispose_preview_transport(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let label = app
        .get_webview_window("preview")
        .map(|window| window.label().to_string())
        .unwrap_or_else(|| "preview".to_string());
    lock_preview_transport(&state)?.dispose_window(&label);
    Ok(())
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
