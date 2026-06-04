use std::path::{Path, PathBuf};

use dawn_app_runtime::app_model::{load_project_workspace, project_root_label_for_path};
use dawn_app_runtime::contracts::DiskVersion;
use dawn_app_runtime::dto::{
    AppCommandDto, AppCommandResponseDto, AppSnapshotDto, EditorViewModeDto, FixtureGuiEditDto,
    LayoutGuiEditDto, SequenceGuiEditDto, SequenceSelectionEditDto, SequenceSelectionEditResultDto,
};
use dawn_app_runtime::fseq_export::{export_fseq_file, FseqExportOptions};
use dawn_app_runtime::services::document_store::ViewMode;
use dawn_app_runtime::services::editor_state::{EditorViewMode, FileVersion};
use dawn_language::path::{serialized_import_path, utf8_path, Utf8PathBuf};
use tauri::{AppHandle, Manager, State};

use crate::app_runtime::{
    emit_runtime_read_models, preload_active_preview_audio, update_preview_from_audio_status,
    valid_preview_audio,
};
use crate::effect_previews::{
    SequenceEffectPreviewRequestEffectDto, SequenceEffectPreviewResultsDto,
};
use crate::new_project::{create_starter_project, STARTER_SEQUENCE_PATH};
use crate::preview::{
    open_or_focus_preview_window, preview_pixel_count, preview_scene_from_frame, PreviewSceneDto,
};
use crate::preview_transport::{PreviewTransportMode, PreviewTransportRuntime};
use crate::runtime_host::BufferTextEdit;
use crate::state::{
    lock_audio_runtime, lock_effect_preview_runtime, lock_live_output, lock_preview_transport,
    lock_runtime, project_path, AppState, CommandResult,
};

#[specta::specta]
#[tauri::command]
fn build_app_snapshot(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    hydrate_startup_session(&state)?;
    let live_output = lock_live_output(&state)?.snapshot();
    let mut runtime = lock_runtime(&state)?;
    runtime.sync_live_output_readout(live_output);
    let read_models = runtime.app_snapshot();
    preload_active_preview_audio(&state, &read_models.preview.preview);
    Ok(read_models)
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

fn emit_runtime_update(
    app: &AppHandle,
    state: &State<'_, AppState>,
    read_models: AppSnapshotDto,
) -> CommandResult<()> {
    let read_models = emit_runtime_read_models(app, read_models)?;
    preload_active_preview_audio(state, &read_models.preview.preview);
    Ok(())
}

fn hydrate_startup_session(state: &State<'_, AppState>) -> CommandResult<()> {
    if state.mark_startup_hydrated() {
        return Ok(());
    }

    let Some(path) = ({
        let model = lock_runtime(state)?;
        model.last_project_root()
    }) else {
        return Ok(());
    };

    let workspace = match load_project_workspace(&path) {
        Ok(workspace) => workspace,
        Err(error) => {
            lock_runtime(state)?
                .runtime_model_mut()
                .set_status(format!("Could not restore last project: {error}"));
            return Ok(());
        }
    };
    let Some(root) = workspace.project_root_display().map(ToString::to_string) else {
        lock_runtime(state)?
            .runtime_model_mut()
            .set_status("Could not restore last project: project root was not opened");
        return Ok(());
    };

    lock_runtime(state)?.open_project(root)?;
    {
        let mut runtime = lock_runtime(state)?;
        runtime
            .runtime_model_mut()
            .sync_project_opened(path, false, "Project restored")?;
    }
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
    open_project_runtime_then_model(&app, &state, path)
}

#[specta::specta]
#[tauri::command]
fn open_project(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    open_project_runtime_then_model(&app, &state, PathBuf::from(path))
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
    let target = create_starter_project(&parent_path, &directory_name)?;
    open_project_runtime_then_model(&app, &state, target)?;
    open_file_runtime_then_model(
        &app,
        &state,
        project_path(STARTER_SEQUENCE_PATH.to_string()),
    )?;
    set_active_view_mode_runtime_then_model(&app, &state, EditorViewModeDto::Gui)
}

#[specta::specta]
#[tauri::command]
fn open_file(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    open_file_runtime_then_model(&app, &state, project_path(path))
}

fn open_project_runtime_then_model(
    app: &AppHandle,
    state: &State<'_, AppState>,
    path: PathBuf,
) -> CommandResult<()> {
    lock_runtime(state)?
        .runtime_model_mut()
        .prepare_for_runtime_project_open()?;
    let root = project_root_display_for_open_path(&path)?;
    lock_runtime(state)?.open_project(root)?;
    let read_models = {
        let mut runtime = lock_runtime(state)?;
        runtime
            .runtime_model_mut()
            .sync_project_opened(path.clone(), true, "Project opened")?;
        runtime.remember_project_root(path)?;
        runtime.app_snapshot()
    };
    let read_models = emit_runtime_read_models(app, read_models)?;
    if let Ok(mut watcher) = crate::state::lock_filesystem_watcher(state) {
        let _ = watcher.sync_project_root(app, read_models.workspace.project_root.clone());
    }
    if let Ok(runtime) = lock_audio_runtime(state) {
        runtime.clear();
    }
    preload_active_preview_audio(state, &read_models.preview.preview);
    Ok(())
}

fn project_root_display_for_open_path(path: &Path) -> CommandResult<String> {
    project_root_label_for_path(path)
}

fn open_file_runtime_then_model(
    app: &AppHandle,
    state: &State<'_, AppState>,
    path: Utf8PathBuf,
) -> CommandResult<()> {
    let (text, disk_version) = {
        let model = lock_runtime(state)?;
        let model = model.runtime_model();
        model.read_file_with_version(path.clone())?
    };
    lock_runtime(state)?.open_buffer(
        path.clone(),
        text.clone(),
        Some(runtime_disk_version(&disk_version)),
    )?;
    let snapshot = {
        let mut runtime = lock_runtime(state)?;
        runtime.runtime_model_mut().sync_file_opened(
            path,
            text,
            disk_version,
            EditorViewMode::Text,
        )?;
        runtime.app_snapshot()
    };
    let read_models = emit_runtime_read_models(app, snapshot)?;
    preload_active_preview_audio(state, &read_models.preview.preview);
    Ok(())
}

#[specta::specta]
#[tauri::command]
fn close_file(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let path = project_path(path);
    lock_runtime(&state)?.close_buffer(path.clone())?;
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.runtime_model_mut().sync_file_closed(path)?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn set_active_file(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let path = project_path(path);
    lock_runtime(&state)?.set_active_buffer(path.clone())?;
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.runtime_model_mut().sync_active_file(path)?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn update_active_text(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> CommandResult<()> {
    let active_buffer = {
        let runtime = lock_runtime(&state)?;
        let snapshot =
            runtime
                .runtime_model()
                .snapshot(false, false, runtime.live_output_readout());
        let Some(buffer) = snapshot.active_buffer else {
            return Ok(());
        };
        let conflicted = buffer.is_conflicted();
        BufferTextEdit {
            project_root: snapshot.project_root,
            path: buffer.path,
            conflicted,
            text: buffer.text,
        }
    };

    lock_runtime(&state)?.update_active_text(active_buffer, text.clone())?;
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.runtime_model_mut().sync_active_text_update(text)?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn set_active_view_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: EditorViewModeDto,
) -> CommandResult<()> {
    set_active_view_mode_runtime_then_model(&app, &state, mode)
}

fn set_active_view_mode_runtime_then_model(
    app: &AppHandle,
    state: &State<'_, AppState>,
    mode: EditorViewModeDto,
) -> CommandResult<()> {
    let active_file = {
        let runtime = lock_runtime(state)?;
        let snapshot =
            runtime
                .runtime_model()
                .snapshot(false, false, runtime.live_output_readout());
        let Some(active_file) = snapshot.active_file else {
            return Ok(());
        };
        active_file
    };
    lock_runtime(state)?.set_view_mode(active_file, runtime_view_mode(&mode))?;
    let snapshot = {
        let mut runtime = lock_runtime(state)?;
        runtime
            .runtime_model_mut()
            .sync_active_view_mode(editor_view_mode(&mode))?;
        runtime.app_snapshot()
    };
    emit_runtime_update(app, state, snapshot)
}

fn runtime_view_mode(mode: &EditorViewModeDto) -> ViewMode {
    match mode {
        EditorViewModeDto::Text => ViewMode::Text,
        EditorViewModeDto::Gui => ViewMode::Gui,
    }
}

fn editor_view_mode(mode: &EditorViewModeDto) -> EditorViewMode {
    match mode {
        EditorViewModeDto::Text => EditorViewMode::Text,
        EditorViewModeDto::Gui => EditorViewMode::Gui,
    }
}

fn runtime_disk_version(version: &FileVersion) -> DiskVersion {
    DiskVersion {
        len: version.len,
        modified_millis: version.modified_millis,
        content_hash: version.content_hash,
    }
}

#[specta::specta]
#[tauri::command]
fn undo_active_edit(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let path = {
        let runtime = lock_runtime(&state)?;
        let snapshot =
            runtime
                .runtime_model()
                .snapshot(false, false, runtime.live_output_readout());
        let Some(path) = snapshot.active_file else {
            return Ok(());
        };
        path
    };
    let Some(text) = lock_runtime(&state)?.undo_buffer_text(path)? else {
        return Ok(());
    };
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .sync_active_history_text(text, "Undo");
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn redo_active_edit(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let path = {
        let runtime = lock_runtime(&state)?;
        let snapshot =
            runtime
                .runtime_model()
                .snapshot(false, false, runtime.live_output_readout());
        let Some(path) = snapshot.active_file else {
            return Ok(());
        };
        path
    };
    let Some(text) = lock_runtime(&state)?.redo_buffer_text(path)? else {
        return Ok(());
    };
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .sync_active_history_text(text, "Redo");
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn apply_sequence_gui_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: SequenceGuiEditDto,
) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .apply_sequence_gui_edit_and_autosave(edit)?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn apply_sequence_selection_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: SequenceSelectionEditDto,
) -> CommandResult<SequenceSelectionEditResultDto> {
    let (result, snapshot) = {
        let mut runtime = lock_runtime(&state)?;
        let result = runtime.apply_sequence_selection_edit(edit)?;
        (result, runtime.app_snapshot())
    };
    emit_runtime_read_models(&app, snapshot)?;
    Ok(result)
}

#[specta::specta]
#[tauri::command]
fn choose_sequence_audio(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let (project_root, sequence_path) = lock_runtime(&state)?
        .runtime_model()
        .active_sequence_audio_context()?;

    let Some(project_root) = project_root else {
        return Err("no project is open".to_string());
    };
    let project_root = Utf8PathBuf::from(project_root);
    let sequence_path = if sequence_path.is_absolute() {
        sequence_path
    } else {
        project_root.join(sequence_path)
    };

    let mut dialog = rfd::FileDialog::new()
        .set_title("Choose Sequence Audio")
        .add_filter("Audio", &["mp3", "wav", "flac", "m4a", "aac", "ogg"]);
    let audio_dir = project_root.join("audio");
    if audio_dir.is_dir() {
        dialog = dialog.set_directory(audio_dir.as_std_path());
    }

    let Some(path) = dialog.pick_file() else {
        return Ok(());
    };
    let import = serialized_import_path(&sequence_path, &utf8_path(path)?);
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .apply_sequence_gui_edit_and_autosave(SequenceGuiEditDto::SetAudio {
                import: Some(import),
            })?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn clear_sequence_audio(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .apply_sequence_gui_edit_and_autosave(SequenceGuiEditDto::SetAudio { import: None })?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn export_active_sequence_fseq(
    app: AppHandle,
    state: State<'_, AppState>,
    step_ms: u8,
) -> CommandResult<()> {
    let (analysis, document, default_name) = lock_runtime(&state)?
        .runtime_model()
        .active_sequence_export_source()?;

    let Some(output_path) = rfd::FileDialog::new()
        .set_title("Export FSEQ")
        .set_file_name(&default_name)
        .add_filter("FSEQ", &["fseq"])
        .save_file()
    else {
        return Ok(());
    };

    let report = export_fseq_file(
        &analysis,
        &document,
        &output_path,
        FseqExportOptions {
            step_ms,
            ..FseqExportOptions::default()
        },
    )
    .map_err(|error| error.to_string())?;

    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.runtime_model_mut().set_status(format!(
            "Exported FSEQ: {} frames, {} channels",
            report.frame_count, report.channel_count
        ));
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
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
    let request_path = path.clone();
    let request_object_key = object_key.clone();
    let (analysis, document) = lock_runtime(&state)?
        .runtime_model()
        .effect_preview_request_source(project_path(path), &object_key)?;

    lock_effect_preview_runtime(&state)?.request(
        request_path,
        request_object_key,
        request_id,
        effects,
        analysis,
        document,
    )
}

#[specta::specta]
#[tauri::command]
fn take_sequence_effect_preview_results(
    state: State<'_, AppState>,
    path: String,
    object_key: String,
) -> CommandResult<SequenceEffectPreviewResultsDto> {
    let results = lock_effect_preview_runtime(&state)?.take_results(path, object_key)?;
    Ok(SequenceEffectPreviewResultsDto { results })
}

#[specta::specta]
#[tauri::command]
fn apply_layout_gui_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: LayoutGuiEditDto,
) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .apply_layout_gui_edit_and_autosave(edit)?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn apply_fixture_gui_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: FixtureGuiEditDto,
) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .apply_fixture_gui_edit_and_autosave(edit)?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn flush_autosave(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.runtime_model_mut().flush_autosave_command()?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn reload_active_buffer_from_disk(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .reload_active_buffer_from_disk_command()?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn keep_active_buffer(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.runtime_model_mut().keep_active_buffer_command()?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn create_file(
    app: AppHandle,
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> CommandResult<()> {
    let created = lock_runtime(&state)?
        .runtime_model_mut()
        .create_file_for_runtime_open(project_path(parent), name)?;
    lock_runtime(&state)?.open_buffer(
        created.path.clone(),
        created.text.clone(),
        Some(runtime_disk_version(&created.disk_version)),
    )?;
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.runtime_model_mut().sync_file_opened(
            created.path,
            created.text,
            created.disk_version,
            EditorViewMode::Text,
        )?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn create_directory(
    app: AppHandle,
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .create_directory(project_path(parent), name)?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn rename_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    new_name: String,
) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .rename_path(project_path(path), new_name)?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn delete_path(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .delete_path(project_path(path))?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn reload_project(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.runtime_model_mut().reload_project()?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn toggle_project_tree(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.toggle_project_tree()?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn set_effect_preview_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.set_effect_preview_enabled(enabled)?;
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn set_effect_preview_effects(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<u32>,
) -> CommandResult<()> {
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.set_effect_preview_effects(ids);
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
async fn open_preview_window(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    open_or_focus_preview_window(app, state)
}

#[specta::specta]
#[tauri::command]
fn preview_play(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let (audio, position_seconds, effect_preview_enabled) = {
        let model = lock_runtime(&state)?;
        let snapshot = model.runtime_model().preview_snapshot();
        (
            valid_preview_audio(&snapshot),
            snapshot.position_seconds,
            model.effect_preview_enabled(),
        )
    };
    if effect_preview_enabled {
        let snapshot = {
            let mut runtime = lock_runtime(&state)?;
            runtime.set_effect_preview_enabled(false)?;
            runtime.app_snapshot()
        };
        emit_runtime_update(&app, &state, snapshot)?;
    }
    let Some(audio) = audio else {
        let snapshot = {
            let mut runtime = lock_runtime(&state)?;
            runtime.preview_play()?;
            runtime.app_snapshot()
        };
        return emit_runtime_update(&app, &state, snapshot);
    };
    let clock = lock_audio_runtime(&state)?.play(&audio, position_seconds)?;
    update_preview_from_audio_status(&app, &state, clock)?;
    Ok(())
}

#[specta::specta]
#[tauri::command]
fn preview_pause(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let has_audio = {
        let model = lock_runtime(&state)?;
        let model = model.runtime_model();
        valid_preview_audio(&model.preview_snapshot()).is_some()
    };
    if !has_audio {
        let snapshot = {
            let mut runtime = lock_runtime(&state)?;
            runtime.runtime_model_mut().preview_pause();
            runtime.app_snapshot()
        };
        return emit_runtime_update(&app, &state, snapshot);
    }
    let clock = lock_audio_runtime(&state)?.pause()?;
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .preview_pause_at_native_audio(clock.position_seconds, clock.status);
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn preview_stop(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let (has_audio, home_seconds) = {
        let model = lock_runtime(&state)?;
        let model = model.runtime_model();
        let snapshot = model.preview_snapshot();
        (
            valid_preview_audio(&snapshot).is_some(),
            snapshot.home_seconds,
        )
    };
    if !has_audio {
        let snapshot = {
            let mut runtime = lock_runtime(&state)?;
            runtime.runtime_model_mut().preview_stop();
            runtime.app_snapshot()
        };
        return emit_runtime_update(&app, &state, snapshot);
    }
    let clock = lock_audio_runtime(&state)?.stop(home_seconds)?;
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .preview_stop_native_audio(clock.status);
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn preview_rewind_to_zero(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let has_audio = {
        let model = lock_runtime(&state)?;
        let model = model.runtime_model();
        valid_preview_audio(&model.preview_snapshot()).is_some()
    };
    if !has_audio {
        let snapshot = {
            let mut runtime = lock_runtime(&state)?;
            runtime.runtime_model_mut().preview_rewind_to_zero();
            runtime.app_snapshot()
        };
        return emit_runtime_update(&app, &state, snapshot);
    }
    let clock = lock_audio_runtime(&state)?.stop(0.0)?;
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime
            .runtime_model_mut()
            .preview_rewind_native_audio(clock.status);
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn preview_seek(
    app: AppHandle,
    state: State<'_, AppState>,
    position_seconds: f64,
) -> CommandResult<()> {
    if !position_seconds.is_finite() || position_seconds < 0.0 {
        return Err("preview seek seconds must be finite and non-negative".to_string());
    }
    let (audio, playing) = {
        let model = lock_runtime(&state)?;
        let model = model.runtime_model();
        let snapshot = model.preview_snapshot();
        (valid_preview_audio(&snapshot), snapshot.is_playing)
    };
    let Some(audio) = audio else {
        let snapshot = {
            let mut runtime = lock_runtime(&state)?;
            runtime.runtime_model_mut().preview_seek(position_seconds);
            runtime.app_snapshot()
        };
        return emit_runtime_update(&app, &state, snapshot);
    };
    let clock = lock_audio_runtime(&state)?.seek(&audio, position_seconds, playing)?;
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.runtime_model_mut().preview_seek_native_audio(
            clock.position_seconds,
            playing,
            clock.status,
        );
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn set_live_output_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<()> {
    let analysis = lock_runtime(&state)?.runtime_model().current_analysis();
    let snapshot = lock_live_output(&state)?.set_enabled(enabled, analysis.as_ref());
    let snapshot = {
        let mut runtime = lock_runtime(&state)?;
        runtime.sync_live_output_readout(snapshot);
        runtime.app_snapshot()
    };
    emit_runtime_update(&app, &state, snapshot)
}

#[specta::specta]
#[tauri::command]
fn get_preview_scene(state: State<'_, AppState>) -> CommandResult<PreviewSceneDto> {
    let snapshot = lock_runtime(&state)?.runtime_model().preview_snapshot();
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
    let pixel_count = preview_pixel_count(
        &lock_runtime(&state)?
            .runtime_model()
            .preview_snapshot()
            .frame,
    );
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
