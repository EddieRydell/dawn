use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn set_preview_window_open(
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
