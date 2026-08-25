use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn sync_packages(state: State<'_, DesktopState>) -> AppSnapshot {
    state.sync_packages()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn check_package_updates(state: State<'_, DesktopState>) -> AppSnapshot {
    state.check_package_updates()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn update_packages(
    alias: Option<String>,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.update_packages(alias)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn remove_package_dependency(
    alias: String,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.remove_package_dependency(&alias)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn fork_package_dependency(
    alias: String,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.fork_package_dependency(&alias)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn open_package_page(alias: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.open_package_page(&alias)
}
