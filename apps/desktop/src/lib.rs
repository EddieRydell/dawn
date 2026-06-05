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
    tauri::Builder::default()
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_snapshot,
            commands::dispatch_app_command,
            commands::request_sequence_effect_previews,
            commands::take_sequence_effect_preview_results,
            commands::get_preview_scene,
            commands::init_preview_transport,
            commands::dispose_preview_transport,
            commands::get_preview_transport_mode
        ])
        .run(tauri::generate_context!())
}
