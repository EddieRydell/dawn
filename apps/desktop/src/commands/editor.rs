use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn open_file(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.open_file_path(&path)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn close_file(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.close_file_path(&path)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn set_active_file(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.set_active_file_path(&path)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn update_active_text(text: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.update_active_text(text)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn autosave_active_text(
    path: String,
    text: String,
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
    state.autosave_active_text(&path, text)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn set_editor_view_mode(
    mode: EditorViewMode,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.set_editor_view_mode(mode)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn save_editor_view_state(
    update: PersistedEditorViewStateUpdate,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.save_editor_view_state(update)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn save_sequence_viewport_state(
    update: PersistedSequenceViewportStateUpdate,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.save_sequence_viewport_state(update)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn undo_active_edit(state: State<'_, DesktopState>) -> AppSnapshot {
    state.undo_active_edit()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn redo_active_edit(state: State<'_, DesktopState>) -> AppSnapshot {
    state.redo_active_edit()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn flush_autosave(state: State<'_, DesktopState>) -> AppSnapshot {
    state.save_active_buffer()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn reload_active_buffer_from_disk(state: State<'_, DesktopState>) -> AppSnapshot {
    state.reload_active_buffer_from_disk()
}
