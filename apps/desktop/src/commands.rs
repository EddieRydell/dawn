use std::path::{Path, PathBuf};

use dawn_app_core::actions::AppAction;
use dawn_app_core::app_model::{ActiveGuiDocument, RuntimeSessionMirrorBuffer};
use dawn_app_core::dto::{
    AppSnapshotDto, EditorViewModeDto, FixtureGuiEditDto, LayoutGuiEditDto, SequenceGuiEditDto,
    SequenceSelectionEditDto, SequenceSelectionEditResultDto,
};
use dawn_app_core::editor_session::EditorViewMode;
use dawn_app_core::fseq_export::{export_fseq_file, FseqExportOptions};
use dawn_app_core::workspace::WorkspaceService;
use dawn_app_runtime::services::document_store::{RuntimeSessionBuffer, ViewMode};
use dawn_project::document::DocumentViewId;
use dawn_project::path::{serialized_import_path, utf8_path, Utf8PathBuf};
use tauri::{AppHandle, Manager, State};

use crate::app_runtime::{
    dispatch, emit_model_snapshot, preload_active_preview_audio, update_preview_from_audio_status,
    valid_sequence_audio,
};
use crate::effect_previews::{
    SequenceEffectPreviewRequestEffectDto, SequenceEffectPreviewResultsDto,
};
use crate::new_project::{create_starter_project, STARTER_SEQUENCE_PATH};
use crate::preview::{
    open_or_focus_preview_window, preview_pixel_count, preview_scene_from_frame, PreviewSceneDto,
};
use crate::preview_transport::{PreviewTransportMode, PreviewTransportRuntime};
use crate::runtime_host::ActiveRuntimeBuffer;
use crate::state::{
    lock_audio_runtime, lock_effect_preview_runtime, lock_live_output, lock_model,
    lock_preview_transport, lock_runtime, project_path, AppState, CommandResult,
};

#[specta::specta]
#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    hydrate_startup_session(&state)?;
    let live_output = lock_live_output(&state)?.snapshot();
    let mut model = lock_model(&state)?;
    model.set_live_output_snapshot(live_output);
    let snapshot = model.snapshot_dto();
    preload_active_preview_audio(&state, &snapshot.preview);
    Ok(snapshot)
}

fn hydrate_startup_session(state: &State<'_, AppState>) -> CommandResult<()> {
    if state.mark_startup_hydrated() {
        return Ok(());
    }

    let (Some(path), editor_session) = ({
        let model = lock_model(state)?;
        (
            model.workbench_layout.last_project_root.clone(),
            model.workbench_layout.editor_session.clone(),
        )
    }) else {
        return Ok(());
    };

    let mut workspace = WorkspaceService::default();
    if let Err(error) = workspace.open_project(&path) {
        lock_model(state)?.status = format!("Could not restore last project: {error}");
        return Ok(());
    }
    let Some(root) = workspace.project_root_display().map(ToString::to_string) else {
        lock_model(state)?.status =
            "Could not restore last project: project root was not opened".to_string();
        return Ok(());
    };

    let mut runtime_buffers = Vec::new();
    let mut mirror_buffers = Vec::new();
    for tab in editor_session.tabs {
        let Ok((text, disk_version)) = workspace.read_file_with_version(tab.path.clone()) else {
            continue;
        };
        runtime_buffers.push(RuntimeSessionBuffer {
            path: tab.path.clone(),
            text: text.clone(),
            view_mode: runtime_view_mode_from_editor(tab.view_mode),
        });
        mirror_buffers.push(RuntimeSessionMirrorBuffer {
            path: tab.path,
            text,
            disk_version,
            view_mode: tab.view_mode,
        });
    }

    lock_runtime(state)?.open_session(root, runtime_buffers, editor_session.active_file.clone())?;
    lock_model(state)?.mirror_runtime_session_opened(
        path,
        mirror_buffers,
        editor_session.active_file,
        "Project restored",
    )?;
    Ok(())
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
    open_project_runtime_then_model(&app, &state, path)
}

#[specta::specta]
#[tauri::command]
fn open_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
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
) -> CommandResult<AppSnapshotDto> {
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
fn open_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    open_file_runtime_then_model(&app, &state, project_path(path))
}

fn open_project_runtime_then_model(
    app: &AppHandle,
    state: &State<'_, AppState>,
    path: PathBuf,
) -> CommandResult<AppSnapshotDto> {
    lock_model(state)?.prepare_for_runtime_project_open()?;
    let root = project_root_display_for_open_path(&path)?;
    lock_runtime(state)?.open_project(root)?;
    {
        let mut model = lock_model(state)?;
        model.mirror_runtime_project_opened(path, true, "Project opened")?;
        let snapshot = emit_model_snapshot(app, &model)?;
        if let Ok(mut watcher) = crate::state::lock_filesystem_watcher(state) {
            let _ = watcher.sync_project_root(app, snapshot.project_root.clone());
        }
        if let Ok(runtime) = lock_audio_runtime(state) {
            runtime.clear();
        }
        preload_active_preview_audio(state, &snapshot.preview);
        Ok(snapshot)
    }
}

fn project_root_display_for_open_path(path: &Path) -> CommandResult<String> {
    let mut workspace = WorkspaceService::default();
    workspace.open_project(path)?;
    workspace
        .project_root_display()
        .map(ToString::to_string)
        .ok_or_else(|| "project root was not opened".to_string())
}

fn open_file_runtime_then_model(
    app: &AppHandle,
    state: &State<'_, AppState>,
    path: Utf8PathBuf,
) -> CommandResult<AppSnapshotDto> {
    let (text, disk_version) = {
        let model = lock_model(state)?;
        model.workspace.read_file_with_version(path.clone())?
    };
    lock_runtime(state)?.open_buffer(path.clone(), text.clone())?;
    let mut model = lock_model(state)?;
    model.mirror_runtime_file_opened(path, text, disk_version, EditorViewMode::Text)?;
    let snapshot = emit_model_snapshot(app, &model)?;
    preload_active_preview_audio(state, &snapshot.preview);
    Ok(snapshot)
}

#[specta::specta]
#[tauri::command]
fn close_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    let path = project_path(path);
    lock_runtime(&state)?.close_buffer(path.clone())?;
    let mut model = lock_model(&state)?;
    model.mirror_runtime_file_closed(path)?;
    let snapshot = emit_model_snapshot(&app, &model)?;
    preload_active_preview_audio(&state, &snapshot.preview);
    Ok(snapshot)
}

#[specta::specta]
#[tauri::command]
fn set_active_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    let path = project_path(path);
    lock_runtime(&state)?.set_active_buffer(path.clone())?;
    let mut model = lock_model(&state)?;
    model.mirror_runtime_active_file(path)?;
    let snapshot = emit_model_snapshot(&app, &model)?;
    preload_active_preview_audio(&state, &snapshot.preview);
    Ok(snapshot)
}

#[specta::specta]
#[tauri::command]
fn update_active_text(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> CommandResult<AppSnapshotDto> {
    let active_buffer = {
        let model = lock_model(&state)?;
        let snapshot = model.snapshot();
        let Some(buffer) = snapshot.active_buffer else {
            return Ok(snapshot.into());
        };
        let conflicted = buffer.is_conflicted();
        ActiveRuntimeBuffer {
            project_root: snapshot.project_root,
            path: buffer.path,
            conflicted,
            text: buffer.text,
        }
    };

    lock_runtime(&state)?.update_active_text(active_buffer, text.clone())?;
    let mut model = lock_model(&state)?;
    model.mirror_runtime_active_text_update(text)?;
    emit_model_snapshot(&app, &model)
}

#[specta::specta]
#[tauri::command]
fn set_active_view_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: EditorViewModeDto,
) -> CommandResult<AppSnapshotDto> {
    set_active_view_mode_runtime_then_model(&app, &state, mode)
}

fn set_active_view_mode_runtime_then_model(
    app: &AppHandle,
    state: &State<'_, AppState>,
    mode: EditorViewModeDto,
) -> CommandResult<AppSnapshotDto> {
    let active_file = {
        let model = lock_model(state)?;
        let snapshot = model.snapshot();
        let Some(active_file) = snapshot.active_file else {
            return Ok(snapshot.into());
        };
        active_file
    };
    lock_runtime(state)?.set_view_mode(active_file, runtime_view_mode(&mode))?;
    let mut model = lock_model(state)?;
    model.mirror_runtime_active_view_mode(editor_view_mode(&mode))?;
    let snapshot = emit_model_snapshot(app, &model)?;
    preload_active_preview_audio(state, &snapshot.preview);
    Ok(snapshot)
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

fn runtime_view_mode_from_editor(mode: EditorViewMode) -> ViewMode {
    match mode {
        EditorViewMode::Text => ViewMode::Text,
        EditorViewMode::Gui => ViewMode::Gui,
    }
}

#[specta::specta]
#[tauri::command]
fn undo_active_edit(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    let path = {
        let model = lock_model(&state)?;
        let snapshot = model.snapshot();
        let Some(path) = snapshot.active_file else {
            return Ok(snapshot.into());
        };
        path
    };
    let Some(text) = lock_runtime(&state)?.undo_buffer_text(path)? else {
        return get_snapshot(state);
    };
    let mut model = lock_model(&state)?;
    model.mirror_runtime_active_history_text(text, "Undo");
    emit_model_snapshot(&app, &model)
}

#[specta::specta]
#[tauri::command]
fn redo_active_edit(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    let path = {
        let model = lock_model(&state)?;
        let snapshot = model.snapshot();
        let Some(path) = snapshot.active_file else {
            return Ok(snapshot.into());
        };
        path
    };
    let Some(text) = lock_runtime(&state)?.redo_buffer_text(path)? else {
        return get_snapshot(state);
    };
    let mut model = lock_model(&state)?;
    model.mirror_runtime_active_history_text(text, "Redo");
    emit_model_snapshot(&app, &model)
}

#[specta::specta]
#[tauri::command]
fn apply_sequence_gui_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: SequenceGuiEditDto,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::ApplySequenceGuiEdit(edit))
}

#[specta::specta]
#[tauri::command]
fn apply_sequence_selection_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: SequenceSelectionEditDto,
) -> CommandResult<SequenceSelectionEditResultDto> {
    let mut model = lock_model(&state)?;
    let result = model.apply_sequence_selection_edit(edit)?;
    emit_model_snapshot(&app, &model)?;
    Ok(result)
}

#[specta::specta]
#[tauri::command]
fn choose_sequence_audio(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AppSnapshotDto> {
    let (project_root, sequence_path) = {
        let model = lock_model(&state)?;
        let snapshot = model.snapshot();
        let Some(sequence_path) = snapshot.active_file else {
            return Err("no active sequence file is selected".to_string());
        };
        if !matches!(
            snapshot.active_gui_document,
            Some(dawn_app_core::app_model::ActiveGuiDocument::Sequence(_))
        ) {
            return Err("active document is not a sequence".to_string());
        }
        (model.project_root.clone(), sequence_path)
    };

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
        return get_snapshot(state);
    };
    let import = serialized_import_path(&sequence_path, &utf8_path(path)?);
    dispatch(
        &app,
        &state,
        AppAction::ApplySequenceGuiEdit(SequenceGuiEditDto::SetAudio {
            import: Some(import),
        }),
    )
}

#[specta::specta]
#[tauri::command]
fn clear_sequence_audio(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AppSnapshotDto> {
    dispatch(
        &app,
        &state,
        AppAction::ApplySequenceGuiEdit(SequenceGuiEditDto::SetAudio { import: None }),
    )
}

#[specta::specta]
#[tauri::command]
fn export_active_sequence_fseq(
    app: AppHandle,
    state: State<'_, AppState>,
    step_ms: u8,
) -> CommandResult<AppSnapshotDto> {
    let (analysis, document, default_name) = {
        let model = lock_model(&state)?;
        let analysis = model
            .analysis
            .as_ref()
            .ok_or_else(|| "project analysis is not available".to_string())?
            .clone();
        if analysis.has_errors() {
            return Err("project has analysis errors".to_string());
        }
        let snapshot = model.snapshot();
        if matches!(
            snapshot.active_gui_document,
            Some(ActiveGuiDocument::Blocked { .. })
        ) {
            return Err("active document is blocked by diagnostics".to_string());
        }
        let path = model
            .editors
            .active_file()
            .cloned()
            .ok_or_else(|| "no active sequence file is selected".to_string())?;
        let overlays = model.editors.dirty_overlays();
        let descriptor = model
            .workspace
            .inspect_document(path.clone(), overlays.clone())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .cloned()
            .ok_or_else(|| "active document is not a sequence".to_string())?;
        let document = model
            .workspace
            .sequence_document(path, &object_key, overlays)?;
        let default_name = format!("{}.fseq", document.object_key);
        (analysis, document, default_name)
    };

    let Some(output_path) = rfd::FileDialog::new()
        .set_title("Export FSEQ")
        .set_file_name(&default_name)
        .add_filter("FSEQ", &["fseq"])
        .save_file()
    else {
        return get_snapshot(state);
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

    let mut model = lock_model(&state)?;
    model.status = format!(
        "Exported FSEQ: {} frames, {} channels",
        report.frame_count, report.channel_count
    );
    emit_model_snapshot(&app, &model)?;
    Ok(model.snapshot_dto())
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
    let model = lock_model(&state)?;
    let analysis = model
        .analysis
        .as_ref()
        .ok_or_else(|| "project analysis is not available".to_string())?
        .clone();
    let request_path = path.clone();
    let request_object_key = object_key.clone();
    let document =
        model.cached_sequence_document_for_preview_request(&project_path(path), &object_key)?;
    drop(model);

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
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::ApplyLayoutGuiEdit(edit))
}

#[specta::specta]
#[tauri::command]
fn apply_fixture_gui_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: FixtureGuiEditDto,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::ApplyFixtureGuiEdit(edit))
}

#[specta::specta]
#[tauri::command]
fn flush_autosave(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::FlushAutosave)
}

#[specta::specta]
#[tauri::command]
fn reload_active_buffer_from_disk(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::ReloadActiveBufferFromDisk)
}

#[specta::specta]
#[tauri::command]
fn keep_active_buffer(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::KeepActiveBuffer)
}

#[specta::specta]
#[tauri::command]
fn create_file(
    app: AppHandle,
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> CommandResult<AppSnapshotDto> {
    let created = lock_model(&state)?.create_file_for_runtime_open(project_path(parent), name)?;
    lock_runtime(&state)?.open_buffer(created.path.clone(), created.text.clone())?;
    let mut model = lock_model(&state)?;
    model.mirror_runtime_file_opened(
        created.path,
        created.text,
        created.disk_version,
        EditorViewMode::Text,
    )?;
    let snapshot = emit_model_snapshot(&app, &model)?;
    preload_active_preview_audio(&state, &snapshot.preview);
    Ok(snapshot)
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
fn set_effect_preview_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::SetEffectPreviewEnabled(enabled))
}

#[specta::specta]
#[tauri::command]
fn set_effect_preview_effects(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<u32>,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::SetEffectPreviewEffects(ids))
}

#[specta::specta]
#[tauri::command]
async fn open_preview_window(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    open_or_focus_preview_window(app, state)
}

#[specta::specta]
#[tauri::command]
fn preview_play(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    let (audio, position_seconds, effect_preview_enabled) = {
        let model = lock_model(&state)?;
        let snapshot = model.preview.snapshot();
        (
            valid_sequence_audio(&snapshot),
            snapshot.position_seconds,
            model.workbench_layout.effect_preview_enabled,
        )
    };
    if effect_preview_enabled {
        dispatch(&app, &state, AppAction::SetEffectPreviewEnabled(false))?;
    }
    let Some(audio) = audio else {
        return dispatch(&app, &state, AppAction::PreviewPlay);
    };
    let clock = lock_audio_runtime(&state)?.play(&audio, position_seconds)?;
    update_preview_from_audio_status(&app, &state, clock)
}

#[specta::specta]
#[tauri::command]
fn preview_pause(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    let has_audio = {
        let model = lock_model(&state)?;
        valid_sequence_audio(&model.preview.snapshot()).is_some()
    };
    if !has_audio {
        return dispatch(&app, &state, AppAction::PreviewPause);
    }
    let clock = lock_audio_runtime(&state)?.pause()?;
    let mut model = lock_model(&state)?;
    let analysis = model.analysis.clone();
    model
        .preview
        .pause_at(clock.position_seconds, analysis.as_ref());
    model.preview.set_timing_status("nativeAudio", clock.status);
    model.status = "Preview paused".to_string();
    emit_model_snapshot(&app, &model)
}

#[specta::specta]
#[tauri::command]
fn preview_stop(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    let (has_audio, home_seconds) = {
        let model = lock_model(&state)?;
        let snapshot = model.preview.snapshot();
        (
            valid_sequence_audio(&snapshot).is_some(),
            snapshot.home_seconds,
        )
    };
    if !has_audio {
        return dispatch(&app, &state, AppAction::PreviewStop);
    }
    let clock = lock_audio_runtime(&state)?.stop(home_seconds)?;
    let mut model = lock_model(&state)?;
    let analysis = model.analysis.clone();
    model.preview.stop_native_audio(analysis.as_ref());
    model.preview.set_timing_status("nativeAudio", clock.status);
    model.status = "Preview stopped".to_string();
    emit_model_snapshot(&app, &model)
}

#[specta::specta]
#[tauri::command]
fn preview_rewind_to_zero(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AppSnapshotDto> {
    let has_audio = {
        let model = lock_model(&state)?;
        valid_sequence_audio(&model.preview.snapshot()).is_some()
    };
    if !has_audio {
        return dispatch(&app, &state, AppAction::PreviewRewindToZero);
    }
    let clock = lock_audio_runtime(&state)?.stop(0.0)?;
    let mut model = lock_model(&state)?;
    let analysis = model.analysis.clone();
    model
        .preview
        .go_to_sequence_beginning_native_audio(analysis.as_ref());
    model.preview.set_timing_status("nativeAudio", clock.status);
    model.status = "Preview rewound".to_string();
    emit_model_snapshot(&app, &model)
}

#[specta::specta]
#[tauri::command]
fn preview_seek(
    app: AppHandle,
    state: State<'_, AppState>,
    position_seconds: f64,
) -> CommandResult<AppSnapshotDto> {
    if !position_seconds.is_finite() || position_seconds < 0.0 {
        return Err("preview seek seconds must be finite and non-negative".to_string());
    }
    let (audio, playing) = {
        let model = lock_model(&state)?;
        let snapshot = model.preview.snapshot();
        (valid_sequence_audio(&snapshot), snapshot.is_playing)
    };
    let Some(audio) = audio else {
        return dispatch(&app, &state, AppAction::PreviewSeek(position_seconds));
    };
    let clock = lock_audio_runtime(&state)?.seek(&audio, position_seconds, playing)?;
    let mut model = lock_model(&state)?;
    let analysis = model.analysis.clone();
    model
        .preview
        .seek_native_audio(clock.position_seconds, playing, analysis.as_ref());
    model.preview.set_timing_status("nativeAudio", clock.status);
    model.status = "Preview seeked".to_string();
    emit_model_snapshot(&app, &model)
}

#[specta::specta]
#[tauri::command]
fn set_live_output_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<AppSnapshotDto> {
    let analysis = lock_model(&state)?.analysis.clone();
    let snapshot = lock_live_output(&state)?.set_enabled(enabled, analysis.as_ref());
    let mut model = lock_model(&state)?;
    model.set_live_output_snapshot(snapshot);
    emit_model_snapshot(&app, &model)
}

#[specta::specta]
#[tauri::command]
fn get_preview_scene(state: State<'_, AppState>) -> CommandResult<PreviewSceneDto> {
    let snapshot = lock_model(&state)?.preview.snapshot();
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
    let pixel_count = preview_pixel_count(&lock_model(&state)?.preview.snapshot().frame);
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
        get_snapshot,
        open_project_dialog,
        open_project,
        choose_new_project_parent_directory,
        create_new_project,
        open_file,
        close_file,
        set_active_file,
        update_active_text,
        set_active_view_mode,
        undo_active_edit,
        redo_active_edit,
        apply_sequence_gui_edit,
        apply_sequence_selection_edit,
        choose_sequence_audio,
        clear_sequence_audio,
        export_active_sequence_fseq,
        request_sequence_effect_previews,
        take_sequence_effect_preview_results,
        apply_layout_gui_edit,
        apply_fixture_gui_edit,
        flush_autosave,
        reload_active_buffer_from_disk,
        keep_active_buffer,
        create_file,
        create_directory,
        rename_path,
        delete_path,
        reload_project,
        toggle_project_tree,
        set_effect_preview_enabled,
        set_effect_preview_effects,
        open_preview_window,
        preview_play,
        preview_pause,
        preview_stop,
        preview_rewind_to_zero,
        preview_seek,
        set_live_output_enabled,
        get_preview_scene,
        init_preview_transport,
        dispose_preview_transport,
        get_preview_transport_mode
    ])
}
