use tauri::State;
use tauri_specta::{collect_commands, Builder};

use crate::dto::{
    AppSnapshot, EditorViewMode, FixtureGuiEdit, LayoutGuiEdit, SequenceGuiEdit,
    SequenceSelectionEdit, SequenceSelectionEditResult, SequenceTransportState,
};
use crate::state::DesktopState;

#[tauri::command]
#[specta::specta]
pub fn get_snapshot(state: State<'_, DesktopState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn open_project_dialog(state: State<'_, DesktopState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn open_project(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    let _path = path;
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn choose_new_project_parent_directory() -> Option<String> {
    None
}

#[tauri::command]
#[specta::specta]
pub fn create_new_project(
    parent_path: String,
    directory_name: String,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    let (_parent_path, _directory_name) = (parent_path, directory_name);
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn open_file(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    let _path = path;
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn close_file(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    let _path = path;
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn set_active_file(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    let _path = path;
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn update_active_text(text: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_snapshot(|snapshot| {
        if let Some(buffer) = snapshot.active_buffer.as_mut() {
            buffer.text = text;
            buffer.dirty = true;
        }
    })
}

#[tauri::command]
#[specta::specta]
pub fn set_active_view_mode(mode: EditorViewMode, state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_snapshot(|snapshot| {
        if let Some(buffer) = snapshot.active_buffer.as_mut() {
            buffer.view_mode = mode;
        }
    })
}

#[tauri::command]
#[specta::specta]
pub fn undo_active_edit(state: State<'_, DesktopState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn redo_active_edit(state: State<'_, DesktopState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn apply_sequence_gui_edit(
    edit: SequenceGuiEdit,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    let _edit = edit;
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn apply_sequence_selection_edit(
    edit: SequenceSelectionEdit,
    state: State<'_, DesktopState>,
) -> SequenceSelectionEditResult {
    let _edit = edit;
    SequenceSelectionEditResult {
        snapshot: state.snapshot(),
        selection: None,
        copied_count: 0,
        skipped_count: 0,
    }
}

#[tauri::command]
#[specta::specta]
pub fn choose_sequence_audio(state: State<'_, DesktopState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn clear_sequence_audio(state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_snapshot(|snapshot| {
        snapshot.sequence_transport.audio = None;
    })
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
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn reload_active_buffer_from_disk(state: State<'_, DesktopState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn keep_active_buffer(state: State<'_, DesktopState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn create_file(parent: String, name: String, state: State<'_, DesktopState>) -> AppSnapshot {
    let (_parent, _name) = (parent, name);
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn create_directory(
    parent: String,
    name: String,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    let (_parent, _name) = (parent, name);
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn rename_path(path: String, new_name: String, state: State<'_, DesktopState>) -> AppSnapshot {
    let (_path, _new_name) = (path, new_name);
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn delete_path(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    let _path = path;
    state.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn reload_project(state: State<'_, DesktopState>) -> AppSnapshot {
    state.snapshot()
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
pub fn sequence_transport_play(state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_snapshot(|snapshot| {
        snapshot.sequence_transport.transport_state = SequenceTransportState::Playing;
        snapshot.sequence_transport.status = "Playing".to_string();
    })
}

#[tauri::command]
#[specta::specta]
pub fn sequence_transport_pause(state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_snapshot(|snapshot| {
        snapshot.sequence_transport.transport_state = SequenceTransportState::Paused;
        snapshot.sequence_transport.status = "Paused".to_string();
    })
}

#[tauri::command]
#[specta::specta]
pub fn sequence_transport_stop(state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_snapshot(|snapshot| {
        snapshot.sequence_transport.transport_state = SequenceTransportState::Stopped;
        snapshot.sequence_transport.position_seconds = snapshot.sequence_transport.home_seconds;
        snapshot.sequence_transport.status = "Stopped".to_string();
    })
}

#[tauri::command]
#[specta::specta]
pub fn sequence_transport_rewind_to_zero(state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_snapshot(|snapshot| {
        snapshot.sequence_transport.position_seconds = 0.0;
    })
}

#[tauri::command]
#[specta::specta]
pub fn sequence_transport_seek(
    position_seconds: f64,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.update_snapshot(|snapshot| {
        snapshot.sequence_transport.position_seconds = position_seconds;
    })
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

pub fn register(builder: Builder<tauri::Wry>) -> Builder<tauri::Wry> {
    builder.commands(collect_commands![
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
        sequence_transport_play,
        sequence_transport_pause,
        sequence_transport_stop,
        sequence_transport_rewind_to_zero,
        sequence_transport_seek,
        set_live_output_enabled
    ])
}
