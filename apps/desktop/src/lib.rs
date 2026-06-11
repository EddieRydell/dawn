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

pub mod audio;
pub mod bindings;
pub mod commands;
pub mod dto;
pub mod gui;
pub mod show_render;
pub mod state;

pub fn run() -> Result<(), tauri::Error> {
    let bindings = bindings::builder();

    tauri::Builder::default()
        .manage(state::DesktopState::new())
        .invoke_handler(bindings.invoke_handler())
        .run(tauri::generate_context!())
}
