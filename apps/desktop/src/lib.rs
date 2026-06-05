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

pub use bindings::{check_bindings, export_bindings};

pub fn run() -> Result<(), tauri::Error> {
    let specta_builder = bindings::specta_builder();
    tauri::Builder::default()
        .manage(state::AppState::default())
        .invoke_handler(specta_builder.invoke_handler())
        .run(tauri::generate_context!())
}
