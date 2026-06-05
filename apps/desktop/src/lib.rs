#![cfg_attr(not(windows), deny(unsafe_code))]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

mod bindings;
mod commands;
mod dto;
mod events;
mod jobs;
mod state;

use dawn_backend::BackendError;
use tauri::Manager;

pub use bindings::{check_bindings, export_bindings};

pub fn run() -> Result<(), tauri::Error> {
    let specta_builder = bindings::specta_builder();
    tauri::Builder::default()
        .manage(state::AppState::default())
        .setup(|app| {
            restore_last_project(app.handle()).map_err(std::io::Error::other)?;
            Ok(())
        })
        .invoke_handler(specta_builder.invoke_handler())
        .run(tauri::generate_context!())
}

fn restore_last_project(app: &tauri::AppHandle) -> state::CommandResult<()> {
    let state = app.state::<state::AppState>();
    let backend = state.backend();
    let update = {
        let mut backend = state.lock_backend()?;
        backend
            .restore_last_project()
            .map_err(|error: BackendError| error.to_string())?
    };
    jobs::handle_backend_update(app, backend, update)
}
