use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_specta::{Builder, collect_commands};

use crate::dto::{
    AppSettings, AppSnapshot, AudioTransportState, DocumentViewId, EditorViewMode, FixtureGuiEdit,
    GuiDocument, GuiDocumentRequest, GuiEditCommand, GuiEditResult, LayoutGuiEdit,
    NewSequenceRequest, SequenceClipRasterRequest, SequenceClipRasterResponse,
    SequenceClipRasterResultBatch, SequenceGuiEdit, SequenceSelectionEdit,
    SequenceSelectionEditResult, WorkspaceLayoutState,
};
use crate::persistence::{
    PersistedEditorViewStateUpdate, PersistedPreviewWindowState,
    PersistedSequenceViewportStateUpdate, ProjectRestoreState, read_window_state,
};
use crate::state::DesktopState;

#[tauri::command]
#[specta::specta]
pub fn get_snapshot(state: State<'_, DesktopState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn update_app_settings(settings: AppSettings, state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_app_settings(settings)
}

#[tauri::command]
#[specta::specta]
pub fn save_workspace_layout_state(
    state_update: WorkspaceLayoutState,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.save_workspace_layout_state(state_update)
}

#[tauri::command]
#[specta::specta]
pub fn get_restored_view_state(state: State<'_, DesktopState>) -> ProjectRestoreState {
    state.restored_view_state()
}

#[tauri::command]
#[specta::specta]
pub fn open_project_dialog(state: State<'_, DesktopState>) -> AppSnapshot {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Dawn project", &["dawn"])
        .set_file_name("project.dawn")
        .pick_file()
    else {
        return state.snapshot();
    };
    let Some(path) = path.to_str() else {
        return state.update_snapshot(|snapshot| {
            snapshot.status = "Selected project path is not valid UTF-8".to_string();
        });
    };
    state.open_project_path(path)
}

#[tauri::command]
#[specta::specta]
pub fn open_project(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.open_project_path(&path)
}

#[tauri::command]
#[specta::specta]
pub fn choose_new_project_parent_directory() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .and_then(|path| path.to_str().map(ToString::to_string))
}

#[tauri::command]
#[specta::specta]
pub fn create_new_project(
    parent_path: String,
    directory_name: String,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.create_new_project(&parent_path, &directory_name)
}

#[tauri::command]
#[specta::specta]
pub fn create_sequence(request: NewSequenceRequest, state: State<'_, DesktopState>) -> AppSnapshot {
    state.create_sequence(request)
}

#[tauri::command]
#[specta::specta]
pub fn open_file(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.open_file_path(&path)
}

#[tauri::command]
#[specta::specta]
pub fn close_file(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.close_file_path(&path)
}

#[tauri::command]
#[specta::specta]
pub fn set_active_file(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.set_active_file_path(&path)
}

#[tauri::command]
#[specta::specta]
pub fn update_active_text(text: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_active_text(text)
}

#[tauri::command]
#[specta::specta]
pub fn set_editor_view_mode(mode: EditorViewMode, state: State<'_, DesktopState>) -> AppSnapshot {
    state.set_editor_view_mode(mode)
}

#[tauri::command]
#[specta::specta]
pub fn save_editor_view_state(
    update: PersistedEditorViewStateUpdate,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.save_editor_view_state(update)
}

#[tauri::command]
#[specta::specta]
pub fn save_sequence_viewport_state(
    update: PersistedSequenceViewportStateUpdate,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.save_sequence_viewport_state(update)
}

#[tauri::command]
#[specta::specta]
pub fn undo_active_edit(state: State<'_, DesktopState>) -> AppSnapshot {
    state.undo_active_edit()
}

#[tauri::command]
#[specta::specta]
pub fn redo_active_edit(state: State<'_, DesktopState>) -> AppSnapshot {
    state.redo_active_edit()
}

#[tauri::command]
#[specta::specta]
pub fn get_gui_document(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> GuiDocument {
    state.get_gui_document(request)
}

#[tauri::command]
#[specta::specta]
pub fn request_sequence_clip_rasters(
    request: SequenceClipRasterRequest,
    state: State<'_, DesktopState>,
) -> SequenceClipRasterResponse {
    state.request_sequence_clip_rasters(request)
}

#[tauri::command]
#[specta::specta]
pub fn take_sequence_clip_raster_results(
    request: GuiDocumentRequest,
    request_id: u32,
    state: State<'_, DesktopState>,
) -> SequenceClipRasterResultBatch {
    state.take_sequence_clip_raster_results(request, request_id)
}

#[tauri::command]
#[specta::specta]
pub fn apply_gui_edit(
    request: GuiDocumentRequest,
    edit: GuiEditCommand,
    state: State<'_, DesktopState>,
) -> GuiEditResult {
    state.apply_gui_edit(request, edit)
}

#[tauri::command]
#[specta::specta]
pub fn apply_sequence_gui_edit(
    edit: SequenceGuiEdit,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.apply_active_sequence_gui_edit(edit)
}

#[tauri::command]
#[specta::specta]
pub fn finish_composition_graph_editing(state: State<'_, DesktopState>) -> AppSnapshot {
    state.finish_composition_graph_editing()
}

#[tauri::command]
#[specta::specta]
pub fn apply_sequence_selection_edit(
    edit: SequenceSelectionEdit,
    state: State<'_, DesktopState>,
) -> SequenceSelectionEditResult {
    state.apply_sequence_selection_edit(edit)
}

#[tauri::command]
#[specta::specta]
pub fn choose_sequence_audio(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> GuiEditResult {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Audio", &["mp3", "wav", "ogg", "flac"])
        .pick_file()
    else {
        return GuiEditResult {
            snapshot: state.snapshot(),
            document: state.get_gui_document(request),
        };
    };
    let Some(import_path) = audio_import_path(&state.snapshot(), &request, &path) else {
        let snapshot = state.update_snapshot(|snapshot| {
            snapshot.status = "Selected audio path is not valid UTF-8".to_string();
        });
        return GuiEditResult {
            snapshot,
            document: state.get_gui_document(request),
        };
    };
    state.apply_gui_edit(
        request,
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::SetAudio {
                import_path: Some(import_path),
            },
        },
    )
}

#[tauri::command]
#[specta::specta]
pub fn clear_sequence_audio(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state
        .apply_gui_edit(
            request,
            GuiEditCommand::Sequence {
                edit: SequenceGuiEdit::SetAudio { import_path: None },
            },
        )
        .snapshot
}

#[tauri::command]
#[specta::specta]
pub fn export_active_sequence_fseq(step_ms: f64, state: State<'_, DesktopState>) -> AppSnapshot {
    let _step_ms = step_ms;
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn apply_layout_gui_edit(edit: LayoutGuiEdit, state: State<'_, DesktopState>) -> AppSnapshot {
    let _edit = edit;
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn apply_fixture_gui_edit(edit: FixtureGuiEdit, state: State<'_, DesktopState>) -> AppSnapshot {
    let _edit = edit;
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn flush_autosave(state: State<'_, DesktopState>) -> AppSnapshot {
    state.save_active_buffer()
}

#[tauri::command]
#[specta::specta]
pub fn reload_active_buffer_from_disk(state: State<'_, DesktopState>) -> AppSnapshot {
    state.reload_active_buffer_from_disk()
}

#[tauri::command]
#[specta::specta]
pub fn keep_active_buffer(state: State<'_, DesktopState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn create_file(parent: String, name: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.create_file(&parent, &name)
}

#[tauri::command]
#[specta::specta]
pub fn create_directory(
    parent: String,
    name: String,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.create_directory(&parent, &name)
}

#[tauri::command]
#[specta::specta]
pub fn rename_path(path: String, new_name: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.rename_path(&path, &new_name)
}

#[tauri::command]
#[specta::specta]
pub fn delete_path(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.delete_path(&path)
}

#[tauri::command]
#[specta::specta]
pub fn reload_project(state: State<'_, DesktopState>) -> AppSnapshot {
    state.reload_project()
}

#[tauri::command]
#[specta::specta]
pub fn toggle_project_tree(state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_snapshot(|snapshot| {
        snapshot.project_tree_visible = !snapshot.project_tree_visible;
    })
}

#[tauri::command]
#[specta::specta]
pub fn load_sequence_audio(
    request: GuiDocumentRequest,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    publish_audio_snapshot(&app, state.load_sequence_audio(request))
}

#[tauri::command]
#[specta::specta]
pub fn unload_audio(app: AppHandle, state: State<'_, DesktopState>) -> AppSnapshot {
    publish_audio_snapshot(&app, state.unload_audio())
}

#[tauri::command]
#[specta::specta]
pub fn audio_play(app: AppHandle, state: State<'_, DesktopState>) -> AppSnapshot {
    let snapshot = publish_audio_snapshot(&app, state.audio_play());
    if matches!(snapshot.audio_transport.state, AudioTransportState::Playing) {
        start_audio_transport_poll(app);
    }
    snapshot
}

#[tauri::command]
#[specta::specta]
pub fn audio_pause(app: AppHandle, state: State<'_, DesktopState>) -> AppSnapshot {
    publish_audio_snapshot(&app, state.audio_pause())
}

#[tauri::command]
#[specta::specta]
pub fn audio_stop(app: AppHandle, state: State<'_, DesktopState>) -> AppSnapshot {
    publish_audio_snapshot(&app, state.audio_stop())
}

#[tauri::command]
#[specta::specta]
pub fn audio_rewind_to_zero(app: AppHandle, state: State<'_, DesktopState>) -> AppSnapshot {
    publish_audio_snapshot(&app, state.audio_rewind_to_zero())
}

#[tauri::command]
#[specta::specta]
pub fn audio_seek(
    position_seconds: f64,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    publish_audio_snapshot(&app, state.audio_seek(position_seconds))
}

#[tauri::command]
#[specta::specta]
pub fn set_live_output_enabled(enabled: bool, state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_snapshot(|snapshot| {
        snapshot.live_output.enabled = enabled;
        snapshot.live_output.status = if enabled {
            "Enabled".to_string()
        } else {
            "Disabled".to_string()
        };
    })
}

#[tauri::command]
#[specta::specta]
pub fn open_preview_window(
    app: AppHandle,
    preview: State<'_, crate::preview::PreviewWindowService>,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    set_preview_window_open(true, app, preview, state)
}

#[tauri::command]
#[specta::specta]
pub fn set_preview_window_open(
    enabled: bool,
    app: AppHandle,
    preview: State<'_, crate::preview::PreviewWindowService>,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    let restore = state.persistence().preview_window();
    let result = if enabled {
        preview.open_or_focus(
            app.clone(),
            PersistedPreviewWindowState {
                open: true,
                ..restore
            },
        )
    } else {
        preview.close(&app, state.persistence())
    };
    match result {
        Ok(()) => state.update_snapshot(|snapshot| {
            snapshot.preview_open = enabled;
            snapshot.preview_error = None;
        }),
        Err(error) => state.update_snapshot(|snapshot| {
            snapshot.preview_error = Some(format!("Preview failed: {error}"));
        }),
    }
}

#[tauri::command]
#[specta::specta]
pub fn persist_app_close(
    app: AppHandle,
    preview: State<'_, crate::preview::PreviewWindowService>,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    if let Some(main) = app
        .get_window("main")
        .and_then(|window| read_window_state(&window))
        && let Err(error) = state.persistence().record_main_window(main)
    {
        return state.set_persistence_error(format!("Main window state was not saved: {error}"));
    }
    if let Err(error) = preview.close_for_main_shutdown(&app, state.persistence()) {
        return state.set_persistence_error(format!("Preview window state was not saved: {error}"));
    }
    state.snapshot()
}

pub fn register(builder: Builder<tauri::Wry>) -> Builder<tauri::Wry> {
    builder.commands(collect_commands![
        get_snapshot,
        update_app_settings,
        save_workspace_layout_state,
        get_restored_view_state,
        open_project_dialog,
        open_project,
        choose_new_project_parent_directory,
        create_new_project,
        create_sequence,
        open_file,
        close_file,
        set_active_file,
        update_active_text,
        set_editor_view_mode,
        save_editor_view_state,
        save_sequence_viewport_state,
        undo_active_edit,
        redo_active_edit,
        get_gui_document,
        request_sequence_clip_rasters,
        take_sequence_clip_raster_results,
        apply_gui_edit,
        apply_sequence_gui_edit,
        finish_composition_graph_editing,
        apply_sequence_selection_edit,
        choose_sequence_audio,
        clear_sequence_audio,
        export_active_sequence_fseq,
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
        load_sequence_audio,
        unload_audio,
        audio_play,
        audio_pause,
        audio_stop,
        audio_rewind_to_zero,
        audio_seek,
        set_live_output_enabled,
        open_preview_window,
        set_preview_window_open,
        persist_app_close
    ])
}

fn publish_audio_snapshot(app: &AppHandle, snapshot: AppSnapshot) -> AppSnapshot {
    let _ = app.emit("audio_transport_changed", snapshot.audio_transport.clone());
    snapshot
}

fn start_audio_transport_poll(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(50));
            let state = app.state::<DesktopState>();
            let snapshot = state.snapshot();
            let _ = app.emit("audio_transport_changed", snapshot.audio_transport.clone());
            if !matches!(snapshot.audio_transport.state, AudioTransportState::Playing) {
                break;
            }
        }
    });
}

fn audio_import_path(
    snapshot: &AppSnapshot,
    request: &GuiDocumentRequest,
    selected_path: &Path,
) -> Option<String> {
    if !matches!(request.view, DocumentViewId::Sequence) {
        return None;
    }
    let selected = selected_path.canonicalize().ok()?;
    let project_root = snapshot.project_root.as_deref().map(PathBuf::from)?;
    let document_path = project_root.join(&request.path);
    let document_dir = document_path.parent()?;
    let document_dir = document_dir.canonicalize().ok()?;
    relative_path(&document_dir, &selected).or_else(|| selected.to_str().map(ToString::to_string))
}

fn relative_path(from: &Path, to: &Path) -> Option<String> {
    let from_parts = path_parts(from)?;
    let to_parts = path_parts(to)?;
    if from_parts.prefix != to_parts.prefix || from_parts.rooted != to_parts.rooted {
        return None;
    }
    let common = from_parts
        .normal
        .iter()
        .zip(&to_parts.normal)
        .take_while(|(left, right)| left.eq_ignore_ascii_case(right))
        .count();
    let mut path = PathBuf::new();
    for _ in common..from_parts.normal.len() {
        path.push("..");
    }
    for part in &to_parts.normal[common..] {
        path.push(part);
    }
    path.to_str().map(|value| value.replace('\\', "/"))
}

struct PathParts {
    prefix: Option<String>,
    rooted: bool,
    normal: Vec<String>,
}

fn path_parts(path: &Path) -> Option<PathParts> {
    let mut prefix = None;
    let mut rooted = false;
    let mut normal = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(value) => {
                prefix = Some(value.as_os_str().to_str()?.to_string());
            }
            Component::RootDir => rooted = true,
            Component::CurDir => {}
            Component::ParentDir => normal.push("..".to_string()),
            Component::Normal(value) => normal.push(value.to_str()?.to_string()),
        }
    }
    Some(PathParts {
        prefix,
        rooted,
        normal,
    })
}
