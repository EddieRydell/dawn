use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn set_live_output_active(
    active: bool,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    let snapshot = state.set_live_output_active(active);
    let _ = app.emit("live_output_changed", snapshot.live_output.clone());
    if active {
        start_live_output_poll(app);
    }
    snapshot
}
