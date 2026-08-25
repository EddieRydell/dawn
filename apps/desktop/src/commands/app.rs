use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn get_snapshot(state: State<'_, DesktopState>) -> AppSnapshot {
    state.snapshot()
}
