use dawn_backend::AppView;
use tauri::{AppHandle, Emitter};

use crate::{
    dto::{AppBackendChangedDto, AppSnapshotDto},
    state::CommandResult,
};

pub(crate) fn emit_app_view(app: &AppHandle, view: AppView) -> CommandResult<()> {
    emit_app_snapshot(app, AppSnapshotDto::from(view))
}

pub(crate) fn emit_app_snapshot(app: &AppHandle, snapshot: AppSnapshotDto) -> CommandResult<()> {
    app.emit(
        "app_backend_changed",
        AppBackendChangedDto {
            snapshot,
            changed_slices: Vec::new(),
        },
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn emit_backend_error(app: &AppHandle, message: String) {
    let _ = app.emit("app_backend_error", message);
}
