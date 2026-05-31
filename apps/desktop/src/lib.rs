#![deny(clippy::disallowed_methods)]
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

mod app_runtime;
mod audio_runtime;
mod bindings;
mod commands;
mod effect_previews;
mod preview_scene;
mod preview_transport;
mod preview_types;
mod preview_window;
mod preview_worker;
mod state;

pub use bindings::{check_bindings, export_bindings, specta_builder};
pub use preview_types::{
    PreviewSceneDto, PreviewSceneFixtureDto, PreviewStateEventDto, PreviewTimingDto,
    SequenceEffectPreviewBatchDto, SequenceEffectPreviewDto,
};
use tauri::Manager;

pub fn run() -> Result<(), tauri::Error> {
    let builder = specta_builder();
    tauri::Builder::default()
        .manage(state::AppState::default())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            let _ = app.get_webview_window("main");
            preview_worker::start_preview_worker(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
}
