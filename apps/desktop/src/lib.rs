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
mod filesystem_watcher;
mod live_output;
mod new_project;
mod project_autosave;
mod sequence_runtime;
mod state;
mod window_layout;

pub use bindings::{check_bindings, export_bindings, specta_builder};
pub use sequence_runtime::{SequenceRenderEventDto, SequenceRenderTimingDto};
use tauri::Manager;

pub fn run() -> Result<(), tauri::Error> {
    let builder = specta_builder();
    tauri::Builder::default()
        .manage(state::AppState::default())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            let _ = app.get_webview_window("main");
            window_layout::restore_main_window_layout(app.handle())
                .map_err(std::io::Error::other)?;
            window_layout::register_main_window_layout_events(app.handle())
                .map_err(std::io::Error::other)?;
            sequence_runtime::start_sequence_runtime(app.handle().clone());
            let state = app.state::<state::AppState>();
            if let Ok(model) = state::lock_model(&state) {
                let root = model.snapshot_dto().project_root;
                drop(model);
                if let Ok(mut watcher) = state::lock_filesystem_watcher(&state) {
                    let _ = watcher.sync_project_root(app.handle(), root);
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
}
