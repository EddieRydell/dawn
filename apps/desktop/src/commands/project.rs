use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn open_project_dialog(state: State<'_, DesktopState>) -> AppSnapshot {
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
pub(crate) fn open_project(path: String, state: State<'_, DesktopState>) -> AppSnapshot {
    state.open_project_path(&path)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn choose_new_project_parent_directory() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .and_then(|path| path.to_str().map(ToString::to_string))
}

#[tauri::command]
#[specta::specta]
pub(crate) fn create_new_project(
    parent_path: String,
    directory_name: String,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.create_new_project(&parent_path, &directory_name)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn create_sequence(
    request: NewSequenceRequest,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.create_sequence(request)
}
