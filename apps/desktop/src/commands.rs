use std::path::PathBuf;

use dawn_app_core::actions::AppAction;
use dawn_app_core::dto::{
    AppSnapshotDto, EditorViewModeDto, FixtureGuiEditDto, LayoutGuiEditDto, SequenceGuiEditDto,
    SequenceSelectionEditDto, SequenceSelectionEditResultDto,
};
use dawn_project::path::{serialized_import_path, utf8_path, Utf8PathBuf};
use tauri::{AppHandle, Manager, State};

use crate::app_runtime::{
    dispatch, emit_model_snapshot, sync_active_audio_load, update_preview_from_audio_status,
    valid_sequence_audio,
};
use crate::effect_previews::{preview_for_effect, SequenceEffectPreviewBatchDto};
use crate::preview::{
    open_or_focus_preview_window, preview_pixel_count, preview_scene_from_frame, PreviewSceneDto,
};
use crate::preview_transport::{PreviewTransportMode, PreviewTransportRuntime};
use crate::state::{
    lock_audio_runtime, lock_live_output, lock_model, lock_preview_transport, project_path,
    AppState, CommandResult,
};

#[specta::specta]
#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    let live_output = lock_live_output(&state)?.snapshot();
    let mut model = lock_model(&state)?;
    model.set_live_output_snapshot(live_output);
    let snapshot = model.snapshot_dto();
    let _ = sync_active_audio_load(&state, &snapshot.preview);
    Ok(snapshot)
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
fn set_active_view_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: EditorViewModeDto,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::SetActiveViewMode(mode))
}

#[specta::specta]
#[tauri::command]
fn undo_active_edit(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::UndoActiveEdit)
}

#[specta::specta]
#[tauri::command]
fn redo_active_edit(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::RedoActiveEdit)
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
fn get_sequence_effect_previews(
    state: State<'_, AppState>,
    path: String,
    object_key: String,
    effect_ids: Vec<u32>,
) -> CommandResult<SequenceEffectPreviewBatchDto> {
    let model = lock_model(&state)?;
    let analysis = model
        .analysis
        .as_ref()
        .ok_or_else(|| "project analysis is not available".to_string())?
        .clone();
    let document = model.workspace.sequence_document(
        project_path(path),
        &object_key,
        model.editors.dirty_overlays(),
    )?;
    drop(model);

    let requested = effect_ids
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let mut previews = Vec::new();
    for effect in document
        .effects
        .iter()
        .filter(|effect| requested.contains(&effect.id))
    {
        if let Some(preview) = preview_for_effect(
            &state,
            &analysis,
            &document.path,
            &document.object_key,
            document.frame_rate,
            &document.mark_collections,
            effect,
        )? {
            previews.push(preview);
        }
    }

    Ok(SequenceEffectPreviewBatchDto { previews })
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
async fn open_preview_window(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    open_or_focus_preview_window(app, state)
}

#[specta::specta]
#[tauri::command]
fn preview_play(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    let (audio, position_seconds) = {
        let model = lock_model(&state)?;
        let snapshot = model.preview.snapshot();
        (valid_sequence_audio(&snapshot), snapshot.position_seconds)
    };
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
        return dispatch(
            &app,
            &state,
            AppAction::PreviewSeek(position_seconds.into()),
        );
    };
    let clock = lock_audio_runtime(&state)?.seek(&audio, position_seconds.into(), playing)?;
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
        get_sequence_effect_previews,
        apply_layout_gui_edit,
        apply_fixture_gui_edit,
        flush_autosave,
        create_file,
        create_directory,
        rename_path,
        delete_path,
        reload_project,
        toggle_project_tree,
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
